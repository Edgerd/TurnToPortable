//! 解包引擎：内嵌优先（zip / 7z），外部工具回退（innoextract / msiexec / unrar）。
//!
//! 与原版的区别：
//! - ZIP/7z/MSIX 不再依赖用户安装 7z.exe
//! - MSI 不再靠 "This installation package" 字符串检测（该字符串根本不在 MSI 文件里）
//! - 解压结果验证更严格：必须有 PE 或 manifest，且无裸节碎片

use crate::builder::copy_dir_contents;
use crate::detect::InstallerKind;
use crate::{Error, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 解压选项：由调用方（CLI/GUI/pipeline）显式传入，不再使用全局开关。
#[derive(Debug, Clone)]
pub struct ExtractOptions {
    /// 外部工具超时
    pub timeout: Duration,
    /// 深度解包：解压成功后继续解内层"最大的压缩档"，直到解出主程序（exe）
    pub deep_extract: bool,
}

impl Default for ExtractOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(300),
            deep_extract: true,
        }
    }
}

/// 深度解包每层最多尝试的候选文件数
const DEEP_MAX_CANDIDATES: usize = 5;
/// 深度解包最大层数（安全上限，防止压缩包套压缩包死循环）
const DEEP_MAX_LAYERS: usize = 8;

pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    let out = h.finalize();
    out.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Clone)]
pub struct Extracted {
    /// 实际内容所在目录
    pub dir: PathBuf,
    /// 使用的解包器名称
    pub via: String,
}

