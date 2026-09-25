/*
 * engine/content_processors/mod.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Native content-processor registry (Plan 7b).
 */

//! Native content-processor registry.
//!
//! A **content processor** owns *sniff + convert + `SourceInfo`* for one
//! non-qmd input format. It is not an engine — an engine merely *names* one
//! via a `processor:` field on a `claims-files` entry
//! (`crate::extension::types::ProcessorSpec`). This is what lets one
//! `percent` processor serve jupyter, julia, and marimo without each engine
//! authoring its own sniff regex.
//!
//! Both entry points (`sniff`, `convert`) are pure functions of
//! already-read bytes: no file I/O, no engine construction, no subprocess.
//! Native content sniffing happens exactly once, at claim time
//! (`stage::stages::source_conversion::SourceConversionStage`) — never at
//! Pass-1 discovery (`project::discovery`), which stays content-blind.

pub mod ipynb;
mod line_writer;
pub mod percent;
pub mod spin;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use quarto_source_map::SourceInfo;
use quarto_system_runtime::SystemRuntime;

use crate::extension::types::ProcessorSpec;

/// The `FileId` a processor's `Converted.source_info` must use for its
/// `Original` pieces (Plan 7b "A+" provenance).
///
/// **Load-bearing convention, not an arbitrary choice.** `ParseDocumentStage`
/// threads `Converted.source_info` through `pampa::readers::qmd::read`'s
/// `parent_source_info` parameter, which makes every AST node's `SourceInfo`
/// a `Substring` over it. For that `Substring` to resolve at render time
/// (ariadne snippets, `map_offset`), the original file must be registered in
/// the *same* `SourceContext` the parsed document uses
/// (`ast_context.source_context`) — but that context is created fresh,
/// empty, inside `qmd::read` itself, which always registers the *converted*
/// buffer first, unconditionally getting `FileId(0)`. `ParseDocumentStage`
/// therefore registers the original file immediately afterward via
/// `add_file_with_id(ORIGINAL_FILE_ID, ..)`, and this constant is the only
/// thing that has to agree between the two call sites. If `qmd::read` ever
/// stops being "exactly one file, always `FileId(0)`," this convention needs
/// to be revisited together with `ParseDocumentStage`.
pub const ORIGINAL_FILE_ID: quarto_source_map::FileId = quarto_source_map::FileId(1);

/// Runtime parameters for one processor invocation, derived from a claim's
/// `ProcessorSpec` (`spec_to_params`). Field names mirror the TS port
/// (`percent-script.ts`) rather than the YAML schema's `language`/`comment`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessorParams {
    Percent {
        comment_open: String,
        fence_language: String,
    },
    Spin,
    Ipynb,
}

/// Thin handle threaded through `convert` for a future asset-writing
/// processor (ipynb figures). Percent and spin never read `runtime` — see
/// the plan's "Forward-compatibility obligations" — but it is threaded
/// through regardless so a future processor needs no trait-signature
/// change to reach it.
#[derive(Clone)]
pub struct ProcessorContext {
    pub runtime: Arc<dyn SystemRuntime>,
}

/// One ephemeral virtual file a converter emitted for its output pieces to
/// point at — one per notebook cell for ipynb; percent/spin emit none. The
/// caller registers each into the document's `SourceContext` (Plan 7c
/// decision 6).
#[derive(Debug, Clone)]
pub struct ConvertedFile {
    /// The registered file's label — the pseudo-path diagnostics display,
    /// `{notebook}[cell N, kind]`.
    pub label: String,
    /// The cell's logical (unescaped) text.
    pub text: String,
    /// nbformat `cell_type` (`code` | `markdown` | `raw`). Feeds
    /// `FileOrigin::NotebookCell::cell_type`; label-parsing cannot recover
    /// it reliably (the notebook name may contain `[cell N, …]`).
    pub cell_type: String,
    /// nbformat `cell.id`, when the notebook carries one (mandatory from
    /// nbformat 4.5, absent in older notebooks). JSON output only.
    pub cell_id: Option<String>,
}

