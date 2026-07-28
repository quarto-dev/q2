//! `q2 use brand` — resolution: gates, brand production, plan (bd-1vlw8).
//!
//! The command has two halves, and the second is the one Quarto 1 does
//! not have:
//!
//! 1. Put a `_brand.yml` in the project.
//! 2. **Declare it** in `_quarto.yml`.
//!
//! Quarto 1 needs only step 1 because it auto-discovers `_brand.yml`
//! (`project-shared.ts:620-628`). Quarto 2 deliberately does not
//! (`quarto-core/src/project/mod.rs:354-376`), so a port that stopped
//! after step 1 would write files that change nothing about the render.
//! That is why the declaration is not an optional convenience here.
//!
//! Everything is checked before anything is written. The gates run in
//! cheapest-first order and, once remote sources land, all of them run
//! *before* any network traffic.

use std::path::{Path, PathBuf};

use crate::commands::common::plan::{
    CommandFailure, FileContent, FilePlan, PlannedEdit, PlannedFile, Precondition, ResolvedPlan,
};

use super::config::{
    BrandDeclSite, CONFIG_FILENAMES, ProjectConfigFile, brand_declaration_block,
    find_project_config,
};

/// The brand file spellings we refuse to write over, and probe for.
const BRAND_FILENAMES: [&str; 2] = ["_brand.yml", "_brand.yaml"];

/// The path `q2 use brand` writes and declares for a source that is a
/// lone brand file with no accompanying assets.
///
/// Root rather than `_brand/` because it is the layout Quarto 1 users
/// recognize, and because Quarto 2 writes the `brand:` key anyway — the
/// location is a readability choice, not a discovery requirement.
const DEFAULT_BRAND_PATH: &str = "_brand.yml";

pub struct BrandRequest {
    pub target: Option<String>,
    pub dry_run: bool,
    pub force: bool,
    /// Waives the remote-source trust prompt. Read once fetching lands;
    /// until then a target is rejected before it could matter.
    #[allow(dead_code)]
    pub trust: bool,
    /// Suppresses interactive prompts. The trust prompt is the only one
    /// this command has, so this is likewise inert until fetching lands.
    #[allow(dead_code)]
    pub no_prompt: bool,
}

