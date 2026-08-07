//! Filesystem watching for continuous sync
//!
//! This module provides filesystem watching capabilities to detect when source
//! files (`.qmd`, `.md`) are modified on disk, enabling real-time
//! synchronization between the filesystem and automerge documents.

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_mini::{DebouncedEvent, Debouncer, new_debouncer};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::error::{Error, Result};

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
    /// Broadened filter for `q2 preview`. Accepts, in addition to
    /// source files:
    ///   - `_quarto.yml` / `_quarto.yaml` (project config)
    ///   - `_metadata.yml` / `_metadata.yaml` (section config)
    ///   - `.png`, `.jpg`, `.jpeg`, `.gif`, `.svg`, `.webp` (media)
    ///   - `.tsx` (custom React components)
    /// `_extensions/**` is intentionally *not* expanded yet — see
    /// `claude-notes/plans/2026-05-13-q2-preview-phase-b.md` Q-B1.
    PreviewBroad,
}

impl WatchFilter {
    /// Returns true if `path` should surface as a [`WatchEvent::Modified`].
    pub fn accepts(self, path: &Path) -> bool {
        match self {
            Self::SourcesOnly => is_source_file(path),
            Self::PreviewBroad => is_preview_relevant(path),
        }
    }
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
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            debounce_ms: DEFAULT_DEBOUNCE_MS,
            filter: WatchFilter::default(),
            single_file: None,
            single_file_deps: Vec::new(),
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
        }
    }
}

/// Filesystem watcher for .qmd files.
///
/// Uses notify-debouncer-mini to watch for filesystem changes with debouncing
/// to batch rapid changes (e.g., multiple saves in quick succession).
pub struct FileWatcher {
    /// The debouncer wrapping the underlying watcher
    _debouncer: Debouncer<notify::RecommendedWatcher>,

    /// Receiver for watch events
    event_rx: mpsc::UnboundedReceiver<WatchEvent>,
}

impl FileWatcher {
    /// Create a new filesystem watcher for the given project root.
    ///
    /// The watcher will recursively watch for changes to .qmd files.
    pub fn new(project_root: &Path, config: WatchConfig) -> Result<Self> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let project_root = project_root.to_path_buf();
        let filter = config.filter;
        let single_file = config.single_file.clone();
        let single_file_deps = config.single_file_deps.clone();

        // In single-file mode, the watcher's allow-list is the deck plus its
        // resolved dependency closure (bd-tnm3k + bd-9cyza5vy). An event
        // surfaces only if its path is in this set, so `notify` directory-level
        // events and unrelated siblings are both dropped. `None` ⇒ project mode
        // (recursive walk, no allow-list).
        let allowed: Option<std::collections::HashSet<PathBuf>> =
            single_file.as_ref().map(|deck| {
                let mut set = std::collections::HashSet::with_capacity(1 + single_file_deps.len());
                set.insert(deck.clone());
                set.extend(single_file_deps.iter().cloned());
                set
            });
        let event_allowed = allowed.clone();

        // Create a debounced watcher
        let mut debouncer = new_debouncer(
            Duration::from_millis(config.debounce_ms),
            move |res: std::result::Result<Vec<DebouncedEvent>, notify::Error>| {
                match res {
                    Ok(events) => {
                        for event in events {
                            // bd-tnm3k / bd-9cyza5vy: in single-file mode, drop
                            // any event whose path isn't in the deck's closure
                            // allow-list. `notify` may report directory-level
                            // events on some platforms even when only specific
                            // files are watched.
                            if let Some(ref allow) = event_allowed
                                && !allow.contains(&event.path)
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
            _debouncer: debouncer,
            event_rx,
        })
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

/// Check if a path matches the [`WatchFilter::PreviewBroad`] allow-list.
///
/// Acceptance is decided by the file's basename / extension only — the
/// containing directory is irrelevant. That keeps the predicate cheap
/// and makes it tolerant of nested project layouts (e.g. a sub-section
/// `posts/_metadata.yml`).
fn is_preview_relevant(path: &Path) -> bool {
    if is_source_file(path) {
        return true;
    }

    // Exact-basename match for Quarto config files. We match both
    // `.yml` and `.yaml` since `.yml` is the canonical Quarto spelling
    // but nothing prevents `.yaml`; missing the alt spelling silently
    // would be a surprising failure mode.
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let lower = name.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "_quarto.yml" | "_quarto.yaml" | "_metadata.yml" | "_metadata.yaml"
        ) {
            return true;
        }
    }

    // Extension-based match for media, custom React components, and
    // project-level CSS. SCSS / SASS / LESS are deliberately omitted —
    // preview-pipeline support for editing them is unverified; track
    // as a follow-up if user demand surfaces. (Phase D.3, bd-kw93.9.)
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let lower = ext.to_ascii_lowercase();
        return matches!(
            lower.as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "tsx" | "css"
        );
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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

    #[test]
    fn test_watch_filter_preview_broad_rejects() {
        let f = WatchFilter::PreviewBroad;

        // Random non-matching files. (README.md moved to the accepts
        // side with bd-6d2wj4zp D10 — .md is a source file now.)
        assert!(!f.accepts(Path::new("notes.txt")));
        assert!(!f.accepts(Path::new("data.csv")));

        // Other YAML files: only the two canonical Quarto names match.
        assert!(!f.accepts(Path::new("config.yml")));
        assert!(!f.accepts(Path::new("settings.yaml")));

        // Backup files where the trailing extension wins. `.tsx.bak` ends
        // in `.bak`, not `.tsx`, so it must be rejected.
        assert!(!f.accepts(Path::new("Component.tsx.bak")));
        assert!(!f.accepts(Path::new("_quarto.yml.bak")));

        // Image formats we explicitly don't watch yet.
        assert!(!f.accepts(Path::new("scan.bmp")));
        assert!(!f.accepts(Path::new("art.tiff")));

        // Extensionless / dotfile.
        assert!(!f.accepts(Path::new(".gitignore")));
        assert!(!f.accepts(Path::new("Makefile")));
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
}
