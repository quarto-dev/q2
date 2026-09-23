//! What a filesystem change means for `q2 preview --static`
//! (bd-sl79jjiq, plan § Watch policy).
//!
//! The watcher (`quarto_hub::watch::FileWatcher` with
//! `WatchFilter::All`) reports every path under the project root; this
//! module decides, purely from the path and a snapshot of what the last
//! render knew, whether to ignore it, re-render just that input
//! ([`Action::Subset`]), or re-render everything ([`Action::Full`]).
//! It never touches the filesystem beyond an existence check, so every
//! rule is a unit test.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// A snapshot of the last render, as far as classification needs it.
/// All paths are absolute.
#[derive(Debug, Clone)]
pub struct WatchContext {
    /// The project root (the input's directory for a single document).
    pub project_dir: PathBuf,
    /// Where the render writes. Equal to `project_dir` for non-website
    /// projects and single documents, in which case the outputs are
    /// excluded individually via [`Self::outputs`].
    pub output_dir: PathBuf,
    /// The project's render-list-filtered inputs (`ProjectContext.files`).
    pub inputs: BTreeSet<PathBuf>,
    /// Output files the last render wrote (`RenderReport::outputs`),
    /// so a render beside the sources never re-triggers itself.
    pub outputs: BTreeSet<PathBuf>,
}

/// What to do about a changed path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Not a source: generated, hidden, editor noise, or an output.
    Ignore,
    /// Re-render these inputs (plus whatever the dependency graph adds).
    Subset(BTreeSet<PathBuf>),
    /// Re-render the whole project.
    Full,
}

impl Action {
    /// Combine two pending actions: `Full` absorbs everything, subsets
    /// union, `Ignore` is the identity.
    pub fn merge(self, other: Action) -> Action {
        match (self, other) {
            (Action::Full, _) | (_, Action::Full) => Action::Full,
            (Action::Ignore, other) | (other, Action::Ignore) => other,
            (Action::Subset(mut a), Action::Subset(b)) => {
                a.extend(b);
                Action::Subset(a)
            }
        }
    }
}

/// Directory names Quarto or its engines own outright. A change inside
/// one is never a source edit.
const GENERATED_DIRS: &[&str] = &["node_modules", "__pycache__", "_freeze", "site_libs"];

/// Suffixes of engine sidecar directories (`<stem>_files/`,
/// `<stem>_cache/`). Matched on every component, so a nested sidecar
/// is caught wherever it sits.
const GENERATED_DIR_SUFFIXES: &[&str] = &["_files", "_cache"];

/// Extensions `ProjectContext::discover` admits as inputs.
const INPUT_EXTENSIONS: &[&str] = &["qmd", "md", "ipynb", "Rmd", "rmd"];

/// Classify one changed path against the last render's snapshot.
///
/// Rules, in order (each has a unit test below):
/// 1. Outside the project, under the output directory (when that is
///    a separate directory), an output the last render wrote, inside a
///    dot-directory or generated directory, or an editor temporary:
///    [`Action::Ignore`]. Dot-*files* are ignored too, except `.env*`,
///    which Quarto reads as config.
/// 2. A known input that still exists: [`Action::Subset`] of itself.
/// 3. Everything else — config, extensions, resources, partials, a
///    deleted or newly created input: [`Action::Full`]. The dependency
///    graph only knows navigation edges, not includes or resources, so
///    a full render is the answer that is always right.
pub fn classify(path: &Path, ctx: &WatchContext) -> Action {
    let Ok(rel) = path.strip_prefix(&ctx.project_dir) else {
        return Action::Ignore;
    };
    if rel.as_os_str().is_empty() {
        // A directory-level event on the root itself (macOS FSEvents
        // reports one when `_site/` or `.quarto/` is created).
        return Action::Ignore;
    }
    if ctx.output_dir != ctx.project_dir && path.starts_with(&ctx.output_dir) {
        return Action::Ignore;
    }
    if ctx.outputs.contains(path) {
        return Action::Ignore;
    }
    let components: Vec<&str> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    let count = components.len();
    for (i, name) in components.iter().enumerate() {
        let last = i + 1 == count;
        if name.starts_with('.') && !(last && name.starts_with(".env")) {
            return Action::Ignore;
        }
        if GENERATED_DIRS.contains(name) || GENERATED_DIR_SUFFIXES.iter().any(|s| name.ends_with(s))
        {
            return Action::Ignore;
        }
        if last && is_editor_temporary(name) {
            return Action::Ignore;
        }
    }
    if ctx.inputs.contains(path) {
        return if path.exists() {
            Action::Subset(BTreeSet::from([path.to_path_buf()]))
        } else {
            Action::Full
        };
    }
    Action::Full
}

