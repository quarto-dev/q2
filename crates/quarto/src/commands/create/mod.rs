//! `q2 create` — scaffold new Quarto artifacts (bd-oa5kd2yr).
//!
//! Two front doors over one engine
//! (`claude-notes/plans/2026-07-23-q2-create-command.md`):
//!
//! - **Positional path**: `q2 create project <type> <directory> [title]`,
//!   non-interactive. Human-readable output on stdout, pretty
//!   diagnostics on stderr.
//! - **Machine path**: `q2 create --json` reads one JSON directive from
//!   stdin and writes exactly one JSON result object to stdout;
//!   diagnostics (errors, warnings) are emitted as JSON lines on
//!   stderr, matching the `q2 render --json-errors` convention.
//!   `q2 create --list [--json]` lists artifact types and choices.
//!
//! Both paths resolve through the same [`artifact::ArtifactProvider`]
//! registry and execute through the same [`writer`], so the two front
//! doors cannot drift semantically.

mod artifact;
mod project;
mod writer;

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use quarto_error_reporting::{DiagnosticMessage, diagnostic_to_json};
use quarto_source_map::SourceContext;
use serde::Serialize;

use artifact::{ArtifactProvider, ChoiceListing, CreateFailure, CreatePlan};
use writer::{ExecutedFile, FileAction};

pub fn execute(
    type_: Option<String>,
    args: Vec<String>,
    json: bool,
    list: bool,
    dry_run: bool,
) -> Result<()> {
    let providers = artifact::providers();
    let cwd = std::env::current_dir().context("determine current directory")?;

    if list {
        run_list(&providers, json);
        return Ok(());
    }
    if json {
        run_json(&providers, &cwd, type_, &args, dry_run);
        return Ok(());
    }
    run_human(&providers, &cwd, type_, &args, dry_run);
    Ok(())
}

// ====================================================================
// Human (positional) path
// ====================================================================

/// Print a diagnostic as pretty text on stderr and exit non-zero.
fn fail_human(diag: &DiagnosticMessage) -> ! {
    eprintln!("{}", diag.to_text(None));
    std::process::exit(1);
}

fn run_human(
    providers: &[Box<dyn ArtifactProvider>],
    cwd: &Path,
    type_: Option<String>,
    args: &[String],
    dry_run: bool,
) {
    let Some(type_id) = type_ else {
        fail_human(
            &CreateFailure::new(
                "Missing artifact type",
                format!(
                    "Valid types: {}. Usage: q2 create <type> ... (or q2 create --list)",
                    artifact::type_ids(providers)
                ),
            )
            .0,
        );
    };
    let Some(provider) = artifact::find_provider(providers, &type_id) else {
        fail_human(
            &CreateFailure::new(
                format!("Unknown artifact type '{type_id}'"),
                format!("Valid types: {}", artifact::type_ids(providers)),
            )
            .0,
        );
    };

    let resolved = match provider.resolve_cli(args, cwd, dry_run) {
        Ok(r) => r,
        Err(f) => fail_human(&f.0),
    };
    for warning in &resolved.warnings {
        eprintln!("{}", warning.to_text(None));
    }
    let files = match writer::execute_plan(&resolved.plan) {
        Ok(files) => files,
        Err(f) => fail_human(&f.0),
    };
    print_human_result(&resolved.plan, &files);
}

fn print_human_result(plan: &CreatePlan, files: &[ExecutedFile]) {
    if plan.dry_run {
        println!("(dry run) would create project in {}:", plan.root_display);
    } else {
        println!("Created project in {}:", plan.root_display);
    }
    println!();
    for f in files {
        let suffix = match f.action {
            FileAction::SkippedExisting => " (already exists, skipped)",
            _ => "",
        };
        println!("  {:<8} {}{}", f.action.as_str(), f.path.display(), suffix);
    }
    println!();
    if plan.dry_run {
        println!("(dry run) nothing was written.");
    } else {
        println!("To render it: q2 render {}", plan.root_display);
    }
}

// ====================================================================
// JSON (machine) path
// ====================================================================

/// The stdout result object for `--json` mode. `version` is cheap
/// insurance for MCP/LSP consumers; bump on breaking shape changes.
#[derive(Serialize)]
struct CreateResultJson {
    version: u32,
    path: String,
    dry_run: bool,
    files: Vec<FileResultJson>,
}

#[derive(Serialize)]
struct FileResultJson {
    path: String,
    action: &'static str,
}

/// Serialize one JSON object per line to stderr (same channel
/// contract as `q2 render --json-errors`).
fn emit_json_line<T: Serialize>(value: &T) {
    match serde_json::to_string(value) {
        Ok(line) => eprintln!("{line}"),
        Err(e) => {
            eprintln!("(internal: failed to serialize diagnostic for --json: {e})");
        }
    }
}

