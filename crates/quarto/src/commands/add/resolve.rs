//! Turning a `q2 add` invocation into an install plan (bd-rfz4e3bw).
//!
//! The order of the steps is the safety argument:
//!
//! 1. **Choose the install root** and resolve the target. Pure; a bad
//!    target is refused without touching the network.
//! 2. **Trust gate** for a remote source — before any download.
//! 3. **Fetch and stage** into caller-owned scratch space.
//! 4. **Validate** every staged extension and its destination name.
//! 5. **Compare** each against what is installed, and refuse to plan
//!    over local edits without `--force`.
//!
//! Nothing here writes into the user's project; [`super::install`]
//! does, once the plan has been shown and confirmed.

use std::path::{Path, PathBuf};

use quarto_core::extension::read_extension;
use quarto_source_fetch::{ExtractLimits, RemoteTarget, SourceFetch, Target, fetch_into};
use quarto_system_runtime::NativeRuntime;

use crate::commands::common::plan::CommandFailure;
use crate::commands::common::prompter::Prompter;
use crate::commands::use_cmd::find_project_config;

use super::record::{EditKind, InstalledState, SourceKind, SourceRecord};
use super::stage::{
    EXTENSIONS_DIR, locate_extensions_dir, read_staged, type_summary, validate_segment,
};
use super::version::{Action, Change, compare};

pub struct AddRequest {
    pub target: String,
    /// Install root override (`--dir`), relative to cwd.
    pub dir: Option<PathBuf>,
    pub dry_run: bool,
    /// Replace an installed extension that has local edits, and
    /// reinstall one that reports No Change.
    pub force: bool,
    /// Waives the remote-source trust prompt. Distinct from `force`
    /// for the reason `q2 use brand` gives: overwriting your own files
    /// and running someone else's code are different risks.
    pub trust: bool,
}

/// Everything `resolve` needs from the outside world, so it stays
/// testable without a network or a terminal.
pub struct ResolveContext<'a> {
    pub cwd: &'a Path,
    /// Scratch space for downloads and extraction. Must outlive the
    /// plan: planned extensions are copied from paths inside it.
    pub work_dir: &'a Path,
    pub fetcher: &'a dyn SourceFetch,
    /// `None` when the command cannot prompt (`--json`, `--no-prompt`,
    /// CI, or no terminal). A remote source then requires `--trust`.
    pub prompter: Option<&'a mut dyn Prompter>,
    pub limits: ExtractLimits,
}

/// Where a plan's extensions came from, for the install record.
#[derive(Debug, Clone)]
pub struct Provenance {
    pub source: String,
    pub kind: SourceKind,
    pub git_ref: Option<String>,
}

#[derive(Debug)]
pub struct AddPlan {
    /// Absolute install root; `_extensions/` lives directly under it.
    pub root: PathBuf,
    /// The root as shown to the user.
    pub root_display: String,
    pub extensions: Vec<PlannedExtension>,
    pub provenance: Provenance,
    pub learn_more: Option<String>,
    pub dry_run: bool,
    /// Whether this run creates `_extensions/.gitattributes` (see
    /// [`super::install::GITATTRIBUTES_FILENAME`]): true when the plan
    /// writes an extension and the file does not exist yet. Under
    /// `--dry-run`, whether it *would*.
    pub writes_gitattributes: bool,
}

impl AddPlan {
    /// Whether executing the plan writes anything.
    pub fn writes_anything(&self) -> bool {
        self.extensions.iter().any(|e| e.writes)
    }
}

#[derive(Debug)]
pub struct PlannedExtension {
    /// The name shown in the table: the title, else the id.
    pub display_name: String,
    /// The id it is installed under (`org/name` or `name`).
    pub id: String,
    /// The staged extension directory to copy from.
    pub source_dir: PathBuf,
    /// Destination relative to the root: `_extensions/[<owner>/]<name>`.
    pub dest: PathBuf,
    pub change: Change,
    pub types: String,
    pub installed: InstalledState,
    /// The installed copy's record, when it could be read. Its keys that
    /// q2 does not define are written into the new record (GH #1175).
    pub prior_record: Option<SourceRecord>,
    /// Whether this extension is (re)installed. False for No Change
    /// unless `--force` was given.
    pub writes: bool,
}

