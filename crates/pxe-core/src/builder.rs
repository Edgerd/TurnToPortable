//! 便携包构建：标准目录结构 + 启动器生成 + 多种打包形态。

use crate::config::Config;
use crate::launcher;
use crate::{Error, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackKind {
    /// 标准目录（App/Data/...）
    #[default]
    Dir,
    /// 便携目录打成单个 zip
    Zip,
    /// 高压缩 7z 单文件
    SevenZip,
}

#[derive(Debug, Clone)]
pub struct BuildOutcome {
    pub portable_dir: PathBuf,
    pub main_exe: Option<PathBuf>,
    pub packed: Option<PathBuf>,
}

/// 创建便携目录结构并返回根目录。
pub fn create_structure(output_dir: &Path, app_name: &str) -> Result<PathBuf> {
    let root = output_dir.join(format!("{app_name}_Portable"));
    for d in [
        "",
        "App",
        "Data",
        "Data/AppData",
        "Data/LocalAppData",
        "Data/Profile",
        "Data/Registry",
        "Data/Temp",
        "Data/Logs",
        "Data/Settings",
        "Documentation",
        "Plugins",
        "Redist",
    ] {
        std::fs::create_dir_all(root.join(d))?;
    }
    Ok(root)
}

/// 把解压内容（或单 exe）拷入 App/。
/// 管线场景（source 是 pxe 的临时解压目录）直接移动而非复制——
/// 对 GB 级应用省一整次全量 IO；用户自己的目录绝不移动，仍然复制。
pub fn fill_app_dir(source: &Path, root: &Path, _is_standalone: bool) -> Result<()> {
    let app = root.join("App");
    if !source.is_dir() {
        let dest = app.join(
            source
                .file_name()
                .ok_or_else(|| Error::InvalidPackage("no file name".into()))?,
        );
        std::fs::copy(source, &dest)?;
        return Ok(());
    }
    if is_pipeline_tmp(source) {
        crate::fsx::move_dir_contents(source, &app)?;
    } else {
        copy_dir_contents(source, &app)?;
    }
    Ok(())
}

/// source 是否为管线自己创建的临时解压目录（仅这种目录允许移动）
fn is_pipeline_tmp(source: &Path) -> bool {
    source.starts_with(std::env::temp_dir())
        && source
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("pxe_extract_"))
            .unwrap_or(false)
}

/// 生成启动器。优先使用原生 stub（无黑窗、单文件），回退到 bat/ps1。
pub fn write_launchers(root: &Path, app_name: &str, cfg: &Config) -> Result<Option<PathBuf>> {
    let app_dir = root.join("App");
    let main = launcher::find_main_exe(&app_dir, app_name)
        .ok_or_else(|| Error::InvalidPackage("no usable .exe found in App/".into()))?;
    let exe_rel = main
        .strip_prefix(&app_dir)
        .unwrap_or(&main)
        .to_string_lossy()
        .replace('/', "\\");

    // 原生 stub：查找与 pxe 可执行文件同目录的 pxe-stub(.exe)
    if cfg.prefer_native_stub {
        if let Some(stub) = find_stub() {
            let dest = root.join(format!("{app_name}.exe"));
            std::fs::copy(&stub, &dest)?;
            std::fs::write(root.join("launcher.toml"), launcher::stub_toml(&exe_rel))?;
            // 仍然生成 bat 作为回退
            write_script_fallbacks(root, app_name, &exe_rel);
            return Ok(Some(dest));
        }
    }

    write_script_fallbacks(root, app_name, &exe_rel);
    Ok(None)
}

fn write_script_fallbacks(root: &Path, app_name: &str, exe_rel: &str) {
    let _ = std::fs::write(
        root.join(format!("{app_name}.bat")),
        launcher::main_bat(app_name, exe_rel),
    );
    let _ = std::fs::write(root.join("RUN.bat"), launcher::run_bat(app_name));
    let _ = std::fs::write(
        root.join(format!("{app_name}.ps1")),
        launcher::ps1(app_name, exe_rel),
    );
}

