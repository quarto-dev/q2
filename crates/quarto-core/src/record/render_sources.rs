/*
 * render_sources.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Where `q2 render` gets a document's execution record (OPR-6).
 */

//! `q2 render` reads `execute: record-sources` (OPR-5) once per document and either splices a
//! stored record or lets the engine run.
//!
//! [`select_render_record`] walks the document's list in order, one source at a time: the
//! first source that supplies a record that is **fresh and recorded for this render's format**
//! wins. A stale or format-mismatched record counts as not supplied (a preview shows a stale
//! record; a render must not). `engine` ends the walk when the engine may run: reaching it
//! means "run the code now", so nothing after it is read. When the engine may not run
//! (`--no-execute`, a narrowed `--static` policy) the entry is skipped, and a document no
//! source supplies is left inert with a warning that names why.
//!
//! The default list is `engine`, which is today's behavior, so the common render does none of
//! this work: [`select_render_record`] returns at once.

use std::path::Path;
use std::sync::Arc;

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_system_runtime::SystemRuntime;

use super::source_key::{CommandSources, RECORD_SOURCES_KEY, Trust, resolve_record_sources};
use super::sources::{
    ImportContext, OutcomeKind, RecordLookup, RecordSource, RecordSources, WalkContext,
    resolve_records,
};
use super::{
    Record, execution_record_path, project_relative_source, read_record, record_format_mismatch,
    source_staleness,
};
use crate::engine::EngineRegistry;
use crate::engine::preview_record::{
    CaptureRun, SourceInputs, SourceInputsOptions, compute_source_inputs_with,
};
use crate::project::ProjectContext;

/// How a render reads the list: which command it speaks for, and whether the author is the
/// host (`None`, `-M` is a plain override) or a guest bounded by a ceiling (`Some`).
#[derive(Debug, Clone)]
pub struct RenderRecordSources {
    pub command: CommandSources,
    /// The host's list, when guests can reach this render (`q2 preview --share`'s render
    /// routes): a document may only reorder or narrow it.
    pub ceiling: Option<RecordSources>,
}

impl Default for RenderRecordSources {
    fn default() -> Self {
        Self {
            command: CommandSources::RENDER,
            ceiling: None,
        }
    }
}

/// What one render decided about its records.
#[derive(Debug, Default)]
pub struct RenderSelection {
    /// A fresh record for this document and format: splice it and do not start the engine.
    pub record: Option<Arc<Record>>,
    /// The engine must not start for this document (a record was supplied, or no source did and
    /// `engine` is not in the list).
    pub no_engine: bool,
    /// Info and warning lines for the render's diagnostics.
    pub diagnostics: Vec<DiagnosticMessage>,
}

/// Everything the selection needs from the render.
pub struct SelectInputs<'a> {
    pub project: &'a ProjectContext,
    pub abs_path: &'a Path,
    pub runtime: &'a Arc<dyn SystemRuntime>,
    pub config: &'a RenderRecordSources,
    /// The render's target format as the splice names it (`ctx.format.identifier`).
    pub target_format: &'a str,
    /// Whether the render's execution policy lets this document run its code.
    pub engine_allowed: bool,
    /// `--replay` or `QUARTO_REPLAY` supplied a trace: it wins over any key.
    pub replay: bool,
    /// A registry override for the engine-ownership resolution (tests).
    pub registry: Option<Arc<EngineRegistry>>,
}

/// The `q2` source for a render: the `.quarto/execution-records/<doc>.ipynb` copy a
/// `record: true` render writes. `Cache` has nothing (a render has no preview cache) and
/// `Engine` is never asked (the walk stops there).
struct RenderLookup<'a> {
    project: &'a ProjectContext,
    abs_path: &'a Path,
}

#[async_trait::async_trait(?Send)]
impl RecordLookup for RenderLookup<'_> {
    async fn lookup(
        &self,
        source: &RecordSource,
        _inputs: &SourceInputs,
    ) -> Result<Option<CaptureRun>, super::sources::LookupError> {
        use super::sources::LookupError;
        if !matches!(source, RecordSource::Q2) {
            return Ok(None);
        }
        let path = execution_record_path(&self.project.dir, self.abs_path);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(LookupError::Unreadable(format!(
                    "reading {}: {e}",
                    path.display()
                )));
            }
        };
        let record = read_record(&text).map_err(|e| {
            LookupError::Unreadable(format!("{} is not a record: {e}", path.display()))
        })?;
        Ok(Some(CaptureRun {
            captures: Vec::new(),
            record: Some(record),
        }))
    }

    async fn run_engine(
        &self,
        _inputs: &SourceInputs,
    ) -> Result<super::sources::EngineResult, super::sources::EngineError> {
        Err(super::sources::EngineError::Failed(
            "a render runs the engine through its pipeline, not through the record walk".into(),
        ))
    }
}

fn diagnostic(warning: bool, text: String) -> DiagnosticMessage {
    if warning {
        DiagnosticMessageBuilder::warning(text).build()
    } else {
        DiagnosticMessageBuilder::info(text).build()
    }
}