/// 解包主入口：按 kind 路由到具体实现，失败时回退链兜底。
pub fn extract(
    path: &Path,
    kind: InstallerKind,
    dest: &Path,
    opts: &ExtractOptions,
) -> Result<Extracted> {
    std::fs::create_dir_all(dest)?;

    let attempts: &[(InstallerKind, &str)] = match kind {
        InstallerKind::InnoSetup => &[
            // innoextract 1.9 封顶（Inno 6.4 之前），新版走 innounp
            (InstallerKind::InnoSetup, "innoextract"),
            (InstallerKind::InnoSetup, "innounp"),
            (InstallerKind::Sfx7z, "embedded-7z"),
        ],
        InstallerKind::Nsis => &[
            (InstallerKind::Sfx7z, "embedded-7z"),
            (InstallerKind::Nsis, "external-7z"),
        ],
        InstallerKind::Sfx7z | InstallerKind::InstallShield | InstallerKind::WiXBurn => &[
            (InstallerKind::Sfx7z, "embedded-7z"),
            // InstallShield 专有容器（DevStudio 9 ~ IS 2021）
            (InstallerKind::InstallShield, "ISx"),
            (InstallerKind::Nsis, "external-7z"),
        ],
        InstallerKind::Msi => &[
            // msiexec（Windows 内置）失败或不可用时回退 lessmsi（免 msiexec、跨平台）
            (InstallerKind::Msi, "msiexec"),
            (InstallerKind::Msi, "lessmsi"),
        ],
        InstallerKind::Msix
        | InstallerKind::MsixBundle
        | InstallerKind::Appx
        | InstallerKind::Zip
        | InstallerKind::Squirrel => &[(InstallerKind::Zip, "embedded-zip")],
        InstallerKind::SevenZip => &[(InstallerKind::Sfx7z, "embedded-7z")],
        InstallerKind::Rar | InstallerKind::SfxRar => &[(InstallerKind::Rar, "unrar")],
        InstallerKind::Tar => &[(InstallerKind::Tar, "embedded-tar")],
        InstallerKind::TarGz => &[(InstallerKind::TarGz, "embedded-targz")],
        InstallerKind::TarXz => &[(InstallerKind::TarXz, "embedded-tarxz")],
        InstallerKind::TarBz2 => &[(InstallerKind::TarBz2, "embedded-tarbz2")],
        InstallerKind::TarZst => &[(InstallerKind::TarZst, "embedded-tarzst")],
        InstallerKind::Gz => &[(InstallerKind::Gz, "embedded-gzip")],
        InstallerKind::Xz => &[(InstallerKind::Xz, "embedded-xz")],
        InstallerKind::Bz2 => &[(InstallerKind::Bz2, "embedded-bz2")],
        InstallerKind::Zst => &[(InstallerKind::Zst, "embedded-zst")],
        InstallerKind::Cab => &[(InstallerKind::Cab, "expand")],
        InstallerKind::Iso => &[(InstallerKind::Iso, "embedded-iso")],
        // UDF 桥接镜像（ISO+UDF 双目录结构）：UDF 层失败时试 ISO 层
        InstallerKind::Udf => &[
            (InstallerKind::Udf, "embedded-udf"),
            (InstallerKind::Iso, "embedded-iso"),
        ],
        InstallerKind::Wim => &[
            (InstallerKind::Wim, "wimextract"),
            (InstallerKind::Nsis, "external-7z"),
        ],
        InstallerKind::Msu => &[(InstallerKind::Nsis, "external-7z")],
        // 分卷：7z/unrar 直接吃首个分卷；zip 分卷需合并
        InstallerKind::Split7z => &[(InstallerKind::Nsis, "external-7z")],
        InstallerKind::SplitRar => &[(InstallerKind::Rar, "unrar")],
        InstallerKind::SplitZip => &[(InstallerKind::SplitZip, "merge-zip")],
        InstallerKind::IExpress => &[
            (InstallerKind::IExpress, "sfx-cab"),
            (InstallerKind::Sfx7z, "embedded-7z"),
        ],
        _ => &[
            (InstallerKind::Zip, "embedded-zip"),
            (InstallerKind::Sfx7z, "embedded-7z"),
        ],
    };

    // 串联所有尝试的错误（主解包器在前 —— 通常信息量最大）
    let mut errs: Vec<String> = Vec::new();
    for (k, label) in attempts {
        let r = match *k {
            InstallerKind::Zip => extract_zip(path, dest),
            InstallerKind::Sfx7z => extract_7z_embedded(path, dest),
            InstallerKind::Nsis => extract_7z_external(path, dest, opts),
            InstallerKind::Tar => {
                extract_tar(std::io::BufReader::new(std::fs::File::open(path)?), dest)
            }
            InstallerKind::TarGz => extract_tar(
                flate2::read::MultiGzDecoder::new(std::fs::File::open(path)?),
                dest,
            ),
            InstallerKind::TarXz => {
                extract_tar(xz2::read::XzDecoder::new(std::fs::File::open(path)?), dest)
            }
            InstallerKind::TarBz2 => extract_tar(
                bzip2::read::BzDecoder::new(std::fs::File::open(path)?),
                dest,
            ),
            InstallerKind::TarZst => {
                let d = zstd::Decoder::new(std::fs::File::open(path)?)
                    .map_err(|e| Error::Extraction(format!("zstd: {e}")))?;
                extract_tar(d, dest)
            }
            InstallerKind::Gz => extract_gz(path, dest),
            InstallerKind::Xz => extract_single(
                path,
                dest,
                |r| Ok(Box::new(xz2::read::XzDecoder::new(r)) as Box<dyn std::io::Read>),
                "xz",
            ),
            InstallerKind::Bz2 => extract_single(
                path,
                dest,
                |r| Ok(Box::new(bzip2::read::BzDecoder::new(r)) as Box<dyn std::io::Read>),
                "bzip2",
            ),
            InstallerKind::Zst => extract_single(
                path,
                dest,
                |r| {
                    let d = zstd::Decoder::new(r)
                        .map_err(|e| Error::Extraction(format!("zstd: {e}")))?;
                    Ok(Box::new(d) as Box<dyn std::io::Read>)
                },
                "zstd",
            ),
            InstallerKind::Iso => extract_iso(path, dest),
            InstallerKind::Udf => extract_udf(path, dest),
            InstallerKind::SplitZip => extract_split_zip(path, dest),
            InstallerKind::IExpress => extract_iexpress(path, dest, opts),
            InstallerKind::Wim => extract_wimextract(path, dest, opts),
            InstallerKind::Cab => extract_external(
                "expand",
                &[
                    "-r".into(),
                    path.display().to_string(),
                    dest.display().to_string(),
                ],
                opts,
            ),
            InstallerKind::Rar => extract_external(
                "unrar",
                &[
                    "x".into(),
                    path.display().to_string(),
                    format!("{}{}", dest.display(), std::path::MAIN_SEPARATOR),
                ],
                opts,
            ),
            // 同一 Kind 可能有多个外部工具候选（innoextract/innounp、msiexec/lessmsi），
            // 具体工具由 label 决定
            _ => match *label {
                "innoextract" => extract_innoextract(path, dest, opts),
                "innounp" => extract_innounp(path, dest, opts),
                "ISx" => extract_isx(path, dest, opts),
                "msiexec" => extract_msi(path, dest, opts),
                "lessmsi" => extract_lessmsi(path, dest, opts),
                _ => continue,
            },
        };
        match r {
            Ok(()) => {
                // innoextract 会保留 Inno 的 {app} 前缀（多套一层 app/ 目录），展平它
                if *k == InstallerKind::InnoSetup {
                    let _ = flatten_app_dir(dest);
                }
                if opts.deep_extract {
                    // 深度解包：反复解"最大的压缩档候选"，直到解出主程序（exe）
                    deep_unpack(dest, opts);
                } else {
                    // electron-builder 的 NSIS 布局：外层壳里只有 $PLUGINSDIR/app-64.7z，
                    // 真正的主程序在内层 7z —— 检测到就二次解包（embedded / external 解出的都适用）
                    if !has_exe_outside_pluginsdir(dest) {
                        if let Some(inner) = find_pluginsdir_7z(dest) {
                            let tmp = dest.join(".__pxe_inner");
                            let _ = std::fs::remove_dir_all(&tmp);
                            if sevenz_rust::decompress_file(&inner, &tmp).is_ok() && validate(&tmp)
                            {
                                // 清空 dest（保留 tmp），再把内层内容移进来
                                for e in std::fs::read_dir(dest)?.flatten() {
                                    if e.file_name() != ".__pxe_inner" {
                                        let p = e.path();
                                        if p.is_dir() {
                                            std::fs::remove_dir_all(&p)?;
                                        } else {
                                            std::fs::remove_file(&p)?;
                                        }
                                    }
                                }
                                copy_dir_contents(&tmp, dest)?;
                                std::fs::remove_dir_all(&tmp)?;
                                return Ok(Extracted {
                                    dir: dest.to_path_buf(),
                                    via: "nsis-app-7z".into(),
                                });
                            }
                            let _ = std::fs::remove_dir_all(&tmp);
                        }
                    }
                }
                if validate(dest) {
                    return Ok(Extracted {
                        dir: dest.to_path_buf(),
                        via: label.to_string(),
                    });
                }
                errs.push(format!("{label}: output failed validation"));
            }
            Err(e) => {
                let msg = e.to_string();
                if !errs.iter().any(|x| x.contains(&msg)) {
                    errs.push(msg);
                }
            }
        }
        // 清空后重试下一种方式
        clear_dir(dest)?;
    }

    Err(Error::Extraction(if errs.is_empty() {
        "no extractor available".into()
    } else {
        errs.join(" | ")
    }))
}

