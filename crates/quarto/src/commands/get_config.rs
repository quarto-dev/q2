//! `quarto get-config` subcommand — print merged document configuration as JSON.
//!
//! External tools call `q2 get-config <file> [yaml-path]` to read a document's
//! effective configuration *after* Quarto 2's full metadata-merge semantics
//! (`_quarto.yml`, directory `_metadata.yml` layers, document frontmatter, and
//! `format.<fmt>.*` flattening) without having to reimplement any of it.
//!
//! See bd-xoaic / GH #256 and
//! `claude-notes/plans/2026-06-02-get-config-command.md`.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::ValueEnum;
use serde_json::Value;

use quarto_core::Format;
use quarto_core::get_config::{ProseMode, config_value_to_json, merge_document_metadata, navigate};
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// How prose-valued metadata (a parsed `title`, etc.) is rendered (D1 / D7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputMode {
    /// Render prose back to a markdown string (e.g. `"Hello *world*!"`).
    Value,
    /// Render prose as a self-contained, source-free Pandoc AST fragment.
    Pandoc,
}

impl From<OutputMode> for ProseMode {
    fn from(mode: OutputMode) -> Self {
        match mode {
            OutputMode::Value => ProseMode::Value,
            OutputMode::Pandoc => ProseMode::Pandoc,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GetConfigArgs {
    /// Document to inspect.
    pub file: PathBuf,
    /// Dot-separated key path; a numeric segment indexes into an array. `None`
    /// (or empty) ⇒ the entire merged metadata.
    pub path: Option<String>,
    /// Target format whose `format.<fmt>.*` overrides are flattened in.
    pub to: String,
    /// Prose representation.
    pub output: OutputMode,
    /// Exit non-zero when the path is absent, instead of printing `null`.
    pub strict: bool,
    /// Emit single-line JSON instead of pretty-printed.
    pub compact: bool,
}

/// Compute the JSON value for the requested path.
///
/// Returns `Ok(None)` when the path does not exist in the merged metadata —
/// distinct from real errors (missing file, parse failure, bad format) which
/// are `Err`. An empty path always resolves to the whole metadata, so it never
/// yields `None`.
pub fn get_config_value(args: &GetConfigArgs) -> Result<Option<Value>> {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());

    let input = args
        .file
        .canonicalize()
        .with_context(|| format!("Cannot read document: {}", args.file.display()))?;

    let project = ProjectContext::discover(&input, runtime.as_ref())
        .context("Failed to discover project context")?;

    let format = Format::from_format_string(&args.to)
        .map_err(|e| anyhow::anyhow!("Invalid --to format '{}': {}", args.to, e))?;

    let doc_info = DocumentInfo::from_path(&input);
    let source_bytes = std::fs::read(&input)
        .with_context(|| format!("Cannot read document: {}", input.display()))?;

    let (meta, ctx) = pollster::block_on(merge_document_metadata(
        runtime.clone(),
        &project,
        &format,
        &doc_info,
        &source_bytes,
    ))
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    let path = args.path.as_deref().unwrap_or("");
    Ok(navigate(&meta, path).map(|value| config_value_to_json(value, args.output.into(), &ctx)))
}

pub fn execute(args: GetConfigArgs) -> Result<()> {
    match get_config_value(&args)? {
        Some(value) => {
            let rendered = if args.compact {
                serde_json::to_string(&value)?
            } else {
                serde_json::to_string_pretty(&value)?
            };
            println!("{rendered}");
            Ok(())
        }
        None => {
            if args.strict {
                anyhow::bail!(
                    "get-config: path '{}' not found",
                    args.path.as_deref().unwrap_or("")
                );
            }
            // D2: a missing path prints `null` and exits 0.
            println!("null");
            Ok(())
        }
    }
}
