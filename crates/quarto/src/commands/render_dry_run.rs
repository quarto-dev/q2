//! Presentation of `q2 render --dry-run` (bd-render-dry-run-dyiq7d08).
//!
//! The orchestrator does the work
//! ([`ProjectPipeline::with_dry_run`](quarto_core::project::orchestrator::ProjectPipeline::with_dry_run))
//! and hands back a [`DryRunReport`] on the render summary. This module
//! turns that report into:
//!
//! - the text summary line (`Checked N files: …`) and, under `-v`, the
//!   list of files a render would produce, both on stderr like every
//!   other status line;
//! - under `--json-errors`, one [`JsonDryRunSummary`] object on
//!   **stdout**. Stderr keeps carrying only the existing diagnostic
//!   NDJSON records (`JsonDiagnostic`, `JsonPass1Failure`), so a consumer
//!   that already parses `--json-errors` needs no change, and the end of
//!   the run is marked by a record that says how much was checked —
//!   which a dry run, having no output files, otherwise could not tell
//!   apart from checking nothing.
//!
//! The summary's JSON Schema is checked in at
//! `crates/quarto/schemas/render-dry-run-summary.json`; the
//! `dry_run_summary_schema_matches_committed` test below guards it
//! against drift (regenerate with `QUARTO_REGEN_SCHEMAS=1`).

use std::path::Path;

use quarto_core::project::orchestrator::{DiagnosticCounts, DryRunFile, DryRunReport};
use schemars::JsonSchema;
use serde::Serialize;

/// The `--dry-run --json-errors` summary object, printed once on stdout
/// at the end of the run.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JsonDryRunSummary {
    /// JSON Schema URI describing this object's shape. Const value is
    /// [`JsonDryRunSummary::SCHEMA_URL`].
    #[serde(rename = "$schema")]
    #[schemars(rename = "$schema")]
    pub schema: &'static str,
    /// Always `true`: this object is only printed by a dry run.
    pub dry_run: bool,
    /// The project root (the document's directory for a single file), in
    /// the same form as `source_file` in the diagnostic records.
    pub project_dir: String,
    /// The project's own format key. Each file may resolve to its own
    /// format; see `files[].format`.
    pub format: String,
    /// Files whose parse and metadata merge ran (successfully or not).
    /// Under `--fail-fast` this can be fewer than the project's files.
    pub files_checked: usize,
    /// Files that failed to parse or merge (each also has a
    /// `JsonPass1Failure` record on stderr).
    pub files_failed: usize,
    /// Error diagnostics reported on stderr, after `--strict` promotion.
    /// A failure with no structured diagnostic counts as one error.
    pub errors: usize,
    /// Warning diagnostics reported on stderr (always 0 under `--strict`).
    pub warnings: usize,
    /// Whether `--strict` was given (warnings promoted to errors).
    pub strict: bool,
    /// `true` when `--fail-fast` stopped the run at the first failure;
    /// the report is then not exhaustive.
    pub stopped_early: bool,
    /// The files a real render would produce, with their resolved
    /// formats. Present only with `-v` (the list can be long); absent,
    /// not empty, otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<JsonDryRunFile>>,
}

/// One file a real render would produce.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JsonDryRunFile {
    /// The source file, in the same form as `source_file` in the
    /// diagnostic records.
    pub source_file: String,
    /// The format key the file resolves to (`html`, `revealjs`, …).
    pub format: String,
    /// `ok`, or `failed` when the file failed to parse (a real render
    /// would skip it).
    pub status: JsonDryRunFileStatus,
}

/// Whether a would-render file passed the dry run's checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum JsonDryRunFileStatus {
    Ok,
    Failed,
}

impl JsonDryRunSummary {
    /// JSON Schema URI for this shape, versioned like the diagnostic
    /// records' (`…/v1/json-diagnostic.json`).
    pub const SCHEMA_URL: &'static str =
        "https://quarto.org/schemas/v1/render-dry-run-summary.json";
}

/// What [`json_summary`] needs besides the orchestrator's report.
pub struct SummaryInputs<'a> {
    pub report: &'a DryRunReport,
    pub counts: DiagnosticCounts,
    pub project_dir: &'a Path,
    pub format: &'a str,
    pub strict: bool,
    pub stopped_early: bool,
    /// `-v` given: include `files`.
    pub list_files: bool,
    /// How a path is spelled on the wire (the `--json-errors` form).
    pub wire_path: fn(&Path) -> String,
}

/// Build the stdout summary object for `--dry-run --json-errors`.
pub fn json_summary(inputs: &SummaryInputs<'_>) -> JsonDryRunSummary {
    let wire = inputs.wire_path;
    JsonDryRunSummary {
        schema: JsonDryRunSummary::SCHEMA_URL,
        dry_run: true,
        project_dir: wire(inputs.project_dir),
        format: inputs.format.to_string(),
        files_checked: inputs.report.files_checked,
        files_failed: inputs.report.files_failed,
        errors: inputs.counts.errors,
        warnings: inputs.counts.warnings,
        strict: inputs.strict,
        stopped_early: inputs.stopped_early,
        files: inputs.list_files.then(|| {
            inputs
                .report
                .would_render
                .iter()
                .map(|f| JsonDryRunFile {
                    source_file: wire(&f.input),
                    format: f.format.clone(),
                    status: if f.failed {
                        JsonDryRunFileStatus::Failed
                    } else {
                        JsonDryRunFileStatus::Ok
                    },
                })
                .collect()
        }),
    }
}

