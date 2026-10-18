//! pxe-stub —— 原生便携启动器（替代 .bat 的 Rust 小程序）。
//!
//! 放在便携包根目录，与 `launcher.toml` 一起工作：
//! 1. 设置便携环境变量（APPDATA/TEMP 等重定向到 Data/）
//! 2. 启动 App/<exe>（windows 子系统，无黑窗）
//! 3. 等待子进程退出并把其退出码作为本进程退出码转发
//! 4. 启动日志写入 Data/Logs/launch.log（UTC ISO8601 时间戳，超过 1MB 自动轮转）

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Deserialize;

/// launch.log 单文件上限，超过则轮转为 launch.log.old
const LOG_MAX_BYTES: u64 = 1024 * 1024;

#[derive(Deserialize)]
struct LauncherConfig {
    exe: String,
    #[serde(default)]
    args: Vec<String>,
}

fn main() {
    if let Err(e) = run() {
        // 无 UI 依赖：失败写入日志（若可能）
        eprintln!("pxe-stub: {e}");
        append_log(&format!("ERROR: {e}"));
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let root = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("no parent dir")?
        .to_path_buf();

    let cfg_path = root.join("launcher.toml");
    let cfg: LauncherConfig = toml::from_str(
        &std::fs::read_to_string(&cfg_path).map_err(|e| format!("read launcher.toml: {e}"))?,
    )
    .map_err(|e| format!("parse launcher.toml: {e}"))?;

    let data = root.join("Data");
    let (appdata, localappdata, profile, temp) = (
        data.join("AppData"),
        data.join("LocalAppData"),
        data.join("Profile"),
        data.join("Temp"),
    );
    for d in [&appdata, &localappdata, &profile, &temp] {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }

    // 环境重定向（仅影响本进程及其子进程）
    std::env::set_var("APPDATA", &appdata);
    std::env::set_var("LOCALAPPDATA", &localappdata);
    std::env::set_var("USERPROFILE", &profile);
    std::env::set_var("TEMP", &temp);
    std::env::set_var("TMP", &temp);
    std::env::set_var("PORTABLE", "1");

    let app_dir = root.join("App");
    let exe = app_dir.join(&cfg.exe);
    if !exe.is_file() {
        return Err(format!("exe not found: {}", exe.display()));
    }

    append_log(&format!("launching {}", exe.display()));

    // PATH 前置 App/Dependencies（join_paths 正确处理分隔符；原实现用
    // PathBuf::push 绝对路径会整体替换 PATH，前缀丢失）
    if let Some(old) = std::env::var_os("PATH") {
        let deps = app_dir.join("Dependencies");
        if let Ok(joined) =
            std::env::join_paths(std::iter::once(deps).chain(std::env::split_paths(&old)))
        {
            std::env::set_var("PATH", joined);
        }
    }

    let mut cmd = std::process::Command::new(&exe);
    cmd.args(&cfg.args).current_dir(&app_dir);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", exe.display()))?;

    append_log(&format!("spawned pid {}", child.id()));

    // 等待主程序退出并转发退出码：调用方（RUN.bat / 资源管理器）能拿到真实状态
    let status = child.wait().map_err(|e| format!("wait: {e}"))?;
    let code = status.code().unwrap_or(1);
    append_log(&format!("exited with code {code}"));
    std::process::exit(code);
}

/// UTC 秒 -> "YYYY-MM-DDTHH:MM:SSZ"（无外部依赖，Howard Hinnant civil_from_days 算法）
fn fmt_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    // days 自 1970-01-01；shift 到 civil 纪元再做 civil_from_days
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let mth = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if mth <= 2 { y + 1 } else { y };

    format!("{y:04}-{mth:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

fn append_log(msg: &str) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(root) = exe.parent() else { return };
    let log_dir = root.join("Data/Logs");
    if std::fs::create_dir_all(&log_dir).is_err() {
        return;
    }
    let log = log_dir.join("launch.log");

    // 轮转：超过上限 -> launch.log.old（先删旧轮转）
    if let Ok(meta) = std::fs::metadata(&log) {
        if meta.len() > LOG_MAX_BYTES {
            let old = log_dir.join("launch.log.old");
            let _ = std::fs::remove_file(&old);
            let _ = std::fs::rename(&log, &old);
        }
    }

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
    {
        use std::io::Write;
        let _ = writeln!(f, "[{}] {msg}", fmt_utc(ts));
    }
}

#[cfg(test)]
mod tests {
    use super::fmt_utc;

    #[test]
    fn epoch() {
        assert_eq!(fmt_utc(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn known_points() {
        // 2026-10-18 00:00:00 UTC
        assert_eq!(fmt_utc(1_792_281_600), "2026-10-18T00:00:00Z");
        // 闰年边界：2024-02-29 23:59:59 UTC = 1709251199
        assert_eq!(fmt_utc(1_709_251_199), "2024-02-29T23:59:59Z");
    }
}
