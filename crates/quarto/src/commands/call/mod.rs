//! Call command implementation
//!
//! The `quarto call` command provides access to Quarto subsystem functions.
//!
//! ## Available functions:
//!
//! - `test`: Run embedded document tests
//! - `build-ts-extension`: Build a TypeScript engine extension into a
//!   deployable `.js` bundle (dispatched from `CallCommands::BuildTsExtension`
//!   in `main.rs`, not through this module's string-based `execute`)

mod test;

use anyhow::{Result, anyhow};

pub fn execute(function: Option<String>, args: Vec<String>) -> Result<()> {
    match function.as_deref() {
        Some("test") => test::execute(args),
        Some(other) => Err(anyhow!(
            "Unknown function: {}\n\nAvailable functions:\n  test    Run embedded document tests",
            other
        )),
        None => Err(anyhow!(
            "Usage: quarto call <function> [args...]\n\nAvailable functions:\n  test    Run embedded document tests"
        )),
    }
}
