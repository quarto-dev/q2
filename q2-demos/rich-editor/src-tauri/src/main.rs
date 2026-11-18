//! The rich-editor prototype as a desktop app, on one file on this machine:
//! `rich-editor-desktop <file>`.
//!
//! The page (`desktop.html`) asks for the file with `open_file`, writes
//! the editor's edits with `save_file`, and hears of changes made to the
//! file by anything else as `file-changed` events, carrying the file's
//! new text.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use tauri::{Emitter, State};

struct OpenFile {
    path: PathBuf,
    /// The file's text as last written or read here. A save and the
    /// watcher's read both hold this lock across their file access, so the
    /// watcher never reads a save's text before the save records it.
    known: Mutex<String>,
}

#[derive(serde::Serialize)]
struct Opened {
    path: String,
    name: String,
    text: String,
}

#[tauri::command]
fn open_file(file: State<Arc<OpenFile>>) -> Opened {
    let text = file.known.lock().unwrap().clone();
    Opened {
        path: file.path.display().to_string(),
        name: file
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        text,
    }
}

#[tauri::command]
fn save_file(text: String, file: State<Arc<OpenFile>>) -> Result<(), String> {
    let mut known = file.known.lock().unwrap();
    write_replacing(&file.path, &text).map_err(|e| format!("{}: {e}", file.path.display()))?;
    *known = text;
    Ok(())
}

/// Write `text` to `path` by writing a new file beside it and renaming it
/// over the old one, so that nothing ever reads half of it.
fn write_replacing(path: &Path, text: &str) -> std::io::Result<()> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temp = path.with_file_name(format!(".{name}.rich-editor-save"));
    fs::write(&temp, text)?;
    if let Ok(meta) = fs::metadata(path) {
        fs::set_permissions(&temp, meta.permissions())?;
    }
    fs::rename(&temp, path)
}

/// Watch the file's directory (editors often save by replacing the file,
/// which a watch on the file itself would lose), and send the page the
/// file's text whenever it differs from what this app knows.
fn watch(app: tauri::AppHandle, file: Arc<OpenFile>) -> notify::Result<()> {
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx)?;
    let dir = file.path.parent().unwrap_or(Path::new("."));
    watcher.watch(dir, RecursiveMode::NonRecursive)?;
    std::thread::spawn(move || {
        let _watcher = watcher; // Watching stops when it's dropped.
        while rx.recv().is_ok() {
            // Let a burst of events (a save is several) settle, then look once.
            std::thread::sleep(Duration::from_millis(50));
            while rx.try_recv().is_ok() {}
            let mut known = file.known.lock().unwrap();
            let Ok(text) = fs::read_to_string(&file.path) else {
                continue; // Removed, or mid-replace: the next event will tell.
            };
            if text != *known {
                *known = text.clone();
                let _ = app.emit("file-changed", text);
            }
        }
    });
    Ok(())
}

fn main() {
    let Some(arg) = std::env::args_os().nth(1) else {
        eprintln!("Usage: rich-editor <file>");
        std::process::exit(2);
    };
    let path = match fs::canonicalize(&arg) {
        Ok(path) => path,
        Err(e) => {
            eprintln!("Can't open {}: {e}", Path::new(&arg).display());
            std::process::exit(1);
        }
    };
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("Can't read {}: {e}", path.display());
            std::process::exit(1);
        }
    };
    let file = Arc::new(OpenFile {
        path,
        known: Mutex::new(text),
    });

    tauri::Builder::default()
        .manage(file.clone())
        .invoke_handler(tauri::generate_handler![open_file, save_file])
        .setup(move |app| {
            watch(app.handle().clone(), file)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
