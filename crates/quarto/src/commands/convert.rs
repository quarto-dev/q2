//! `q2 convert`: convert a document to another representation, with the output
//! format explicit (`--to`, never inferred).
//!
//! Supported: `--from qmd --to positron-ipynb`, the Positron output-cache notebook
//! (RIU-5 Phase B2, moved here from the removed `q2 record` command by RFU-1).
//! `--to ipynb` and `--from ipynb` are reserved by name (strand `bd-ju7u8a73`).
//!
//! Contract of the `positron-ipynb` target
//! (see `claude-notes/plans/2026-11-02-riu-05-export-for-positron.md`): the JSON report
//! is exactly one line (on stdout, or on stderr when the notebook itself goes to
//! stdout with `-o -`), diagnostics go to stderr, and the exit code says which class of
//! outcome occurred (the table is [`exit`]). There is no consent prompt: it is a command
//! the user types on their own files. The `.quarto` copy is deliberately not checked for
//! staleness here.

use std::io::Write;
use std::path::{Path, PathBuf};
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

/// The conversions this command supports, as `(from, to)`. Listed in usage errors.
const SUPPORTED_PAIRS: &[(&str, &str)] = &[("qmd", "positron-ipynb")];

/// The record source list `convert` uses until OPR-5 replaces [`record_sources`] with the
/// reader of the `execute: record-sources` metadata key (allowed values `q2` and
/// `engine`, default `q2,engine`). There is deliberately no command-line option for it.
const DEFAULT_RECORD_SOURCES: [RecordSource; 2] = [RecordSource::Q2, RecordSource::Engine];

/// The one place `convert` decides where a record comes from.
fn record_sources() -> Vec<RecordSource> {
    DEFAULT_RECORD_SOURCES.to_vec()
}

pub struct ConvertArgs {
    pub input: PathBuf,
    pub from: Option<String>,
    pub to: String,
    pub output: Option<PathBuf>,
    /// `-M` arguments, unparsed, as `q2 render -M`.
    pub metadata: Vec<String>,
    pub source_uri: Option<String>,
}

fn usage(msg: &str) -> i32 {
    eprintln!("error: {msg}");
    exit::USAGE
}