/// The text summary line, without the counts clause (the caller appends
/// it, colored, when there is something to report): `Checked 1 file` /
/// `Checked 42 files`.
pub fn summary_line(report: &DryRunReport) -> String {
    let n = report.files_checked;
    format!("Checked {n} {}", if n == 1 { "file" } else { "files" })
}

/// The `-v` list of files a render would produce, one per line, paths
/// relative to `project_dir`. Empty when nothing would render.
pub fn would_render_lines(files: &[DryRunFile], project_dir: &Path) -> Vec<String> {
    if files.is_empty() {
        return vec!["Would render no files".to_string()];
    }
    let n = files.len();
    let mut lines = vec![format!(
        "Would render {n} {}:",
        if n == 1 { "file" } else { "files" }
    )];
    for f in files {
        let rel = f.input.strip_prefix(project_dir).unwrap_or(&f.input);
        let mut line = format!("  {} ({})", rel.display(), f.format);
        if f.failed {
            line.push_str(" — would be skipped: failed to parse");
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn report() -> DryRunReport {
        DryRunReport {
            files_checked: 2,
            files_failed: 1,
            would_render: vec![
                DryRunFile {
                    input: PathBuf::from("/p/index.qmd"),
                    format: "html".into(),
                    failed: false,
                },
                DryRunFile {
                    input: PathBuf::from("/p/sub/broken.qmd"),
                    format: "revealjs".into(),
                    failed: true,
                },
            ],
        }
    }

    fn plain(p: &Path) -> String {
        p.display().to_string()
    }

    fn inputs(report: &DryRunReport, list_files: bool) -> SummaryInputs<'_> {
        SummaryInputs {
            report,
            counts: DiagnosticCounts {
                errors: 1,
                warnings: 3,
            },
            project_dir: Path::new("/p"),
            format: "html",
            strict: false,
            stopped_early: false,
            list_files,
            wire_path: plain,
        }
    }

    #[test]
    fn json_summary_omits_files_without_verbose() {
        let r = report();
        let value = serde_json::to_value(json_summary(&inputs(&r, false))).unwrap();
        assert!(value.get("files").is_none(), "{value}");
        assert_eq!(value["$schema"], JsonDryRunSummary::SCHEMA_URL);
        assert_eq!(value["dry_run"], true);
        assert_eq!(value["files_checked"], 2);
        assert_eq!(value["files_failed"], 1);
        assert_eq!(value["errors"], 1);
        assert_eq!(value["warnings"], 3);
    }

    #[test]
    fn json_summary_lists_files_with_verbose() {
        let r = report();
        let value = serde_json::to_value(json_summary(&inputs(&r, true))).unwrap();
        let files = value["files"].as_array().expect("files array");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0]["status"], "ok");
        assert_eq!(files[1]["status"], "failed");
        assert_eq!(files[1]["format"], "revealjs");
    }

    #[test]
    fn summary_line_pluralizes() {
        let mut r = report();
        assert_eq!(summary_line(&r), "Checked 2 files");
        r.files_checked = 1;
        assert_eq!(summary_line(&r), "Checked 1 file");
    }

    #[test]
    fn would_render_lines_are_project_relative() {
        let r = report();
        let lines = would_render_lines(&r.would_render, Path::new("/p"));
        assert_eq!(lines[0], "Would render 2 files:");
        assert_eq!(lines[1], "  index.qmd (html)");
        let broken = Path::new("sub").join("broken.qmd");
        assert_eq!(
            lines[2],
            format!(
                "  {} (revealjs) — would be skipped: failed to parse",
                broken.display()
            )
        );
        assert_eq!(
            would_render_lines(&[], Path::new("/p")),
            vec!["Would render no files".to_string()]
        );
    }

    // ── Schema drift guard (same pattern as quarto-error-reporting's
    //    tests/schema_drift.rs) ─────────────────────────────────────────

    /// Sort object keys at every depth so the checked-in file does not
    /// depend on whether `serde_json/preserve_order` is active.
    fn canonicalize_keys(value: serde_json::Value) -> serde_json::Value {
        use serde_json::Value;
        match value {
            Value::Object(map) => {
                let mut entries: Vec<(String, Value)> = map.into_iter().collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                let mut out = serde_json::Map::new();
                for (k, v) in entries {
                    out.insert(k, canonicalize_keys(v));
                }
                Value::Object(out)
            }
            Value::Array(arr) => Value::Array(arr.into_iter().map(canonicalize_keys).collect()),
            other => other,
        }
    }

    /// The checked-in schema must match the one generated from
    /// [`JsonDryRunSummary`]. Regenerate after a wire-shape change with
    /// `QUARTO_REGEN_SCHEMAS=1 cargo nextest run -p quarto render_dry_run`.
    #[test]
    fn dry_run_summary_schema_matches_committed() {
        let schema = schemars::schema_for!(JsonDryRunSummary);
        let value = canonicalize_keys(serde_json::to_value(&schema).unwrap());
        let mut generated = serde_json::to_string_pretty(&value).unwrap();
        generated.push('\n');

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("schemas")
            .join("render-dry-run-summary.json");
        let existing = std::fs::read_to_string(&path).ok();
        if existing.as_deref().map(|s| s.replace("\r\n", "\n")) == Some(generated.clone()) {
            return;
        }
        if std::env::var("QUARTO_REGEN_SCHEMAS").is_ok_and(|v| v == "1") {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &generated).unwrap();
            return;
        }
        panic!(
            "JSON Schema drift for {}: it does not match the schema generated from \
             JsonDryRunSummary. Regenerate with\n\n\
             \tQUARTO_REGEN_SCHEMAS=1 cargo nextest run -p quarto render_dry_run\n\n\
             and review the diff before committing.",
            path.display()
        );
    }
}
