//! Filesystem watching for continuous sync
//!
//! This module provides filesystem watching capabilities to detect when source
//! files (`.qmd`, `.md`) are modified on disk, enabling real-time
//! synchronization between the filesystem and automerge documents.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_mini::{DebouncedEvent, Debouncer, new_debouncer};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::error::{Error, Result};
use crate::exposure::ExposurePolicy;

/// Default debounce duration for filesystem events (in milliseconds).
/// This batches rapid file saves into a single event.
const DEFAULT_DEBOUNCE_MS: u64 = 500;

/// Events emitted by the filesystem watcher.
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// A watched file was modified (created, written, or metadata changed).
    /// The set of files that produce this event is governed by
    /// [`WatchFilter`].
    Modified(PathBuf),
}

/// Which kinds of files the watcher should surface events for.
///
/// The two modes correspond to the two consumers of the hub today:
/// `quarto hub` (a long-lived sync server, which only observes source
/// content) and `q2 preview` (which needs config, metadata,
/// custom-component, and asset edits to trigger re-render).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WatchFilter {
    /// Hub default: only source files (`.qmd`, `.md`) surface events.
    /// (`.md` joined `.qmd` with bd-6d2wj4zp D10 — sync and watch are
    /// extension-based; render-list membership is decided at render
    /// time, not here.)
    #[default]
    SourcesOnly,
    /// Filter for `q2 preview`: every file the exposure policy classifies
    /// (preview VFS contract D4): sources, config, stylesheets,
    /// components, bibliographies, data, templates, binaries and
    /// `_extensions/` contents. The watcher additionally drops hidden and
    /// generated paths by asking the policy itself (see
    /// [`FileWatcher::new`]). One list, so a file a render pulled is also
    /// a file whose edits surface.
    PreviewBroad,
    /// Every path surfaces. For consumers that classify changes
    /// themselves against state the watcher cannot see — `q2 preview
    /// --static` (bd-sl79jjiq) decides "ignore / subset / full" from the
    /// last render's inputs and outputs, so filtering here would only
    /// hide the events it needs. The generated-directory exclusions
    /// (`_site/`, `.quarto/`, `*_files/`) live in its policy, not here.
    All,
}

impl WatchFilter {
    /// Returns true if `path` should surface as a [`WatchEvent::Modified`].
    pub fn accepts(self, path: &Path) -> bool {
        match self {
            Self::SourcesOnly => is_source_file(path) && !in_preview_out(path),
            Self::PreviewBroad => {
                crate::exposure::classify(path).is_some() && !in_preview_out(path)
            }
            Self::All => true,
        }
    }
}

/// Whether `path` is under a `.quarto/preview-out/` request directory (epic
/// E9). Preview renders write images (capture plots, copies) there, and
/// `PreviewBroad` accepts images by extension alone, so without this a
/// render would raise watcher events for its own output. Matched as the two
/// adjacent components rather than "any hidden component": the project
/// itself may live under a hidden directory (`~/.worktrees/...`).
fn in_preview_out(path: &Path) -> bool {
    let mut prev_is_quarto = false;
    for c in path.components() {
        let name = c.as_os_str();
        if prev_is_quarto && name == "preview-out" {
            return true;
        }
        prev_is_quarto = name == ".quarto";
    }
    false
}

/// Configuration for the filesystem watcher.
#[derive(Debug, Clone)]
pub struct WatchConfig {
    /// Debounce duration in milliseconds
    pub debounce_ms: u64,
    /// Which files trigger events. See [`WatchFilter`].
    pub filter: WatchFilter,
    /// Single-file mode (bd-tnm3k): when `Some(path)`, the watcher
    /// subscribes only to that one file (`RecursiveMode::NonRecursive`)
    /// and additionally rejects any event whose path is not exactly
    /// that file. This guards `q2 preview ~/Downloads/draft.qmd` from
    /// observing or surfacing sibling-file edits. The path must be
    /// absolute / canonicalized to match the events `notify` produces.
    pub single_file: Option<PathBuf>,