/// Files editors leave beside what they save: Emacs/vim backups and
/// swap files, Emacs lock files (`.#name`, also caught by the dot
/// rule), and vim's write-probe `4913`.
fn is_editor_temporary(name: &str) -> bool {
    name.ends_with('~')
        || name.ends_with(".swp")
        || name.ends_with(".swx")
        || name.starts_with(".#")
        || name == "4913"
}

/// Remembers the content hash of every path it has been asked about,
/// so a filesystem event counts as a change only when the bytes differ.
///
/// Why this exists: the watcher reports *events*, not edits. On Linux
/// `notify`'s inotify backend subscribes to `OPEN`, so every file the
/// render itself reads comes back as an event — and a re-render that
/// re-triggers itself never stops (observed on PR #712's ubuntu leg:
/// an hour of back-to-back renders after one save). Editors that touch
/// metadata or rewrite identical bytes produce the same false positive
/// on every platform. Quarto 1 guards the same way, comparing md5s
/// against the last render (`project/serve/watch.ts:134-140`).
#[derive(Debug, Default)]
pub struct ContentTracker {
    hashes: BTreeMap<PathBuf, [u8; 32]>,
}

impl ContentTracker {
    /// Record the current content of `paths` without reporting a
    /// change. Unreadable paths (missing, a directory) are skipped.
    pub fn seed(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for path in paths {
            if let Some(hash) = hash_file(&path) {
                self.hashes.insert(path, hash);
            }
        }
    }

    /// True when `path`'s bytes differ from the last time it was seen,
    /// when it is a readable file seen for the first time, or when a
    /// tracked file is gone. False for a rewrite with identical bytes,
    /// for a mere open/read, for a directory, and for an unknown path
    /// that cannot be read. Records the new state either way.
    pub fn changed(&mut self, path: &Path) -> bool {
        match hash_file(path) {
            Some(hash) => match self.hashes.insert(path.to_path_buf(), hash) {
                Some(previous) => previous != hash,
                None => true,
            },
            None => self.hashes.remove(path).is_some(),
        }
    }
}

fn hash_file(path: &Path) -> Option<[u8; 32]> {
    let bytes = std::fs::read(path).ok()?;
    Some(Sha256::digest(&bytes).into())
}

/// Extensions the project discovery treats as inputs. A *new* file with
/// one of these is not yet in `WatchContext::inputs`, and classifies as
/// `Full` so sidebars and listings pick it up.
pub fn is_input_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| INPUT_EXTENSIONS.contains(&e))
}

