use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("7z error: {0}")]
    SevenZ(String),

    #[error("PE parse error: {0}")]
    Pe(String),

    #[error("extraction failed: {0}")]
    Extraction(String),

    #[error("invalid package: {0}")]
    InvalidPackage(String),

    #[error("config error: {0}")]
    Config(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
