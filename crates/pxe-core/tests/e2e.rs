//! 端到端测试：从"独立 exe"到 zip 打包便携包的完整管线。

use pxe_core::{builder, config::Config, detect::InstallerKind};
use std::path::PathBuf;

fn tempdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pxe_e2e_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn build_standalone_zip_end_to_end() {
    let src = tempdir("src");
    let out = tempdir("out");

    // 伪应用：App 目录含一个主 exe
    std::fs::write(src.join("MyTool.exe"), b"fake pe").unwrap();
    std::fs::write(src.join("data.bin"), b"x").unwrap();

    let cfg = Config::default();
    let r = builder::build(
        &src,
        &out,
        "MyTool",
        true,
        "standalone",
        builder::PackKind::Zip,
        &cfg,
        None,
    )
    .expect("build should succeed");

    // 结构断言
    for p in [
        "App",
        "Data/AppData",
        "Data/Registry",
        "Documentation",
        "Plugins",
        "Redist/README.md",
        "portable.toml",
    ] {
        assert!(r.portable_dir.join(p).exists(), "missing {p}");
    }
    // 主程序识别：bat 启动器里指向 MyTool.exe（main_exe 仅在 stub 存在时为 Some）
    assert!(r
        .main_exe
        .as_ref()
        .is_none_or(|p| p.ends_with("MyTool.exe")));
    let bat = std::fs::read_to_string(r.portable_dir.join("MyTool.bat")).unwrap();
    assert!(bat.contains("MyTool.exe"));
    // 启动器
    assert!(r.portable_dir.join("MyTool.bat").exists());
    assert!(r.portable_dir.join("RUN.bat").exists());
    assert!(r.portable_dir.join("MyTool.ps1").exists());
    // bat 语法健康
    assert!(!bat.lines().any(|l| l.trim() == "return"));
    assert!(bat.contains("exit /b"));
    // zip 产物
    assert!(r
        .packed
        .as_ref()
        .is_some_and(|p| p.extension().is_some_and(|e| e == "zip")));

    // zip 内容校验
    let f = std::fs::File::open(r.packed.unwrap()).unwrap();
    let mut z = zip::ZipArchive::new(f).unwrap();
    let names: Vec<String> = (0..z.len())
        .filter_map(|i| {
            z.by_index_raw(i)
                .ok()
                .and_then(|e| e.name().ok().map(|n| n.into_owned()))
        })
        .collect();
    assert!(
        names.iter().any(|n| n == "App/MyTool.exe"),
        "zip missing App/MyTool.exe: {names:?}"
    );

    std::fs::remove_dir_all(&src).ok();
    std::fs::remove_dir_all(&out).ok();
    let _ = InstallerKind::Standalone;
}

#[test]
fn detect_unknown_file_is_not_installer() {
    let src = tempdir("plain");
    let f = src.join("plain.exe");
    std::fs::write(&f, b"not a pe at all").unwrap();
    let d = pxe_core::detect::detect(&f).unwrap();
    assert_eq!(d.kind, pxe_core::InstallerKind::Unknown);
    assert!(!d.is_installer());
    std::fs::remove_dir_all(&src).ok();
}