/// Config the render reads regardless of which page is rendering:
/// `_quarto*.yml` (and its profile / `.local` overlays), `_metadata.yml`,
/// `_brand.yml`, `_variables.yml`, `.env*`, and anything under
/// `_extensions/`. The driver never drops a change to one of these,
/// even while a render is in flight; other `Full`-classified paths
/// that appear mid-render are presumed to be the render's own writes.
pub fn is_config_like(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if ["_quarto", "_metadata", "_brand", "_variables", ".env"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
    {
        return true;
    }
    path.components()
        .any(|c| c.as_os_str().to_str() == Some("_extensions"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(temp: &tempfile::TempDir, website: bool) -> WatchContext {
        let dir = temp.path().canonicalize().unwrap();
        let output_dir = if website {
            dir.join("_site")
        } else {
            dir.clone()
        };
        let inputs: BTreeSet<PathBuf> = ["index.qmd", "about.qmd", "posts/one.qmd"]
            .iter()
            .map(|p| dir.join(p))
            .collect();
        for input in &inputs {
            std::fs::create_dir_all(input.parent().unwrap()).unwrap();
            std::fs::write(input, "x").unwrap();
        }
        let outputs: BTreeSet<PathBuf> = ["index.html", "about.html", "posts/one.html"]
            .iter()
            .map(|p| output_dir.join(p))
            .collect();
        WatchContext {
            project_dir: dir,
            output_dir,
            inputs,
            outputs,
        }
    }

    fn subset(paths: &[&Path]) -> Action {
        Action::Subset(paths.iter().map(|p| p.to_path_buf()).collect())
    }

    #[test]
    fn existing_input_is_a_subset_of_itself() {
        let temp = tempfile::TempDir::new().unwrap();
        let c = ctx(&temp, true);
        let p = c.project_dir.join("posts/one.qmd");
        assert_eq!(classify(&p, &c), subset(&[&p]));
    }

    #[test]
    fn deleted_input_is_full() {
        let temp = tempfile::TempDir::new().unwrap();
        let c = ctx(&temp, true);
        let p = c.project_dir.join("about.qmd");
        std::fs::remove_file(&p).unwrap();
        assert_eq!(classify(&p, &c), Action::Full);
    }

    #[test]
    fn new_input_not_yet_known_is_full() {
        let temp = tempfile::TempDir::new().unwrap();
        let c = ctx(&temp, true);
        for name in ["new.qmd", "notes.md", "nb.ipynb", "r.Rmd"] {
            let p = c.project_dir.join(name);
            std::fs::write(&p, "x").unwrap();
            assert_eq!(classify(&p, &c), Action::Full, "{name}");
            assert!(is_input_extension(&p), "{name}");
        }
        assert!(!is_input_extension(&c.project_dir.join("a.png")));
    }

    #[test]
    fn config_extensions_and_resources_are_full() {
        let temp = tempfile::TempDir::new().unwrap();
        let c = ctx(&temp, true);
        for rel in [
            "_quarto.yml",
            "_quarto-preview.yml",
            "_quarto.yml.local",
            "posts/_metadata.yml",
            "_brand.yml",
            "_extensions/acme/_extension.yml",
            "_extensions/acme/filter.lua",
            "styles.scss",
            "images/logo.png",
            "_partial.qmd",
            ".env",
        ] {
            let p = c.project_dir.join(rel);
            assert_eq!(classify(&p, &c), Action::Full, "{rel}");
        }
    }

    #[test]
    fn output_dir_and_generated_dirs_are_ignored() {
        let temp = tempfile::TempDir::new().unwrap();
        let c = ctx(&temp, true);
        for rel in [
            "_site/index.html",
            "_site/site_libs/bootstrap/bootstrap.css",
            ".quarto/cache/profiles/x.json",
            ".git/index",
            ".jupyter_cache/global.db",
            "_freeze/index/execute-results/html.json",
            "node_modules/pkg/index.js",
            "__pycache__/m.pyc",
            ".ipynb_checkpoints/nb-checkpoint.ipynb",
            "index_files/figure-html/plot-1.png",
            "posts/one_files/libs/x.js",
            "posts/one_cache/html/chunk.rds",
            "site_libs/quarto-html/quarto.js",
        ] {
            let p = c.project_dir.join(rel);
            assert_eq!(classify(&p, &c), Action::Ignore, "{rel}");
        }
    }

    #[test]
    fn editor_temporaries_are_ignored() {
        let temp = tempfile::TempDir::new().unwrap();
        let c = ctx(&temp, true);
        for rel in [
            "index.qmd~",
            ".index.qmd.swp",
            ".index.qmd.swx",
            ".#index.qmd",
            "4913",
            "posts/.one.qmd.swp",
        ] {
            let p = c.project_dir.join(rel);
            assert_eq!(classify(&p, &c), Action::Ignore, "{rel}");
        }
    }

    /// Non-website projects render beside their sources. The outputs the
    /// last render wrote must not look like resource edits, or every
    /// render would trigger the next.
    #[test]
    fn outputs_beside_sources_are_ignored() {
        let temp = tempfile::TempDir::new().unwrap();
        let c = ctx(&temp, false);
        assert_eq!(c.output_dir, c.project_dir);
        for rel in ["index.html", "posts/one.html", "index_files/x.css"] {
            let p = c.project_dir.join(rel);
            assert_eq!(classify(&p, &c), Action::Ignore, "{rel}");
        }
        // But an HTML file the render did *not* write is a resource.
        let p = c.project_dir.join("header.html");
        assert_eq!(classify(&p, &c), Action::Full);
    }

    #[test]
    fn paths_outside_the_project_are_ignored() {
        let temp = tempfile::TempDir::new().unwrap();
        let c = ctx(&temp, true);
        let outside = c.project_dir.parent().unwrap().join("elsewhere.qmd");
        assert_eq!(classify(&outside, &c), Action::Ignore);
    }

    #[test]
    fn the_project_root_itself_is_ignored() {
        let temp = tempfile::TempDir::new().unwrap();
        let c = ctx(&temp, true);
        assert_eq!(classify(&c.project_dir, &c), Action::Ignore);
    }

    #[test]
    fn config_like_paths_are_recognised() {
        for rel in [
            "_quarto.yml",
            "_quarto-preview.yml",
            "_quarto.yml.local",
            "posts/_metadata.yml",
            "_brand.yml",
            "_variables.yml",
            ".env",
            ".env.production",
            "_extensions/acme/filter.lua",
        ] {
            assert!(is_config_like(Path::new(rel)), "{rel}");
        }
        for rel in ["styles.scss", "images/logo.png", "index.qmd", "data/x.csv"] {
            assert!(!is_config_like(Path::new(rel)), "{rel}");
        }
    }

    #[test]
    fn tracker_reports_only_real_content_changes() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path().canonicalize().unwrap();
        let a = dir.join("a.qmd");
        std::fs::write(&a, "one").unwrap();
        let mut t = ContentTracker::default();
        t.seed([a.clone()]);

        assert!(!t.changed(&a), "seeded and untouched");
        // A read (what a render does) or an identical rewrite is not a change.
        let _ = std::fs::read(&a).unwrap();
        std::fs::write(&a, "one").unwrap();
        assert!(!t.changed(&a), "identical bytes rewritten");

        std::fs::write(&a, "two").unwrap();
        assert!(t.changed(&a), "bytes changed");
        assert!(!t.changed(&a), "…and only reported once");

        std::fs::remove_file(&a).unwrap();
        assert!(t.changed(&a), "a tracked file disappearing is a change");
        assert!(!t.changed(&a), "…reported once");
    }

    #[test]
    fn tracker_first_sight_of_a_readable_file_is_a_change_but_directories_are_not() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path().canonicalize().unwrap();
        let b = dir.join("b.png");
        std::fs::write(&b, "img").unwrap();
        let mut t = ContentTracker::default();
        assert!(t.changed(&b), "never seen before");
        assert!(!t.changed(&b));
        assert!(!t.changed(&dir), "a directory event carries no content");
        assert!(!t.changed(&dir.join("never-existed.qmd")));
    }

    #[test]
    fn merge_full_absorbs_subsets_union_and_ignore_is_identity() {
        let a = PathBuf::from("/p/a.qmd");
        let b = PathBuf::from("/p/b.qmd");
        assert_eq!(Action::Ignore.merge(Action::Ignore), Action::Ignore);
        assert_eq!(Action::Ignore.merge(subset(&[&a])), subset(&[&a]));
        assert_eq!(subset(&[&a]).merge(Action::Ignore), subset(&[&a]));
        assert_eq!(subset(&[&a]).merge(subset(&[&b])), subset(&[&a, &b]));
        assert_eq!(subset(&[&a]).merge(Action::Full), Action::Full);
        assert_eq!(Action::Full.merge(subset(&[&a])), Action::Full);
        assert_eq!(Action::Full.merge(Action::Ignore), Action::Full);
    }
}
