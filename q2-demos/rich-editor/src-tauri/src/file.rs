//! The file the app edits: reading it, saving the editor's text to it, and
//! watching it for changes made by anything else.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{RecursiveMode, Watcher};

pub struct OpenFile {
    path: PathBuf,
    /// The file's text as last written or read here. A save and the
    /// watcher's read both hold this lock across their file access, so the
    /// watcher never reads a save's text before the save records it.
    known: Mutex<String>,
}

impl OpenFile {
    /// Open the file at `path` (relative to the current directory).
    pub fn open(path: &Path) -> io::Result<OpenFile> {
        let path = fs::canonicalize(path)?;
        let text = fs::read_to_string(&path)?;
        Ok(OpenFile {
            path,
            known: Mutex::new(text),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// The file's text as last saved or read.
    pub fn text(&self) -> String {
        self.known.lock().unwrap().clone()
    }

    /// Write `text` to the file, by writing a new file beside it and
    /// renaming it over the old one, so that nothing ever reads half of it.
    pub fn save(&self, text: String) -> io::Result<()> {
        let mut known = self.known.lock().unwrap();
        let name = self.name();
        let temp = self
            .path
            .with_file_name(format!(".{name}.rich-editor-save"));
        fs::write(&temp, &text)?;
        if let Ok(meta) = fs::metadata(&self.path) {
            fs::set_permissions(&temp, meta.permissions())?;
        }
        fs::rename(&temp, &self.path)?;
        *known = text;
        Ok(())
    }

    /// Look at the file, and return its text if it differs from what's
    /// known here (which it then is).
    fn changed_text(&self) -> Option<String> {
        let mut known = self.known.lock().unwrap();
        // Removed, or mid-replace: the next look will tell.
        let text = fs::read_to_string(&self.path).ok()?;
        if text == *known {
            return None;
        }
        *known = text.clone();
        Some(text)
    }
}

/// Watching a file; it stops when this is dropped.
pub struct Watching {
    _watcher: notify::RecommendedWatcher,
}

/// Call `on_change` with the file's text whenever something other than
/// `OpenFile::save` changes it.
///
/// It watches the file's directory: editors often save by replacing the
/// file, which a watch on the file itself would lose.
pub fn watch(
    file: Arc<OpenFile>,
    on_change: impl Fn(String) + Send + 'static,
) -> notify::Result<Watching> {
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx)?;
    let dir = file.path().parent().unwrap_or(Path::new("."));
    watcher.watch(dir, RecursiveMode::NonRecursive)?;
    std::thread::spawn(move || {
        // Ends when the watcher, and so the sender, is dropped.
        while rx.recv().is_ok() {
            // Let a burst of events (a save is several) settle, then look once.
            std::thread::sleep(Duration::from_millis(50));
            while rx.try_recv().is_ok() {}
            if let Some(text) = file.changed_text() {
                on_change(text);
            }
        }
    });
    Ok(Watching { _watcher: watcher })
}

#[cfg(test)]
mod tests {
    use super::*;

    const WAIT: Duration = Duration::from_secs(5);

    fn file_with(text: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.qmd");
        fs::write(&path, text).unwrap();
        (dir, path)
    }

    fn watched(path: &Path) -> (Arc<OpenFile>, mpsc::Receiver<String>, Watching) {
        let file = Arc::new(OpenFile::open(path).unwrap());
        let (tx, rx) = mpsc::channel();
        let watching = watch(file.clone(), move |text| {
            let _ = tx.send(text);
        })
        .unwrap();
        (file, rx, watching)
    }

    #[test]
    fn opens_the_file_with_its_text_and_name() {
        let (_dir, path) = file_with("One.\n");
        let file = OpenFile::open(&path).unwrap();
        assert_eq!(file.text(), "One.\n");
        assert_eq!(file.name(), "doc.qmd");
    }

    #[test]
    fn opening_a_missing_file_fails() {
        let dir = tempfile::tempdir().unwrap();
        assert!(OpenFile::open(&dir.path().join("missing.qmd")).is_err());
    }

    #[test]
    fn save_replaces_the_text_and_leaves_nothing_beside_it() {
        let (dir, path) = file_with("One.\n");
        let file = OpenFile::open(&path).unwrap();
        file.save("Two.\n".into()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "Two.\n");
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["doc.qmd"]);
    }

    #[cfg(unix)]
    #[test]
    fn save_keeps_the_files_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, path) = file_with("One.\n");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        OpenFile::open(&path)
            .unwrap()
            .save("Two.\n".into())
            .unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[test]
    fn reports_a_change_made_by_something_else() {
        let (_dir, path) = file_with("One.\n");
        let (file, changes, _watching) = watched(&path);
        fs::write(&path, "Changed.\n").unwrap();
        assert_eq!(changes.recv_timeout(WAIT).unwrap(), "Changed.\n");
        assert_eq!(file.text(), "Changed.\n");
    }

    #[test]
    fn reports_a_change_made_by_replacing_the_file() {
        let (dir, path) = file_with("One.\n");
        let (_file, changes, _watching) = watched(&path);
        let other = dir.path().join("doc.qmd~");
        fs::write(&other, "Replaced.\n").unwrap();
        fs::rename(&other, &path).unwrap();
        assert_eq!(changes.recv_timeout(WAIT).unwrap(), "Replaced.\n");
    }

    #[test]
    fn does_not_report_its_own_saves() {
        let (_dir, path) = file_with("One.\n");
        let (file, changes, _watching) = watched(&path);
        file.save("Saved.\n".into()).unwrap();
        // Something else changes it after: that, and only that, is reported.
        std::thread::sleep(Duration::from_millis(500));
        fs::write(&path, "Changed.\n").unwrap();
        assert_eq!(changes.recv_timeout(WAIT).unwrap(), "Changed.\n");
        assert!(changes.recv_timeout(Duration::from_millis(500)).is_err());
    }
}