/// Resolve a `q2 add` invocation into an install plan.
pub fn resolve(req: &AddRequest, ctx: &mut ResolveContext) -> Result<AddPlan, CommandFailure> {
    let (root, root_display) = install_root(req, ctx.cwd)?;

    let target_input = req.target.as_str();
    let target = quarto_source_fetch::resolve_target(target_input).map_err(|e| {
        CommandFailure::new(
            format!("Cannot use {target_input:?} as an extension source"),
            format!(
                "{e}\n\nA source can be a local folder or archive, a GitHub \
                 `<org>/<repo>[/<subdir>][@<ref>]`, or the URL of a .tar.gz or .zip archive."
            ),
        )
    })?;

    let (provenance, owner, learn_more) = match &target {
        Target::Local(path) => (
            Provenance {
                source: target_input.to_string(),
                kind: if path.is_dir() {
                    SourceKind::LocalDir
                } else {
                    SourceKind::LocalArchive
                },
                git_ref: None,
            },
            None,
            None,
        ),
        Target::Remote(remote) => {
            confirm_trust(remote, req, ctx)?;
            let is_url =
                target_input.starts_with("http://") || target_input.starts_with("https://");
            let provenance = Provenance {
                source: target_input.to_string(),
                kind: if is_url {
                    SourceKind::Url
                } else {
                    SourceKind::Github
                },
                git_ref: if is_url {
                    None
                } else {
                    // Same split `resolve_target` makes: the last `@`.
                    target_input.rsplit_once('@').map(|(_, r)| r.to_string())
                },
            };
            (provenance, remote.owner.clone(), remote.learn_more.clone())
        }
    };
    if let Some(owner) = &owner {
        validate_segment(owner, "owner")?;
    }

    let fetched = fetch_into(&target, ctx.work_dir, ctx.fetcher, &ctx.limits).map_err(|e| {
        CommandFailure::new(
            format!("Could not get an extension from {target_input}"),
            e.to_string(),
        )
    })?;
    let staged = read_staged(&locate_extensions_dir(&fetched.root), target_input)?;
    refuse_skipped_links(&staged, &fetched.skipped_links, target_input)?;

    let mut extensions: Vec<PlannedExtension> = Vec::with_capacity(staged.len());
    for ext in &staged {
        validate_segment(&ext.id.name, "name")?;
        // Q1 installs under the *source's* owner and drops any
        // organization folder the source itself used
        // (`copyExtensions`, `install.ts:321`).
        let mut dest = PathBuf::from(EXTENSIONS_DIR);
        if let Some(owner) = &owner {
            dest.push(owner);
        }
        dest.push(&ext.id.name);
        let id = match &owner {
            Some(owner) => format!("{owner}/{}", ext.id.name),
            None => ext.id.name.clone(),
        };
        if extensions.iter().any(|e| e.dest == dest) {
            return Err(CommandFailure::new(
                format!(
                    "{target_input} contains two extensions named {:?}",
                    ext.id.name
                ),
                format!(
                    "Both would install to {}. Nothing was written.",
                    dest.display()
                ),
            ));
        }

        let installed_dir = root.join(&dest);
        let installed_version = installed_version(&installed_dir);
        let change = compare(
            ext.version.as_deref(),
            installed_version.as_ref().map(Option::as_deref),
        );
        let inspection = super::record::inspect(&installed_dir)?;
        let writes = change.action != Action::NoChange || req.force;

        extensions.push(PlannedExtension {
            display_name: ext.title.clone().unwrap_or_else(|| ext.id.to_string()),
            id,
            source_dir: ext.path.clone(),
            dest,
            change,
            types: type_summary(ext),
            installed: inspection.state,
            prior_record: inspection.record,
            writes,
        });
    }

    refuse_local_edits(&extensions, req)?;

    let writes_gitattributes = extensions.iter().any(|e| e.writes)
        && std::fs::symlink_metadata(
            root.join(EXTENSIONS_DIR)
                .join(super::install::GITATTRIBUTES_FILENAME),
        )
        .is_err();

    Ok(AddPlan {
        root,
        root_display,
        extensions,
        provenance,
        learn_more,
        dry_run: req.dry_run,
        writes_gitattributes,
    })
}

