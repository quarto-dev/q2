//! The rich-editor prototype as a desktop app, on one file on this machine:
//! `rich-editor-desktop <file>`.
//!
//! The page (`desktop.html`) asks for the file with `open_file`, writes
//! the editor's edits with `save_file`, and hears of changes made to the
//! file by anything else as `file-changed` events, carrying the file's
//! new text.

mod file;

use std::path::Path;
use std::sync::Arc;

use tauri::{Emitter, Manager, State};

use file::OpenFile;

#[derive(serde::Serialize)]
struct Opened {
    path: String,
    name: String,
    text: String,
}

#[tauri::command]
fn open_file(file: State<Arc<OpenFile>>) -> Opened {
    Opened {
        path: file.path().display().to_string(),
        name: file.name(),
        text: file.text(),
    }
}

#[tauri::command]
fn save_file(text: String, file: State<Arc<OpenFile>>) -> Result<(), String> {
    file.save(text)
        .map_err(|e| format!("{}: {e}", file.path().display()))
}

fn main() {
    let Some(arg) = std::env::args_os().nth(1) else {
        eprintln!("Usage: rich-editor <file>");
        std::process::exit(2);
    };
    let file = match OpenFile::open(Path::new(&arg)) {
        Ok(file) => Arc::new(file),
        Err(e) => {
            eprintln!("Can't open {}: {e}", Path::new(&arg).display());
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .manage(file.clone())
        .invoke_handler(tauri::generate_handler![open_file, save_file])
        .setup(move |app| {
            let handle = app.handle().clone();
            let watching = file::watch(file, move |text| {
                let _ = handle.emit("file-changed", text);
            })?;
            // Watch for as long as the app runs.
            app.manage(watching);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
