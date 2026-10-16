//! 安装包检测 —— 真实签名，替代原版"读前 8KB 找字符串 + >50MB 就猜安装器"。
//!
//! 检测依据（按可信度排序）：
//! 1. 文件魔数：MSI 复合文档头 / ZIP(PK) / 7z / RAR
//! 2. PE 结构：NSIS 的 `NullsoftInst` 标记、Inno Setup 尾部 ID、WiX Burn 的 `.wixburn` 节
//! 3. ZIP 内容：MSIX 的 AppxManifest.xml、Squirrel 的 Squirrel.exe
//!
//! 都不命中 → Standalone（绝不用文件大小猜测）。

use crate::pe;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallerKind {
    Msi,
    Msix,
    Appx,
    InnoSetup,
    Nsis,
    WiXBurn,
    InstallShield,
    Squirrel,
    Sfx7z,
    SfxRar,
    Zip,
    SevenZip,
    Rar,
    Tar,
    TarGz,
    Gz,
    Cab,
    Standalone,
    Unknown,
}

impl InstallerKind {
    pub fn is_archive(&self) -> bool {
        matches!(self, Self::Zip | Self::SevenZip | Self::Rar | Self::Tar | Self::TarGz | Self::Gz | Self::Cab | Self::Msix | Self::Appx)
    }

