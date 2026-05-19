//! Filesystem watching for continuous sync
//!
//! This module provides filesystem watching capabilities to detect when .qmd files
//! are modified on disk, enabling real-time synchronization between the filesystem
//! and automerge documents.

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
/// `quarto hub` (a long-lived sync server, which historically only
/// observed `.qmd` content) and `q2 preview` (which needs config,
/// metadata, custom-component, and asset edits to trigger re-render).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WatchFilter {
    /// Legacy hub behaviour: only `.qmd` files surface events.
    #[default]
    QmdOnly,
    /// Broadened filter for `q2 preview`. Accepts, in addition to
    /// `.qmd`:
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
            Self::QmdOnly => is_qmd_file(path),
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
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            debounce_ms: DEFAULT_DEBOUNCE_MS,
            filter: WatchFilter::default(),
        }
    }
}

impl WatchConfig {
    /// `WatchConfig` with the default debounce and the supplied filter.
    pub fn with_filter(filter: WatchFilter) -> Self {
        Self {
            debounce_ms: DEFAULT_DEBOUNCE_MS,
            filter,
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

        // Create a debounced watcher
        let mut debouncer = new_debouncer(
            Duration::from_millis(config.debounce_ms),
            move |res: std::result::Result<Vec<DebouncedEvent>, notify::Error>| {
                match res {
                    Ok(events) => {
                        for event in events {
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

        // Start watching the project root recursively
        debouncer
            .watcher()
            .watch(&project_root, RecursiveMode::Recursive)
            .map_err(|e| Error::Sync(format!("failed to watch project root: {}", e)))?;

        info!(
            path = %project_root.display(),
            debounce_ms = config.debounce_ms,
            filter = ?filter,
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

/// Check if a path is a .qmd file.
fn is_qmd_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("qmd"))
}

/// Check if a path matches the [`WatchFilter::PreviewBroad`] allow-list.
///
/// Acceptance is decided by the file's basename / extension only — the
/// containing directory is irrelevant. That keeps the predicate cheap
/// and makes it tolerant of nested project layouts (e.g. a sub-section
/// `posts/_metadata.yml`).
fn is_preview_relevant(path: &Path) -> bool {
    if is_qmd_file(path) {
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
    fn test_is_qmd_file() {
        assert!(is_qmd_file(Path::new("test.qmd")));
        assert!(is_qmd_file(Path::new("test.QMD")));
        assert!(is_qmd_file(Path::new("/path/to/file.qmd")));
        assert!(!is_qmd_file(Path::new("test.md")));
        assert!(!is_qmd_file(Path::new("test.txt")));
        assert!(!is_qmd_file(Path::new("test")));
    }

    #[test]
    fn test_watch_filter_qmd_only() {
        let f = WatchFilter::QmdOnly;
        assert!(f.accepts(Path::new("doc.qmd")));
        assert!(f.accepts(Path::new("doc.QMD")));
        assert!(!f.accepts(Path::new("_quarto.yml")));
        assert!(!f.accepts(Path::new("image.png")));
        assert!(!f.accepts(Path::new("Component.tsx")));
        assert!(!f.accepts(Path::new("doc.md")));
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

        // Random non-matching files.
        assert!(!f.accepts(Path::new("README.md")));
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
                filter: WatchFilter::QmdOnly,
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
    async fn test_watcher_ignores_non_qmd_files() {
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
                filter: WatchFilter::QmdOnly,
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

    /// QmdOnly watcher must *not* surface a `_quarto.yml` edit. This
    /// guards against an accidental future broadening of the default
    /// filter that would change hub semantics.
    #[tokio::test]
    async fn test_watcher_qmd_only_ignores_quarto_yml() {
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
                filter: WatchFilter::QmdOnly,
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
}