/// Emit a diagnostic as a JSON line on stderr and exit non-zero.
/// Stdout stays empty on failure — it is reserved for the result.
fn fail_json(diag: &DiagnosticMessage) -> ! {
    emit_json_line(&diagnostic_to_json(diag, &SourceContext::new()));
    std::process::exit(1);
}

fn run_json(
    providers: &[Box<dyn ArtifactProvider>],
    cwd: &Path,
    type_: Option<String>,
    args: &[String],
    dry_run: bool,
) {
    if type_.is_some() || !args.is_empty() {
        fail_json(
            &CreateFailure::new(
                "Cannot combine --json with positional arguments",
                "Pass the create directive as a JSON object on stdin instead",
            )
            .0,
        );
    }

    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        fail_json(&CreateFailure::new("Failed to read stdin", e.to_string()).0);
    }
    let value: serde_json::Value = match serde_json::from_str(input.trim()) {
        Ok(v) => v,
        Err(e) => {
            fail_json(&CreateFailure::new("Invalid JSON directive on stdin", e.to_string()).0)
        }
    };
    let serde_json::Value::Object(mut obj) = value else {
        fail_json(
            &CreateFailure::new(
                "Invalid create directive",
                "The directive must be a JSON object",
            )
            .0,
        );
    };

    // The `artifact` tag selects the provider; the rest of the object
    // is the provider's payload (each provider rejects unknown fields).
    let artifact_tag = match obj.remove("artifact") {
        Some(serde_json::Value::String(s)) => s,
        _ => fail_json(
            &CreateFailure::new(
                "Invalid create directive",
                format!(
                    "A string \"artifact\" field is required. Valid types: {}",
                    artifact::type_ids(providers)
                ),
            )
            .0,
        ),
    };
    let Some(provider) = artifact::find_provider(providers, &artifact_tag) else {
        fail_json(
            &CreateFailure::new(
                format!("Unknown artifact type '{artifact_tag}'"),
                format!("Valid types: {}", artifact::type_ids(providers)),
            )
            .0,
        );
    };

    let resolved = match provider.resolve_json(serde_json::Value::Object(obj), cwd, dry_run) {
        Ok(r) => r,
        Err(f) => fail_json(&f.0),
    };
    for warning in &resolved.warnings {
        emit_json_line(&diagnostic_to_json(warning, &SourceContext::new()));
    }
    let files = match writer::execute_plan(&resolved.plan) {
        Ok(files) => files,
        Err(f) => fail_json(&f.0),
    };

    let result = CreateResultJson {
        version: 1,
        path: resolved.plan.root.display().to_string(),
        dry_run: resolved.plan.dry_run,
        files: files
            .iter()
            .map(|f| FileResultJson {
                path: f.path.display().to_string(),
                action: f.action.as_str(),
            })
            .collect(),
    };
    match serde_json::to_string(&result) {
        Ok(line) => println!("{line}"),
        Err(e) => {
            // Should be impossible for these derived shapes; keep
            // stdout clean and fail loudly on stderr.
            fail_json(&CreateFailure::new("Failed to serialize result", e.to_string()).0);
        }
    }
}

// ====================================================================
// Capability discovery (--list)
// ====================================================================

#[derive(Serialize)]
struct ArtifactListingJson {
    #[serde(rename = "type")]
    type_id: &'static str,
    display_name: &'static str,
    choices: Vec<ChoiceListing>,
}

#[derive(Serialize)]
struct ListingJson {
    version: u32,
    artifacts: Vec<ArtifactListingJson>,
}

fn run_list(providers: &[Box<dyn ArtifactProvider>], json: bool) {
    if json {
        let listing = ListingJson {
            version: 1,
            artifacts: providers
                .iter()
                .map(|p| ArtifactListingJson {
                    type_id: p.type_id(),
                    display_name: p.display_name(),
                    choices: p.choices(),
                })
                .collect(),
        };
        match serde_json::to_string(&listing) {
            Ok(line) => println!("{line}"),
            Err(e) => {
                fail_json(&CreateFailure::new("Failed to serialize listing", e.to_string()).0)
            }
        }
        return;
    }

    println!("Available artifact types:");
    for p in providers {
        println!();
        println!("{} ({})", p.type_id(), p.display_name());
        for c in p.choices() {
            let marker = if c.implemented {
                ""
            } else {
                " (not yet implemented)"
            };
            println!("  {:<12} {}{}", c.id, c.description, marker);
        }
    }
    println!();
    println!("Usage: q2 create project <type> <directory> [title]");
}