/// 展平 innoextract 的 {app} 包装目录：dest 下只有 app/ 一个目录时，把其内容上移一层。
fn flatten_app_dir(dest: &Path) -> Result<()> {
    let entries: Vec<_> = std::fs::read_dir(dest)?.flatten().collect();
    if entries.len() != 1 {
        return Ok(());
    }
    let only = &entries[0].path();
    let is_app_dir = only.is_dir()
        && only
            .file_name()
            .map(|n| {
                let n = n.to_string_lossy();
                n.eq_ignore_ascii_case("app") || n.eq_ignore_ascii_case("{app}")
            })
            .unwrap_or(false);
    if !is_app_dir {
        return Ok(());
    }
    for e in std::fs::read_dir(only)?.flatten() {
        std::fs::rename(e.path(), dest.join(e.file_name()))?;
    }
    std::fs::remove_dir(only)?;
    Ok(())
}

/// 展平"dest 下只有一个目录"的布局（外部工具以 cwd 方式解包时的常见输出）：
/// 把唯一子目录的内容上移一层。dest 为空或不只一个条目时不动。
fn flatten_single_dir(dest: &Path) {
    let Ok(entries) = std::fs::read_dir(dest) else {
        return;
    };
    let entries: Vec<_> = entries.flatten().collect();
    if entries.len() != 1 || !entries[0].path().is_dir() {
        return;
    }
    let only = entries[0].path();
    let Ok(inner) = std::fs::read_dir(&only) else {
        return;
    };
    for e in inner.flatten() {
        let _ = std::fs::rename(e.path(), dest.join(e.file_name()));
    }
    let _ = std::fs::remove_dir(&only);
}

// ---------- 深度解包 ----------

