//! pxe-stub —— 原生便携启动器（替代 .bat 的 Rust 小程序）。
//!
//! 放在便携包根目录，与 `launcher.toml` 一起工作：
//! 1. 设置便携环境变量（APPDATA/TEMP 等重定向到 Data/）
//! 2. 启动 App/<exe>（windows 子系统，无黑窗）
//! 3. 启动日志写入 Data/Logs/launch.log

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Deserialize;
use std::path::PathBuf;

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
        if let Ok(joined) = std::env::join_paths(
            std::iter::once(deps).chain(std::env::split_paths(&old)),
        ) {
            std::env::set_var("PATH", joined);
        }
    }

    let mut cmd = std::process::Command::new(&exe);
    cmd.args(&cfg.args).current_dir(&app_dir);
    let child = cmd
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", exe.display()))?;

    append_log(&format!("spawned pid {}", child.id()));
    Ok(())
}

fn append_log(msg: &str) {
    let Ok(exe) = std::env::current_exe() else { return };
    let Some(root) = exe.parent() else { return };
    let log_dir = root.join("Data/Logs");
    if std::fs::create_dir_all(&log_dir).is_err() {
        return;
    }
    let log = log_dir.join("launch.log");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(PathBuf::from(&log)) {
        use std::io::Write;
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}
