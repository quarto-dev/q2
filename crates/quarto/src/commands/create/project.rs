//! The `project` artifact provider.
//!
//! Thin adapter between the artifact seam and `quarto-project-create`
//! (the platform-agnostic scaffolding engine shared with the WASM hub
//! client). All file-set knowledge lives in that crate; this module
//! owns CLI/JSON argument resolution, title defaulting, and the
//! mapping into a `CreatePlan`.

use std::path::{Path, PathBuf};

use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_project_create::{
    ProjectTypeWithTemplate, ScaffoldedFile, available_choices, create_scaffolded_files,
    find_choice, get_scaffold, implemented_choices,
};
use serde::Deserialize;

use super::artifact::{ArtifactProvider, ChoiceListing};
use crate::commands::common::plan::{
    CommandFailure, FileContent, FilePlan, PlannedEdit, PlannedFile, Precondition, ResolvedPlan,
};
use crate::commands::common::prompter::{PromptItem, Prompter};

pub struct ProjectProvider;

/// JSON payload for `{"artifact": "project", ...}` directives.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectDirective {
    directory: String,
    choice: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    dry_run: bool,
}

/// One-line summary of valid choice ids, split by implementation
/// status, for error messages.
fn valid_choices_summary() -> String {
    let mut implemented = Vec::new();
    let mut pending = Vec::new();
    for c in available_choices() {
        if c.implemented {
            implemented.push(c.id);
        } else {
            pending.push(c.id);
        }
    }
    format!(
        "Valid project types: {}. Not yet implemented: {}.",
        implemented.join(", "),
        pending.join(", ")
    )
}

fn not_yet_implemented(choice: &str) -> CommandFailure {
    CommandFailure::new(
        format!("Project type '{choice}' is not yet implemented in Quarto 2"),
        valid_choices_summary(),
    )
}

/// Map a user-supplied choice string to a scaffold target. Accepts
/// both choice ids (`website`, `blog`) and the colon form
/// (`website:blog`) — the latter routes through
/// `ProjectTypeWithTemplate::parse`.
fn resolve_target(choice: &str) -> Result<ProjectTypeWithTemplate, CommandFailure> {
    if let Some(c) = find_choice(choice) {
        if !c.implemented {
            return Err(not_yet_implemented(choice));
        }
        return Ok(c.target);
    }
    if choice.contains(':')
        && let Ok(target) = ProjectTypeWithTemplate::parse(choice)
    {
        return Ok(target);
    }
    Err(CommandFailure::new(
        format!("Unknown project type '{choice}'"),
        valid_choices_summary(),
    ))
}

impl ProjectProvider {
    /// Shared resolution for both front doors.
    fn resolve(
        &self,
        choice: &str,
        directory: &str,
        title: Option<&str>,
        dry_run: bool,
        cwd: &Path,
    ) -> Result<ResolvedPlan, CommandFailure> {
        let target = resolve_target(choice)?;
        // A parseable target without a scaffold is a valid type/template
        // combination we simply haven't implemented (e.g. website:blog).
        let scaffold = get_scaffold(&target).ok_or_else(|| not_yet_implemented(choice))?;

        let mut warnings = Vec::new();
        let title = match title {
            Some(t) => t.to_string(),
            None => {
                // Q1 parity: default the title to the directory name,
                // or to the choice id when the directory is `.`.
                let default = if directory == "." {
                    choice.to_string()
                } else {
                    directory.to_string()
                };
                warnings.push(
                    DiagnosticMessageBuilder::warning(format!(
                        "No title provided; using \"{default}\" as the project title"
                    ))
                    .build(),
                );
                default
            }
        };

        let files = create_scaffolded_files(&scaffold, &title)
            .map_err(|e| CommandFailure::new("Failed to render project scaffold", e.to_string()))?;

        Ok(ResolvedPlan {
            plan: FilePlan::new(
                cwd.join(directory),
                directory.to_string(),
                files.into_iter().map(planned_file).collect(),
                dry_run,
            )
            // Q1 parity (`project-create.ts`): creating *into* an
            // existing directory is fine, but a directory that is
            // already a Quarto project is a hard error — checked
            // before anything is written, and under --dry-run too.
            .with_preconditions(
                ["_quarto.yml", "_quarto.yaml"]
                    .into_iter()
                    .map(|config| Precondition {
                        path: PathBuf::from(config),
                        title: format!("Directory '{directory}' already contains a Quarto project"),
                        problem: format!("Found {config}. Choose a different directory."),
                    })
                    .collect(),
            )
            // Audited 2026-07-23 (see the plan): `/.quarto/` is the
            // only Q2-written project-tree artifact worth ignoring;
            // Q1's `**/*.quarto_ipynb` has no Q2 producer, and the
            // output dir is deliberately left unignored.
            .with_edits(vec![PlannedEdit::EnsureLines {
                path: PathBuf::from(".gitignore"),
                lines: vec!["/.quarto/".to_string()],
            }]),
            warnings,
        })
    }
}