/// Resolve a `q2 use brand` invocation into an executable plan.
pub fn resolve(req: &BrandRequest, cwd: &Path) -> Result<ResolvedPlan, CommandFailure> {
    // ── Gate 1: a project must already exist ────────────────────────
    //
    // Deliberately not created for the user. `q2 use brand` adds a
    // brand *to a project*; conjuring a `_quarto.yml` would guess at
    // the project type, the title, and the output layout — decisions
    // `q2 create` exists to ask about.
    let Some((root, config_path)) = find_project_config(cwd) else {
        return Err(CommandFailure::new(
            "No Quarto project found",
            format!(
                "Looked for {} in {} and every parent directory. \
                 `q2 use brand` adds a brand to an existing project and will not \
                 create one. Run `q2 create project default .` first, or run this \
                 from inside a project.",
                CONFIG_FILENAMES.join(" or "),
                cwd.display()
            ),
        ));
    };

    // ── Gate 2: the config must be one we can safely edit ───────────
    let config = ProjectConfigFile::load(&config_path)?;
    let config_rel = PathBuf::from(&config.filename);

    // ── Gate 3: no brand may already be declared ────────────────────
    let existing = config.brand_declaration();
    let declaration_edit = match &existing {
        None => PlannedEdit::AppendBlock {
            path: config_rel.clone(),
            block: brand_declaration_block(DEFAULT_BRAND_PATH),
        },
        Some(decl) if !req.force => {
            return Err(CommandFailure::new(
                format!(
                    "This project already declares a brand in {}",
                    config.filename
                ),
                format!(
                    "{} line {} sets `{}: {}`. Adding another would leave two \
                     declarations, and the one that wins is not obvious. Edit that \
                     line by hand, or pass --force to repoint it.",
                    config.filename, decl.line, decl.site, decl.value_summary
                ),
            ));
        }
        // --force, and the declaration is a plain top-level path: we can
        // repoint it precisely.
        //
        // `expected` is sliced straight out of the config text rather
        // than reused from `value_summary`. The two are equal today, but
        // `value_summary` is a *display* field — if it ever gained
        // truncation or quoting for readability, silently feeding it to
        // the writer's byte-exact guard would break the guard rather
        // than the display. Slicing keeps the guard's input byte-exact
        // by construction.
        Some(decl) if decl.site == BrandDeclSite::TopLevel && decl.value_span.is_some() => {
            let (start, end) = decl.value_span.expect("guarded by the match arm");
            PlannedEdit::ReplaceRange {
                path: config_rel.clone(),
                start,
                end,
                replacement: DEFAULT_BRAND_PATH.to_string(),
                expected: config.text[start..end].to_string(),
            }
        }
        // --force, but the declaration is a shape we cannot rewrite in
        // place. Appending would produce a duplicate top-level key (an
        // invalid config), and leaving a `format.<fmt>.brand` in place
        // would silently shadow what we just wrote for that format.
        // Refusing is the only honest option.
        Some(decl) => {
            return Err(CommandFailure::new(
                format!("Cannot repoint the brand declared in {}", config.filename),
                format!(
                    "{} line {} declares `{}`, which --force cannot rewrite safely: {}. \
                     Edit that declaration by hand, then re-run.",
                    config.filename,
                    decl.line,
                    decl.site,
                    match &decl.site {
                        BrandDeclSite::Format(f) => format!(
                            "a format-scoped brand would still override the project-level \
                             one for `{f}` output"
                        ),
                        BrandDeclSite::TopLevel =>
                            "an inline brand block would have to be replaced wholesale".to_string(),
                    }
                ),
            ));
        }
    };

    // ── The brand file itself ───────────────────────────────────────
    let files = resolve_brand_files(req)?;

    // ── Gate 4 (as a plan precondition): no brand file may exist ────
    //
    // Expressed as a precondition rather than an inline check so it is
    // re-verified at write time. That is not redundant once fetching
    // lands: the gap between resolving and writing includes a network
    // round trip, and a brand file appearing in that window must not be
    // clobbered.
    let preconditions = if req.force {
        Vec::new()
    } else {
        BRAND_FILENAMES
            .iter()
            .map(|name| Precondition {
                path: PathBuf::from(name),
                title: format!("This project already has a {name}"),
                problem: format!(
                    "{} exists. `q2 use brand` will not overwrite it. Remove or rename \
                     it first, or pass --force to keep it and (re)declare it in {}.",
                    root.join(name).display(),
                    config.filename
                ),
            })
            .collect()
    };

    Ok(ResolvedPlan {
        plan: FilePlan::new(root.clone(), root.display().to_string(), files, req.dry_run)
            .with_preconditions(preconditions)
            .with_edits(vec![declaration_edit]),
        warnings: Vec::new(),
    })
}

