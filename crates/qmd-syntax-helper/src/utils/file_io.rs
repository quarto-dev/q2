use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Read a file to a string
pub fn read_file(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("Failed to read file: {}", path.display()))
}

/// Write content to a file
pub fn write_file(path: &Path, content: &str) -> Result<()> {
    fs::write(path, content).with_context(|| format!("Failed to write file: {}", path.display()))
}
