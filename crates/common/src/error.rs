use thiserror::Error;

#[derive(Debug, Error)]
pub enum SwaniumError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("config parse error: {0}")]
    ConfigParse(#[from] toml::de::Error),

    #[error("config serialize error: {0}")]
    ConfigSerialize(#[from] toml::ser::Error),

    #[error("could not locate a platform config directory")]
    NoConfigDir,
}
