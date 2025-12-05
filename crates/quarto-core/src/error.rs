//! Error types for quarto-core

use thiserror::Error;

#[derive(Error, Debug)]
pub enum QuartoError {
    #[error("Command not yet implemented: {0}")]
    NotImplemented(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, QuartoError>;