/// 深度解包：每层在解压结果里找最大的压缩档文件按大小降序依次尝试（最多 5 个），
/// 解出且校验通过就用内层内容替换当前目录；一旦目录里出现主程序 exe 就停止。
/// 覆盖 electron-builder（$PLUGINSDIR/app-64.7z）、zip 套 zip、分卷合并后套壳等场景。
fn deep_unpack(dest: &Path, opts: &ExtractOptions) {
    for _ in 0..DEEP_MAX_LAYERS {
        // 收集本层候选：按大小降序，最多取 5 个压缩档
        let mut candidates: Vec<(u64, PathBuf)> = crate::fsx::walk_files(dest, 4)
            .into_iter()
            .filter(|p| is_deep_candidate(p))
            .filter_map(|p| {
                let sz = std::fs::metadata(&p).ok()?.len();
                Some((sz, p))
            })
            .collect();
        candidates.sort_by(|a, b| b.0.cmp(&a.0));
        candidates.truncate(DEEP_MAX_CANDIDATES);

        // 已解出主程序 → 到此为止。判定：存在 $PLUGINSDIR 之外的 exe，
        // 且它比剩余的最大压缩档候选更大（避免把 NSIS 壳里的卸载器
        // /$R0 小 exe 误判成主程序而提前停止）。
        if let Some(max_exe) = max_exe_outside_pluginsdir(dest) {
            let max_cand = candidates.first().map(|(s, _)| *s).unwrap_or(0);
            if max_exe > max_cand {
                return;
            }
        }

        let mut progressed = false;
        for (_, cand) in candidates {
            let tmp = dest.join(".__pxe_deep");
            let _ = std::fs::remove_dir_all(&tmp);
            let ok = try_extract_candidate(&cand, &tmp, opts) && validate(&tmp);
            if ok {
                // 用内层内容替换当前目录（保留 tmp 自身）
                if clear_dir_keep(dest, &tmp).is_ok() {
                    progressed = true;
                }
            }
            let _ = std::fs::remove_dir_all(&tmp);
            if progressed {
                break;
            }
        }
        // 本层 5 个候选都解不动 → 结束
        if !progressed {
            return;
        }
    }
}

/// 深度解包候选：压缩档扩展名（不含 .exe —— 避免把主程序/安装器本体再拆一遍）
fn is_deep_candidate(p: &Path) -> bool {
    let ext = p
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    matches!(ext.as_str(), "7z" | "zip" | "tar" | "gz" | "tgz")
}

/// 按扩展名路由到内嵌解压器
fn try_extract_candidate(path: &Path, tmp: &Path, opts: &ExtractOptions) -> bool {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let r = match ext.as_str() {
        // 7z：内嵌优先（无依赖），失败回退外部 7z（sevenz_rust 不支持部分压缩方法）
        "7z" => {
            let embedded = extract_7z_embedded(path, tmp);
            if embedded.is_ok() {
                embedded
            } else {
                extract_7z_external(path, tmp, opts)
            }
        }
        "zip" => extract_zip(path, tmp),
        "tar" => match std::fs::File::open(path) {
            Ok(f) => extract_tar(std::io::BufReader::new(f), tmp),
            Err(e) => Err(e.into()),
        },
        "tgz" => match std::fs::File::open(path) {
            Ok(f) => extract_tar(flate2::read::MultiGzDecoder::new(f), tmp),
            Err(e) => Err(e.into()),
        },
        "gz" => extract_gz(path, tmp),
        _ => return false,
    };
    r.is_ok()
}

/// 清空 dest 但保留 tmp（tmp 是 dest 的子目录），再把 tmp 内容移到 dest
fn clear_dir_keep(dest: &Path, tmp: &Path) -> Result<()> {
    for e in std::fs::read_dir(dest)?.flatten() {
        if e.path() == tmp {
            continue;
        }
        let p = e.path();
        if p.is_dir() {
            std::fs::remove_dir_all(&p)?;
        } else {
            std::fs::remove_file(&p)?;
        }
    }
    copy_dir_contents(tmp, dest)?;
    Ok(())
}

// ---------- 内嵌实现 ----------

fn extract_zip(path: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(path)?;
    let mut z = zip::ZipArchive::new(file)?;
    z.extract(dest)?;
    Ok(())
}

fn extract_7z_embedded(path: &Path, dest: &Path) -> Result<()> {
    sevenz_rust::decompress_file(path, dest).map_err(|e| Error::SevenZ(e.to_string()))
}

