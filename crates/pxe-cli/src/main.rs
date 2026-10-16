//! pxe-cli —— 一等公民命令行（原版 README 声称支持 CLI 但根本没有实现参数解析）。

use anyhow::Context;
use clap::{Parser, Subcommand};
use pxe_core::{builder, config::Config, detect, extract};
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

fn cmd_detect(file: &Path) -> anyhow::Result<()> {
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

fn cmd_extract(file: &Path, out: &Path) -> anyhow::Result<()> {
    let d = detect::detect(file)?;
    println!("[pxe] type: {}", d.kind.label());
    let r = extract::extract(file, d.kind, out)?;
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
    let tmp = std::env::temp_dir().join(format!("pxe_extract_{}", std::process::id()));
    let using_tmp = d.is_installer() || d.kind.is_archive();
    if using_tmp {
        match extract::extract(file, d.kind, &tmp) {
            Ok(r) => {
                via = r.via.clone();
                source = r.dir;
                is_standalone = false;
            }
            Err(e) if d.kind.is_installer() => {
                // 真安装包解不开就必须失败，绝不能把安装包本体当"便携应用"
                let _ = std::fs::remove_dir_all(&tmp);
                anyhow::bail!("解包失败（{} 类型）：{e}。这个安装包可能已损坏或使用了未支持的加密方式。", d.kind.label());
            }
            Err(e) => {
                tracing::warn!("extraction failed ({e}); treating as standalone");
                let _ = std::fs::remove_dir_all(&tmp);
                via = "none".into();
                source = file.to_path_buf();
                is_standalone = true;
            }
        }
    } else {
        via = "standalone".into();
        source = file.to_path_buf();
        is_standalone = true;
    }

    prog(0.55, "正在生成便携应用…");
    let r = builder::build(&source, &out_dir, &app_name, is_standalone, &via, pack_kind, &cfg, redist.as_deref());
    if using_tmp {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    let r = r?;
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

/// 从文件名推导应用名：去掉 setup/install 字样，保留原始大小写（"Qoder-Setup" → "Qoder"）。
fn derive_name(file: &Path) -> String {
    let stem = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("App")
        .to_string();
    let stem = remove_tokens_ci(&stem, ["setup", "install", "installer"]);
    let stem = stem.trim_matches(|c: char| c == '-' || c == '_' || c == ' ');
    if stem.is_empty() { "App".into() } else { capitalize(stem) }
}

/// 大小写不敏感地移除词法片段，保留其余部分原始大小写
fn remove_tokens_ci(s: &str, tokens: impl IntoIterator<Item = &'static str>) -> String {
    let lower = s.to_lowercase();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for t in tokens {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(t) {
            let start = from + pos;
            ranges.push((start, start + t.len()));
            from = start + t.len();
        }
    }
    ranges.sort();
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for (a, b) in ranges {
        if a >= last {
            out.push_str(&s[last..a]);
            last = b;
        }
    }
    out.push_str(&s[last..]);
    out
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
