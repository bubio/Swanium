use thiserror::Error;

#[derive(Debug, Error)]
pub enum SwaniumError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