fn planned_file(file: ScaffoldedFile) -> PlannedFile {
    match file {
        ScaffoldedFile::Text { path, content } => PlannedFile {
            path,
            content: FileContent::Text(content),
        },
        ScaffoldedFile::Binary { path, content, .. } => PlannedFile {
            path,
            content: FileContent::Binary(content),
        },
    }
}

impl ArtifactProvider for ProjectProvider {
    fn type_id(&self) -> &'static str {
        "project"
    }

    fn display_name(&self) -> &'static str {
        "Project"
    }

    fn resolve_cli(
        &self,
        args: &[String],
        cwd: &Path,
        dry_run: bool,
    ) -> Result<ResolvedPlan, CommandFailure> {
        match args {
            [] => Err(CommandFailure::new(
                "Missing project type",
                valid_choices_summary(),
            )),
            [_choice] => Err(CommandFailure::new(
                "Missing directory argument",
                "Usage: q2 create project <type> <directory> [title]",
            )),
            [choice, directory] => self.resolve(choice, directory, None, dry_run, cwd),
            [choice, directory, title] => {
                self.resolve(choice, directory, Some(title), dry_run, cwd)
            }
            [_, _, _, extra, ..] => Err(CommandFailure::new(
                format!("Unexpected argument '{extra}'"),
                "Usage: q2 create project <type> <directory> [title]",
            )),
        }
    }

    fn resolve_json(
        &self,
        payload: serde_json::Value,
        cwd: &Path,
        dry_run: bool,
    ) -> Result<ResolvedPlan, CommandFailure> {
        let directive: ProjectDirective = serde_json::from_value(payload)
            .map_err(|e| CommandFailure::new("Invalid create directive", e.to_string()))?;
        self.resolve(
            &directive.choice,
            &directive.directory,
            directive.title.as_deref(),
            directive.dry_run || dry_run,
            cwd,
        )
    }

    fn resolve_interactive(
        &self,
        args: &[String],
        cwd: &Path,
        dry_run: bool,
        prompter: &mut dyn Prompter,
    ) -> Result<ResolvedPlan, CommandFailure> {
        if let [_, _, _, extra, ..] = args {
            return Err(CommandFailure::new(
                format!("Unexpected argument '{extra}'"),
                "Usage: q2 create project <type> <directory> [title]",
            ));
        }

        let choice = match args.first() {
            Some(c) => {
                // Validate a typed choice up front so a bad one fails
                // before any prompting happens.
                resolve_target(c)?;
                c.clone()
            }
            None => {
                let choices = implemented_choices();
                let items: Vec<PromptItem> = choices
                    .iter()
                    .map(|c| PromptItem {
                        label: c.name.clone(),
                        help: c.description.clone(),
                    })
                    .collect();
                let idx = prompter.select("Project type", &items)?;
                choices[idx].id.clone()
            }
        };

        let directory = match args.get(1) {
            Some(d) => d.clone(),
            None => {
                let d = prompter.input("Directory", None)?.trim().to_string();
                if d.is_empty() {
                    return Err(CommandFailure::new(
                        "Directory must not be empty",
                        "Provide a directory name, or `.` for the current directory",
                    ));
                }
                d
            }
        };

        let title = match args.get(2) {
            Some(t) => t.clone(),
            None => {
                // Same defaulting rule as the non-interactive path,
                // but shown to the user for acceptance — which is why
                // this path never emits the defaulted-title warning.
                let default = if directory == "." {
                    choice.clone()
                } else {
                    directory.clone()
                };
                prompter.input("Title", Some(&default))?
            }
        };

        self.resolve(&choice, &directory, Some(&title), dry_run, cwd)
    }

    fn choices(&self) -> Vec<ChoiceListing> {
        available_choices()
            .into_iter()
            .map(|c| ChoiceListing {
                id: c.id,
                name: c.name,
                description: c.description,
                implemented: c.implemented,
            })
            .collect()
    }
}