/// Produce the brand file(s) to write.
fn resolve_brand_files(req: &BrandRequest) -> Result<Vec<PlannedFile>, CommandFailure> {
    match &req.target {
        None => Ok(vec![PlannedFile {
            path: PathBuf::from(DEFAULT_BRAND_PATH),
            content: FileContent::Text(quarto_project_create::starter_brand_yml().to_string()),
        }]),
        Some(target) => Err(CommandFailure::new(
            "Brand sources are not supported yet",
            format!(
                "`q2 use brand {target}` will fetch a brand from a local path or a \
                 remote source, but that is not implemented yet. Run `q2 use brand` \
                 with no target to scaffold a starter brand you can edit."
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn request() -> BrandRequest {
        BrandRequest {
            target: None,
            dry_run: false,
            force: false,
            trust: false,
            no_prompt: true,
        }
    }

    fn project(config: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("_quarto.yml"), config).unwrap();
        dir
    }

    #[test]
    fn missing_project_is_refused_with_a_next_step() {
        let dir = TempDir::new().unwrap();
        let err = resolve(&request(), dir.path()).unwrap_err();
        let text = err.0.to_text(None);
        assert!(text.contains("_quarto.yml"));
        assert!(text.contains("q2 create"));
    }

    #[test]
    fn plan_writes_brand_and_declares_it() {
        let dir = project("project:\n  type: website\n");
        let resolved = resolve(&request(), dir.path()).unwrap();

        assert_eq!(resolved.plan.files.len(), 1);
        assert_eq!(resolved.plan.files[0].path, PathBuf::from("_brand.yml"));
        match &resolved.plan.edits[0] {
            PlannedEdit::AppendBlock { path, block } => {
                assert_eq!(path, &PathBuf::from("_quarto.yml"));
                assert!(block.contains("brand: _brand.yml"), "got: {block}");
            }
            other => panic!("expected an AppendBlock edit, got {other:?}"),
        }
    }

    #[test]
    fn plan_targets_the_config_spelling_that_exists() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("_quarto.yaml"), "project:\n").unwrap();
        let resolved = resolve(&request(), dir.path()).unwrap();
        match &resolved.plan.edits[0] {
            PlannedEdit::AppendBlock { path, .. } => {
                assert_eq!(path, &PathBuf::from("_quarto.yaml"))
            }
            other => panic!("expected an AppendBlock edit, got {other:?}"),
        }
    }

    #[test]
    fn existing_declaration_refuses_and_names_it() {
        let dir = project("project:\n  type: website\nbrand: other.yml\n");
        let err = resolve(&request(), dir.path()).unwrap_err();
        let text = err.0.to_text(None);
        assert!(text.contains("other.yml"), "got: {text}");
        assert!(text.contains("--force"), "the error should offer a way out");
    }

    #[test]
    fn force_repoints_a_top_level_path_declaration_in_place() {
        let dir = project("project:\n  type: website\nbrand: other.yml\n");
        let resolved = resolve(
            &BrandRequest {
                force: true,
                ..request()
            },
            dir.path(),
        )
        .unwrap();

        match &resolved.plan.edits[0] {
            PlannedEdit::ReplaceRange {
                replacement,
                expected,
                ..
            } => {
                assert_eq!(replacement, "_brand.yml");
                assert_eq!(expected, "other.yml");
            }
            other => panic!("expected a ReplaceRange edit, got {other:?}"),
        }
    }

    #[test]
    fn force_refuses_a_format_scoped_declaration() {
        // Appending a project-level brand would leave the format-scoped
        // one winning for that format — a silently wrong result.
        let dir = project("project:\n  type: website\nformat:\n  html:\n    brand: o.yml\n");
        let err = resolve(
            &BrandRequest {
                force: true,
                ..request()
            },
            dir.path(),
        )
        .unwrap_err();
        let text = err.0.to_text(None);
        assert!(text.contains("format.html.brand"), "got: {text}");
        assert!(text.contains("override"), "got: {text}");
    }

    #[test]
    fn force_refuses_an_inline_brand_block() {
        let dir = project("project:\n  type: website\nbrand:\n  color:\n    primary: red\n");
        let err = resolve(
            &BrandRequest {
                force: true,
                ..request()
            },
            dir.path(),
        )
        .unwrap_err();
        assert!(err.0.to_text(None).contains("inline brand block"));
    }

    #[test]
    fn force_drops_the_existing_file_preconditions() {
        let dir = project("project:\n  type: website\n");
        let plain = resolve(&request(), dir.path()).unwrap();
        assert_eq!(plain.plan.preconditions.len(), 2);

        let forced = resolve(
            &BrandRequest {
                force: true,
                ..request()
            },
            dir.path(),
        )
        .unwrap();
        assert!(forced.plan.preconditions.is_empty());
    }

    #[test]
    fn a_target_is_rejected_until_fetching_lands() {
        let dir = project("project:\n  type: website\n");
        let err = resolve(
            &BrandRequest {
                target: Some("org/repo".into()),
                ..request()
            },
            dir.path(),
        )
        .unwrap_err();
        assert!(err.0.to_text(None).contains("not implemented"));
    }
}
