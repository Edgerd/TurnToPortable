//! 统一后端管线：CLI 与 GUI 共用 detect → extract → build 全流程。
//!
//! 前端（CLI / GUI / 未来 Flutter）只需提供进度回调，不再各自维护一套流程逻辑，
//! 保证两条入口行为一致（深度解包、超时、错误语义都由这里决定）。

use crate::{builder, config::Config, detect, extract, Error, Result};
use std::path::Path;

/// 进度回调：(0.0-1.0, 消息)
pub type Progress<'a> = dyn FnMut(f32, &str) + 'a;

/// 运行完整管线：识别安装包类型 → 解压（含深度解包）→ 生成便携应用。
pub fn run(
    file: &Path,
    name: &str,
    out_dir: &Path,
    pack_kind: builder::PackKind,
    redist: Option<&Path>,
    cfg: &Config,
    progress: &mut Progress,
) -> Result<builder::BuildOutcome> {
    let opts = extract::ExtractOptions {
        timeout: std::time::Duration::from_secs(cfg.timeout_seconds),
        deep_extract: cfg.deep_extract,
    };

    progress(0.05, "正在识别…");
    let d = detect::detect(file)?;
    progress(0.15, &format!("正在解压（{}）…", d.kind.label()));

    let (source, is_standalone, via);
    let tmp = std::env::temp_dir().join(format!("pxe_extract_{}", std::process::id()));
    let using_tmp = d.is_installer() || d.kind.is_archive();
    if using_tmp {
        match extract::extract(file, d.kind, &tmp, &opts) {
            Ok(r) => {
                via = r.via;
                source = r.dir;
                is_standalone = false;
            }
            // 真安装包解不开就必须失败，绝不能把安装包本体当"便携应用"
            Err(e) if d.kind.is_installer() => {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(Error::Extraction(format!(
                    "解包失败（{} 类型）：{e}。这个安装包可能已损坏或使用了未支持的加密方式。",
                    d.kind.label()
                )));
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

    progress(0.55, "正在生成便携应用…");
    let r = builder::build(
        &source,
        out_dir,
        name,
        is_standalone,
        &via,
        pack_kind,
        cfg,
        redist,
    );
    if using_tmp {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    let outcome = r?;
    progress(1.0, "完成");
    Ok(outcome)
}

/// 从文件名推导应用名：去掉 setup/install 字样（"Qoder-Setup" → "Qoder"）。
/// CLI 与 GUI 共用，避免两端推导结果不一致。
pub fn derive_name(file: &Path) -> String {
    let stem = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("App")
        .to_string();
    let stem = remove_tokens_ci(&stem, ["installer", "install", "setup"]);
    let stem = stem.trim_matches(|c: char| c == '-' || c == '_' || c == ' ');
    if stem.is_empty() {
        "App".into()
    } else {
        capitalize(stem)
    }
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
    // 按起点升序、终点降序排序：同起点时优先匹配更长的词（installer 先于 install）
    ranges.sort_by(|x, y| x.0.cmp(&y.0).then(y.1.cmp(&x.1)));
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for (a, b) in ranges {
        if a < last {
            continue; // 与前一个区间重叠，跳过
        }
        out.push_str(&s[last..a]);
        last = b;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_name_strips_installer_words() {
        assert_eq!(derive_name(Path::new("Qoder-Setup.exe")), "Qoder");
        assert_eq!(derive_name(Path::new("app-installer.exe")), "App");
        assert_eq!(derive_name(Path::new("MyTool.exe")), "MyTool");
        assert_eq!(derive_name(Path::new("setup.exe")), "App");
    }
}
