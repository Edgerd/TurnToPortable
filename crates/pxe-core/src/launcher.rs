//! 启动器生成：bat / ps1 模板（语法修复：`exit /b` 而非非法的 `return`）
//! 以及原生 stub 启动器（pxe-stub.exe）。

use std::path::{Path, PathBuf};

/// bat 模板 —— 所有子例程以 `exit /b` 结尾（原版用非法的 `return`，产物无法运行）。
/// 行尾统一 CRLF：cmd.exe 对纯 LF 的 bat 在标签/多行块上有已知解析问题。
pub fn main_bat(app_name: &str, exe_rel: &str) -> String {
    let lf = format!(
        r#"@echo off
setlocal EnableDelayedExpansion
title {app_name} - Portable Launcher

set "PORTABLE_DIR=%~dp0"
set "APP_DIR=%PORTABLE_DIR%App"
set "DATA_DIR=%PORTABLE_DIR%Data"

:: 便携环境隔离（仅环境变量层；注册表隔离见 portable.toml 说明）
set "APPDATA=%DATA_DIR%\AppData"
set "LOCALAPPDATA=%DATA_DIR%\LocalAppData"
set "USERPROFILE=%DATA_DIR%\Profile"
set "TEMP=%DATA_DIR%\Temp"
set "TMP=%DATA_DIR%\Temp"
set "PORTABLE=1"

for %%d in ("%APPDATA%" "%LOCALAPPDATA%" "%USERPROFILE%" "%TEMP%") do (
    if not exist "%%~d" mkdir "%%~d"
)

if exist "%APP_DIR%\Dependencies" set "PATH=%APP_DIR%\Dependencies;%PATH%"
set "PATH=%APP_DIR%;%PATH%"

pushd "%APP_DIR%"
start "" "%APP_DIR%\{exe_rel}" %*
popd
exit /b 0
"#
    );
    lf.replace('\n', "\r\n")
}

pub fn run_bat(app_name: &str) -> String {
    format!("@echo off\r\necho Starting {app_name}...\r\ncall \"%~dp0{app_name}.bat\"\r\n")
}

pub fn ps1(app_name: &str, exe_rel: &str) -> String {
    format!(
        r#"# PortableXE (Rust) launcher for {app_name}
$PortableDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$AppDir = Join-Path $PortableDir "App"
$DataDir = Join-Path $PortableDir "Data"

$env:APPDATA = Join-Path $DataDir "AppData"
$env:LOCALAPPDATA = Join-Path $DataDir "LocalAppData"
$env:USERPROFILE = Join-Path $DataDir "Profile"
$env:TEMP = Join-Path $DataDir "Temp"
$env:TMP = Join-Path $DataDir "Temp"
$env:PORTABLE = "1"

foreach ($d in @($env:APPDATA, $env:LOCALAPPDATA, $env:USERPROFILE, $env:TEMP)) {{
    if (!(Test-Path $d)) {{ New-Item -ItemType Directory -Path $d -Force | Out-Null }}
}}

Start-Process -FilePath (Join-Path $AppDir "{exe_rel}") -WorkingDirectory $AppDir
"#
    )
}

/// stub 启动器所需的配置文件（pxe-stub.exe 与之同目录时优先使用原生启动）
pub fn stub_toml(exe_rel: &str) -> String {
    format!(
        r#"# pxe-stub launcher config
exe = "{exe_rel}"
"#
    )
}

/// 优先选择主程序 exe：名字精确匹配 > 名字包含 app 名 > 排除卸载器/安装器后取最大的 exe
/// （原版"取最小的非 unins/setup exe"会命中崩溃上报器之类的辅助进程）。
pub fn find_main_exe(app_dir: &Path, app_name: &str) -> Option<PathBuf> {
    let helpers = [
        "unins",
        "setup",
        "install",
        "update",
        "crash",
        "error",
        "report",
        "vc_redist",
        "eula",
    ];
    let lower_name = app_name.to_lowercase();

    let mut best: Option<(i32, u64, PathBuf)> = None;
    for p in collect_exes(app_dir) {
        let file = match p.file_name().and_then(|s| s.to_str()) {
            Some(f) => f.to_lowercase(),
            None => continue,
        };
        if helpers.iter().any(|h| file.contains(h)) {
            continue;
        }
        let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
        let stem = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        let score = if stem == lower_name {
            2
        } else if stem.contains(&lower_name) {
            1
        } else {
            0
        };
        if best
            .as_ref()
            .is_none_or(|(s, sz, _)| (score, size) > (*s, *sz))
        {
            best = Some((score, size, p));
        }
    }
    best.map(|(_, _, p)| p)
}

fn collect_exes(dir: &Path) -> Vec<PathBuf> {
    crate::fsx::walk_files(dir, 3)
        .into_iter()
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")))
        .collect()
}

/// 模板测试：确保 bat 语法健康（无非法 `return`、以 exit /b 结尾）
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bat_has_valid_syntax() {
        let bat = main_bat("MyApp", "MyApp.exe");
        assert!(bat.contains("exit /b"));
        assert!(
            !bat.lines().any(|l| l.trim() == "return"),
            "bat must not use invalid `return`"
        );
    }
}