/// The project root when cwd is inside a project, else cwd; `--dir`
/// overrides both. Q1 uses cwd unless the user answers "Install
/// extension into project?" — a question whose right answer is almost
/// always yes, so q2 does not ask it.
fn install_root(req: &AddRequest, cwd: &Path) -> Result<(PathBuf, String), CommandFailure> {
    if let Some(dir) = &req.dir {
        let abs = cwd.join(dir);
        if !abs.is_dir() {
            return Err(CommandFailure::new(
                format!("--dir {} is not a directory", dir.display()),
                "Create it first, or point --dir at an existing folder.",
            ));
        }
        return Ok((abs, dir.display().to_string()));
    }
    if let Some((root, _config)) = find_project_config(cwd) {
        let display = if root == cwd {
            ".".to_string()
        } else {
            root.display().to_string()
        };
        return Ok((root, display));
    }
    Ok((cwd.to_path_buf(), ".".to_string()))
}

/// What the installed copy's version says, in [`compare`]'s terms:
/// `None` when nothing is installed, `Some(None)` when something is but
/// it has no readable version (including a manifest that no longer
/// loads — that copy is about to be replaced either way).
fn installed_version(dir: &Path) -> Option<Option<String>> {
    if !dir.exists() {
        return None;
    }
    let manifest = dir.join("_extension.yml");
    Some(
        read_extension(&manifest, &NativeRuntime::new())
            .ok()
            .and_then(|ext| ext.version),
    )
}

/// Refuse a source whose extensions needed an archive link.
///
/// Extraction never creates links (`quarto-source-fetch`'s
/// `ExtractSink::link`). A link elsewhere in the archive is harmless —
/// product-doc-theme's `example/_extensions` link is the motivating
/// case — but one inside an extension means that extension would
/// install with files missing, and fail later in a way that never
/// mentions a link.
fn refuse_skipped_links(
    staged: &[quarto_core::extension::Extension],
    skipped: &[PathBuf],
    target_label: &str,
) -> Result<(), CommandFailure> {
    let mut inside: Vec<String> = Vec::new();
    for ext in staged {
        for link in skipped {
            if let Ok(rel) = link.strip_prefix(&ext.path) {
                inside.push(format!("{}: {}", ext.id, rel.display()));
            }
        }
    }
    if inside.is_empty() {
        return Ok(());
    }
    Err(CommandFailure::new(
        format!("{target_label} contains an extension that uses symbolic links"),
        format!(
            "Links are never copied out of a downloaded archive, so these would be missing \
             from the installed extension:\n\n  {}\n\nNothing was written. Replace the links \
             with real files in the extension's source.",
            inside.join("\n  ")
        ),
    ))
}

/// Refuse to replace locally edited extensions without `--force`.
fn refuse_local_edits(
    extensions: &[PlannedExtension],
    req: &AddRequest,
) -> Result<(), CommandFailure> {
    if req.force {
        return Ok(());
    }
    let mut lines = Vec::new();
    let mut line_endings = false;
    let mut newer_record = false;
    for ext in extensions.iter().filter(|e| e.writes) {
        if let InstalledState::Modified(edits) = &ext.installed {
            lines.push(format!("{}:", ext.dest.display()));
            for edit in edits {
                lines.push(format!("  {} ({})", edit.path, edit.kind.describe()));
                match edit.kind {
                    EditKind::LineEndings => line_endings = true,
                    EditKind::NewerRecord(_) => newer_record = true,
                    _ => {}
                }
            }
        }
    }
    if lines.is_empty() {
        return Ok(());
    }
    let mut hints = String::new();
    if line_endings {
        hints.push_str(
            "\n\nFiles marked \"line endings changed\" differ from the record only in CRLF \
             vs LF line endings. This is usually git's core.autocrlf setting converting them \
             on checkout. To stop it, add the line `* -text` to `_extensions/.gitattributes` \
             (`q2 add` creates that file when it is missing) and check the files out again.",
        );
    }
    if newer_record {
        hints.push_str(
            "\n\nAn install record from a newer version of q2 can't be checked by this one. \
             Upgrade q2, or use --force to replace the extension and its record.",
        );
    }
    Err(CommandFailure::new(
        "Installed extensions have local changes",
        format!(
            "Installing would replace files that changed since `q2 add` installed them:\n\n{}\n\n\
             Nothing was written. Re-run with --force to replace them.{hints}",
            lines.join("\n")
        ),
    ))
}