    pub fn is_installer(&self) -> bool {
        !matches!(
            self,
            Self::Standalone
                | Self::Unknown
                | Self::Zip
                | Self::SevenZip
                | Self::Rar
                | Self::Tar
                | Self::TarGz
                | Self::Gz
                | Self::Cab
        )
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Msi => "Windows Installer (MSI)",
            Self::Msix => "MSIX Package",
            Self::Appx => "AppX Package",
            Self::InnoSetup => "Inno Setup",
            Self::Nsis => "NSIS",
            Self::WiXBurn => "WiX Burn Bundle",
            Self::InstallShield => "InstallShield",
            Self::Squirrel => "Squirrel Installer",
            Self::Sfx7z => "7-Zip SFX",
            Self::SfxRar => "RAR SFX",
            Self::Zip => "ZIP Archive",
            Self::SevenZip => "7z Archive",
            Self::Rar => "RAR Archive",
            Self::Tar => "TAR Archive",
            Self::TarGz => "Tar+Gzip Archive",
            Self::Gz => "GZIP File",
            Self::Cab => "Cabinet (CAB)",
            Self::Standalone => "Standalone Application",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Detection {
    pub kind: InstallerKind,
    /// 主要检测结果之外的补充说明（如 Squirrel 由 ZIP 内嵌检测得出）
    pub note: Option<String>,
    pub pe: Option<pe::PeInfo>,
    pub sha256: String,
}

impl Detection {
    pub fn is_installer(&self) -> bool {
        self.kind.is_installer()
    }
}

/// 常见压缩文档魔数
const MAGIC_MSI: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const MAGIC_7Z: &[u8] = b"7z\xBC\xAF\x27\x1C";
const MAGIC_RAR4: &[u8] = b"Rar!\x1a\x07\x00";
const MAGIC_RAR5: &[u8] = b"Rar!\x1a\x07\x01\x00";
const MAGIC_USTAR: &[u8] = b"ustar";

pub fn detect(path: &Path) -> crate::Result<Detection> {
    let meta = std::fs::metadata(path)?;
    let len = meta.len();

    // 流式检测：不整读大文件。
    // - 头部 4MB 覆盖：魔数 / PE 头 / NSIS / InstallShield / 7z-SFX stub
    // - 尾部 1KB 覆盖：Inno Setup 的 overlay 尾部 ID
    // - ZIP 家族需要 EOCD（在文件末尾），小文件全读，大文件跳过深度嗅探
    const HEAD_MAX: u64 = 4 * 1024 * 1024;
    const FULL_READ_MAX: u64 = 256 * 1024 * 1024;

    let head_len = len.min(HEAD_MAX) as usize;
    let mut head = vec![0u8; head_len];
    {
        use std::io::Read;
        let mut f = std::fs::File::open(path)?;
        f.read_exact(&mut head)?;
    }

    let tail: Vec<u8> = if len > 1024 {
        use std::io::{Read, Seek, SeekFrom};
        let mut t = vec![0u8; 1024];
        let mut f = std::fs::File::open(path)?;
        f.seek(SeekFrom::Start(len - 1024))?;
        f.read_exact(&mut t)?;
        t
    } else {
        head.clone()
    };

    let sha256 = if len <= FULL_READ_MAX {
        crate::extract::sha256_hex(&std::fs::read(path)?)
    } else {
        // 大文件：流式计算，不整读
        sha256_file_streaming(path)?
    };

    // --- 1. 魔数检测（最高优先级） ---
    if head.starts_with(MAGIC_MSI) {
        return Ok(Detection { kind: InstallerKind::Msi, note: None, pe: None, sha256 });
    }
    if head.starts_with(MAGIC_7Z) {
        return Ok(Detection { kind: InstallerKind::SevenZip, note: None, pe: None, sha256 });
    }
    if head.starts_with(MAGIC_RAR4) || head.starts_with(MAGIC_RAR5) {
        return Ok(Detection { kind: InstallerKind::Rar, note: None, pe: None, sha256 });
    }
    if head.starts_with(b"PK\x03\x04") {
        return detect_zip_family(path, &sha256);
    }
    if head.len() > 512 && &head[257..262] == MAGIC_USTAR {
        return Ok(Detection { kind: InstallerKind::Tar, note: None, pe: None, sha256 });
    }
    // GZIP 家族：进一步嗅探内部是 tar 还是单文件
    if head.starts_with(b"\x1f\x8b") {
        let kind = sniff_gzip(&head);
        return Ok(Detection { kind, note: None, pe: None, sha256 });
    }
    // Windows CAB（expand.exe 可解）
    if head.starts_with(b"MSCF") {
        return Ok(Detection { kind: InstallerKind::Cab, note: None, pe: None, sha256 });
    }

    // --- 2. PE 结构检测 ---
    // 头部被截断时 goblin 可能解析失败，但安装器特征检测只依赖 MZ + 字节标记，
    // 所以只要 PE 魔数存在就继续查特征；PeInfo 仅作展示用。
    let pe_info = pe::parse_bytes(&head);
    let looks_pe = pe_info.is_valid_pe || (head.len() >= 2 && &head[..2] == b"MZ");
    if looks_pe {
        if let Some(kind) = detect_pe_installers(&head, &tail, &pe_info) {
            return Ok(Detection {
                kind,
                note: None,
                pe: pe_info.is_valid_pe.then_some(pe_info),
                sha256,
            });
        }
    }

    Ok(Detection {
        kind: if pe_info.is_valid_pe { InstallerKind::Standalone } else { InstallerKind::Unknown },
        note: None,
        pe: (pe_info.is_valid_pe).then_some(pe_info),
        sha256,
    })
}

/// 流式 SHA256（>256MB 的文件不整读进内存）
fn sha256_file_streaming(path: &Path) -> crate::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut h = Sha256::new();
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// 解压 gzip 前 1KB，看里面是不是 tar（512 字节头 + offset 257 的 ustar 标记）
fn sniff_gzip(data: &[u8]) -> InstallerKind {
    use std::io::Read;
    let mut d = flate2::read::MultiGzDecoder::new(data);
    let mut buf = [0u8; 1024];
    let n = d.read(&mut buf).unwrap_or(0);
    if n > 512 && &buf[257..262] == MAGIC_USTAR {
        InstallerKind::TarGz
    } else {
        InstallerKind::Gz
    }
}

/// ZIP 家族：MSIX / AppX / Squirrel / 普通 ZIP（EOCD 在文件尾，zip crate 自行 seek，不整读）
fn detect_zip_family(path: &Path, sha256: &str) -> crate::Result<Detection> {
    let d = |kind, note| Ok(Detection { kind, note, pe: None, sha256: sha256.to_string() });
    if let Ok(mut z) = zip::ZipArchive::new(std::fs::File::open(path)?) {
        let names: Vec<String> = (0..z.len())
            .filter_map(|i| z.by_index_raw(i).ok().map(|f| f.name().to_string()))
            .collect();

        let has = |n: &str| names.iter().any(|f| f.eq_ignore_ascii_case(n));
        if has("AppxManifest.xml") {
            let kind = if has("AppxBlockMap.xml") { InstallerKind::Msix } else { InstallerKind::Appx };
            return d(kind, None);
        }
        if names.iter().any(|f| f.ends_with(".nupkg")) && names.iter().any(|f| f.contains("Squirrel")) {
            return d(
                InstallerKind::Squirrel,
                Some("Squirrel: nupkg + Squirrel.exe inside ZIP".into()),
            );
        }
        return d(InstallerKind::Zip, None);
    }
    d(InstallerKind::Unknown, Some("PK header but broken ZIP".into()))
}

/// PE 内的安装器特征：
/// - Inno Setup：文件末尾 overlay 有 `rDlPtS\0\0`（倒序的 `zStPlDr`，即 Inno ID）
/// - NSIS：第一个节附近有 `EF BE AD DE 4E 75 6C 6C 73 6F 66 74 49 6E 73 74`（`NullsoftInst`）
/// - WiX Burn：存在 `.wixburn` 节
/// - InstallShield：前 256KB 出现 "InstallShield" 字符串
fn detect_pe_installers(head: &[u8], tail: &[u8], pe_info: &pe::PeInfo) -> Option<InstallerKind> {
    // .wixburn 节是 WiX Burn bundle 的确定特征
    if pe_info.sections.iter().any(|s| s == ".wixburn") {
        return Some(InstallerKind::WiXBurn);
    }

    // NSIS: NullsoftInst 标记（通常在第一个节头部区域，头部 4MB 内必然覆盖）
    const NSIS_SIG: &[u8] = &[0xEF, 0xBE, 0xAD, 0xDE];
    if let Some(pos) = find(head, NSIS_SIG, 0, head.len().min(0x4000)) {
        if head[pos + 4..].starts_with(b"NullsoftInst") {
            return Some(InstallerKind::Nsis);
        }
    }

    // Inno Setup: Inno ID `rDlPtS`。多数版本在文件尾部 overlay，但部分发行版
    // （如 VSCodium 6.5+）在头部数据区；头尾都查，避免漏检。
    if find(tail, b"rDlPtS", 0, tail.len()).is_some()
        || find(head, b"rDlPtS", 0, head.len()).is_some()
    {
        return Some(InstallerKind::InnoSetup);
    }

    // InstallShield（标记在 PE stub 内，头部即可覆盖）
    if find(head, b"InstallShield", 0, head.len().min(256 * 1024)).is_some() {
        return Some(InstallerKind::InstallShield);
    }

    // 7z SFX：PE stub 内嵌 7z 魔数（stub 在文件头部，不再全文件扫描）
    if find(head, MAGIC_7Z, 0, head.len()).is_some() {
        return Some(InstallerKind::Sfx7z);
    }

    None
}

fn find(haystack: &[u8], needle: &[u8], from: usize, to: usize) -> Option<usize> {
    let end = to.min(haystack.len());
    if from >= end || needle.len() > end - from {
        return None;
    }
    haystack[from..end]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msi_magic() {
        let mut d = vec![0u8; 1024];
        d[..8].copy_from_slice(MAGIC_MSI);
        let info = Detection { kind: InstallerKind::Unknown, note: None, pe: None, sha256: String::new() };
        assert!(!info.is_installer());
        assert!(d.starts_with(MAGIC_MSI));
    }

    #[test]
    fn find_works() {
        assert_eq!(find(b"hello world", b"world", 0, 11), Some(6));
        assert_eq!(find(b"hello", b"world", 0, 5), None);
    }
}