/// 用外部 7-Zip 解（NSIS 外壳只有完整版 7z 支持；7z.exe/7z.dll 需在本程序目录或 PATH）
fn extract_7z_external(path: &Path, dest: &Path, opts: &ExtractOptions) -> Result<()> {
    let exe = find_7z().ok_or_else(|| {
        Error::Extraction(
            "7-Zip not found: this NSIS package needs 7z.exe + 7z.dll (place them next to the program, or install 7-Zip)".into(),
        )
    })?;
    let mut cmd = std::process::Command::new(&exe);
    cmd.args(["x", "-y", &format!("-o{}", dest.display())]);
    cmd.arg(path);
    let (status, stderr) =
        run_with_timeout(no_window(cmd), opts).map_err(|e| Error::Extraction(e.to_string()))?;
    if !status.success() {
        let detail = stderr.trim();
        let last = detail
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        return Err(Error::Extraction(format!(
            "7z exited with {status}: {last}"
        )));
    }
    Ok(())
}

/// 找 7z.exe：本程序同目录优先，其次 PATH（统一走 which）
fn find_7z() -> Option<PathBuf> {
    let self_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    for name in ["7z.exe", "7za.exe"] {
        let p = self_dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    which("7z").or_else(|| which("7za"))
}

fn extract_tar<R: std::io::Read>(reader: R, dest: &Path) -> Result<()> {
    let mut tar = tar::Archive::new(reader);
    tar.set_preserve_permissions(false);
    tar.unpack(dest)
        .map_err(|e| Error::Extraction(format!("tar: {e}")))
}

/// 单文件 gzip：解出来若内嵌 tar 则继续按 tar 解包，否则落成单个文件
fn extract_gz(path: &Path, dest: &Path) -> Result<()> {
    use std::io::Read;
    let out_name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "payload".into());
    let out_path = dest.join(&out_name);
    let f = std::fs::File::open(path)?;
    let mut d = flate2::read::MultiGzDecoder::new(f);
    let mut buf = Vec::new();
    d.read_to_end(&mut buf)
        .map_err(|e| Error::Extraction(format!("gzip: {e}")))?;
    // tar 头（前 512 字节 offset 257 = ustar）→ 按 tar 解
    if buf.len() > 512 && &buf[257..262] == b"ustar" {
        return extract_tar(std::io::Cursor::new(buf), dest);
    }
    std::fs::write(out_path, &buf)?;
    Ok(())
}

/// 单文件压缩档（xz/bz2/zst）：流式解压为 dest/去后缀文件（不整读进内存）。
fn extract_single(
    path: &Path,
    dest: &Path,
    decoder: impl FnOnce(std::fs::File) -> Result<Box<dyn std::io::Read>>,
    label: &str,
) -> Result<()> {
    let out_name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "payload".into());
    let f = std::fs::File::open(path)?;
    let mut d = decoder(f)?;
    let mut out = std::fs::File::create(dest.join(&out_name))?;
    std::io::copy(&mut d, &mut out).map_err(|e| Error::Extraction(format!("{label}: {e}")))?;
    Ok(())
}

// ---------- 光盘镜像（ISO / UDF，纯内嵌） ----------

/// ISO/UDF 文件名清洗：去掉 ISO9660 的 ";1" 版本后缀，替换 Windows 非法字符
fn sanitize_disc_name(name: &str) -> String {
    let base = name.split(';').next().unwrap_or(name).trim_end_matches('.');
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect();
    if cleaned.is_empty() {
        "_".into()
    } else {
        cleaned
    }
}

fn extract_iso(path: &Path, dest: &Path) -> Result<()> {
    let f = std::fs::File::open(path)?;
    let vol = iso9660::ISO9660::new(f).map_err(|e| Error::Extraction(format!("iso9660: {e}")))?;
    iso_walk(&vol.root, dest).map_err(|e| Error::Extraction(format!("iso9660: {e}")))?;
    Ok(())
}

fn iso_walk(dir: &iso9660::ISODirectory<std::fs::File>, out: &Path) -> std::io::Result<()> {
    for entry in dir.contents() {
        let entry = entry.map_err(std::io::Error::other)?;
        let name = sanitize_disc_name(entry.identifier());
        // 跳过 . / .. 自引用条目
        if name == "_" && entry.identifier().is_empty() {
            continue;
        }
        if entry.identifier() == "." || entry.identifier() == ".." {
            continue;
        }
        match entry {
            iso9660::DirectoryEntry::Directory(d) => {
                let sub = out.join(&name);
                std::fs::create_dir_all(&sub)?;
                iso_walk(&d, &sub)?;
            }
            iso9660::DirectoryEntry::File(f) => {
                let mut reader = f.read();
                let mut out_f = std::fs::File::create(out.join(&name))?;
                std::io::copy(&mut reader, &mut out_f)?;
            }
        }
    }
    Ok(())
}

