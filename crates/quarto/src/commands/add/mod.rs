//! `q2 add` — install an extension into a project (bd-rfz4e3bw).
//!
//! Ports Quarto 1's `quarto add` (`src/command/add/cmd.ts`,
//! `src/extension/install.ts`): the same source grammar, the same
//! `_extensions/<owner>/<name>/` layout, and the same
//! Install / Update / Revert / No Change table. Deliberate deviations,
//! each explained where it is implemented:
//!
//! - remote sources need trust: a prompt that defaults to **no**, or
//!   `--trust` (see [`resolve::confirm_trust`]);
//! - the install root is the project root, not cwd, with `--dir` to
//!   override;
//! - each install writes a `.quarto-source.yml` record, and replacing
//!   an extension with local edits needs `--force` ([`record`]);
//! - No Change writes nothing (Q1 re-copies);
//! - `--dry-run` and `--json`, as in `q2 use brand`; no "open docs in
//!   browser" prompt — the learn-more URL is printed instead.
//!
//! Two front doors over one engine: human output, or (`--json`)
//! exactly one result object on stdout with diagnostics as JSON lines
//! on stderr.

mod install;
mod record;
mod resolve;
mod stage;
mod version;

use std::path::PathBuf;

use anyhow::{Context, Result};
use quarto_error_reporting::{DiagnosticMessage, diagnostic_to_json};
use quarto_source_map::SourceContext;
use serde::Serialize;

use crate::commands::common::plan::CommandFailure;
use crate::commands::common::prompter::{InquirePrompter, Prompter};

use record::InstalledState;
use resolve::{AddPlan, AddRequest, ResolveContext};

/// CLI arguments for `q2 add`, passed through from `main`.
pub struct AddArgs {
    pub extension: String,
    pub dir: Option<PathBuf>,
    pub dry_run: bool,
    pub force: bool,
    pub trust: bool,
    pub no_prompt: bool,
    pub json: bool,
}

pub fn execute(args: AddArgs) -> Result<()> {
    let cwd = std::env::current_dir().context("determine current directory")?;

    // Same rule as `q2 use`: `--dry-run` reports what would happen, and
    // `--force` only authorizes overwriting local edits, which a dry run
    // never does, so the pairing is a contradiction worth rejecting.
    // `--trust` is different: a dry run of a remote source still
    // downloads it to compute the table, and `--trust` authorizes that
    // fetch, so it combines with `--dry-run`.
    if args.dry_run && args.force {
        let failure = CommandFailure::new(
            "--force cannot be combined with --dry-run",
            "--dry-run writes nothing, so there is nothing for --force to \
             authorize. Run with --dry-run alone to preview, then re-run \
             with --force to proceed.",
        );
        fail(&failure.0, args.json);
    }

    let request = AddRequest {
        target: args.extension,
        dir: args.dir,
        dry_run: args.dry_run,
        force: args.force,
        trust: args.trust,
    };

    // Scratch space for the fetched source. Held until after the
    // install: planned extensions are copied from paths inside it.
    let work_dir = tempfile::tempdir().context("create a temporary working directory")?;

    let can_prompt = allow_prompt(args.no_prompt, args.json);
    let mut inquire_prompter = InquirePrompter;

    let plan = {
        let mut ctx = ResolveContext {
            cwd: &cwd,
            work_dir: work_dir.path(),
            fetcher: &quarto_source_fetch::UreqFetch,
            prompter: if can_prompt {
                Some(&mut inquire_prompter as &mut dyn Prompter)
            } else {
                None
            },
            limits: quarto_source_fetch::ExtractLimits::default(),
        };
        match resolve::resolve(&request, &mut ctx) {
            Ok(plan) => plan,
            Err(f) => fail(&f.0, args.json),
        }
    };

    if !args.json {
        print_table(&plan);
    }
    let prompter: Option<&mut dyn Prompter> = if can_prompt {
        Some(&mut inquire_prompter)
    } else {
        None
    };
    if let Err(f) = resolve::confirm_install(&plan, prompter) {
        fail(&f.0, args.json);
    }
    if let Err(f) = install::install(&plan) {
        fail(&f.0, args.json);
    }

    if args.json {
        print_json_result(&plan);
    } else {
        print_human_footer(&plan);
    }
    Ok(())
}

/// Whether an interactive prompt is possible. Same gate as
/// `q2 use brand`: never under `--json`, `--no-prompt`, or CI, and only
/// with a terminal on stdin and stderr (prompt UI renders on stderr).
fn allow_prompt(no_prompt: bool, json: bool) -> bool {
    use std::io::IsTerminal;
    !no_prompt
        && !json
        && std::env::var_os("CI").is_none()
        && std::io::stdin().is_terminal()
        && std::io::stderr().is_terminal()
}

