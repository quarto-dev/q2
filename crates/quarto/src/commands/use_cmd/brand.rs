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
//! **Everything local is checked before anything remote happens.** The
//! gates run cheapest-first and all of them run *before* any network
//! traffic — a project that will be refused is refused without
//! downloading anything, and without asking the user to trust anything.

use std::path::{Path, PathBuf};

use quarto_source_fetch::{ExtractLimits, RemoteTarget, SourceFetch, Target, fetch_into};

use crate::commands::common::plan::{
    CommandFailure, FileContent, FilePlan, PlannedEdit, PlannedFile, Precondition, ResolvedPlan,
};
use crate::commands::common::prompter::Prompter;

use super::config::{
    BrandDeclSite, BrandDeclaration, CONFIG_FILENAMES, ProjectConfigFile, brand_declaration_block,
    find_project_config,
};
use super::source::{Destination, SourceBrand, read_source_brand};

/// The brand file spellings we refuse to write over, and probe for.
const BRAND_FILENAMES: [&str; 2] = ["_brand.yml", "_brand.yaml"];

pub struct BrandRequest {
    pub target: Option<String>,
    pub dry_run: bool,
    pub force: bool,
    /// Waives the remote-source trust prompt. Deliberately distinct
    /// from `force`: clobbering your own file and executing someone
    /// else's fetched content are different risks, and one flag
    /// authorizing both is how a convenience becomes a supply-chain
    /// problem.
    pub trust: bool,
}

/// Everything `resolve` needs from the outside world, so the resolution
/// logic stays testable without a network or a terminal.
pub struct ResolveContext<'a> {
    pub cwd: &'a Path,
    /// Scratch space for downloads and extraction. Must outlive the
    /// returned plan: planned files reference paths inside it.
    pub work_dir: &'a Path,
    pub fetcher: &'a dyn SourceFetch,
    /// `None` when the command cannot prompt (`--json`, `--no-prompt`,
    /// CI, or no terminal). A remote source then requires `--trust`.
    pub prompter: Option<&'a mut dyn Prompter>,
    pub limits: ExtractLimits,
}