/// Ask before fetching someone else's extension.
///
/// Same posture as `q2 use brand`'s gate: the default answer is **no**,
/// and when the command cannot ask it refuses rather than proceeding.
/// Q1 defaults to yes and installs silently under `--no-prompt`; that
/// is a deliberate deviation, because an extension runs Lua (and can
/// run engines) every time a document using it renders.
fn confirm_trust(
    remote: &RemoteTarget,
    req: &AddRequest,
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
        // A dry run downloads too, so it needs the same consent; say so
        // in the flags the caller should actually re-run with.
        let rerun = if req.dry_run {
            "Re-run with --dry-run --trust to preview, confirming you trust the authors of \
             this extension."
        } else {
            "Re-run with --trust to confirm you trust the authors of this extension."
        };
        return Err(CommandFailure::new(
            format!(
                "Refusing to download {} without confirmation",
                remote.description
            ),
            format!(
                "Quarto extensions may execute code when documents are rendered. There is no \
                 terminal to ask on, so nothing was fetched.\n\n\
                 {rerun}{learn_more}"
            ),
        ));
    };

    // Q1's preamble (`install.ts:124-126`), shown before the question.
    eprintln!(
        "Quarto extensions may execute code when documents are rendered. If you do not trust \
         the authors of the extension, we recommend that you do not install or use it."
    );
    if let Some(url) = &remote.learn_more {
        eprintln!("About this source: {url}");
    }
    let trusted = prompter.confirm(
        &format!("Do you trust the authors of {}?", remote.description),
        false,
    )?;
    if trusted {
        Ok(())
    } else {
        Err(CommandFailure::new(
            "Installation canceled",
            "You declined to trust the source. Nothing was fetched or written.",
        ))
    }
}

