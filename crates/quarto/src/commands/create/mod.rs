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

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use quarto_error_reporting::{DiagnosticMessage, diagnostic_to_json};
use quarto_source_map::SourceContext;
use serde::Serialize;

use crate::commands::common::plan::{CommandFailure, FilePlan};
use crate::commands::common::writer::{self, ExecutedFile, FileAction};
use crate::commands::common::prompter;
use artifact::{ArtifactProvider, ChoiceListing};

pub fn execute(
    type_: Option<String>,
    args: Vec<String>,
    json: bool,
    list: bool,
    dry_run: bool,
    no_prompt: bool,
) -> Result<()> {
    let providers = artifact::providers();
    let cwd = std::env::current_dir().context("determine current directory")?;

    if list {
        run_list(&providers, json);
        return Ok(());
    }
    if json {
        // The machine path never prompts, TTY or not.
        run_json(&providers, &cwd, type_, &args, dry_run);
        return Ok(());
    }
    run_human(&providers, &cwd, type_, &args, dry_run, no_prompt);
    Ok(())
}

/// Q1-parity prompt gate (`cmd.ts:62–74`): prompts fire only on a real
/// terminal (stdin *and* stderr — the prompt UI renders on stderr),
/// outside CI, without an explicit opt-out.
fn allow_prompt(no_prompt: bool) -> bool {
    use std::io::IsTerminal;
    !no_prompt
        && std::env::var_os("CI").is_none()
        && std::io::stdin().is_terminal()
        && std::io::stderr().is_terminal()
}

