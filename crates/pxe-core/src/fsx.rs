//! 文件系统工具 —— 全项目统一的目录遍历与拷贝/移动原语。
//! （此前 extract / launcher / builder 各自实现了遍历，行为不一且重复。）

use crate::Result;
use std::path::{Path, PathBuf};

/// 收集 dir 下的文件（不含目录），最大深度 max_depth（0 = 仅 dir 本层）。
pub fn walk_files(dir: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    rec(dir, 0, max_depth, &[], &mut out);
    out
}

/// 同 walk_files，但跳过指定名字的目录（任意层级，如 "$PLUGINSDIR"、临时目录）。
pub fn walk_files_skipping(dir: &Path, max_depth: usize, skip_dirs: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    rec(dir, 0, max_depth, skip_dirs, &mut out);
    out
}

fn rec(dir: &Path, depth: usize, max: usize, skip: &[&str], out: &mut Vec<PathBuf>) {
    if depth > max {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| skip.contains(&n))
                .unwrap_or(false)
            {
                continue;
            }
            rec(&p, depth + 1, max, skip, out);
        } else {
            out.push(p);
        }
    }
}

/// 递归移动 src 内容到 dst：优先 rename（同一文件系统零拷贝），
/// 跨设备失败时回退到拷贝+删除。用于"解压到临时目录 → 挪进 App/"，省一整次大拷贝。
pub fn move_dir_contents(src: &Path, dst: &Path) -> Result<()> {
    for e in std::fs::read_dir(src)?.flatten() {
        let from = e.path();
        let to = dst.join(e.file_name());
        if from.is_dir() {
            std::fs::create_dir_all(&to)?;
            move_dir_contents(&from, &to)?;
            let _ = std::fs::remove_dir(&from);
        } else {
            if std::fs::rename(&from, &to).is_err() {
                std::fs::copy(&from, &to)?;
                let _ = std::fs::remove_file(&from);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_respects_depth_and_skip() {
        let tmp = std::env::temp_dir().join(format!("pxe_fsx_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("a/$PLUGINSDIR")).unwrap();
        std::fs::create_dir_all(tmp.join("a/b/c")).unwrap();
        std::fs::write(tmp.join("top.txt"), "").unwrap();
        std::fs::write(tmp.join("a/$PLUGINSDIR/inner.dll"), "").unwrap();
        std::fs::write(tmp.join("a/b/deep.txt"), "").unwrap();
        std::fs::write(tmp.join("a/b/c/deeper.txt"), "").unwrap();

        let files = walk_files_skipping(&tmp, 4, &["$PLUGINSDIR"]);
        let names: Vec<String> = files.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert!(names.contains(&"top.txt".to_string()));
        assert!(names.contains(&"deep.txt".to_string()));
        assert!(!names.iter().any(|n| n == "inner.dll"));

        let shallow = walk_files(&tmp, 0);
        assert_eq!(shallow.len(), 1);
        std::fs::remove_dir_all(&tmp).ok();
    }
}