/// The result of converting one non-qmd input to qmd markdown.
#[derive(Debug, Clone)]
pub struct Converted {
    pub markdown: String,
    pub source_info: SourceInfo,
    /// Ephemeral source files the pieces above point at, for the caller to
    /// register in `SourceContext`. Empty for percent/spin — they map back
    /// into the already-registered original file.
    pub files: Vec<ConvertedFile>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProcessorError {
    #[error("{0}")]
    Other(String),
}

/// A content processor: native, zero-load sniff + convert.
pub trait ContentProcessor: Send + Sync {
    /// Fast, native, no-launch content sniff over already-read bytes.
    fn sniff(&self, path: &Path, content: &str, params: &ProcessorParams) -> bool;

    /// Convert to qmd, producing precise `SourceInfo` back into the
    /// original bytes.
    fn convert(
        &self,
        path: &Path,
        content: &str,
        params: &ProcessorParams,
        ctx: &ProcessorContext,
    ) -> Result<Converted, ProcessorError>;
}

struct Registry {
    processors: HashMap<&'static str, Box<dyn ContentProcessor>>,
}

impl Registry {
    fn new() -> Self {
        let mut processors: HashMap<&'static str, Box<dyn ContentProcessor>> = HashMap::new();
        processors.insert("percent", Box::new(percent::Percent));
        processors.insert("spin", Box::new(spin::Spin));
        processors.insert("ipynb", Box::new(ipynb::Ipynb));
        Self { processors }
    }
}

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::new)
}

/// Resolve a processor by name (test/diagnostic seam — production code goes
/// through `sniff`/`convert`, which also carry the params).
fn resolve(name: &str) -> Option<&'static dyn ContentProcessor> {
    registry().processors.get(name).map(|p| p.as_ref())
}

/// Map a schema-layer `ProcessorSpec` to a processor name + its runtime
/// params.
fn spec_to_params(spec: &ProcessorSpec) -> (&'static str, ProcessorParams) {
    match spec {
        ProcessorSpec::Percent { language, comment } => (
            "percent",
            ProcessorParams::Percent {
                comment_open: comment.clone(),
                fence_language: language.clone(),
            },
        ),
        ProcessorSpec::Spin => ("spin", ProcessorParams::Spin),
        ProcessorSpec::Ipynb => ("ipynb", ProcessorParams::Ipynb),
    }
}

/// Native, zero-load content sniff for a claim's processor. Pure — `content`
/// must already be read; this never touches disk or spawns anything.
pub fn sniff(spec: &ProcessorSpec, path: &Path, content: &str) -> bool {
    let (name, params) = spec_to_params(spec);
    match resolve(name) {
        Some(p) => p.sniff(path, content, &params),
        None => false,
    }
}

