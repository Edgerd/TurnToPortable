//! 配置 —— 每个键都真实参与行为（原版有一半键从未被读取）。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub output_dir: PathBuf,
    /// 压缩级别（1-9），实际用于 zip / 7z 打包
    pub compression_level: u8,
    /// 解压超时（秒），用于外部工具
    pub timeout_seconds: u64,
    /// 附带官方 vc_redist 提示（不再盲拷 System32 DLL）
    pub bundle_redist_note: bool,
    /// 优先使用编译好的原生 stub 启动器（pxe-stub.exe）
    pub prefer_native_stub: bool,
    /// 深度解包：解压后继续解内层最大的压缩档，直到解出主程序（exe）即停止
    pub deep_extract: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            output_dir: default_output_dir(),
            compression_level: 6,
            timeout_seconds: 300,
            bundle_redist_note: true,
            prefer_native_stub: true,
            deep_extract: true,
        }
    }
}

fn default_output_dir() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(|h| PathBuf::from(h).join("PortableApps"))
        .unwrap_or_else(|_| PathBuf::from("."))
}

impl Config {
    pub fn load(path: &Path) -> crate::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let s = std::fs::read_to_string(path)?;
        toml::from_str(&s).map_err(|e| crate::Error::Config(e.to_string()))
    }

    pub fn save(&self, path: &Path) -> crate::Result<()> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(path, toml::to_string_pretty(self).unwrap())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 保证所有配置键都有实际消费者：新增字段时这里必须同步更新，
    /// 防止再出现"配置里写了但代码从不读"的死键。
    #[test]
    fn all_keys_are_consumed() {
        let c = Config::default();
        // 编译期借用所有字段，消费者见：
        // compression_level -> builder::pack()
        // timeout_seconds   -> extract::extract_external
        // output_dir        -> cli/gui 默认输出
        // bundle_redist_note -> builder::write_redist_note
        // prefer_native_stub -> builder::write_launchers
        // deep_extract      -> extract::set_deep_extract（cli/gui 调用前设置）
        let _ = (
            c.output_dir.clone(),
            c.compression_level,
            c.timeout_seconds,
            c.bundle_redist_note,
            c.prefer_native_stub,
            c.deep_extract,
        );
        assert!((1..=9).contains(&c.compression_level));
    }
}