/// Report a fatal diagnostic on the right channel and exit non-zero.
/// Stdout carries no result on failure.
fn fail(diag: &DiagnosticMessage, json: bool) -> ! {
    if json {
        match serde_json::to_string(&diagnostic_to_json(diag, &SourceContext::new())) {
            Ok(line) => eprintln!("{line}"),
            Err(e) => eprintln!("(internal: failed to serialize diagnostic for --json: {e})"),
        }
    } else {
        eprintln!("{}", diag.to_text(None));
    }
    std::process::exit(1);
}

// ====================================================================
// Output
// ====================================================================

/// The Q1 confirmation table (`install.ts:514-541`):
///
/// ```text
/// The following changes will be made:
/// Font Awesome support  [Update]  1.0.0  ->  1.1.0  (shortcode)
/// ```
fn print_table(plan: &AddPlan) {
    if !plan.extensions.iter().any(|e| e.writes) {
        println!("No changes required - extensions already installed.");
        return;
    }
    let rows: Vec<[String; 6]> = plan
        .extensions
        .iter()
        .map(|e| {
            let arrow = if e.change.from.is_empty() || e.change.to.is_empty() {
                ""
            } else {
                "->"
            };
            [
                e.display_name.clone(),
                format!("[{}]", e.change.action.label()),
                e.change.from.clone(),
                arrow.to_string(),
                e.change.to.clone(),
                e.types.clone(),
            ]
        })
        .collect();
    let mut widths = [0usize; 6];
    for row in &rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.chars().count());
        }
    }

    if plan.dry_run {
        println!(
            "(dry run) The following changes would be made in {}:",
            plan.root_display
        );
    } else {
        println!(
            "The following changes will be made in {}:",
            plan.root_display
        );
    }
    for (row, ext) in rows.iter().zip(&plan.extensions) {
        let line: Vec<String> = row
            .iter()
            .zip(widths)
            .map(|(cell, w)| format!("{cell:<w$}"))
            .collect();
        println!("  {}", line.join("  ").trim_end());
        if ext.writes && ext.installed == InstalledState::Untracked {
            println!(
                "    note: {} has no install record, so local changes to it cannot be detected",
                ext.dest.display()
            );
        }
    }
    println!();
}

fn print_human_footer(plan: &AddPlan) {
    let gitattributes =
        std::path::Path::new(stage::EXTENSIONS_DIR).join(install::GITATTRIBUTES_FILENAME);
    if plan.dry_run {
        if plan.writes_gitattributes {
            println!(
                "(dry run) would also create {}, so git keeps extension files byte-for-byte.",
                gitattributes.display()
            );
        }
        println!("(dry run) nothing was written.");
        return;
    }
    if !plan.writes_anything() {
        return;
    }
    println!("Extension installation complete.");
    for ext in plan.extensions.iter().filter(|e| e.writes) {
        println!("  {}", ext.dest.display());
    }
    if plan.writes_gitattributes {
        println!(
            "Created {}, so git keeps extension files byte-for-byte \
             (line-ending conversion would look like a local edit).",
            gitattributes.display()
        );
    }
    if let Some(url) = &plan.learn_more {
        println!();
        println!("Learn more about this extension at:");
        println!("{url}");
    }
}

/// The stdout result object for `--json`. `version` is cheap
/// insurance for MCP/LSP consumers; bump on breaking shape changes.
#[derive(Serialize)]
struct AddResultJson<'a> {
    version: u32,
    path: String,
    dry_run: bool,
    source: &'a str,
    extensions: Vec<ExtensionResultJson<'a>>,
    /// Whether this run created `_extensions/.gitattributes`. False
    /// under `--dry-run`, like `written`.
    gitattributes_written: bool,
}

#[derive(Serialize)]
struct ExtensionResultJson<'a> {
    id: &'a str,
    title: &'a str,
    path: String,
    action: &'static str,
    from: Option<&'a str>,
    to: Option<&'a str>,
    /// Whether this run (re)installed it — false for No Change, and
    /// for everything under `--dry-run`.
    written: bool,
    /// The installed copy had no `.quarto-source.yml`, so local edits
    /// could not be checked.
    untracked: bool,
}

fn print_json_result(plan: &AddPlan) {
    let non_empty = |s: &'_ str| -> bool { !s.is_empty() };
    let result = AddResultJson {
        version: 1,
        path: plan.root.display().to_string(),
        dry_run: plan.dry_run,
        source: &plan.provenance.source,
        extensions: plan
            .extensions
            .iter()
            .map(|e| ExtensionResultJson {
                id: &e.id,
                title: &e.display_name,
                path: record::slash_path(&e.dest),
                action: e.change.action.as_str(),
                from: Some(e.change.from.as_str()).filter(|s| non_empty(s)),
                to: Some(e.change.to.as_str()).filter(|s| non_empty(s)),
                written: e.writes && !plan.dry_run,
                untracked: e.installed == InstalledState::Untracked,
            })
            .collect(),
        gitattributes_written: plan.writes_gitattributes && !plan.dry_run,
    };
    match serde_json::to_string(&result) {
        Ok(line) => println!("{line}"),
        Err(e) => {
            let failure = CommandFailure::new("Failed to serialize result", e.to_string());
            fail(&failure.0, true);
        }
    }
}
