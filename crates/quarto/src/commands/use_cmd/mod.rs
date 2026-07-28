//! `q2 use` — automate project setup tasks (bd-1vlw8).
//!
//! Today the only subcommand is `brand`. The module is shaped for more
//! (Quarto 1 also has `template` and `binder`), which is why the CLI
//! surface is a subcommand enum rather than a `<type> <target>` pair:
//! each task needs its own flags.
//!
//! Two front doors over one engine, matching `q2 create`:
//!
//! - **Human path** — readable output on stdout, pretty diagnostics on
//!   stderr, non-zero exit on failure.
//! - **Machine path** (`--json`) — exactly one JSON result object on
//!   stdout, diagnostics as JSON lines on stderr.
//!
//! Both resolve through the same [`brand::resolve`] and execute through
//! the same [`crate::commands::common::writer::execute_plan`], so they
//! cannot disagree about what the command did.

mod brand;
mod config;

use anyhow::{Context, Result};
use quarto_error_reporting::{DiagnosticMessage, diagnostic_to_json};
use quarto_source_map::SourceContext;
use serde::Serialize;

use crate::commands::common::plan::{CommandFailure, FilePlan};
use crate::commands::common::writer::{self, ExecutedFile};

/// CLI arguments for `q2 use brand`, passed through from `main`.
pub struct BrandArgs {
    pub target: Option<String>,
    pub dry_run: bool,
    pub force: bool,
    pub trust: bool,
    pub no_prompt: bool,
    pub json: bool,
}

pub fn execute_brand(args: BrandArgs) -> Result<()> {
    let cwd = std::env::current_dir().context("determine current directory")?;

    // `--dry-run` reports what *would* happen. Combining it with a flag
    // whose only effect is to authorize something is a contradiction
    // worth rejecting rather than silently resolving: the user has
    // asked both "don't do it" and "do it even though".
    for (flag, set) in [("--force", args.force), ("--trust", args.trust)] {
        if args.dry_run && set {
            let failure = CommandFailure::new(
                format!("{flag} cannot be combined with --dry-run"),
                format!(
                    "--dry-run writes nothing, so there is nothing for {flag} to \
                     authorize. Run with --dry-run alone to preview, then re-run \
                     with {flag} to proceed."
                ),
            );
            fail(&failure.0, args.json);
        }
    }

    let request = brand::BrandRequest {
        target: args.target,
        dry_run: args.dry_run,
        force: args.force,
        trust: args.trust,
        no_prompt: args.no_prompt,
    };

    let resolved = match brand::resolve(&request, &cwd) {
        Ok(r) => r,
        Err(f) => fail(&f.0, args.json),
    };
    for warning in &resolved.warnings {
        emit_warning(warning, args.json);
    }
    let executed = match writer::execute_plan(&resolved.plan) {
        Ok(files) => files,
        Err(f) => fail(&f.0, args.json),
    };

    if args.json {
        print_json_result(&resolved.plan, &executed);
    } else {
        print_human_result(&resolved.plan, &executed);
    }
    Ok(())
}

/// Report a fatal diagnostic on the right channel and exit non-zero.
/// Stdout stays empty on failure — it is reserved for the result.
fn fail(diag: &DiagnosticMessage, json: bool) -> ! {
    if json {
        emit_json_line(&diagnostic_to_json(diag, &SourceContext::new()));
    } else {
        eprintln!("{}", diag.to_text(None));
    }
    std::process::exit(1);
}

fn emit_warning(diag: &DiagnosticMessage, json: bool) {
    if json {
        emit_json_line(&diagnostic_to_json(diag, &SourceContext::new()));
    } else {
        eprintln!("{}", diag.to_text(None));
    }
}

fn emit_json_line<T: Serialize>(value: &T) {
    match serde_json::to_string(value) {
        Ok(line) => eprintln!("{line}"),
        Err(e) => eprintln!("(internal: failed to serialize diagnostic for --json: {e})"),
    }
}

// ====================================================================
// Output
// ====================================================================

fn print_human_result(plan: &FilePlan, files: &[ExecutedFile]) {
    if plan.dry_run {
        println!("(dry run) would add a brand to {}:", plan.root_display);
    } else {
        println!("Added a brand to {}:", plan.root_display);
    }
    println!();
    for f in files {
        println!("  {:<16} {}", f.action.as_str(), f.path.display());
    }
    println!();
    if plan.dry_run {
        println!("(dry run) nothing was written.");
    } else {
        // Say what the declaration *does*. Quarto 1 users will not
        // expect this step to be necessary, and a user who does not
        // know Quarto 2 skips auto-discovery has no way to guess why
        // their config was touched.
        println!("_brand.yml now applies to this project: Quarto 2 does not");
        println!("auto-discover brand files, so the `brand:` key above is what");
        println!("makes it take effect. Edit _brand.yml, then re-render.");
    }
}

/// The stdout result object for `--json` mode. `version` is cheap
/// insurance for MCP/LSP consumers; bump on breaking shape changes.
#[derive(Serialize)]
struct BrandResultJson {
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

fn print_json_result(plan: &FilePlan, files: &[ExecutedFile]) {
    let result = BrandResultJson {
        version: 1,
        path: plan.root.display().to_string(),
        dry_run: plan.dry_run,
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
            let failure = CommandFailure::new("Failed to serialize result", e.to_string());
            fail(&failure.0, true);
        }
    }
}