/// Ask whether to go ahead with a plan that writes something.
///
/// Unlike the trust gate, this defaults to **yes**, as in Q1: the user
/// has already chosen the source and can read exactly what will change
/// in the table above the question. With no prompter (`--no-prompt`,
/// `--json`), the plan proceeds — the table was still printed.
pub fn confirm_install(
    plan: &AddPlan,
    prompter: Option<&mut dyn Prompter>,
) -> Result<(), CommandFailure> {
    if plan.dry_run || !plan.writes_anything() {
        return Ok(());
    }
    let Some(prompter) = prompter else {
        return Ok(());
    };
    if prompter.confirm("Would you like to continue?", true)? {
        Ok(())
    } else {
        Err(CommandFailure::new(
            "Installation canceled",
            "Nothing was written.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::common::prompter::PromptItem;
    use tempfile::TempDir;

    const MANIFEST_V1: &str =
        "title: Demo\nversion: 1.0.0\ncontributes:\n  shortcodes:\n    - demo.lua\n";

    /// A fetcher that must never be called — using it asserts that a
    /// gate fired before any network traffic.
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

    struct ScriptedPrompter {
        answer: bool,
        prompts: Vec<(String, bool)>,
    }
    impl Prompter for ScriptedPrompter {
        fn select(&mut self, _: &str, _: &[PromptItem]) -> Result<usize, CommandFailure> {
            unreachable!("q2 add shows no selection prompt")
        }
        fn input(&mut self, _: &str, _: Option<&str>) -> Result<String, CommandFailure> {
            unreachable!("q2 add shows no text prompt")
        }
        fn confirm(&mut self, prompt: &str, default: bool) -> Result<bool, CommandFailure> {
            self.prompts.push((prompt.to_string(), default));
            Ok(self.answer)
        }
    }

    fn request(target: &str) -> AddRequest {
        AddRequest {
            target: target.to_string(),
            dir: None,
            dry_run: false,
            force: false,
            trust: false,
        }
    }

    fn tree(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (name, content) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        dir
    }

    fn resolve_with(
        req: &AddRequest,
        cwd: &Path,
        prompter: Option<&mut dyn Prompter>,
    ) -> Result<AddPlan, CommandFailure> {
        let work = TempDir::new().unwrap();
        let mut ctx = ResolveContext {
            cwd,
            work_dir: work.path(),
            fetcher: &NeverFetch,
            // Reborrow to the context's shorter lifetime.
            prompter: prompter.map(|p| &mut *p as &mut dyn Prompter),
            limits: ExtractLimits::default(),
        };
        resolve(req, &mut ctx)
    }

    #[test]
    fn a_remote_source_without_trust_or_a_prompter_is_refused_before_any_fetch() {
        let cwd = TempDir::new().unwrap();
        let err = resolve_with(&request("org/repo"), cwd.path(), None).unwrap_err();
        let text = err.0.to_text(None);
        assert!(text.contains("--trust"), "{text}");
        assert!(text.contains("execute code"), "{text}");
    }

    #[test]
    fn the_trust_refusal_hint_matches_the_mode() {
        let cwd = TempDir::new().unwrap();
        let err = resolve_with(&request("org/repo"), cwd.path(), None).unwrap_err();
        let text = err.0.to_text(None);
        assert!(!text.contains("--dry-run"), "{text}");

        let req = AddRequest {
            dry_run: true,
            ..request("org/repo")
        };
        let err = resolve_with(&req, cwd.path(), None).unwrap_err();
        let text = err.0.to_text(None);
        assert!(text.contains("--dry-run --trust"), "{text}");
    }

    #[test]
    fn the_trust_prompt_defaults_to_no_and_declining_fetches_nothing() {
        let cwd = TempDir::new().unwrap();
        let mut prompter = ScriptedPrompter {
            answer: false,
            prompts: Vec::new(),
        };
        let err = resolve_with(&request("org/repo"), cwd.path(), Some(&mut prompter)).unwrap_err();
        assert!(err.0.to_text(None).contains("canceled"));
        assert_eq!(prompter.prompts.len(), 1);
        assert!(
            !prompter.prompts[0].1,
            "the trust prompt must default to no"
        );
    }

    #[test]
    fn a_local_source_needs_no_trust_and_plans_an_install() {
        let src = tree(&[("_extensions/demo/_extension.yml", MANIFEST_V1)]);
        let cwd = TempDir::new().unwrap();
        let req = request(&src.path().to_string_lossy());
        let plan = resolve_with(&req, cwd.path(), None).unwrap();
        assert_eq!(plan.extensions.len(), 1);
        let ext = &plan.extensions[0];
        assert_eq!(ext.dest, Path::new("_extensions").join("demo"));
        assert_eq!(ext.change.action, Action::Install);
        assert_eq!(ext.display_name, "Demo");
        assert_eq!(ext.types, "(shortcode)");
        assert!(ext.writes);
        assert_eq!(plan.provenance.kind, SourceKind::LocalDir);
    }

    #[test]
    fn the_project_root_is_the_default_install_root_and_dir_overrides_it() {
        let src = tree(&[("_extensions/demo/_extension.yml", MANIFEST_V1)]);
        let project = tree(&[
            ("_quarto.yml", "project:\n  type: website\n"),
            ("sub/x.qmd", ""),
        ]);
        let req = request(&src.path().to_string_lossy());

        let plan = resolve_with(&req, &project.path().join("sub"), None).unwrap();
        assert_eq!(plan.root, project.path());

        let mut req = req;
        req.dir = Some(PathBuf::from("sub"));
        let plan = resolve_with(&req, project.path(), None).unwrap();
        assert_eq!(plan.root, project.path().join("sub"));

        req.dir = Some(PathBuf::from("does-not-exist"));
        assert!(resolve_with(&req, project.path(), None).is_err());
    }

    #[test]
    fn the_install_confirmation_defaults_to_yes_and_declining_cancels() {
        let src = tree(&[("_extensions/demo/_extension.yml", MANIFEST_V1)]);
        let cwd = TempDir::new().unwrap();
        let plan = resolve_with(&request(&src.path().to_string_lossy()), cwd.path(), None).unwrap();

        let mut yes = ScriptedPrompter {
            answer: true,
            prompts: Vec::new(),
        };
        confirm_install(&plan, Some(&mut yes)).unwrap();
        assert!(yes.prompts[0].1, "the install confirmation defaults to yes");

        let mut no = ScriptedPrompter {
            answer: false,
            prompts: Vec::new(),
        };
        assert!(confirm_install(&plan, Some(&mut no)).is_err());

        confirm_install(&plan, None).expect("no prompter: proceed");
    }

    #[test]
    fn a_plan_that_writes_nothing_asks_nothing() {
        let src = tree(&[("_extensions/demo/_extension.yml", MANIFEST_V1)]);
        let cwd = tree(&[("_extensions/demo/_extension.yml", MANIFEST_V1)]);
        let plan = resolve_with(&request(&src.path().to_string_lossy()), cwd.path(), None).unwrap();
        assert_eq!(plan.extensions[0].change.action, Action::NoChange);
        assert!(!plan.writes_anything());
        let mut p = ScriptedPrompter {
            answer: false,
            prompts: Vec::new(),
        };
        confirm_install(&plan, Some(&mut p)).unwrap();
        assert!(p.prompts.is_empty());
    }

    /// Serves one canned `.tar.gz` at one URL, 404 elsewhere.
    struct OneArchive {
        url: String,
        body: Vec<u8>,
        requested: std::cell::RefCell<Vec<String>>,
    }
    impl SourceFetch for OneArchive {
        fn get_to_file(
            &self,
            url: &str,
            dest: &Path,
            _limits: &ExtractLimits,
        ) -> Result<u16, quarto_source_fetch::FetchError> {
            self.requested.borrow_mut().push(url.to_string());
            if url == self.url {
                std::fs::write(dest, &self.body).unwrap();
                Ok(200)
            } else {
                Ok(404)
            }
        }
    }

    fn demo_tar_gz(root: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let gz = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::default());
            let mut builder = tar::Builder::new(gz);
            for (name, content) in [
                ("_extensions/quarto-ext/demo/_extension.yml", MANIFEST_V1),
                ("_extensions/quarto-ext/demo/demo.lua", "return {}\n"),
            ] {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Regular);
                header.set_mode(0o644);
                header.set_size(content.len() as u64);
                header.set_path(format!("{root}/{name}")).unwrap();
                header.set_cksum();
                builder.append(&header, content.as_bytes()).unwrap();
            }
            builder.into_inner().unwrap().finish().unwrap();
        }
        buf
    }

    #[test]
    fn github_shorthand_installs_under_the_org_and_records_the_ref() {
        let fetcher = OneArchive {
            url: "https://github.com/acme/demo-ext/archive/refs/tags/v1.0.0.tar.gz".into(),
            body: demo_tar_gz("demo-ext-1.0.0"),
            requested: Default::default(),
        };
        let cwd = TempDir::new().unwrap();
        let work = TempDir::new().unwrap();
        let mut req = request("acme/demo-ext@v1.0.0");
        req.trust = true;
        let mut ctx = ResolveContext {
            cwd: cwd.path(),
            work_dir: work.path(),
            fetcher: &fetcher,
            prompter: None,
            limits: ExtractLimits::default(),
        };
        let plan = resolve(&req, &mut ctx).unwrap();

        let ext = &plan.extensions[0];
        // The source's own `quarto-ext/` folder is replaced by the
        // GitHub owner, as in Q1's `copyExtensions`.
        assert_eq!(ext.dest, Path::new("_extensions").join("acme").join("demo"));
        assert_eq!(ext.id, "acme/demo");
        assert_eq!(plan.provenance.kind, SourceKind::Github);
        assert_eq!(plan.provenance.git_ref.as_deref(), Some("v1.0.0"));
        assert_eq!(
            plan.learn_more.as_deref(),
            Some("https://github.com/acme/demo-ext")
        );
        assert_eq!(
            fetcher.requested.borrow().len(),
            1,
            "the tag URL is tried first"
        );
    }
}