fn supported_list() -> String {
    SUPPORTED_PAIRS
        .iter()
        .map(|(f, t)| format!("--from {f} --to {t}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Infer the input format from the extension (`.qmd` and `.md` are qmd, `.ipynb` is ipynb).
fn infer_from(input: &Path) -> Option<&'static str> {
    match input
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("qmd" | "md") => Some("qmd"),
        Some("ipynb") => Some("ipynb"),
        _ => None,
    }
}

/// Run the command and return its exit code. Never panics on expected failures.
pub fn execute(args: ConvertArgs) -> i32 {
    let from = match args.from.as_deref() {
        Some(f) => f.to_string(),
        None => match infer_from(&args.input) {
            Some(f) => f.to_string(),
            None => {
                return usage(&format!(
                    "cannot infer the input format of {} from its extension; pass `--from` \
                     (supported: {})",
                    args.input.display(),
                    supported_list()
                ));
            }
        },
    };
    if !SUPPORTED_PAIRS.contains(&(from.as_str(), args.to.as_str())) {
        return usage(&format!(
            "cannot convert `{from}` to `{}`; supported: {}",
            args.to,
            supported_list()
        ));
    }
    // From here on the pair is `qmd` to `positron-ipynb`.
    let Some(out) = args.output.clone() else {
        return usage(
            "`--to positron-ipynb` needs `-o <path>` (use `-o -` for stdout): the file name \
             belongs to the consumer",
        );
    };
    let overlay = match quarto_core::metadata_flag::overlay_from_metadata_flags(&args.metadata) {
        Ok(o) => o,
        Err(e) => return usage(&e),
    };
    let native: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let runtime = match &overlay {
        Some(o) => o.apply(native),
        None => native,
    };
    export_positron(&args, &out, &record_sources(), runtime)
}

fn export_positron(
    args: &ConvertArgs,
    out: &Path,
    sources: &[RecordSource],
    runtime: Arc<dyn SystemRuntime>,
) -> i32 {
    let to_stdout = out == Path::new("-");

    let doc = match quarto_system_runtime::canonicalize(&args.input) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: cannot read document {}: {e}", args.input.display());
            return exit::USAGE;
        }
    };
    let project = match ProjectContext::discover(&doc, runtime.as_ref()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: project discovery failed: {e}");
            return exit::USAGE;
        }
    };

    let mut record = None;
    for source in sources {
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
            other => {
                eprintln!(
                    "error: record source `{}` is not valid for `q2 convert`: only `q2` (the \
                     .quarto copy) and `engine` (run the document) are accepted",
                    other.label()
                );
                return exit::USAGE;
            }
        }
        if record.is_some() {
            break;
        }
    }

    let Some(record) = record else {
        if sources.contains(&RecordSource::Engine) {
            // The engine ran and produced no record (no code): nothing to export.
            report(&ExportedCache::default(), to_stdout);
            return exit::NO_CELLS;
        }
        eprintln!(
            "error: no record found for {} (looked for the .quarto copy) and the record \
             sources do not allow running the document",
            doc.display()
        );
        return exit::NO_RECORD;
    };

    let last_updated = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let options = ExportOptions {
        // A path that leaves the process uses forward slashes.
        source: args
            .source_uri
            .clone()
            .unwrap_or_else(|| quarto_util::to_forward_slashes(&doc)),
        last_updated,
    };
    let exported = match export_for_positron(&record, &options) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("error: {e}");
            return exit::BAD_RECORD;
        }
    };
    report(&exported, to_stdout);
    if exported.cells == 0 {
        eprintln!("error: the record has no cells Positron can display");
        return exit::NO_CELLS;
    }
    if to_stdout {
        let mut stdout = std::io::stdout().lock();
        if let Err(e) = stdout
            .write_all(&exported.bytes)
            .and_then(|()| stdout.flush())
        {
            eprintln!("error: cannot write the notebook to stdout: {e}");
            return exit::USAGE;
        }
    } else {
        if out.exists() {
            eprintln!("info: overwriting {}", out.display());
        }
        if let Err(e) = std::fs::write(out, &exported.bytes) {
            eprintln!("error: cannot write {}: {e}", out.display());
            return exit::USAGE;
        }
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

/// The one-line JSON report: stdout normally, stderr when the notebook takes stdout.
fn report(e: &ExportedCache, to_stdout: bool) {
    let line = json!({
        "cells": e.cells,
        "skipped": e.skipped.iter().map(|s| json!({"id": s.id, "reason": s.reason.as_str()})).collect::<Vec<_>>(),
        "dropped": e.dropped.iter().map(|d| json!({
            "id": d.cell_id, "mime": d.mime, "reason": d.reason.as_str(),
        })).collect::<Vec<_>>(),
    });
    if to_stdout {
        eprintln!("{line}");
    } else {
        println!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(input: PathBuf) -> ConvertArgs {
        ConvertArgs {
            input,
            from: None,
            to: "positron-ipynb".into(),
            output: None,
            metadata: vec![],
            source_uri: None,
        }
    }

    fn scratch(record: Option<&str>) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        let qmd = dir.join("doc.qmd");
        std::fs::write(&qmd, "---\ntitle: No code\n---\n\nJust prose.\n").unwrap();
        if let Some(rec) = record {
            let path = execution_record_path(&dir, &qmd);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, rec).unwrap();
        }
        (tmp, qmd)
    }

    fn native() -> Arc<dyn SystemRuntime> {
        Arc::new(NativeRuntime::new())
    }

    #[test]
    fn the_default_source_list_is_q2_then_engine() {
        assert_eq!(
            record_sources(),
            vec![RecordSource::Q2, RecordSource::Engine]
        );
    }

    #[test]
    fn q2_only_with_no_record_exits_10_and_runs_nothing() {
        let (tmp, qmd) = scratch(None);
        let out = tmp.path().join("o.ipynb");
        let code = export_positron(&args(qmd), &out, &[RecordSource::Q2], native());
        assert_eq!(code, exit::NO_RECORD);
        assert!(!out.exists());
    }

    #[test]
    fn engine_only_ignores_an_existing_dot_quarto_copy() {
        let (tmp, qmd) = scratch(Some("this is not json"));
        let out = tmp.path().join("o.ipynb");
        let code = export_positron(&args(qmd), &out, &[RecordSource::Engine], native());
        assert_ne!(code, exit::BAD_RECORD);
    }

    #[test]
    fn a_source_other_than_q2_or_engine_is_a_usage_error() {
        let (tmp, qmd) = scratch(None);
        let out = tmp.path().join("o.ipynb");
        let code = export_positron(&args(qmd), &out, &[RecordSource::Cache], native());
        assert_eq!(code, exit::USAGE);
    }
}