fn extract_udf(path: &Path, dest: &Path) -> Result<()> {
    let f = std::fs::File::open(path)?;
    let vol = hadris_udf::UdfVolume::open(std::io::BufReader::new(f))
        .map_err(|e| Error::Extraction(format!("udf: {e}")))?;
    let root = vol
        .root_dir()
        .map_err(|e| Error::Extraction(format!("udf: {e}")))?;
    udf_walk(&vol, &root, dest).map_err(|e| Error::Extraction(format!("udf: {e}")))?;
    Ok(())
}

fn udf_walk(
    vol: &hadris_udf::UdfVolume<std::io::BufReader<std::fs::File>>,
    dir: &hadris_udf::UdfDir,
    out: &Path,
) -> std::io::Result<()> {
    for e in dir.entries() {
        let name = sanitize_disc_name(e.name());
        if name == "." || name == ".." {
            continue;
        }
        if e.is_directory {
            let sub = out.join(&name);
            std::fs::create_dir_all(&sub)?;
            let sub_dir = vol.read_directory(&e.icb).map_err(std::io::Error::other)?;
            udf_walk(vol, &sub_dir, &sub)?;
        } else {
            let data = vol.read_file(e).map_err(std::io::Error::other)?;
            std::fs::write(out.join(&name), &data)?;
        }
    }
    Ok(())
}

/// ZIP 分卷：合并 .z01..zNN + 主卷 .zip 为临时 zip 再解压（ZIP 分卷是纯字节切分）
fn extract_split_zip(path: &Path, dest: &Path) -> Result<()> {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .ok_or_else(|| Error::Extraction("bad split name".into()))?;
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    if !ext.starts_with('z') || ext.len() != 3 {
        return Err(Error::Extraction("ZIP 分卷请选择 .z01 首卷".into()));
    }
    let base = name[..name.len() - 4].to_string(); // 去掉 .zNN
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let main = dir.join(format!("{base}.zip"));
    if !main.exists() {
        return Err(Error::Extraction(format!(
            "缺少 ZIP 主卷 {}",
            main.display()
        )));
    }
    let mut volumes: Vec<std::fs::File> = Vec::new();
    for i in 1..=99u32 {
        let p = dir.join(format!("{base}.z{i:02}"));
        if p.exists() {
            volumes.push(std::fs::File::open(&p)?);
        }
    }
    let merged = dest.join(".__pxe_merged.zip");
    {
        let mut out = std::io::BufWriter::new(std::fs::File::create(&merged)?);
        for v in &mut volumes {
            std::io::copy(v, &mut out)?;
        }
        std::io::copy(&mut std::fs::File::open(&main)?, &mut out)?;
    }
    let r = extract_zip(&merged, dest);
    let _ = std::fs::remove_file(&merged);
    r
}

// ---------- 外部工具回退 ----------

#[cfg(windows)]
fn no_window(mut cmd: std::process::Command) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

#[cfg(not(windows))]
fn no_window(cmd: std::process::Command) -> std::process::Command {
    cmd
}

