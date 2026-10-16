//! pxe-core — PortableXE 的 Rust 重写核心库。
//!
//! 相比 Python 原版的改进：
//! - 真实的安装包签名检测（魔数 + 结构），不再"前 8KB 找字符串 + 大小猜测"
//! - 内嵌 7z/zip 解压，无外部依赖；innoextract/msiexec 仅作可选回退
//! - 生成的启动器脚本语法正确（`exit /b`，模板有测试保证）
//! - 不再从 System32 盲拷 DLL，改为可选附带官方 vc_redist
//! - 配置键全部被实际读取（有测试保证）

pub mod builder;
pub mod config;
pub mod detect;
pub mod error;
pub mod extract;
pub mod launcher;
pub mod pe;

pub use detect::{detect, Detection, InstallerKind};
pub use error::{Error, Result};

/// 项目版本（单一来源，避免原版 v1.0.0/v1.0.1 混乱问题）
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