    /// Single-file mode (bd-9cyza5vy): the deck's *other* synced
    /// dependency files — included `.qmd`, referenced images, a sibling
    /// `_brand.yml` — as absolute paths. Only meaningful when
    /// [`single_file`](Self::single_file) is `Some`.
    ///
    /// The watcher subscribes to each of these (in addition to the deck)
    /// and accepts their events, so editing an included file or a
    /// referenced image re-renders the preview — matching project mode's
    /// dir-watch for the deck's closure. Crucially it does **not** widen
    /// the watch to the whole directory: only the resolved closure is
    /// watched, so an unrelated sibling (the `bd-tnm3k` concern) still
    /// never surfaces. Built the same way as `single_file`
    /// (`project_root.join(rel)`) so paths match the events `notify`
    /// echoes back.
    pub single_file_deps: Vec<PathBuf>,

    /// The project's `output-dir`, project-relative or absolute. Under
    /// [`WatchFilter::PreviewBroad`] an event inside it, or inside any
    /// other generated directory, is dropped; see
    /// [`ExposurePolicy::dir_is_generated`].
    pub output_dir: Option<PathBuf>,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            debounce_ms: DEFAULT_DEBOUNCE_MS,
            filter: WatchFilter::default(),
            single_file: None,
            single_file_deps: Vec::new(),
            output_dir: None,
        }
    }
}

impl WatchConfig {
    /// `WatchConfig` with the default debounce and the supplied filter.
    pub fn with_filter(filter: WatchFilter) -> Self {
        Self {
            debounce_ms: DEFAULT_DEBOUNCE_MS,
            filter,
            single_file: None,
            single_file_deps: Vec::new(),
            output_dir: None,
        }
    }
}

/// The single-file watch scope (bd-tnm3k, preview VFS contract D8): the
/// files and directories a single-file preview has been asked about. An
/// event surfaces only if its path is one of the files, or sits directly in
/// one of the directories.
#[derive(Debug, Default)]
struct WatchScope {
    files: HashSet<PathBuf>,
    /// Each watched directory with the names it held when it was first
    /// watched, so a file *created* later can be told apart from a sibling
    /// nothing asked for.
    dirs: HashMap<PathBuf, HashSet<OsString>>,
}

impl WatchScope {
    fn admits(&self, path: &Path) -> bool {
        self.files.contains(path) || path.parent().is_some_and(|dir| self.dirs.contains_key(dir))
    }
}

/// A cloneable handle that widens a running watcher (preview VFS contract
/// D8): the server watches what it has served. In project mode the tree is
/// already watched and every call is a no-op.
#[derive(Clone)]
pub struct WatchHandle {
    debouncer: Arc<Mutex<Debouncer<notify::RecommendedWatcher>>>,
    /// `None` in project mode.
    scope: Option<Arc<RwLock<WatchScope>>>,
}

impl WatchHandle {
    /// Watch `dir`'s direct children (absolute, canonical). Records the
    /// names `dir` holds now; see [`is_new_in_watched_dir`](Self::is_new_in_watched_dir).
    pub fn watch_dir(&self, dir: &Path) -> Result<()> {
        let Some(scope) = &self.scope else {
            return Ok(());
        };
        if scope
            .read()
            .expect("watch scope lock")
            .dirs
            .contains_key(dir)
        {
            return Ok(());
        }
        let names = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name())
                    .collect()
            })
            .unwrap_or_default();
        self.debouncer
            .lock()
            .expect("watcher lock")
            .watcher()
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(|e| Error::Sync(format!("failed to watch {}: {e}", dir.display())))?;
        scope
            .write()
            .expect("watch scope lock")
            .dirs
            .insert(dir.to_path_buf(), names);
        debug!(dir = %dir.display(), "Watching directory on demand");
        Ok(())
    }

    /// Watch one file (absolute, canonical).
    pub fn watch_file(&self, file: &Path) -> Result<()> {
        let Some(scope) = &self.scope else {
            return Ok(());
        };
        if scope.read().expect("watch scope lock").files.contains(file) {
            return Ok(());
        }
        self.debouncer
            .lock()
            .expect("watcher lock")
            .watcher()
            .watch(file, RecursiveMode::NonRecursive)
            .map_err(|e| Error::Sync(format!("failed to watch {}: {e}", file.display())))?;
        scope
            .write()
            .expect("watch scope lock")
            .files
            .insert(file.to_path_buf());
        Ok(())
    }

    /// Whether `path` may be enrolled as a new file. Project mode: always
    /// (the tree is watched; the caller applies the walk's rules).
    /// Single-file mode: only a file that sits directly in a watched
    /// directory and was not there when the directory was first watched.
    pub fn is_new_in_watched_dir(&self, path: &Path) -> bool {
        let Some(scope) = &self.scope else {
            return true;
        };
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return false;
        };
        scope
            .read()
            .expect("watch scope lock")
            .dirs
            .get(dir)
            .is_some_and(|before| !before.contains(name))
    }
}

