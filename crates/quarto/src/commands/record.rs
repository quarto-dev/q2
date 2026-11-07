//! `q2 record export-positron` (RIU-5 Phase B2): a thin wrapper over
//! [`quarto_core::record::export_for_positron`] so the Positron commands do not
//! reimplement the export.
//!
//! Contract (see `claude-notes/plans/2026-11-02-riu-05-export-for-positron.md`):
//! stdout is exactly one JSON line, diagnostics go to stderr, and the exit code
//! says which class of outcome occurred (the table is [`exit`]). There is no
//! consent prompt: it is a command the user types on their own files. The
//! `.quarto` copy is deliberately not checked for staleness here.

use std::path::PathBuf;
use std::sync::Arc;

use quarto_core::project::ProjectContext;
use quarto_core::record::{
    ExportOptions, ExportedCache, RecordSource, execution_record_path, export_for_positron,
    read_record,
};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use serde_json::json;

/// The stable exit codes. Kept in one place; the integration test defines the
/// same table independently.
pub mod exit {
    pub const OK: i32 = 0;
    /// A bad argument or usage error (clap's own code too).
    pub const USAGE: i32 = 2;
    /// No record, and execution is forbidden.
    pub const NO_RECORD: i32 = 10;
    /// No exportable cells (the JSON line is still printed).
    pub const NO_CELLS: i32 = 11;
    /// The record is unreadable or invalid.
    pub const BAD_RECORD: i32 = 12;
    /// The engine run failed.
    pub const ENGINE_FAILED: i32 = 13;
}

pub struct ExportPositronArgs {
    pub doc: PathBuf,
    pub out: PathBuf,
    /// Empty means the default, `q2,engine`.
    pub records: Vec<RecordSource>,
    pub source_uri: Option<String>,
}

/// Run the command and return its exit code. Never panics on expected failures.
pub fn export_positron(args: ExportPositronArgs) -> i32 {
    let sources = if args.records.is_empty() {
        vec![RecordSource::Q2, RecordSource::Engine]
    } else {
        args.records
    };
    if let Some(bad) = sources
        .iter()
        .find(|s| !matches!(s, RecordSource::Q2 | RecordSource::Engine))
    {
        eprintln!(
            "error: `--records {}` is not valid for `q2 record export-positron`: only `q2` \
             (the .quarto copy) and `engine` (run the document) are accepted",
            bad.label()
        );
        return exit::USAGE;
    }

    let doc = match quarto_system_runtime::canonicalize(&args.doc) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: cannot read document {}: {e}", args.doc.display());
            return exit::USAGE;
        }
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = match ProjectContext::discover(&doc, runtime.as_ref()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: project discovery failed: {e}");
            return exit::USAGE;
        }
    };

    let mut record = None;
    for source in &sources {
        match source {
            RecordSource::Q2 => {
                let path = execution_record_path(&project.dir, &doc);
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                match read_record(&text) {
                    Ok(r) => record = Some(r),
                    Err(e) => {
                        eprintln!("error: record {} is invalid: {e}", path.display());
                        return exit::BAD_RECORD;
                    }
                }
            }
            RecordSource::Engine => {
                eprintln!("no record found; running the document to produce one");
                match run_engine(&doc, &project, runtime.clone()) {
                    Ok(r) => record = r,
                    Err(msg) => {
                        eprintln!("error: the engine run failed: {msg}");
                        eprintln!(
                            "hint: a cell that raises fails the run; set `#| error: true` on \
                             the cell or `execute: error: true` to keep going and export the \
                             error output"
                        );
                        return exit::ENGINE_FAILED;
                    }
                }
            }
            _ => unreachable!("validated above"),
        }
        if record.is_some() {
            break;
        }
    }

    let Some(record) = record else {
        if sources.contains(&RecordSource::Engine) {
            // The engine ran and produced no record (no code): nothing to export.
            print_json(&ExportedCache::default());
            return exit::NO_CELLS;
        }
        eprintln!(
            "error: no record found for {} (looked for the .quarto copy) and `--records` \
             does not allow running the document",
            doc.display()
        );
        return exit::NO_RECORD;
    };

    let last_updated = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let options = ExportOptions {
        source: args
            .source_uri
            .unwrap_or_else(|| doc.to_string_lossy().into_owned()),
        last_updated,
    };
    let exported = match export_for_positron(&record, &options) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("error: {e}");
            return exit::BAD_RECORD;
        }
    };
    print_json(&exported);
    if exported.cells == 0 {
        eprintln!("error: the record has no cells Positron can display");
        return exit::NO_CELLS;
    }
    if let Err(e) = std::fs::write(&args.out, &exported.bytes) {
        eprintln!("error: cannot write {}: {e}", args.out.display());
        return exit::USAGE;
    }
    exit::OK
}

fn run_engine(
    doc: &std::path::Path,
    project: &ProjectContext,
    runtime: Arc<dyn SystemRuntime>,
) -> Result<Option<quarto_core::record::Record>, String> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let _kernel_scope = quarto_core::engine::jupyter::kernel_scope();
    // The pipeline blocks on its own runtimes internally (the kernel daemon), so it must
    // not run inside `Runtime::block_on`: enter the runtime for reactor access and drive
    // the future with `pollster`, as the other native callers do.
    let _enter = rt.enter();
    let run = pollster::block_on(quarto_core::engine::preview_record::record_capture_run(
        doc, project, runtime, None,
    ))
    .map_err(|e| e.to_string())?;
    Ok(run.record)
}

fn print_json(e: &ExportedCache) {
    let line = json!({
        "cells": e.cells,
        "skipped": e.skipped.iter().map(|s| json!({"id": s.id, "reason": s.reason.as_str()})).collect::<Vec<_>>(),
        "dropped": e.dropped.iter().map(|d| json!({
            "id": d.cell_id, "mime": d.mime, "reason": d.reason.as_str(),
        })).collect::<Vec<_>>(),
    });
    println!("{line}");
}