fn find_stub() -> Option<PathBuf> {
    // 1) 显式环境变量
    if let Ok(p) = std::env::var("PXE_STUB_PATH") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let name = if cfg!(windows) {
        "pxe-stub.exe"
    } else {
        "pxe-stub"
    };
    // 2) 与 pxe 可执行文件同目录（发布形态）
    if let Ok(exe) = std::env::current_exe() {
        let cand = exe.parent()?.join(name);
        if cand.is_file() {
            return Some(cand);
        }
        // 3) cargo target 目录（开发形态）
        if let Some(t) = exe.ancestors().nth(2) {
            let cand = t.join(name);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

/// 把官方 vc_redist 安装器放进 Redist/（用户提供路径时），否则只写 README 说明。
pub fn write_redist(
    root: &Path,
    app_name: &str,
    _cfg: &Config,
    redist: Option<&Path>,
) -> Result<()> {
    let dir = root.join("Redist");
    std::fs::write(
        dir.join("README.md"),
        format!(
            "# Runtime Dependencies\n\n\
            If `{app_name}` fails to start with a missing `VCRUNTIME140.dll` / `MSVCP140.dll` error,\n\
            run the bundled `vc_redist.x64.exe` below. Do **not** copy DLLs out of `C:\\Windows\\System32`\n\
            (version mismatch and DLL hijacking risks; redistribution terms apply).\n"
        ),
    )?;
    if let Some(src) = redist {
        let file_name = src
            .file_name()
            .ok_or_else(|| Error::InvalidPackage("bad redist path".into()))?;
        std::fs::copy(src, dir.join(file_name))?;
    }
    Ok(())
}

pub fn write_metadata(root: &Path, app_name: &str, via: &str) -> Result<()> {
    let meta = format!(
        "# portable metadata\napp = \"{app_name}\"\ncreated_by = \"PortableXE-RS v{}\"\nextracted_via = \"{via}\"\n",
        crate::VERSION
    );
    std::fs::write(root.join("portable.toml"), meta)?;
    Ok(())
}

/// 打包为分发形态。
pub fn pack(root: &Path, kind: PackKind, level: u8) -> Result<Option<PathBuf>> {
    match kind {
        PackKind::Dir => Ok(None),
        PackKind::Zip => {
            let dest = PathBuf::from(format!("{}.zip", root.display()));
            let file = std::fs::File::create(&dest)?;
            let mut z = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .compression_level(Some(level.min(9) as i64));
            add_dir_to_zip(root, root, &mut z, opts)?;
            z.finish()?;
            Ok(Some(dest))
        }
        PackKind::SevenZip => {
            let dest = PathBuf::from(format!("{}.7z", root.display()));
            sevenz_rust::compress_to_path(root, &dest).map_err(|e| Error::SevenZ(e.to_string()))?;
            Ok(Some(dest))
        }
    }
}

fn add_dir_to_zip(
    dir: &Path,
    base: &Path,
    z: &mut zip::ZipWriter<std::fs::File>,
    opts: zip::write::SimpleFileOptions,
) -> Result<()> {
    for e in std::fs::read_dir(dir)?.flatten() {
        let p = e.path();
        let rel = p
            .strip_prefix(base)
            .unwrap_or(&p)
            .to_string_lossy()
            .replace('\\', "/");
        if p.is_dir() {
            z.add_directory(&rel, opts)?;
            add_dir_to_zip(&p, base, z, opts)?;
        } else {
            z.start_file(&rel, opts)?;
            std::io::copy(&mut std::fs::File::open(&p)?, z)?;
        }
    }
    Ok(())
}

pub(crate) fn copy_dir_contents(src: &Path, dst: &Path) -> Result<()> {
    for e in std::fs::read_dir(src)?.flatten() {
        let p = e.path();
        let d = dst.join(e.file_name());
        if p.is_dir() {
            std::fs::create_dir_all(&d)?;
            copy_dir_contents(&p, &d)?;
        } else {
            std::fs::copy(&p, &d)?;
        }
    }
    Ok(())
}

/// 一站式构建入口。
#[allow(clippy::too_many_arguments)]
pub fn build(
    source: &Path,
    output_dir: &Path,
    app_name: &str,
    is_standalone: bool,
    extracted_via: &str,
    pack_kind: PackKind,
    cfg: &Config,
    redist: Option<&Path>,
) -> Result<BuildOutcome> {
    let root = create_structure(output_dir, app_name)?;
    fill_app_dir(source, &root, is_standalone)?;
    let main_exe = write_launchers(&root, app_name, cfg)?;
    if cfg.bundle_redist_note {
        write_redist(&root, app_name, cfg, redist)?;
    }
    write_metadata(&root, app_name, extracted_via)?;
    let packed = pack(&root, pack_kind, cfg.compression_level)?;
    Ok(BuildOutcome {
        portable_dir: root,
        main_exe,
        packed,
    })
}