/// Filesystem watcher for project files.
///
/// Uses notify-debouncer-mini to watch for filesystem changes with debouncing
/// to batch rapid changes (e.g., multiple saves in quick succession).
pub struct FileWatcher {
    /// Shared with [`WatchHandle`]s, which widen the watch at runtime.
    handle: WatchHandle,

    /// Receiver for watch events
    event_rx: mpsc::UnboundedReceiver<WatchEvent>,
}

impl FileWatcher {
    /// Create a new filesystem watcher for the given project root.
    ///
    /// Project mode watches the tree recursively; single-file mode watches
    /// the deck and its dependencies, and grows through [`WatchHandle`].
    pub fn new(project_root: &Path, config: WatchConfig) -> Result<Self> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let project_root = project_root.to_path_buf();
        let filter = config.filter;
        let single_file = config.single_file.clone();
        let single_file_deps = config.single_file_deps.clone();

        // In single-file mode, the watcher's allow-list starts as the deck
        // plus its resolved dependency closure (bd-tnm3k + bd-9cyza5vy) and
        // grows with what the miss routes serve (contract D8). An event
        // surfaces only if its path is in scope, so `notify` directory-level
        // events and unrelated siblings are both dropped. `None` ⇒ project
        // mode (recursive walk, no allow-list).
        let scope: Option<Arc<RwLock<WatchScope>>> = single_file.as_ref().map(|deck| {
            let mut files = HashSet::with_capacity(1 + single_file_deps.len());
            files.insert(deck.clone());
            files.extend(single_file_deps.iter().cloned());
            Arc::new(RwLock::new(WatchScope {
                files,
                dirs: HashMap::new(),
            }))
        });
        let event_scope = scope.clone();

        // Preview VFS contract D4: the preview watcher drops events the
        // exposure policy refuses on the path alone: hidden, inside a
        // generated directory (a sidecar beside its source, `site_libs`, the
        // output directory, ...), or an unknown type. A render's own output
        // never looks like an author's edit.
        let policy = (filter == WatchFilter::PreviewBroad).then(|| {
            let policy = ExposurePolicy::new(&project_root);
            match config.output_dir.as_deref() {
                Some(out) => policy.with_output_dir(out),
                None => policy,
            }
        });