/// Native content conversion for a claim's processor. `runtime` is threaded
/// through as `ProcessorContext::runtime` — unused by percent/spin, kept
/// available for a future asset-writing processor (see the plan's
/// "Forward-compatibility obligations").
pub fn convert(
    spec: &ProcessorSpec,
    path: &Path,
    content: &str,
    runtime: &Arc<dyn SystemRuntime>,
) -> Result<Converted, ProcessorError> {
    let (name, params) = spec_to_params(spec);
    let processor = resolve(name)
        .ok_or_else(|| ProcessorError::Other(format!("unknown content processor '{name}'")))?;
    let ctx = ProcessorContext {
        runtime: Arc::clone(runtime),
    };
    processor.convert(path, content, &params, &ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- T-registry (Phase 2): registry resolves percent/spin by name. ---

    #[test]
    fn registry_resolves_percent_by_name() {
        assert!(resolve("percent").is_some());
    }

    #[test]
    fn registry_resolves_spin_by_name() {
        assert!(resolve("spin").is_some());
    }

    #[test]
    fn registry_resolves_ipynb_by_name() {
        assert!(resolve("ipynb").is_some());
    }

    #[test]
    fn registry_unknown_name_resolves_to_none() {
        assert!(resolve("bogus").is_none());
    }

    #[test]
    fn spec_to_params_percent_carries_language_and_comment() {
        let spec = ProcessorSpec::Percent {
            language: "julia".to_string(),
            comment: "#".to_string(),
        };
        let (name, params) = spec_to_params(&spec);
        assert_eq!(name, "percent");
        assert_eq!(
            params,
            ProcessorParams::Percent {
                comment_open: "#".to_string(),
                fence_language: "julia".to_string(),
            }
        );
    }

    #[test]
    fn spec_to_params_spin_carries_no_params() {
        let (name, params) = spec_to_params(&ProcessorSpec::Spin);
        assert_eq!(name, "spin");
        assert_eq!(params, ProcessorParams::Spin);
    }

    #[test]
    fn spec_to_params_ipynb_carries_no_params() {
        let (name, params) = spec_to_params(&ProcessorSpec::Ipynb);
        assert_eq!(name, "ipynb");
        assert_eq!(params, ProcessorParams::Ipynb);
    }

    #[test]
    fn convert_dispatches_through_registry_by_spec() {
        // "hello" has no chunk-opener, so real `spin` wraps it as a bare
        // code chunk — this test pins dispatch-by-spec (registry lookup +
        // params threading), not spin's own grammar (see
        // `content_processors::spin::tests` for that).
        let runtime: Arc<dyn SystemRuntime> = Arc::new(quarto_system_runtime::NativeRuntime::new());
        let result = convert(
            &ProcessorSpec::Spin,
            Path::new("script.R"),
            "hello",
            &runtime,
        )
        .expect("spin must succeed");
        assert_eq!(result.markdown, "\n```{r}\nhello\n```\n\n");
    }

    #[test]
    fn sniff_dispatches_through_registry_by_spec() {
        // "anything" has no roxygen `#' ---` header, so real `spin` sniff
        // correctly answers false — this test pins dispatch-by-spec, not
        // the sniff grammar itself (see `content_processors::spin::tests`).
        assert!(!sniff(
            &ProcessorSpec::Spin,
            Path::new("script.R"),
            "anything"
        ));
    }

    #[test]
    fn ipynb_dispatches_through_registry_by_spec() {
        // Pins dispatch-by-spec for ipynb (registry lookup + params
        // threading), not the notebook grammar itself (see
        // `content_processors::ipynb::tests` and the
        // `integration/ipynb_content_processor.rs` harness for that).
        let notebook = r#"{"cells":[{"cell_type":"markdown","metadata":{},"source":["hello"]}],"metadata":{"kernelspec":{"name":"python3"}},"nbformat":4,"nbformat_minor":5}"#;
        assert!(sniff(
            &ProcessorSpec::Ipynb,
            Path::new("nb.ipynb"),
            notebook
        ));
        assert!(!sniff(
            &ProcessorSpec::Ipynb,
            Path::new("nb.ipynb"),
            "not a notebook"
        ));

        let runtime: Arc<dyn SystemRuntime> = Arc::new(quarto_system_runtime::NativeRuntime::new());
        let converted = convert(
            &ProcessorSpec::Ipynb,
            Path::new("nb.ipynb"),
            notebook,
            &runtime,
        )
        .expect("ipynb must convert through the registry");
        assert!(converted.markdown.contains("hello"));
        // One ephemeral virtual file per cell (plan decision 5).
        assert_eq!(converted.files.len(), 1);
    }

    // --- ProcessorContext threads a runtime handle (T-registry, Phase 2). ---

    #[test]
    fn processor_context_threads_runtime_handle() {
        struct Capture {
            seen: std::sync::Mutex<Option<Arc<dyn SystemRuntime>>>,
        }

        impl ContentProcessor for Capture {
            fn sniff(&self, _path: &Path, _content: &str, _params: &ProcessorParams) -> bool {
                false
            }

            fn convert(
                &self,
                _path: &Path,
                content: &str,
                _params: &ProcessorParams,
                ctx: &ProcessorContext,
            ) -> Result<Converted, ProcessorError> {
                *self.seen.lock().unwrap() = Some(Arc::clone(&ctx.runtime));
                Ok(Converted {
                    markdown: content.to_string(),
                    source_info: SourceInfo::for_test(),
                    files: Vec::new(),
                })
            }
        }

        let runtime: Arc<dyn SystemRuntime> = Arc::new(quarto_system_runtime::NativeRuntime::new());
        let capture = Capture {
            seen: std::sync::Mutex::new(None),
        };
        let ctx = ProcessorContext {
            runtime: Arc::clone(&runtime),
        };
        capture
            .convert(Path::new("x"), "y", &ProcessorParams::Spin, &ctx)
            .unwrap();

        let seen = capture
            .seen
            .lock()
            .unwrap()
            .clone()
            .expect("convert must have run");
        assert!(
            Arc::ptr_eq(&seen, &runtime),
            "ProcessorContext.runtime must be the same handle passed to `convert`"
        );
    }
}
