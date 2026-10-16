//! pxe-cli —— 一等公民命令行（原版 README 声称支持 CLI 但根本没有实现参数解析）。

use anyhow::Context;
use clap::{Parser, Subcommand};
use pxe_core::{builder, config::Config, detect, extract, pe};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "pxe",
    version = pxe_core::VERSION,
    about = "PortableXE (Rust) — turn Windows installers into portable apps"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Analyze a file: type, PE structure, SHA256
    Detect { file: PathBuf },
    /// Extract an installer to a directory
    Extract {
        file: PathBuf,
        #[arg(short, long, default_value = "./extracted")]
        out: PathBuf,
    },
    /// Full pipeline: detect -> extract -> build portable app
    Build {
        file: PathBuf,
        #[arg(short = 'n', long)]
        name: Option<String>,
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Distribution format
        #[arg(long, value_enum, default_value = "dir")]
        pack: builder::PackKind,
        /// Optional official vc_redist installer to bundle into Redist/
        #[arg(long)]
        redist: Option<PathBuf>,
        #[arg(long, default_value = "portablexe.toml")]
        config: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
    ).init();

    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Detect { file } => cmd_detect(&file)?,
        Cmd::Extract { file, out } => cmd_extract(&file, &out)?,
        Cmd::Build { file, name, out, pack, redist, config } => cmd_build(&file, name, out, pack, redist, &config)?,
    }
    Ok(())
}

fn cmd_detect(file: &PathBuf) -> anyhow::Result<()> {
    let d = detect::detect(file).context("detect failed")?;
    println!("File:        {}", file.display());
    println!("Type:        {}", d.kind.label());
    println!("Installer:   {}", if d.is_installer() { "yes" } else { "no" });
    println!("SHA256:      {}", d.sha256);
    if let Some(note) = &d.note {
        println!("Note:        {note}");
    }
    if let Some(p) = &d.pe {
        println!("{p}");
    }
    Ok(())
}

fn cmd_extract(file: &PathBuf, out: &PathBuf) -> anyhow::Result<()> {
    let d = detect::detect(file)?;
    println!("[pxe] type: {}", d.kind.label());
    let r = extract::extract(file, d.kind, out)?;
    println!("[pxe] extracted via '{}' -> {}", r.via, r.dir.display());
    Ok(())
}

fn cmd_build(
    file: &PathBuf,
    name: Option<String>,
    out: Option<PathBuf>,
    pack_kind: builder::PackKind,
    redist: Option<PathBuf>,
    config: &PathBuf,
) -> anyhow::Result<()> {
    // 结构化进度行：[pxe|progress|0.15|消息]，GUI（Flutter）按此协议解析
    let prog = |p: f32, msg: &str| println!("[pxe|progress|{p:.2}|{msg}]");

    let cfg = Config::load(config)?;
    extract::set_timeout(cfg.timeout_seconds);
    let out_dir = out.or_else(|| Some(cfg.output_dir.clone())).unwrap();
    let app_name = name.unwrap_or_else(|| derive_name(file));

    prog(0.05, "正在识别…");
    let d = detect::detect(file)?;
    println!("[pxe] detected: {}", d.kind.label());
    prog(0.15, "正在解压…");

    let (source, is_standalone, via);
    if d.is_installer() || d.kind.is_archive() {
        let tmp = std::env::temp_dir().join(format!("pxe_extract_{}", std::process::id()));
        match extract::extract(file, d.kind, &tmp) {
            Ok(r) => {
                via = r.via.clone();
                source = r.dir;
                is_standalone = false;
            }
            Err(e) if d.kind.is_installer() => {
                // 真安装包解不开就必须失败，绝不能把安装包本体当"便携应用"
                anyhow::bail!("解包失败（{} 类型）：{e}。这个安装包可能已损坏或使用了未支持的加密方式。", d.kind.label());
            }
            Err(e) => {
                tracing::warn!("extraction failed ({e}); treating as standalone");
                via = "none".into();
                source = file.clone();
                is_standalone = true;
            }
        }
    } else {
        via = "standalone".into();
        source = file.clone();
        is_standalone = true;
    }

    prog(0.55, "正在生成便携应用…");
    let r = builder::build(&source, &out_dir, &app_name, is_standalone, &via, pack_kind, &cfg, redist.as_deref())?;
    println!("[pxe] portable app: {}", r.portable_dir.display());
    prog(0.85, "正在打包…");
    if let Some(exe) = &r.main_exe {
        println!("[pxe] native stub launcher: {}", exe.display());
    }
    if let Some(p) = &r.packed {
        println!("[pxe] packed: {}", p.display());
    }
    prog(1.0, "完成");
    Ok(())
}

fn derive_name(file: &PathBuf) -> String {
    let mut stem = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("App")
        .to_string();
    for t in ["setup", "install", "installer"] {
        stem = stem.to_lowercase().replace(t, "");
    }
    let stem = stem.trim_matches(|c: char| c == '-' || c == '_' || c == ' ');
    if stem.is_empty() { "App".into() } else { capitalize(stem) }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