/// Resolve a `q2 use brand` invocation into an executable plan.
pub fn resolve(
    req: &BrandRequest,
    ctx: &mut ResolveContext,
) -> Result<ResolvedPlan, CommandFailure> {
    // ── Gate 1: a project must already exist ────────────────────────
    //
    // Deliberately not created for the user. `q2 use brand` adds a
    // brand *to a project*; conjuring a `_quarto.yml` would guess at
    // the project type, the title, and the output layout — decisions
    // `q2 create` exists to ask about.
    let Some((root, config_path)) = find_project_config(ctx.cwd) else {
        return Err(CommandFailure::new(
            "No Quarto project found",
            format!(
                "Looked for {} in {} and every parent directory. \
                 `q2 use brand` adds a brand to an existing project and will not \
                 create one. Run `q2 create project default .` first, or run this \
                 from inside a project.",
                CONFIG_FILENAMES.join(" or "),
                ctx.cwd.display()
            ),
        ));
    };

    // ── Gate 2: the config must be one we can safely edit ───────────
    let config = ProjectConfigFile::load(&config_path)?;
    let config_rel = PathBuf::from(&config.filename);

    // ── Gate 3: no brand file may already be present ────────────────
    //
    // Checked here eagerly, *and* as a plan precondition below. The
    // precondition alone would fire only after a download; a user whose
    // project already has a brand should not have to wait for a network
    // round trip — or be asked to trust a remote source — to be told no.
    if !req.force
        && let Some(existing) = BRAND_FILENAMES
            .iter()
            .map(|name| root.join(name))
            .find(|p| p.exists())
    {
        return Err(existing_brand_file_failure(&existing, &config.filename));
    }

    // ── Gate 4: no brand may already be declared ────────────────────
    let existing_declaration = config.brand_declaration();
    if let Some(decl) = &existing_declaration
        && !req.force
    {
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

    // ── Only now: fetch, which may prompt for trust ─────────────────
    let (files, destination) = resolve_brand_files(req, ctx)?;

    let declaration_edit = build_declaration_edit(
        &config,
        &config_rel,
        existing_declaration.as_ref(),
        destination.declared_path(),
    )?;

    // Re-checked at write time. Not redundant with gate 3: the gap
    // between resolving and writing spans a network round trip and
    // possibly a prompt, and a brand file appearing in that window must
    // not be clobbered.
    let preconditions = if req.force {
        Vec::new()
    } else {
        let mut paths: Vec<PathBuf> = BRAND_FILENAMES.iter().map(PathBuf::from).collect();
        if let Some(dir) = destination.directory() {
            paths.push(PathBuf::from(dir));
        }
        paths
            .into_iter()
            .map(|path| {
                let absolute = root.join(&path);
                Precondition {
                    title: format!("This project already has a {}", path.display()),
                    problem: format!(
                        "{} exists. `q2 use brand` will not overwrite it. Remove or rename \
                         it first, or pass --force to keep it and (re)declare it in {}.",
                        absolute.display(),
                        config.filename
                    ),
                    path,
                }
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

fn existing_brand_file_failure(existing: &Path, config_filename: &str) -> CommandFailure {
    let name = existing.file_name().map_or_else(
        || existing.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    CommandFailure::new(
        format!("This project already has a {name}"),
        format!(
            "{} exists. `q2 use brand` will not overwrite it. Remove or rename it \
             first, or pass --force to keep it and (re)declare it in {config_filename}.",
            existing.display()
        ),
    )
}

/// Decide how to write the `brand:` key, given what is already there.
fn build_declaration_edit(
    config: &ProjectConfigFile,
    config_rel: &Path,
    existing: Option<&BrandDeclaration>,
    declared_path: &str,
) -> Result<PlannedEdit, CommandFailure> {
    let Some(decl) = existing else {
        return Ok(PlannedEdit::AppendBlock {
            path: config_rel.to_path_buf(),
            block: brand_declaration_block(declared_path),
        });
    };

    // --force, and the declaration is a plain top-level path: repoint
    // it precisely. Appending instead would leave two top-level `brand:`
    // keys — a duplicate-key config whose meaning depends on parser
    // tie-breaking, which is corruption, not an override.
    //
    // `expected` is sliced straight out of the config text rather than
    // reused from `value_summary`. The two are equal today, but
    // `value_summary` is a *display* field — if it ever gained
    // truncation for readability, silently feeding it to the writer's
    // byte-exact guard would break the guard rather than the display.
    if decl.site == BrandDeclSite::TopLevel
        && let Some((start, end)) = decl.value_span
    {
        return Ok(PlannedEdit::ReplaceRange {
            path: config_rel.to_path_buf(),
            start,
            end,
            replacement: declared_path.to_string(),
            expected: config.text[start..end].to_string(),
        });
    }

    // --force, but the declaration is a shape we cannot rewrite in
    // place. Refusing is the only honest option: a format-scoped brand
    // would keep overriding the project-level one we just wrote for
    // that format, so succeeding here would leave the render unchanged
    // — the exact failure this command exists to prevent.
    Err(CommandFailure::new(
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
    ))
}

/// Produce the brand file(s) to write, and where they go.
fn resolve_brand_files(
    req: &BrandRequest,
    ctx: &mut ResolveContext,
) -> Result<(Vec<PlannedFile>, Destination), CommandFailure> {
    let Some(target_input) = &req.target else {
        return Ok((
            vec![PlannedFile {
                path: PathBuf::from("_brand.yml"),
                content: FileContent::Text(quarto_project_create::starter_brand_yml().to_string()),
            }],
            Destination::ProjectRoot,
        ));
    };

    let target = quarto_source_fetch::resolve_target(target_input).map_err(|e| {
        CommandFailure::new(
            format!("Cannot use {target_input:?} as a brand source"),
            format!(
                "{e}\n\nA source can be a local path, a GitHub \
                 `<org>/<repo>[/<subdir>][@<ref>]`, or the URL of a .tar.gz or \
                 .zip archive."
            ),
        )
    })?;

    if let Target::Remote(remote) = &target {
        confirm_trust(remote, req, ctx)?;
    }

    let source_dir = fetch_into(&target, ctx.work_dir, ctx.fetcher, &ctx.limits).map_err(|e| {
        CommandFailure::new(
            format!("Could not get a brand from {target_input}"),
            e.to_string(),
        )
    })?;

    let brand = read_source_brand(&source_dir, target_input)?;
    let destination = brand.destination();
    Ok((planned_files(&brand, destination), destination))
}

/// Map a validated source brand onto project-relative planned files.
fn planned_files(brand: &SourceBrand, destination: Destination) -> Vec<PlannedFile> {
    let prefix = destination.directory().map(PathBuf::from);
    let at = |rel: PathBuf| match &prefix {
        Some(dir) => dir.join(rel),
        None => rel,
    };
    let brand_dir = brand.brand_file.parent().unwrap_or(&brand.brand_file);

    // The brand file is always written as `_brand.yml`, whichever
    // spelling the source used, so the declared path is predictable.
    let mut files = vec![PlannedFile {
        path: at(PathBuf::from("_brand.yml")),
        content: FileContent::CopyFrom(brand.brand_file.clone()),
    }];
    files.extend(brand.assets.iter().map(|asset| PlannedFile {
        path: at(asset.clone()),
        content: FileContent::CopyFrom(brand_dir.join(asset)),
    }));
    files
}

/// Ask before fetching and extracting someone else's content.
///
/// Ported from Quarto 1's `isTrusted` (`brand.ts:602-621`), with two
/// changes. The default answer is **no**, so the safe outcome is what a
/// distracted user gets by pressing Enter. And when the command cannot
/// ask — `--json`, `--no-prompt`, CI, no terminal — it **refuses**
/// rather than proceeding: failing closed is the only safe default for
/// "should I run content from this URL?" on a machine that cannot
/// answer.
fn confirm_trust(
    remote: &RemoteTarget,
    req: &BrandRequest,
    ctx: &mut ResolveContext,
) -> Result<(), CommandFailure> {
    if req.trust {
        return Ok(());
    }

    let learn_more = remote
        .learn_more
        .as_ref()
        .map(|url| format!("\n\nAbout this source: {url}"))
        .unwrap_or_default();

    let Some(prompter) = ctx.prompter.as_deref_mut() else {
        return Err(CommandFailure::new(
            format!(
                "Refusing to download {} without confirmation",
                remote.description
            ),
            format!(
                "Downloading a brand runs someone else's content through this machine. \
                 There is no terminal to ask on, so nothing was fetched.\n\n\
                 Re-run with --trust to confirm you trust this source.{learn_more}"
            ),
        ));
    };

    if let Some(url) = &remote.learn_more {
        eprintln!("About this source: {url}");
    }
    let trusted = prompter.confirm(
        &format!("Download and use the brand from {}?", remote.description),
        false,
    )?;

    if trusted {
        Ok(())
    } else {
        Err(CommandFailure::new(
            "Brand not installed",
            "You declined the download. Nothing was written.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::common::prompter::PromptItem;
    use tempfile::TempDir;

    fn request() -> BrandRequest {
        BrandRequest {
            target: None,
            dry_run: false,
            force: false,
            trust: false,
        }
    }

    /// A fetcher that must never be called — its use in a test asserts
    /// that a gate fired before any network traffic.
    struct NeverFetch;
    impl SourceFetch for NeverFetch {
        fn get_to_file(
            &self,
            url: &str,
            _dest: &Path,
            _limits: &ExtractLimits,
        ) -> Result<u16, quarto_source_fetch::FetchError> {
            panic!("no request should have been made, but got {url}");
        }
    }

    /// Answers every confirmation with a fixed reply, recording prompts.
    struct ScriptedPrompter {
        answer: bool,
        prompts: Vec<String>,
    }
    impl Prompter for ScriptedPrompter {
        fn select(
            &mut self,
            _prompt: &str,
            _items: &[PromptItem],
        ) -> Result<usize, CommandFailure> {
            unreachable!("q2 use brand shows no selection prompt")
        }
        fn input(
            &mut self,
            _prompt: &str,
            _default: Option<&str>,
        ) -> Result<String, CommandFailure> {
            unreachable!("q2 use brand shows no text prompt")
        }
        fn confirm(&mut self, prompt: &str, _default: bool) -> Result<bool, CommandFailure> {
            self.prompts.push(prompt.to_string());
            Ok(self.answer)
        }
    }

    fn project(config: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("_quarto.yml"), config).unwrap();
        dir
    }

    /// Resolve with no prompter and a fetcher that panics on use.
    fn resolve_offline(req: &BrandRequest, cwd: &Path) -> Result<ResolvedPlan, CommandFailure> {
        let work = TempDir::new().unwrap();
        let mut ctx = ResolveContext {
            cwd,
            work_dir: work.path(),
            fetcher: &NeverFetch,
            prompter: None,
            limits: ExtractLimits::default(),
        };
        resolve(req, &mut ctx)
    }

    #[test]
    fn missing_project_is_refused_with_a_next_step() {
        let dir = TempDir::new().unwrap();
        let err = resolve_offline(&request(), dir.path()).unwrap_err();
        let text = err.0.to_text(None);
        assert!(text.contains("_quarto.yml"));
        assert!(text.contains("q2 create"));
    }

    #[test]
    fn plan_writes_brand_and_declares_it() {
        let dir = project("project:\n  type: website\n");
        let resolved = resolve_offline(&request(), dir.path()).unwrap();

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
        let resolved = resolve_offline(&request(), dir.path()).unwrap();
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
        let err = resolve_offline(&request(), dir.path()).unwrap_err();
        let text = err.0.to_text(None);
        assert!(text.contains("other.yml"), "got: {text}");
        assert!(text.contains("--force"), "the error should offer a way out");
    }

    #[test]
    fn force_repoints_a_top_level_path_declaration_in_place() {
        let dir = project("project:\n  type: website\nbrand: other.yml\n");
        let resolved = resolve_offline(
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
        let dir = project("project:\n  type: website\nformat:\n  html:\n    brand: o.yml\n");
        let err = resolve_offline(
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
        let err = resolve_offline(
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
    fn an_existing_brand_file_is_refused_before_any_fetch() {
        // `NeverFetch` panics if called, so this also proves the gate
        // fires before the network — and before a trust prompt.
        let dir = project("project:\n  type: website\n");
        std::fs::write(dir.path().join("_brand.yml"), "color:\n").unwrap();

        let err = resolve_offline(
            &BrandRequest {
                target: Some("org/repo".into()),
                ..request()
            },
            dir.path(),
        )
        .unwrap_err();
        assert!(err.0.to_text(None).contains("_brand.yml"));
    }

    #[test]
    fn a_remote_target_without_trust_or_a_prompter_is_refused_before_any_fetch() {
        let dir = project("project:\n  type: website\n");
        let err = resolve_offline(
            &BrandRequest {
                target: Some("org/repo".into()),
                ..request()
            },
            dir.path(),
        )
        .unwrap_err();
        let text = err.0.to_text(None);
        assert!(text.contains("--trust"), "got: {text}");
        assert!(text.contains("nothing was fetched"), "got: {text}");
    }

    #[test]
    fn declining_the_trust_prompt_fetches_nothing() {
        let dir = project("project:\n  type: website\n");
        let work = TempDir::new().unwrap();
        let mut prompter = ScriptedPrompter {
            answer: false,
            prompts: Vec::new(),
        };
        let mut ctx = ResolveContext {
            cwd: dir.path(),
            work_dir: work.path(),
            fetcher: &NeverFetch,
            prompter: Some(&mut prompter),
            limits: ExtractLimits::default(),
        };
        let err = resolve(
            &BrandRequest {
                target: Some("org/repo".into()),
                ..request()
            },
            &mut ctx,
        )
        .unwrap_err();

        assert!(err.0.to_text(None).contains("declined"));
        assert_eq!(prompter.prompts.len(), 1);
        assert!(
            prompter.prompts[0].contains("org/repo"),
            "the prompt should name the source: {:?}",
            prompter.prompts
        );
    }

    #[test]
    fn a_local_source_needs_no_trust_prompt() {
        let dir = project("project:\n  type: website\n");
        let src = TempDir::new().unwrap();
        std::fs::write(src.path().join("_brand.yml"), "color:\n  primary: red\n").unwrap();

        let resolved = resolve_offline(
            &BrandRequest {
                target: Some(src.path().to_string_lossy().into_owned()),
                ..request()
            },
            dir.path(),
        )
        .expect("a local directory is not a download");

        assert_eq!(resolved.plan.files.len(), 1);
        assert_eq!(resolved.plan.files[0].path, PathBuf::from("_brand.yml"));
    }

    #[test]
    fn a_local_source_with_assets_lands_in_the_brand_directory() {
        let dir = project("project:\n  type: website\n");
        let src = TempDir::new().unwrap();
        std::fs::write(src.path().join("_brand.yml"), "logo:\n  small: logo.png\n").unwrap();
        std::fs::write(src.path().join("logo.png"), "x").unwrap();

        let resolved = resolve_offline(
            &BrandRequest {
                target: Some(src.path().to_string_lossy().into_owned()),
                ..request()
            },
            dir.path(),
        )
        .unwrap();

        let paths: Vec<&PathBuf> = resolved.plan.files.iter().map(|f| &f.path).collect();
        assert_eq!(
            paths,
            [
                &PathBuf::from("_brand").join("_brand.yml"),
                &PathBuf::from("_brand").join("logo.png")
            ]
        );
        match &resolved.plan.edits[0] {
            PlannedEdit::AppendBlock { block, .. } => {
                assert!(block.contains("brand: _brand/_brand.yml"), "got: {block}");
            }
            other => panic!("expected an AppendBlock edit, got {other:?}"),
        }
        // The `_brand` directory joins the must-not-exist list.
        assert!(
            resolved
                .plan
                .preconditions
                .iter()
                .any(|p| p.path == *Path::new("_brand")),
            "a _brand/ destination must be guarded too"
        );
    }

    #[test]
    fn force_drops_the_existing_file_preconditions() {
        let dir = project("project:\n  type: website\n");
        let plain = resolve_offline(&request(), dir.path()).unwrap();
        assert_eq!(plain.plan.preconditions.len(), 2);

        let forced = resolve_offline(
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
    fn an_unrecognizable_target_explains_what_is_accepted() {
        let dir = project("project:\n  type: website\n");
        let err = resolve_offline(
            &BrandRequest {
                target: Some("not a source".into()),
                ..request()
            },
            dir.path(),
        )
        .unwrap_err();
        let text = err.0.to_text(None);
        assert!(text.contains("local path"), "got: {text}");
        assert!(text.contains("<org>/<repo>"), "got: {text}");
    }
}
