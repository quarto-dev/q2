//! Add command implementation

use anyhow::Result;
use quarto_core::QuartoError;

pub fn execute() -> Result<()> {
    Err(QuartoError::NotImplemented("add".to_string()).into())
}