        // Create a debounced watcher
        let mut debouncer = new_debouncer(
            Duration::from_millis(config.debounce_ms),
            move |res: std::result::Result<Vec<DebouncedEvent>, notify::Error>| {
                match res {
                    Ok(events) => {
                        for event in events {
                            // bd-tnm3k / bd-9cyza5vy: in single-file mode, drop
                            // any event whose path isn't in scope. `notify` may
                            // report directory-level events on some platforms
                            // even when only specific files are watched.
                            if let Some(ref scope) = event_scope
                                && !scope.read().expect("watch scope lock").admits(&event.path)
                            {
                                continue;
                            }
                            if let Some(policy) = &policy
                                && !policy.admits_event(&event.path)
                            {
                                continue;
                            }
                            if filter.accepts(&event.path) {
                                debug!(path = %event.path.display(), "File change detected");
                                if event_tx.send(WatchEvent::Modified(event.path)).is_err() {
                                    // Receiver dropped, watcher should stop
                                    debug!("Event receiver dropped, stopping watcher");
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "Filesystem watch error");
                    }
                }
            },
        )
        .map_err(|e| Error::Sync(format!("failed to create filesystem watcher: {}", e)))?;

        // bd-tnm3k / bd-9cyza5vy: single-file mode subscribes to the deck and
        // each closure dependency individually (NonRecursive) — so a dep in a
        // subdirectory is seen without widening the watch to the whole
        // directory. Project mode keeps the recursive walk of `project_root`.
        match single_file.as_deref() {
            Some(deck) => {
                // The deck is required: a failure here is fatal.
                debouncer
                    .watcher()
                    .watch(deck, RecursiveMode::NonRecursive)
                    .map_err(|e| Error::Sync(format!("failed to watch single file: {}", e)))?;
                // Dependencies are best-effort: a missing/edge-case dep must not
                // tank the whole watcher (the deck still re-renders on edit).
                for dep in &single_file_deps {
                    if let Err(e) = debouncer.watcher().watch(dep, RecursiveMode::NonRecursive) {
                        warn!(
                            path = %dep.display(),
                            error = %e,
                            "failed to watch single-file dependency; edits to it won't refresh the preview"
                        );
                    }
                }
            }
            None => {
                debouncer
                    .watcher()
                    .watch(project_root.as_path(), RecursiveMode::Recursive)
                    .map_err(|e| Error::Sync(format!("failed to watch project root: {}", e)))?;
            }
        }

        info!(
            path = %project_root.display(),
            debounce_ms = config.debounce_ms,
            filter = ?filter,
            single_file = ?single_file,
            single_file_dep_count = single_file_deps.len(),
            "Started filesystem watcher"
        );

        Ok(Self {
            handle: WatchHandle {
                debouncer: Arc::new(Mutex::new(debouncer)),
                scope,
            },
            event_rx,
        })
    }

    /// A handle that widens this watcher at runtime.
    pub fn handle(&self) -> WatchHandle {
        self.handle.clone()
    }

    /// Receive the next watch event.
    ///
    /// Returns `None` if the watcher has been stopped.
    pub async fn recv(&mut self) -> Option<WatchEvent> {
        self.event_rx.recv().await
    }
}

/// Check if a path is a source file (`.qmd` or `.md`).
fn is_source_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("qmd") || ext.eq_ignore_ascii_case("md"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn preview_out_request_directories_never_surface() {
        let f = WatchFilter::PreviewBroad;
        assert!(!f.accepts(Path::new("/p/.quarto/preview-out/abc/fig/plot.png")));
        assert!(!f.accepts(Path::new("/p/sub/.quarto/preview-out/abc/doc.md")));
        assert!(!WatchFilter::SourcesOnly.accepts(Path::new("/p/.quarto/preview-out/a/x.qmd")));
        // A project under a hidden directory still works.
        assert!(f.accepts(Path::new("/home/u/.worktrees/w/img.png")));
        assert!(f.accepts(Path::new("/p/.quarto/other/img.png")));
    }

    #[test]
    fn test_is_source_file() {
        assert!(is_source_file(Path::new("test.qmd")));
        assert!(is_source_file(Path::new("test.QMD")));
        assert!(is_source_file(Path::new("/path/to/file.qmd")));
        // bd-6d2wj4zp Phase 5 (D10): .md is a source file.
        assert!(is_source_file(Path::new("test.md")));
        assert!(is_source_file(Path::new("test.MD")));
        assert!(!is_source_file(Path::new("test.txt")));
        assert!(!is_source_file(Path::new("test")));
    }

    /// bd-6d2wj4zp Phase 5 (D10): `.md` is a source file for watching,
    /// symmetric with the sync layer — both filters must surface `.md`
    /// edits so a rendered `.md` page live-updates in the preview and
    /// stays in sync on the hub. (Whether a given `.md` triggers a
    /// re-render is dep-filtered downstream in the SPA, not here.)
    #[test]
    fn test_filters_accept_md_as_source() {
        assert!(WatchFilter::SourcesOnly.accepts(Path::new("doc.md")));
        assert!(WatchFilter::SourcesOnly.accepts(Path::new("doc.MD")));
        assert!(WatchFilter::PreviewBroad.accepts(Path::new("doc.md")));
        assert!(WatchFilter::PreviewBroad.accepts(Path::new("posts/README.md")));
    }

    #[test]
    fn test_watch_filter_sources_only() {
        let f = WatchFilter::SourcesOnly;
        assert!(f.accepts(Path::new("doc.qmd")));
        assert!(f.accepts(Path::new("doc.QMD")));
        assert!(f.accepts(Path::new("doc.md")));
        assert!(!f.accepts(Path::new("_quarto.yml")));
        assert!(!f.accepts(Path::new("image.png")));
        assert!(!f.accepts(Path::new("Component.tsx")));
    }

    #[test]
    fn test_watch_filter_preview_broad_accepts() {
        let f = WatchFilter::PreviewBroad;
        // .qmd still accepted.
        assert!(f.accepts(Path::new("doc.qmd")));
        assert!(f.accepts(Path::new("posts/intro.qmd")));

        // Config files (both .yml and .yaml).
        assert!(f.accepts(Path::new("_quarto.yml")));
        assert!(f.accepts(Path::new("_quarto.yaml")));
        assert!(f.accepts(Path::new("project/_quarto.yml")));
        assert!(f.accepts(Path::new("_metadata.yml")));
        assert!(f.accepts(Path::new("_metadata.yaml")));
        assert!(f.accepts(Path::new("posts/_metadata.yml")));

        // Common image formats (case-insensitive).
        assert!(f.accepts(Path::new("logo.png")));
        assert!(f.accepts(Path::new("LOGO.PNG")));
        assert!(f.accepts(Path::new("photo.jpg")));
        assert!(f.accepts(Path::new("photo.jpeg")));
        assert!(f.accepts(Path::new("anim.gif")));
        assert!(f.accepts(Path::new("icon.svg")));
        assert!(f.accepts(Path::new("hero.webp")));

        // Custom React components.
        assert!(f.accepts(Path::new("Component.tsx")));
        assert!(f.accepts(Path::new("ui/Button.TSX")));

        // Phase D.3 (bd-kw93.9): CSS files round-trip through samod
        // binary-doc sync so users can edit `_extensions/foo/foo.css`
        // (or any project-level CSS) and have the preview pick it up.
        assert!(f.accepts(Path::new("styles.css")));
        assert!(f.accepts(Path::new("_extensions/foo/foo.css")));
        assert!(f.accepts(Path::new("THEME.CSS")));
    }

    /// Preview VFS contract D4: `PreviewBroad` is the exposure policy's
    /// class list, so every type a render can pull (stylesheets,
    /// bibliographies, data) also surfaces its edits; unknown types do not.
    #[test]
    fn test_watch_filter_preview_broad_follows_the_policy_classes() {
        let f = WatchFilter::PreviewBroad;
        for p in [
            "notes.txt",
            "data.csv",
            "config.yml",
            "refs.bib",
            "apa.csl",
            "theme.scss",
            "_partial.sass",
            "scan.bmp",
            "_brand.yml",
            "_extensions/acme/filter.lua",
        ] {
            assert!(f.accepts(Path::new(p)), "{p}");
        }
        for p in [
            // The trailing extension wins: a backup is not its original.
            "Component.tsx.bak",
            "_quarto.yml.bak",
            "a.exe",
            ".gitignore",
            "Makefile",
        ] {
            assert!(!f.accepts(Path::new(p)), "{p}");
        }
    }

    #[tokio::test]
    async fn test_watcher_creation() {
        let temp = TempDir::new().unwrap();
        let watcher = FileWatcher::new(temp.path(), WatchConfig::default());
        assert!(watcher.is_ok());
    }

    #[tokio::test]
    async fn test_watcher_detects_file_change() {
        let temp = TempDir::new().unwrap();
        // Canonicalize to handle macOS /var -> /private/var symlinks
        let temp_path = temp.path().canonicalize().unwrap();
        let qmd_path = temp_path.join("test.qmd");

        // Create initial file
        std::fs::write(&qmd_path, "initial content").unwrap();

        // Wait a bit for the file to be fully created
        tokio::time::sleep(Duration::from_millis(100)).await;

        let mut watcher = FileWatcher::new(
            &temp_path,
            WatchConfig {
                debounce_ms: 100,
                filter: WatchFilter::SourcesOnly,
                single_file: None,
                single_file_deps: Vec::new(),
                output_dir: None,
            },
        )
        .unwrap();

        // Modify the file
        std::fs::write(&qmd_path, "modified content").unwrap();

        // Wait for the debounced event with timeout
        let event = tokio::time::timeout(Duration::from_secs(2), watcher.recv()).await;

        match event {
            Ok(Some(WatchEvent::Modified(path))) => {
                assert_eq!(path, qmd_path);
            }
            Ok(None) => panic!("Watcher stopped unexpectedly"),
            Err(_) => panic!("Timeout waiting for file change event"),
        }
    }

    #[tokio::test]
    async fn test_watcher_ignores_non_source_files() {
        let temp = TempDir::new().unwrap();
        // Canonicalize to handle macOS /var -> /private/var symlinks
        let temp_path = temp.path().canonicalize().unwrap();
        let txt_path = temp_path.join("test.txt");
        let qmd_path = temp_path.join("test.qmd");

        // Create initial files
        std::fs::write(&txt_path, "initial").unwrap();
        std::fs::write(&qmd_path, "initial").unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;

        let mut watcher = FileWatcher::new(
            &temp_path,
            WatchConfig {
                debounce_ms: 100,
                filter: WatchFilter::SourcesOnly,
                single_file: None,
                single_file_deps: Vec::new(),
                output_dir: None,
            },
        )
        .unwrap();

        // Modify the txt file (should be ignored)
        std::fs::write(&txt_path, "modified").unwrap();

        // Modify the qmd file (should be detected)
        std::fs::write(&qmd_path, "modified").unwrap();

        // Wait for event
        let event = tokio::time::timeout(Duration::from_secs(2), watcher.recv()).await;

        match event {
            Ok(Some(WatchEvent::Modified(path))) => {
                // Should be the qmd file, not the txt file
                assert_eq!(path, qmd_path);
            }
            Ok(None) => panic!("Watcher stopped unexpectedly"),
            Err(_) => panic!("Timeout waiting for file change event"),
        }
    }

    /// PreviewBroad watcher must surface a `_quarto.yml` edit. This is
    /// the integration counterpart to `test_watch_filter_preview_broad_accepts`
    /// — that test pins the predicate; this one pins the wiring all the
    /// way through notify-debouncer.
    #[tokio::test]
    async fn test_watcher_preview_broad_detects_quarto_yml_change() {
        let temp = TempDir::new().unwrap();
        let temp_path = temp.path().canonicalize().unwrap();
        let yml_path = temp_path.join("_quarto.yml");

        std::fs::write(&yml_path, "project:\n  type: website\n").unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        let mut watcher = FileWatcher::new(
            &temp_path,
            WatchConfig {
                debounce_ms: 100,
                filter: WatchFilter::PreviewBroad,
                single_file: None,
                single_file_deps: Vec::new(),
                output_dir: None,
            },
        )
        .unwrap();

        std::fs::write(&yml_path, "project:\n  type: website\n  title: Edited\n").unwrap();

        let event = tokio::time::timeout(Duration::from_secs(2), watcher.recv()).await;
        match event {
            Ok(Some(WatchEvent::Modified(path))) => assert_eq!(path, yml_path),
            Ok(None) => panic!("Watcher stopped unexpectedly"),
            Err(_) => panic!("Timeout waiting for _quarto.yml change event"),
        }
    }

    /// SourcesOnly watcher must *not* surface a `_quarto.yml` edit. This
    /// guards against an accidental future broadening of the default
    /// filter that would change hub semantics.
    #[tokio::test]
    async fn test_watcher_sources_only_ignores_quarto_yml() {
        let temp = TempDir::new().unwrap();
        let temp_path = temp.path().canonicalize().unwrap();
        let yml_path = temp_path.join("_quarto.yml");
        let qmd_path = temp_path.join("doc.qmd");

        std::fs::write(&yml_path, "project:\n").unwrap();
        std::fs::write(&qmd_path, "initial").unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        let mut watcher = FileWatcher::new(
            &temp_path,
            WatchConfig {
                debounce_ms: 100,
                filter: WatchFilter::SourcesOnly,
                single_file: None,
                single_file_deps: Vec::new(),
                output_dir: None,
            },
        )
        .unwrap();

        // Edit _quarto.yml (should be ignored by QmdOnly).
        std::fs::write(&yml_path, "project:\n  title: Edited\n").unwrap();
        // Edit doc.qmd (should be reported).
        std::fs::write(&qmd_path, "edited").unwrap();

        let event = tokio::time::timeout(Duration::from_secs(2), watcher.recv()).await;
        match event {
            Ok(Some(WatchEvent::Modified(path))) => assert_eq!(path, qmd_path),
            Ok(None) => panic!("Watcher stopped unexpectedly"),
            Err(_) => panic!("Timeout waiting for doc.qmd change event"),
        }
    }

    /// bd-tnm3k: in single-file mode (no `_quarto.yml` ancestor), the
    /// watcher must isolate to the one file `q2 preview` was invoked
    /// on — sibling `.qmd`s in the same parent directory must not
    /// surface. Otherwise `q2 preview ~/Downloads/draft.qmd` would
    /// watch the whole of `~/Downloads`.
    #[tokio::test]
    async fn test_watcher_single_file_ignores_sibling_qmd() {
        let temp = TempDir::new().unwrap();
        let temp_path = temp.path().canonicalize().unwrap();
        let target = temp_path.join("target.qmd");
        let sibling = temp_path.join("sibling.qmd");

        std::fs::write(&target, "initial").unwrap();
        std::fs::write(&sibling, "initial").unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        let mut watcher = FileWatcher::new(
            &temp_path,
            WatchConfig {
                debounce_ms: 100,
                filter: WatchFilter::PreviewBroad,
                single_file: Some(target.clone()),
                single_file_deps: Vec::new(),
                output_dir: None,
            },
        )
        .unwrap();

        // Touch the sibling first (must be filtered out).
        std::fs::write(&sibling, "edited sibling").unwrap();
        // Then touch the target (must surface).
        std::fs::write(&target, "edited target").unwrap();

        let event = tokio::time::timeout(Duration::from_secs(2), watcher.recv()).await;
        match event {
            Ok(Some(WatchEvent::Modified(path))) => assert_eq!(path, target),
            Ok(None) => panic!("Watcher stopped unexpectedly"),
            Err(_) => panic!("Timeout waiting for target.qmd change event"),
        }
    }

    /// bd-9cyza5vy: an edit to a file in the deck's resolved closure (here an
    /// included `.qmd`) must surface so the preview re-renders — while an
    /// unrelated sibling still must NOT (the bd-tnm3k safety property is
    /// preserved: only the closure is watched, not the whole directory).
    #[tokio::test]
    async fn test_watcher_single_file_watches_closure_dep() {
        let temp = TempDir::new().unwrap();
        let temp_path = temp.path().canonicalize().unwrap();
        let deck = temp_path.join("main.qmd");
        let dep = temp_path.join("part.qmd");
        let unrelated = temp_path.join("unrelated.qmd");

        std::fs::write(&deck, "{{< include part.qmd >}}\n").unwrap();
        std::fs::write(&dep, "initial").unwrap();
        std::fs::write(&unrelated, "initial").unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        let mut watcher = FileWatcher::new(
            &temp_path,
            WatchConfig {
                debounce_ms: 100,
                filter: WatchFilter::PreviewBroad,
                single_file: Some(deck.clone()),
                single_file_deps: vec![dep.clone()],
                output_dir: None,
            },
        )
        .unwrap();

        // Edit the unrelated sibling first (must be filtered out)...
        std::fs::write(&unrelated, "edited unrelated").unwrap();
        // ...then the included dep (must surface).
        std::fs::write(&dep, "edited dep").unwrap();

        let event = tokio::time::timeout(Duration::from_secs(2), watcher.recv()).await;
        match event {
            Ok(Some(WatchEvent::Modified(path))) => assert_eq!(
                path, dep,
                "expected the included dep edit to surface, got {path:?}"
            ),
            Ok(None) => panic!("Watcher stopped unexpectedly"),
            Err(_) => panic!("Timeout waiting for included-dep change event"),
        }
    }

    /// The preview watcher asks the exposure policy whether an event lies
    /// in a generated directory (preview VFS contract D4): a sidecar beside
    /// its source and the configured output directory are dropped; a
    /// sidecar-shaped directory with no source beside it is an author's.
    #[tokio::test]
    async fn test_watcher_preview_broad_drops_generated_dirs() {
        let temp = TempDir::new().unwrap();
        let temp_path = temp.path().canonicalize().unwrap();
        std::fs::write(temp_path.join("index.qmd"), "# Hi").unwrap();
        for dir in ["index_files", "public", "static_files"] {
            std::fs::create_dir(temp_path.join(dir)).unwrap();
        }
        let generated = [
            temp_path.join("index_files/plot.png"),
            temp_path.join("public/out.png"),
        ];
        let admitted = temp_path.join("static_files/logo.png");
        tokio::time::sleep(Duration::from_millis(100)).await;

        let mut watcher = FileWatcher::new(
            &temp_path,
            WatchConfig {
                debounce_ms: 100,
                filter: WatchFilter::PreviewBroad,
                single_file: None,
                single_file_deps: Vec::new(),
                output_dir: Some(PathBuf::from("public")),
            },
        )
        .unwrap();

        for path in &generated {
            std::fs::write(path, [0x89]).unwrap();
        }
        std::fs::write(&admitted, [0x89]).unwrap();

        let mut seen = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while let Ok(Some(WatchEvent::Modified(path))) =
            tokio::time::timeout_at(deadline, watcher.recv()).await
        {
            seen.push(path);
        }
        assert!(
            seen.contains(&admitted),
            "expected {admitted:?} in {seen:?}"
        );
        for path in &generated {
            assert!(!seen.contains(path), "{path:?} must not surface: {seen:?}");
        }
    }
}

#[cfg(test)]
mod all_filter_tests {
    use super::*;

    /// `q2 preview --static` classifies changes itself (its policy needs
    /// the last render's snapshot), so its watcher must surface every
    /// path and filter nothing.
    #[test]
    fn all_filter_accepts_every_path() {
        for p in [
            "a.qmd",
            "_quarto.yml",
            "img.png",
            "styles.scss",
            "_extensions/x/filter.lua",
            "data.csv",
            "no-extension",
            "_site/index.html",
        ] {
            assert!(WatchFilter::All.accepts(Path::new(p)), "{p}");
        }
    }

    #[test]
    fn narrower_filters_still_reject_what_all_accepts() {
        assert!(!WatchFilter::SourcesOnly.accepts(Path::new("data.csv")));
        assert!(!WatchFilter::PreviewBroad.accepts(Path::new("no-extension")));
    }
}
