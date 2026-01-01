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
    /// A .qmd file was modified (created, written, or metadata changed)
    Modified(PathBuf),
}

/// Configuration for the filesystem watcher.
#[derive(Debug, Clone)]
pub struct WatchConfig {
    /// Debounce duration in milliseconds
    pub debounce_ms: u64,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            debounce_ms: DEFAULT_DEBOUNCE_MS,
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

        // Create a debounced watcher
        let mut debouncer = new_debouncer(
            Duration::from_millis(config.debounce_ms),
            move |res: std::result::Result<Vec<DebouncedEvent>, notify::Error>| {
                match res {
                    Ok(events) => {
                        for event in events {
                            // Filter for .qmd files
                            if is_qmd_file(&event.path) {
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

        let mut watcher = FileWatcher::new(&temp_path, WatchConfig { debounce_ms: 100 }).unwrap();

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

        let mut watcher = FileWatcher::new(&temp_path, WatchConfig { debounce_ms: 100 }).unwrap();

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
}
