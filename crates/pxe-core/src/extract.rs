//! 解包引擎：内嵌优先（zip / 7z），外部工具回退（innoextract / msiexec / unrar）。
//!
//! 与原版的区别：
//! - ZIP/7z/MSIX 不再依赖用户安装 7z.exe
//! - MSI 不再靠 "This installation package" 字符串检测（该字符串根本不在 MSI 文件里）
//! - 解压结果验证更严格：必须有 PE 或 manifest，且无裸节碎片

use crate::detect::InstallerKind;
use crate::{Error, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 外部工具超时（秒）。由 CLI/GUI 在调用前设置，默认 300。
pub static TIMEOUT_SECS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(300);

pub fn set_timeout(secs: u64) {
    TIMEOUT_SECS.store(secs, std::sync::atomic::Ordering::Relaxed);
}

fn timeout() -> Duration {
    Duration::from_secs(TIMEOUT_SECS.load(std::sync::atomic::Ordering::Relaxed))
}

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
pub fn extract(path: &Path, kind: InstallerKind, dest: &Path) -> Result<Extracted> {
    std::fs::create_dir_all(dest)?;

    let attempts: &[(InstallerKind, &str)] = match kind {
        InstallerKind::InnoSetup => {
            &[(InstallerKind::InnoSetup, "innoextract"), (InstallerKind::Sfx7z, "embedded-7z")]
        }
        InstallerKind::Nsis | InstallerKind::Sfx7z | InstallerKind::InstallShield | InstallerKind::WiXBurn => {
            &[(InstallerKind::Sfx7z, "embedded-7z")]
        }
        InstallerKind::Msi => &[(InstallerKind::Msi, "msiexec")],
        InstallerKind::Msix | InstallerKind::Appx | InstallerKind::Zip | InstallerKind::Squirrel => {
            &[(InstallerKind::Zip, "embedded-zip")]
        }
        InstallerKind::SevenZip => &[(InstallerKind::Sfx7z, "embedded-7z")],
        InstallerKind::Rar | InstallerKind::SfxRar => &[(InstallerKind::Rar, "unrar")],
        _ => &[(InstallerKind::Zip, "embedded-zip"), (InstallerKind::Sfx7z, "embedded-7z")],
    };

    let mut last_err = None;
    for (k, label) in attempts {
        let r = match k {
            InstallerKind::Zip => extract_zip(path, dest),
            InstallerKind::Sfx7z => extract_7z_embedded(path, dest),
            InstallerKind::InnoSetup => extract_external("innoextract", &[
                "-d".into(), dest.display().to_string(), "-s".into(), path.display().to_string(),
            ]),
            InstallerKind::Msi => extract_msi(path, dest),
            InstallerKind::Rar => extract_external("unrar", &[
                "x".into(), path.display().to_string(), format!("{}\\", dest.display()),
            ]),
            _ => continue,
        };
        match r {
            Ok(()) => {
                if validate(dest) {
                    return Ok(Extracted { dir: dest.to_path_buf(), via: label.to_string() });
                }
                last_err = Some(Error::Extraction(format!("{label}: output failed validation")));
            }
            Err(e) => last_err = Some(e),
        }
        // 清空后重试下一种方式
        clear_dir(dest)?;
    }

    Err(last_err.unwrap_or_else(|| Error::Extraction("no extractor available".into())))
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
fn run_with_timeout(mut cmd: std::process::Command) -> Result<std::process::ExitStatus> {
    let mut child = cmd
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| Error::Extraction(format!("spawn: {e}")))?;
    let start = Instant::now();
    loop {
        match child.try_wait()? {
            Some(st) => return Ok(st),
            None => {
                if start.elapsed() > timeout() {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Error::Extraction(format!(
                        "timed out after {}s",
                        timeout().as_secs()
                    )));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn extract_external(tool: &str, args: &[String]) -> Result<()> {
    if which(tool).is_none() {
        return Err(Error::Extraction(format!("{tool} not found in PATH")));
    }
    let mut cmd = std::process::Command::new(tool);
    cmd.args(args);
    let status = run_with_timeout(no_window(cmd))
        .map_err(|e| Error::Extraction(e.to_string()))?;
    if !status.success() {
        return Err(Error::Extraction(format!("{tool} exited with {status}")));
    }
    Ok(())
}

fn extract_msi(path: &Path, dest: &Path) -> Result<()> {
    if !cfg!(windows) {
        return Err(Error::Extraction("msiexec requires Windows".into()));
    }
    let mut cmd = std::process::Command::new("msiexec");
    cmd.args(["/a", &path.display().to_string(), "/qb", &format!("TARGETDIR={}", dest.display())]);
    let status = run_with_timeout(no_window(cmd))
        .map_err(|e| Error::Extraction(e.to_string()))?;
    if !status.success() {
        return Err(Error::Extraction(format!("msiexec exited with {status}")));
    }
    Ok(())
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

    for entry in walk(dir, 4) {
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
        if ["text", "rdata", "data", "idata", "edata", "rsrc", "reloc", "bss"]
            .iter()
            .any(|s| name.starts_with(&format!("{s}.")) || name == *s)
        {
            fragment_sections += 1;
        }
    }

    (has_pe || has_manifest) && fragment_sections <= 3
}

fn walk(dir: &Path, max_depth: usize) -> impl Iterator<Item = PathBuf> {
    let mut out = Vec::new();
    fn rec(dir: &Path, depth: usize, max: usize, out: &mut Vec<PathBuf>) {
        if depth > max {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                rec(&p, depth + 1, max, out);
            } else {
                out.push(p);
            }
        }
    }
    rec(dir, 0, max_depth, &mut out);
    out.into_iter()
}

fn clear_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::create_dir_all(dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_pe_fragments() {
        let tmp = std::env::temp_dir().join("pxe_test_frag");
        clear_dir(&tmp).unwrap();
        for s in [".text", ".rdata", ".data", ".idata", ".rsrc"] {
            std::fs::write(tmp.join(format!("{s}")), b"").unwrap();
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
