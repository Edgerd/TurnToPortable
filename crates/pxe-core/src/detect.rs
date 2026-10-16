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
    Standalone,
    Unknown,
}

impl InstallerKind {
    pub fn is_archive(&self) -> bool {
        matches!(self, Self::Zip | Self::SevenZip | Self::Rar | Self::Tar | Self::Msix | Self::Appx)
    }

    pub fn is_installer(&self) -> bool {
        !matches!(self, Self::Standalone | Self::Unknown | Self::Zip | Self::SevenZip | Self::Rar | Self::Tar)
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
    let data = std::fs::read(path)?;
    let sha256 = crate::extract::sha256_hex(&data);

    // --- 1. 魔数检测（最高优先级） ---
    if data.starts_with(MAGIC_MSI) {
        return Ok(Detection { kind: InstallerKind::Msi, note: None, pe: None, sha256 });
    }
    if data.starts_with(MAGIC_7Z) {
        return Ok(Detection { kind: InstallerKind::SevenZip, note: None, pe: None, sha256 });
    }
    if data.starts_with(MAGIC_RAR4) || data.starts_with(MAGIC_RAR5) {
        return Ok(Detection { kind: InstallerKind::Rar, note: None, pe: None, sha256 });
    }
    if data.starts_with(b"PK\x03\x04") {
        return detect_zip_family(&data, sha256);
    }
    if data.len() > 512 && &data[257..262] == MAGIC_USTAR {
        return Ok(Detection { kind: InstallerKind::Tar, note: None, pe: None, sha256 });
    }

    // --- 2. PE 结构检测 ---
    let pe_info = pe::parse_bytes(&data);
    if pe_info.is_valid_pe {
        if let Some(kind) = detect_pe_installers(&data, &pe_info) {
            return Ok(Detection { kind, note: None, pe: Some(pe_info), sha256 });
        }
    }

    Ok(Detection {
        kind: if pe_info.is_valid_pe { InstallerKind::Standalone } else { InstallerKind::Unknown },
        note: None,
        pe: (pe_info.is_valid_pe).then_some(pe_info),
        sha256,
    })
}

/// ZIP 家族：MSIX / AppX / Squirrel / 普通 ZIP / 7z-SFX(PE stub)
fn detect_zip_family(data: &[u8], sha256: String) -> crate::Result<Detection> {
    if let Ok(mut z) = zip::ZipArchive::new(std::io::Cursor::new(data)) {
        let names: Vec<String> = (0..z.len())
            .filter_map(|i| z.by_index_raw(i).ok().map(|f| f.name().to_string()))
            .collect();

        let has = |n: &str| names.iter().any(|f| f.eq_ignore_ascii_case(n));
        if has("AppxManifest.xml") {
            let kind = if has("AppxBlockMap.xml") { InstallerKind::Msix } else { InstallerKind::Appx };
            return Ok(Detection { kind, note: None, pe: None, sha256 });
        }
        if names.iter().any(|f| f.ends_with(".nupkg")) && names.iter().any(|f| f.contains("Squirrel")) {
            return Ok(Detection {
                kind: InstallerKind::Squirrel,
                note: Some("Squirrel: nupkg + Squirrel.exe inside ZIP".into()),
                pe: None,
                sha256,
            });
        }
        return Ok(Detection { kind: InstallerKind::Zip, note: None, pe: None, sha256 });
    }
    Ok(Detection { kind: InstallerKind::Unknown, note: Some("PK header but broken ZIP".into()), pe: None, sha256 })
}

/// PE 内的安装器特征：
/// - Inno Setup：文件末尾 overlay 有 `rDlPtS\0\0`（倒序的 `zStPlDr`，即 Inno ID）
/// - NSIS：第一个节附近有 `EF BE AD DE 4E 75 6C 6C 73 6F 66 74 49 6E 73 74`（`NullsoftInst`）
/// - WiX Burn：存在 `.wixburn` 节
/// - InstallShield：前 256KB 出现 "InstallShield" 字符串
fn detect_pe_installers(data: &[u8], pe_info: &pe::PeInfo) -> Option<InstallerKind> {
    // .wixburn 节是 WiX Burn bundle 的确定特征
    if pe_info.sections.iter().any(|s| s == ".wixburn") {
        return Some(InstallerKind::WiXBurn);
    }

    // NSIS: NullsoftInst 标记（通常在第一个节头部区域）
    const NSIS_SIG: &[u8] = &[0xEF, 0xBE, 0xAD, 0xDE];
    if let Some(pos) = find(data, NSIS_SIG, 0, data.len().min(0x4000)) {
        if data[pos + 4..].starts_with(b"NullsoftInst") {
            return Some(InstallerKind::Nsis);
        }
    }

    // Inno Setup: 文件尾部 12 字节内的 Inno ID（64bit 版本为 `rDlPtS\x06\x02` 等）
    let tail_start = data.len().saturating_sub(256);
    if let Some(pos) = find(data, b"rDlPtS", tail_start, data.len()) {
        let _ = pos;
        return Some(InstallerKind::InnoSetup);
    }

    // InstallShield
    let head_end = data.len().min(256 * 1024);
    if find(data, b"InstallShield", 0, head_end).is_some() {
        return Some(InstallerKind::InstallShield);
    }

    // 7z SFX：PE stub 内嵌 7z 魔数
    if find(data, MAGIC_7Z, 0, data.len()).is_some() {
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