/// spawn 子进程并带超时轮询等待（超时不留僵尸进程：先 kill 再收尸）。
/// 返回退出码与 stderr（截断到 4KB，用于生成可读错误）。
fn run_with_timeout(
    mut cmd: std::process::Command,
    opts: &ExtractOptions,
) -> Result<(std::process::ExitStatus, String)> {
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| Error::Extraction(format!("spawn: {e}")))?;
    let mut stderr_pipe = child.stderr.take();
    let start = Instant::now();
    let status;
    loop {
        match child.try_wait()? {
            Some(st) => {
                status = st;
                break;
            }
            None => {
                if start.elapsed() > opts.timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Error::Extraction(format!(
                        "timed out after {}s",
                        opts.timeout.as_secs()
                    )));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
    // 进程已退出，读管道不会阻塞
    let mut buf = String::new();
    if let Some(p) = stderr_pipe.as_mut() {
        let mut raw = Vec::new();
        if std::io::Read::read_to_end(p, &mut raw).is_ok() {
            raw.truncate(4096);
            buf = String::from_utf8_lossy(&raw).to_string();
        }
    }
    Ok((status, buf))
}

fn extract_external(tool: &str, args: &[String], opts: &ExtractOptions) -> Result<()> {
    if which(tool).is_none() {
        return Err(Error::Extraction(format!("{tool} not found in PATH")));
    }
    let mut cmd = std::process::Command::new(tool);
    cmd.args(args);
    let (status, stderr) =
        run_with_timeout(no_window(cmd), opts).map_err(|e| Error::Extraction(e.to_string()))?;
    if !status.success() {
        let detail = stderr.trim();
        if detail.is_empty() {
            return Err(Error::Extraction(format!("{tool} exited with {status}")));
        }
        // 优先取首条含 "error"/"failed" 的行（主错误行，通常在摘要之前），否则取最后一行非空输出
        let last = detail
            .lines()
            .find(|l| {
                let t = l.to_lowercase();
                (t.contains("error") || t.contains("failed")) && !t.is_empty()
            })
            .or_else(|| detail.lines().rev().find(|l| !l.trim().is_empty()))
            .unwrap_or(detail);
        return Err(Error::Extraction(format!("{tool}: {last}")));
    }
    Ok(())
}

fn extract_innoextract(path: &Path, dest: &Path, opts: &ExtractOptions) -> Result<()> {
    extract_external(
        "innoextract",
        &[
            "-d".into(),
            dest.display().to_string(),
            "-s".into(),
            path.display().to_string(),
        ],
        opts,
    )
}

/// innounp：支持新版 Inno Setup（innoextract 1.9 解不动的 6.4+）。
/// 输出会带 {app} 前缀目录，成功后同样展平。
fn extract_innounp(path: &Path, dest: &Path, opts: &ExtractOptions) -> Result<()> {
    let r = extract_external(
        "innounp",
        &[
            "-x".into(),
            format!("-d{}", dest.display()),
            path.display().to_string(),
        ],
        opts,
    );
    if r.is_ok() {
        let _ = flatten_app_dir(dest);
        flatten_single_dir(dest);
    }
    r
}

/// ISx：InstallShield 专有容器解包器（DevStudio 9 ~ InstallShield 2021）。
/// 无 -d 参数，在工作目录解出 —— 用 cwd=dest，解出后把唯一子目录内容上移。
fn extract_isx(path: &Path, dest: &Path, opts: &ExtractOptions) -> Result<()> {
    if which("ISx").is_none() {
        return Err(Error::Extraction("ISx not found in PATH".into()));
    }
    let mut cmd = std::process::Command::new("ISx");
    cmd.arg(path);
    cmd.current_dir(dest);
    let (status, stderr) =
        run_with_timeout(no_window(cmd), opts).map_err(|e| Error::Extraction(e.to_string()))?;
    if !status.success() {
        let detail = stderr.trim();
        let last = detail
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("no output");
        return Err(Error::Extraction(format!("ISx: {last}")));
    }
    flatten_single_dir(dest);
    Ok(())
}

/// lessmsi：免 msiexec 的 MSI 解包（跨平台 / 无管理员权限场景的回退）。
fn extract_lessmsi(path: &Path, dest: &Path, opts: &ExtractOptions) -> Result<()> {
    extract_external(
        "lessmsi",
        &[
            "x".into(),
            path.display().to_string(),
            dest.display().to_string(),
        ],
        opts,
    )
}

/// IExpress（Windows 自带 SFX-CAB）：CAB 贴在 PE stub 之后，expand 可直接抽取；
/// 非 Windows 用 cabextract。失败后回退链会继续试 embedded-7z。
fn extract_iexpress(path: &Path, dest: &Path, opts: &ExtractOptions) -> Result<()> {
    if cfg!(windows) {
        extract_external(
            "expand",
            &[
                "-r".into(),
                path.display().to_string(),
                "-F:*".into(),
                dest.display().to_string(),
            ],
            opts,
        )
    } else {
        extract_external(
            "cabextract",
            &[
                "-d".into(),
                dest.display().to_string(),
                path.display().to_string(),
            ],
            opts,
        )
    }
}

/// wimlib 解 WIM（非 Windows 也可用）。失败后回退 external-7z。
fn extract_wimextract(path: &Path, dest: &Path, opts: &ExtractOptions) -> Result<()> {
    extract_external(
        "wimextract",
        &[
            path.display().to_string(),
            "all".into(),
            format!("--dest={}", dest.display()),
        ],
        opts,
    )
}

fn extract_msi(path: &Path, dest: &Path, opts: &ExtractOptions) -> Result<()> {
    if cfg!(windows) {
        let mut cmd = std::process::Command::new("msiexec");
        cmd.args([
            "/a",
            &path.display().to_string(),
            "/qb",
            &format!("TARGETDIR={}", dest.display()),
        ]);
        let (status, _stderr) =
            run_with_timeout(no_window(cmd), opts).map_err(|e| Error::Extraction(e.to_string()))?;
        if !status.success() {
            return Err(Error::Extraction(format!("msiexec exited with {status}")));
        }
        Ok(())
    } else {
        // 非 Windows（CI/开发环境）：用 msitools 的 msiextract 做管理解包
        extract_external(
            "msiextract",
            &[
                "--directory".into(),
                dest.display().to_string(),
                path.display().to_string(),
            ],
            opts,
        )
    }
}

fn which(tool: &str) -> Option<PathBuf> {
    let ext = if cfg!(windows) { ".exe" } else { "" };
    for dir in std::env::split_paths(&std::env::var_os("PATH")?) {
        let p = dir.join(format!("{tool}{ext}"));
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

// ---------- 解压结果验证 ----------

/// 严格验证：目录非空，且（含 PE/manifest）且没有把 PE 拆成节碎片。
pub fn validate(dir: &Path) -> bool {
    let mut has_pe = false;
    let mut has_manifest = false;
    let mut fragment_sections = 0u32;
    let top = match std::fs::read_dir(dir) {
        Ok(t) => t,
        Err(_) => return false,
    };
    if top.count() == 0 {
        return false;
    }

    for entry in crate::fsx::walk_files(dir, 4) {
        let name = entry
            .file_name()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        match name.as_str() {
            "appxmanifest.xml" | "package.xml" | "squirrel.exe" => has_manifest = true,
            _ => {}
        }
        if name.ends_with(".exe") || name.ends_with(".dll") || name.ends_with(".sys") {
            has_pe = true;
        }
        // 原版的问题：7z 硬解 PE 会吐出 .text/.rdata 碎片 —— 视为失败
        if [
            "text", "rdata", "data", "idata", "edata", "rsrc", "reloc", "bss",
        ]
        .iter()
        .any(|s| name.starts_with(&format!("{s}.")) || name == *s)
        {
            fragment_sections += 1;
        }
    }

    (has_pe || has_manifest) && fragment_sections <= 3
}

fn clear_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::create_dir_all(dir)?;
    Ok(())
}

/// 在 dest/$PLUGINSDIR 下找内层 7z（electron-builder 命名为 app-64.7z 等），取最大的一个
fn find_pluginsdir_7z(dest: &Path) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dest.join("$PLUGINSDIR")).ok()?;
    let mut best: Option<(u64, PathBuf)> = None;
    for e in rd.flatten() {
        let p = e.path();
        if p.extension()
            .map(|x| x.eq_ignore_ascii_case("7z"))
            .unwrap_or(false)
        {
            let sz = e.metadata().map(|m| m.len()).unwrap_or(0);
            if best.as_ref().map(|(s, _)| sz > *s).unwrap_or(true) {
                best = Some((sz, p));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// 除 $PLUGINSDIR 外是否存在可执行文件（有则说明外层就是程序本体，无需二次解包）
fn has_exe_outside_pluginsdir(dir: &Path) -> bool {
    max_exe_outside_pluginsdir(dir).is_some()
}

/// $PLUGINSDIR 之外最大的 exe 体积（不存在则 None）。
/// 复用统一遍历，跳过 $PLUGINSDIR（安装器辅助 exe 所在）与深解包临时目录。
fn max_exe_outside_pluginsdir(dir: &Path) -> Option<u64> {
    crate::fsx::walk_files_skipping(dir, 4, &["$PLUGINSDIR", ".__pxe_deep"])
        .into_iter()
        .filter(|p| {
            p.extension()
                .map(|x| x.eq_ignore_ascii_case("exe"))
                .unwrap_or(false)
        })
        .filter_map(|p| std::fs::metadata(&p).ok().map(|m| m.len()))
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_pe_fragments() {
        let tmp = std::env::temp_dir().join("pxe_test_frag");
        clear_dir(&tmp).unwrap();
        for s in [".text", ".rdata", ".data", ".idata", ".rsrc"] {
            std::fs::write(tmp.join(s.to_string()), b"").unwrap();
        }
        assert!(!validate(&tmp));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn validate_accepts_real_app() {
        let tmp = std::env::temp_dir().join("pxe_test_app");
        clear_dir(&tmp).unwrap();
        std::fs::write(tmp.join("app.exe"), b"app").unwrap();
        std::fs::write(tmp.join("data.bin"), b"x").unwrap();
        assert!(validate(&tmp));
        std::fs::remove_dir_all(&tmp).ok();
    }
}