/// Interactive artifact-type selection over the provider registry.
/// Deliberately prompts even with a single registered provider, so
/// behavior stays stable when `extension` lands.
fn select_artifact<'a>(
    providers: &'a [Box<dyn ArtifactProvider>],
    prompter: &mut dyn prompter::Prompter,
) -> Result<&'a dyn ArtifactProvider, CommandFailure> {
    let items: Vec<prompter::PromptItem> = providers
        .iter()
        .map(|p| prompter::PromptItem {
            label: p.type_id().to_string(),
            help: p.display_name().to_string(),
        })
        .collect();
    let idx = prompter.select("Artifact type", &items)?;
    Ok(providers[idx].as_ref())
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
    no_prompt: bool,
) {
    let interactive = allow_prompt(no_prompt);

    let provider: &dyn ArtifactProvider = match &type_ {
        Some(type_id) => match artifact::find_provider(providers, type_id) {
            Some(p) => p,
            None => fail_human(
                &CommandFailure::new(
                    format!("Unknown artifact type '{type_id}'"),
                    format!("Valid types: {}", artifact::type_ids(providers)),
                )
                .0,
            ),
        },
        None if interactive => match select_artifact(providers, &mut prompter::InquirePrompter) {
            Ok(p) => p,
            Err(f) => fail_human(&f.0),
        },
        None => fail_human(
            &CommandFailure::new(
                "Missing artifact type",
                format!(
                    "Valid types: {}. Usage: q2 create <type> ... (or q2 create --list)",
                    artifact::type_ids(providers)
                ),
            )
            .0,
        ),
    };

    let resolution = if interactive {
        provider.resolve_interactive(args, cwd, dry_run, &mut prompter::InquirePrompter)
    } else {
        provider.resolve_cli(args, cwd, dry_run)
    };
    let resolved = match resolution {
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

fn print_human_result(plan: &FilePlan, files: &[ExecutedFile]) {
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
            &CommandFailure::new(
                "Cannot combine --json with positional arguments",
                "Pass the create directive as a JSON object on stdin instead",
            )
            .0,
        );
    }

    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        fail_json(&CommandFailure::new("Failed to read stdin", e.to_string()).0);
    }
    let value: serde_json::Value = match serde_json::from_str(input.trim()) {
        Ok(v) => v,
        Err(e) => {
            fail_json(&CommandFailure::new("Invalid JSON directive on stdin", e.to_string()).0)
        }
    };
    let serde_json::Value::Object(mut obj) = value else {
        fail_json(
            &CommandFailure::new(
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
            &CommandFailure::new(
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
            &CommandFailure::new(
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
            fail_json(&CommandFailure::new("Failed to serialize result", e.to_string()).0);
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
                fail_json(&CommandFailure::new("Failed to serialize listing", e.to_string()).0)
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

// ====================================================================
// Interactive prompt-flow tests (bd-hh1erpfx)
// ====================================================================
//
// These drive `resolve_interactive` through a scripted `Prompter`, so
// the prompt *flow* (which prompts appear, in what order, with what
// defaults and choices) is covered deterministically in CI without a
// PTY. The real terminal implementation is verified manually via an
// expect-script PTY run (see the plan's Phase 3).
#[cfg(test)]
mod interactive_tests {
    use std::path::Path;

    use super::artifact::ArtifactProvider;
    use super::project::ProjectProvider;
    use super::select_artifact;
    use crate::commands::common::plan::{CommandFailure, FileContent, FilePlan};
    use crate::commands::common::prompter::{PromptItem, Prompter};

    /// Scripted prompter: queued answers plus a transcript of every
    /// prompt shown, so tests assert both the answers' effect and
    /// which prompts appeared, in what order, with what payload.
    struct ScriptedPrompter {
        select_answers: Vec<usize>,
        /// `None` = accept the offered default.
        input_answers: Vec<Option<&'static str>>,
        cancel_at: Option<usize>,
        prompt_count: usize,
        transcript: Vec<String>,
        select_items: Vec<Vec<PromptItem>>,
        input_defaults: Vec<Option<String>>,
    }

    impl ScriptedPrompter {
        fn new(select_answers: Vec<usize>, input_answers: Vec<Option<&'static str>>) -> Self {
            Self {
                select_answers,
                input_answers,
                cancel_at: None,
                prompt_count: 0,
                transcript: Vec::new(),
                select_items: Vec::new(),
                input_defaults: Vec::new(),
            }
        }

        fn cancel_at(mut self, idx: usize) -> Self {
            self.cancel_at = Some(idx);
            self
        }
    }

    impl Prompter for ScriptedPrompter {
        fn select(&mut self, prompt: &str, items: &[PromptItem]) -> Result<usize, CommandFailure> {
            let idx = self.prompt_count;
            self.prompt_count += 1;
            if self.cancel_at == Some(idx) {
                return Err(CommandFailure::cancelled());
            }
            self.transcript.push(format!("select:{prompt}"));
            self.select_items.push(items.to_vec());
            Ok(self.select_answers.remove(0))
        }

        fn input(&mut self, prompt: &str, default: Option<&str>) -> Result<String, CommandFailure> {
            let idx = self.prompt_count;
            self.prompt_count += 1;
            if self.cancel_at == Some(idx) {
                return Err(CommandFailure::cancelled());
            }
            self.transcript.push(format!("input:{prompt}"));
            self.input_defaults.push(default.map(str::to_string));
            match self.input_answers.remove(0) {
                Some(v) => Ok(v.to_string()),
                None => Ok(default
                    .expect("test accepted a default where none was offered")
                    .to_string()),
            }
        }
    }

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn quarto_yml(plan: &FilePlan) -> &str {
        plan.files
            .iter()
            .find_map(|f| match &f.content {
                FileContent::Text(s) if f.path.to_str() == Some("_quarto.yml") => Some(s.as_str()),
                _ => None,
            })
            .expect("_quarto.yml in plan")
    }

    #[test]
    fn no_args_prompts_type_directory_title() {
        // Registry order of implemented choices is [default, website];
        // select index 1 = website, type a directory, accept the
        // default title.
        let mut p = ScriptedPrompter::new(vec![1], vec![Some("mysite"), None]);
        let resolved = ProjectProvider
            .resolve_interactive(&args(&[]), Path::new("/x"), false, &mut p)
            .unwrap();

        assert_eq!(
            p.transcript,
            ["select:Project type", "input:Directory", "input:Title"]
        );
        // Only implemented choices are offered.
        let labels: Vec<&str> = p.select_items[0].iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["Default", "Website"]);
        // The accepted default title is the directory name, and the
        // interactive path emits no defaulted-title warning — the user
        // saw and accepted the default.
        assert_eq!(p.input_defaults.last().unwrap().as_deref(), Some("mysite"));
        assert!(resolved.warnings.is_empty());
        assert!(quarto_yml(&resolved.plan).contains("title: \"mysite\""));
        assert!(resolved.plan.root.ends_with("mysite"));
    }

    #[test]
    fn choice_arg_skips_type_prompt() {
        let mut p = ScriptedPrompter::new(vec![], vec![Some("d"), Some("My T")]);
        let resolved = ProjectProvider
            .resolve_interactive(&args(&["website"]), Path::new("/x"), false, &mut p)
            .unwrap();
        assert_eq!(p.transcript, ["input:Directory", "input:Title"]);
        assert!(quarto_yml(&resolved.plan).contains("title: \"My T\""));
    }

    #[test]
    fn dir_arg_prompts_title_only_with_dir_default() {
        let mut p = ScriptedPrompter::new(vec![], vec![None]);
        let resolved = ProjectProvider
            .resolve_interactive(&args(&["website", "proj"]), Path::new("/x"), false, &mut p)
            .unwrap();
        assert_eq!(p.transcript, ["input:Title"]);
        assert_eq!(p.input_defaults[0].as_deref(), Some("proj"));
        assert!(quarto_yml(&resolved.plan).contains("title: \"proj\""));
        assert!(resolved.warnings.is_empty());
    }

    #[test]
    fn dot_directory_title_default_is_choice_id() {
        let mut p = ScriptedPrompter::new(vec![], vec![None]);
        let resolved = ProjectProvider
            .resolve_interactive(&args(&["website", "."]), Path::new("/x"), false, &mut p)
            .unwrap();
        assert_eq!(p.input_defaults[0].as_deref(), Some("website"));
        assert!(quarto_yml(&resolved.plan).contains("title: \"website\""));
    }

    #[test]
    fn full_args_prompt_nothing_and_propagate_dry_run() {
        let mut p = ScriptedPrompter::new(vec![], vec![]);
        let resolved = ProjectProvider
            .resolve_interactive(&args(&["website", "d", "T"]), Path::new("/x"), true, &mut p)
            .unwrap();
        assert!(p.transcript.is_empty());
        assert!(resolved.plan.dry_run);
        assert!(quarto_yml(&resolved.plan).contains("title: \"T\""));
    }

    #[test]
    fn cancel_at_first_prompt_fails() {
        let mut p = ScriptedPrompter::new(vec![0], vec![]).cancel_at(0);
        let err = ProjectProvider
            .resolve_interactive(&args(&[]), Path::new("/x"), false, &mut p)
            .unwrap_err();
        assert!(
            err.0.to_text(None).to_lowercase().contains("cancel"),
            "error should mention cancellation: {}",
            err.0.to_text(None)
        );
    }

    #[test]
    fn typed_unimplemented_choice_errors_without_prompting_further() {
        let mut p = ScriptedPrompter::new(vec![], vec![]);
        let err = ProjectProvider
            .resolve_interactive(&args(&["blog", "d"]), Path::new("/x"), false, &mut p)
            .unwrap_err();
        assert!(err.0.to_text(None).contains("not yet implemented"));
    }

    #[test]
    fn artifact_select_prompts_even_with_single_provider() {
        // Prompting (rather than auto-selecting) keeps behavior stable
        // when a second artifact type (extension) is registered.
        let providers = super::artifact::providers();
        let mut p = ScriptedPrompter::new(vec![0], vec![]);
        let provider = select_artifact(&providers, &mut p).unwrap();
        assert_eq!(provider.type_id(), "project");
        assert_eq!(p.transcript, ["select:Artifact type"]);
        assert_eq!(p.select_items[0][0].label, "project");
    }
}
