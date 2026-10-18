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
        /// Emit machine-readable NDJSON progress events (for GUI frontends)
        #[arg(long)]
        json: bool,
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
            let opts = pxe_core::extract::ExtractOptions {
                deep_extract: !no_deep_extract,
                ..Default::default()
            };
            cmd_extract(&file, &out, &opts)?;
        }
        Cmd::Build {
            file,
            name,
            out,
            pack,
            redist,
            config,
            no_deep_extract,
            json,
        } => {
            cmd_build(
                &file,
                name,
                out,
                pack,
                redist,
                &config,
                no_deep_extract,
                json,
            )?;
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

fn cmd_extract(
    file: &Path,
    out: &Path,
    opts: &pxe_core::extract::ExtractOptions,
) -> anyhow::Result<()> {
    let d = detect::detect(file)?;
    println!("[pxe] type: {}", d.kind.label());
    let r = pxe_core::extract::extract(file, d.kind, out, opts)?;
    println!("[pxe] extracted via '{}' -> {}", r.via, r.dir.display());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_build(
    file: &Path,
    name: Option<String>,
    out: Option<PathBuf>,
    pack_kind: builder::PackKind,
    redist: Option<PathBuf>,
    config: &Path,
    no_deep_extract: bool,
    json: bool,
) -> anyhow::Result<()> {
    let mut cfg = Config::load(config)?;
    // CLI 开关优先于配置文件
    if no_deep_extract {
        cfg.deep_extract = false;
    }
    let out_dir = out.unwrap_or_else(|| cfg.output_dir.clone());
    let app_name = name.unwrap_or_else(|| pipeline::derive_name(file));

    if json {
        // NDJSON 事件协议（GUI 前端解析）：{"progress":0.15,"msg":"..."}
        let mut progress = |p: f32, msg: &str| {
            let event = serde_json::json!({
                "progress": (p * 100.0).round() / 100.0,
                "msg": msg,
            });
            println!("{event}");
        };
        run_pipeline(
            file,
            &app_name,
            &out_dir,
            pack_kind,
            redist.as_deref(),
            &cfg,
            &mut progress,
        )
    } else {
        // 人类可读日志
        let mut progress = |p: f32, msg: &str| {
            let _ = p;
            println!("[pxe] {msg}");
        };
        run_pipeline(
            file,
            &app_name,
            &out_dir,
            pack_kind,
            redist.as_deref(),
            &cfg,
            &mut progress,
        )
    }
}

fn run_pipeline(
    file: &Path,
    app_name: &str,
    out_dir: &Path,
    pack_kind: builder::PackKind,
    redist: Option<&Path>,
    cfg: &Config,
    progress: &mut dyn FnMut(f32, &str),
) -> anyhow::Result<()> {
    let r = pipeline::run(file, app_name, out_dir, pack_kind, redist, cfg, progress)
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