/// Read the document's list and choose a record, or decide the engine runs, or leave the
/// document inert. An `Err` is a key the render cannot use (an unknown entry, a bad directory).
pub async fn select_render_record(inp: SelectInputs<'_>) -> Result<RenderSelection, String> {
    let content = inp
        .runtime
        .file_read_string(inp.abs_path)
        .unwrap_or_default();
    let trust = match &inp.config.ceiling {
        Some(allowed) => Trust::Ceiling { allowed },
        None => Trust::Override,
    };
    let resolved = resolve_record_sources(
        inp.project,
        inp.abs_path,
        Some(&content),
        inp.runtime.as_ref(),
        &inp.config.command,
        trust,
    )
    .map_err(|e| e.to_string())?;
    let mut selection = RenderSelection::default();
    for note in &resolved.notes {
        selection
            .diagnostics
            .push(diagnostic(note.warning, note.text.clone()));
    }
    let explicit = resolved.origin.is_some();

    if inp.replay {
        if explicit {
            selection.diagnostics.push(diagnostic(
                false,
                format!(
                    "`--replay` (or `QUARTO_REPLAY`) is set, so `{RECORD_SOURCES_KEY}` is \
                     ignored for this render"
                ),
            ));
        }
        return Ok(selection);
    }

    // The default, `engine` alone, is today's behavior: no inputs, no walk.
    // A ceiling's default can hold what this command cannot use (`cache`); drop it quietly.
    let list = RecordSources::new(
        resolved
            .sources
            .iter()
            .filter(|s| inp.config.command.allows(s))
            .cloned()
            .collect(),
    );
    if list.as_slice() == [RecordSource::Engine] && inp.engine_allowed {
        return Ok(selection);
    }
    if list.as_slice() == [RecordSource::Engine] && !explicit {
        // Vetoed by the policy with the default list: the policy's own quiet skip applies.
        return Ok(selection);
    }

    let wants_import = list
        .iter()
        .any(|s| matches!(s, RecordSource::Positron | RecordSource::Dir(_)));
    let needs_inputs = list.iter().any(|s| *s != RecordSource::Engine)
        || !inp.engine_allowed
        || list.as_slice().is_empty();
    let inputs = if needs_inputs {
        Some(
            compute_source_inputs_with(
                inp.abs_path,
                inp.project,
                inp.runtime.clone(),
                SourceInputsOptions {
                    ownership: wants_import,
                    registry: inp.registry.clone(),
                    format: Some(inp.target_format.to_string()),
                },
            )
            .await
            .map_err(|e| format!("reading the record sources failed: {e}"))?,
        )
    } else {
        None
    };

    let relative = project_relative_source(&inp.project.dir, inp.abs_path);
    let lookup = RenderLookup {
        project: inp.project,
        abs_path: inp.abs_path,
    };
    let mut reasons: Vec<String> = Vec::new();
    let mut engine_runs = false;
    for source in list.iter() {
        if *source == RecordSource::Engine {
            if inp.engine_allowed {
                engine_runs = true;
                break;
            }
            reasons.push("engine: not allowed to run (the render's execution policy)".into());
            continue;
        }
        let inputs = inputs.as_ref().expect("computed when a source is read");
        let one = RecordSources::new(vec![source.clone()]);
        let walk = WalkContext {
            import: Some(ImportContext {
                abs_path: inp.abs_path,
                source: Some(&relative),
                positron_dir: None,
            }),
            list: &one,
            engine_enabled: false,
            record_gate: true,
        };
        let resolution = resolve_records(&walk, inputs, &lookup).await;
        let label = source.label();
        match resolution.chosen {
            Some(super::sources::Chosen::Run { source, run }) => {
                if source_staleness(&source, &run, inputs) {
                    reasons.push(format!(
                        "{label}: the record is stale (the document's code or options changed \
                         since it was recorded)"
                    ));
                    continue;
                }
                if let Some(record) = run.record.as_ref()
                    && let Some(recorded) = record_format_mismatch(record, inp.target_format)
                {
                    reasons.push(format!(
                        "{label}: the record is stale for this render (it was recorded for \
                         `{recorded}`, this render is `{}`)",
                        inp.target_format
                    ));
                    continue;
                }
                if let Some(record) = run.record {
                    selection.record = Some(Arc::new(record));
                    selection.no_engine = true;
                    return Ok(selection);
                }
                reasons.push(format!("{label}: it supplied no execution record"));
            }
            _ => {
                let why = resolution.outcomes.first().map_or_else(
                    || "no record for this document".to_string(),
                    |o| match &o.kind {
                        OutcomeKind::Nothing | OutcomeKind::Supplied => {
                            "no record for this document".to_string()
                        }
                        OutcomeKind::Refused(m) => format!("refused: {m}"),
                        OutcomeKind::Unavailable(m) => format!("not available: {m}"),
                        OutcomeKind::Failed(m) => format!("failed: {m}"),
                    },
                );
                reasons.push(format!("{label}: {why}"));
            }
        }
    }

    if engine_runs {
        return Ok(selection);
    }
    // Nothing supplied a record and the engine may not run: the cells stay inert.
    selection.no_engine = true;
    let has_code = inputs
        .as_ref()
        .is_some_and(|i| !i.sugared_blocks.is_empty());
    if has_code && explicit {
        let detail = if reasons.is_empty() {
            "no source is listed".to_string()
        } else {
            reasons.join("; ")
        };
        selection.diagnostics.push(
            DiagnosticMessageBuilder::warning("No execution record for this document")
                .problem(format!(
                    "No source had a record (or none was fresh) and `engine` is not available, \
                     so the code cells are shown without output ({detail}). Add `engine` to \
                     `{RECORD_SOURCES_KEY}` to run them."
                ))
                .build(),
        );
    }
    Ok(selection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{Record, read_record};
    use quarto_system_runtime::NativeRuntime;

    fn basic() -> Record {
        read_record(include_str!("../../tests/fixtures/records/basic.json")).unwrap()
    }

    /// `record_format_mismatch` needs a cell that names a hash profile and carries a format;
    /// a record with neither (Jupyter's) never mismatches.
    #[test]
    fn a_record_that_names_no_format_never_mismatches() {
        assert_eq!(record_format_mismatch(&basic(), "html"), None);
    }

    fn select(
        front_matter: &str,
        replay: bool,
        engine_allowed: bool,
    ) -> (tempfile::TempDir, RenderSelection) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        let doc = dir.join("doc.qmd");
        std::fs::write(
            &doc,
            format!("---\n{front_matter}---\n\n```{{python}}\n1 + 1\n```\n"),
        )
        .unwrap();
        let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        let project = ProjectContext::discover(&doc, runtime.as_ref()).unwrap();
        let config = RenderRecordSources::default();
        let selection = pollster::block_on(select_render_record(SelectInputs {
            project: &project,
            abs_path: &doc,
            runtime: &runtime,
            config: &config,
            target_format: "html",
            engine_allowed,
            replay,
            registry: None,
        }))
        .unwrap();
        (tmp, selection)
    }

    fn texts(s: &RenderSelection) -> Vec<String> {
        s.diagnostics.iter().map(|d| d.title.clone()).collect()
    }

    /// `--replay` wins over a key, with one info line, and reads nothing.
    #[test]
    fn replay_wins_over_a_key() {
        let key = "execute:\n  record-sources: q2\n";
        let (_t, s) = select(key, true, true);
        assert!(s.record.is_none() && !s.no_engine);
        let titles = texts(&s);
        assert_eq!(titles.len(), 1, "{titles:?}");
        assert!(titles[0].contains("--replay") && titles[0].contains("record-sources"));
        // No key: nothing to say.
        let (_t, s) = select("", true, true);
        assert!(s.diagnostics.is_empty());
    }

    /// OPR-6, preview render routes under `--share`: the request's `metadata` (the runtime layer)
    /// and a document key outside the host's list are both ignored, with the warning on the
    /// page, and nothing is read from a source the host did not allow.
    #[test]
    fn a_ceiling_ignores_request_metadata_and_out_of_set_document_keys() {
        use quarto_system_runtime::NativeRuntime;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        let doc = dir.join("doc.qmd");
        std::fs::write(
            &doc,
            "---\nexecute:\n  record-sources: \"positron,q2\"\n---\n\n```{python}\n1\n```\n",
        )
        .unwrap();
        let native: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
        // The guest request's `metadata` asks for the host's Positron cache.
        let overlay = Arc::new(crate::overlay_runtime::RenderOverlay::with_metadata(
            serde_json::json!({"execute": {"record-sources": "positron"}}),
        ));
        let runtime = overlay.apply(native);
        let project = ProjectContext::discover(&doc, runtime.as_ref()).unwrap();
        let config = RenderRecordSources {
            command: CommandSources::RENDER,
            ceiling: Some(RecordSources::new(vec![RecordSource::Q2])),
        };
        let selection = pollster::block_on(select_render_record(SelectInputs {
            project: &project,
            abs_path: &doc,
            runtime: &runtime,
            config: &config,
            target_format: "html",
            engine_allowed: false,
            replay: false,
            registry: None,
        }))
        .unwrap();
        let titles = texts(&selection);
        // One warning for the document's `positron`, and the "no record" warning for the `q2`
        // that was allowed and had nothing. No Positron source was walked.
        assert!(
            titles
                .iter()
                .any(|t| t.contains("`positron`") && t.contains("ignored")),
            "{titles:?}"
        );
        assert!(selection.record.is_none() && selection.no_engine);
        let detail: String = selection
            .diagnostics
            .iter()
            .filter_map(|d| d.problem.as_ref().map(|p| p.as_str().to_string()))
            .collect();
        assert!(!detail.contains("positron"), "{detail}");
    }

    /// The default list (`engine`) does no work and says nothing, vetoed or not.
    #[test]
    fn the_default_list_is_a_no_op() {
        for allowed in [true, false] {
            let (_t, s) = select("", false, allowed);
            assert!(s.record.is_none() && !s.no_engine && s.diagnostics.is_empty());
        }
    }
}
