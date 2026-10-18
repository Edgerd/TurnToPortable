//! pxe-cli —— 一等公民命令行。流程逻辑全部在 pxe-core::pipeline，这里只做参数解析与输出。

use anyhow::Context;
use clap::{Parser, Subcommand};
use pxe_core::{builder, config::Config, detect, pipeline};
use std::path::{Path, PathBuf};

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
        /// Disable deep unpacking (extract inner largest archive until main program)
        #[arg(long)]
        no_deep_extract: bool,
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
        /// Disable deep unpacking (overrides config)
        #[arg(long)]
        no_deep_extract: bool,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Detect { file } => cmd_detect(&file)?,
        Cmd::Extract {
            file,
            out,
            no_deep_extract,
        } => {
            pxe_core::extract::set_deep_extract(!no_deep_extract);
            cmd_extract(&file, &out)?;
        }
        Cmd::Build {
            file,
            name,
            out,
            pack,
            redist,
            config,
            no_deep_extract,
        } => {
            cmd_build(&file, name, out, pack, redist, &config, no_deep_extract)?;
        }
    }
    Ok(())
}

fn cmd_detect(file: &Path) -> anyhow::Result<()> {
    let d = detect::detect(file).context("detect failed")?;
    println!("File:        {}", file.display());
    println!("Type:        {}", d.kind.label());
    println!(
        "Installer:   {}",
        if d.is_installer() { "yes" } else { "no" }
    );
    println!("SHA256:      {}", d.sha256);
    if let Some(note) = &d.note {
        println!("Note:        {note}");
    }
    if let Some(p) = &d.pe {
        println!("{p}");
    }
    Ok(())
}

fn cmd_extract(file: &Path, out: &Path) -> anyhow::Result<()> {
    let d = detect::detect(file)?;
    println!("[pxe] type: {}", d.kind.label());
    let r = pxe_core::extract::extract(file, d.kind, out)?;
    println!("[pxe] extracted via '{}' -> {}", r.via, r.dir.display());
    Ok(())
}

fn cmd_build(
    file: &Path,
    name: Option<String>,
    out: Option<PathBuf>,
    pack_kind: builder::PackKind,
    redist: Option<PathBuf>,
    config: &Path,
    no_deep_extract: bool,
) -> anyhow::Result<()> {
    let mut cfg = Config::load(config)?;
    // CLI 开关优先于配置文件
    if no_deep_extract {
        cfg.deep_extract = false;
    }
    let out_dir = out.unwrap_or_else(|| cfg.output_dir.clone());
    let app_name = name.unwrap_or_else(|| pipeline::derive_name(file));

    // 结构化进度行：[pxe|progress|0.15|消息]，GUI（Flutter）按此协议解析
    let mut progress = |p: f32, msg: &str| {
        println!("[pxe] {msg}");
        println!("[pxe|progress|{p:.2}|{msg}]");
    };

    let r = pipeline::run(
        file,
        &app_name,
        &out_dir,
        pack_kind,
        redist.as_deref(),
        &cfg,
        &mut progress,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("[pxe] portable app: {}", r.portable_dir.display());
    if let Some(exe) = &r.main_exe {
        println!("[pxe] native stub launcher: {}", exe.display());
    }
    if let Some(p) = &r.packed {
        println!("[pxe] packed: {}", p.display());
    }
    Ok(())
}
