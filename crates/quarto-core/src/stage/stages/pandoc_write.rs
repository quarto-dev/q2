/*
 * stage/stages/pandoc_write.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The Pandoc-hybrid leg's writer stage: serializes the wire-format AST,
 * shells out to a real `pandoc` subprocess (with the vendored Q1 Lua
 * filters), and writes its own output file.
 */

//! `PandocWriteStage` — the docx/pptx tail of the Pandoc-hybrid render leg.
//!
//! Native-only: this stage shells out to a real `pandoc` binary via
//! `std::process::Command` and materializes the vendored filter tree via
//! `crate::pandoc_filters::bundle::extract_share_tree`, into the per-render
//! `ctx.temp_dir()` (no real filesystem on `wasm32-unknown-unknown`).
//! Mirrors the same gate as `crate::pandoc_filters::{bundle, harness}`.
//!
//! Unlike [`super::render_html::RenderHtmlBodyStage`], this stage's
//! `RenderedOutput.content` is always empty — no binary bytes travel
//! through `PipelineData`. The pandoc subprocess writes the output file
//! directly at `ctx.output_path()`, and downstream consumers (`FinalOutput`
//! relocation, etc.) key off `output_path`, not `content`.
//!
//! See `claude-notes/plans/2026-09-18-pandoc-hybrid-P4-implementation.md`
//! Task 9.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use async_trait::async_trait;
use quarto_error_reporting::DiagnosticMessage;

use crate::format::FormatIdentifier;
use crate::language::LanguageTerms;
use crate::pandoc_filters::bundle::{extract_formats_tree, extract_share_tree};
use crate::pandoc_filters::diagnostics::{classify_pandoc_stderr, nonzero_exit_error};
use crate::pandoc_filters::params::FilterParamsBuilder;
use crate::pandoc_filters::params_codec::encode_params_blob;
use crate::pandoc_filters::version;
use crate::stage::{
    PipelineData, PipelineDataKind, PipelineError, PipelineStage, RenderedOutput, StageContext,
};

/// Decides how to handle a completed pandoc invocation's exit status and
/// captured stderr — the corrected policy (commit `0b295831e`): capture
/// stderr **unconditionally**, not only on failure.
///
/// On success, classifies `[WARNING]`-shaped stderr lines as `Q-11-1`
/// diagnostics via [`classify_pandoc_stderr`] (may return an empty `Vec`).
/// On failure, builds the `Q-20-3` error via [`nonzero_exit_error`],
/// wrapping stderr verbatim and naming `json_path`.
///
/// `success`/`status_desc` are taken separately rather than as a single
/// `std::process::ExitStatus` so this function stays platform-neutral and
/// directly unit-testable with injected values (see
/// `crates/quarto-core/tests/integration/pandoc_transport.rs` T10.1-T10.5).
pub fn classify_pandoc_completion(
    stage_name: &str,
    success: bool,
    status_desc: &str,
    stderr: &str,
    json_path: &Path,
) -> Result<Vec<DiagnosticMessage>, PipelineError> {
    if success {
        Ok(classify_pandoc_stderr(stderr))
    } else {
        Err(nonzero_exit_error(
            stage_name,
            status_desc,
            stderr,
            json_path,
        ))
    }
}

/// Removes the temp JSON input on a successful render; leaves it on disk
/// on failure, where a future debugging session may want to inspect the
/// exact input pandoc choked on.
///
/// A missing/already-removed file on the success path is not an error —
/// `remove_file`'s result is deliberately discarded.
pub fn retain_temp_json_unless_success(success: bool, json_path: &Path) {
    if success {
        let _ = std::fs::remove_file(json_path);
    }
}

/// Resolves the `pandoc` binary via `runtime.find_binary` (honouring
/// `QUARTO_PANDOC`, per `render.rs`'s `BinaryDependencies::discover`) and
/// runs [`version::gate`] against it, surfacing `Q-20-1`/`Q-20-2` as a
/// `PipelineError` before any real invocation is attempted.
///
/// Returns the resolved binary path (or the literal `"pandoc"` when
/// resolution fails — a state `gate` above already turned into an `Err`,
/// so this fallback value is never actually reached in a caller that
/// propagates the `?`).
fn resolve_and_gate_pandoc(
    stage_name: &str,
    runtime: &dyn quarto_system_runtime::SystemRuntime,
) -> Result<std::path::PathBuf, PipelineError> {
    let pandoc_path = runtime.find_binary("pandoc", "QUARTO_PANDOC");

    let version_output = pandoc_path
        .as_ref()
        .and_then(|p| Command::new(p).arg("--version").output().ok());
    let version_str = version_output.map(|o| String::from_utf8_lossy(&o.stdout).into_owned());

    version::gate(version_str.as_deref())
        .map_err(|diag| PipelineError::stage_error_with_diagnostics(stage_name, vec![diag]))?;

    Ok(pandoc_path.unwrap_or_else(|| std::path::PathBuf::from("pandoc")))
}

/// Format-specific extra pandoc CLI args, appended before `-o <output>`.
///
/// Epub only, for now (Q1's `createEbookFormat`,
/// `claude-notes/plans/2026-09-18-pandoc-hybrid-epub.md` Phase 1):
/// `--default-image-extension=png`, `--math-method=mathml` (epub3's own
/// default already produces MathML — set explicitly to match Q1's intent
/// rather than rely on an unstated Pandoc default), two
/// `--include-in-header` CSS files extracted from the embedded
/// `FORMATS_DIR` tree, and (when the document sets it) `--split-level`
/// from the `epub-chapter-level` metadata key — Q1's name for what Pandoc
/// itself calls `--split-level` (`--epub-chapter-level` is Pandoc's own
/// deprecated synonym for the same flag). Deliberately **not** merged into
/// one file (Q1's `merge-includes: false`) — passing two separate
/// `--include-in-header` flags already keeps them from colliding, with no
/// merge step to disable.
/// Collects the string-bearing leaves of a scalar-or-array metadata value
/// (mirrors `project::format_paths::for_each_entry`, which is private to
/// that module).
fn entry_strings(value: &quarto_pandoc_types::ConfigValue) -> Vec<String> {
    match value.as_array() {
        Some(items) => items.iter().filter_map(|v| v.as_plain_text()).collect(),
        None => value.as_plain_text().into_iter().collect(),
    }
}

/// Resolves a `mark_format_path_values`-normalized path value (already
/// document-relative — see `project::format_paths`) against the
/// document's own directory, producing an absolute path pandoc's
/// subprocess can open regardless of its own cwd.
fn resolve_doc_relative(doc_dir: &Path, declared: &str) -> PathBuf {
    doc_dir.join(declared)
}

fn epub_extra_args(
    temp_dir: &Path,
    doc_path: &Path,
    meta: &quarto_pandoc_types::ConfigValue,
) -> Result<Vec<OsString>, PipelineError> {
    let doc_dir = doc_path.parent().unwrap_or_else(|| Path::new("."));
    let formats_root = temp_dir.join("pandoc-formats");
    std::fs::create_dir_all(&formats_root).map_err(|e| {
        PipelineError::stage_error(
            "pandoc-write",
            format!("failed to create formats directory: {e}"),
        )
    })?;
    extract_formats_tree(&formats_root).map_err(|e| {
        PipelineError::stage_error(
            "pandoc-write",
            format!("failed to materialize vendored format resources: {e}"),
        )
    })?;
    let formats_dest = formats_root.join("formats");
    let mut args = vec![
        OsString::from("--default-image-extension=png"),
        OsString::from("--math-method=mathml"),
        {
            let mut arg = OsString::from("--include-in-header=");
            arg.push(formats_dest.join("html").join("styles-callout.html"));
            arg
        },
        {
            let mut arg = OsString::from("--include-in-header=");
            arg.push(formats_dest.join("epub").join("styles.html"));
            arg
        },
    ];
    if let Some(level) = meta
        .get("epub-chapter-level")
        .and_then(|v| v.as_int_lenient())
    {
        args.push(OsString::from(format!("--split-level={level}")));
    }

    // Single-valued path keys: mark_format_path_values normalized these
    // to a document-relative `Path` value at merge time (see
    // `project::format_paths::FORMAT_PATH_KEYS`); resolve against the
    // document's own directory to hand pandoc an absolute path.
    for (key, flag) in [
        ("epub-cover-image", "--epub-cover-image="),
        ("epub-metadata", "--epub-metadata="),
    ] {
        if let Some(declared) = meta.get(key).and_then(|v| v.as_plain_text()) {
            let mut arg = OsString::from(flag);
            arg.push(resolve_doc_relative(doc_dir, &declared));
            args.push(arg);
        }
    }

    // Repeatable path keys: pandoc accepts multiple --epub-embed-font /
    // --css flags, one per file.
    for (key, flag) in [("epub-embed-font", "--epub-embed-font="), ("css", "--css=")] {
        if let Some(value) = meta.get(key) {
            for declared in entry_strings(value) {
                let mut arg = OsString::from(flag);
                arg.push(resolve_doc_relative(doc_dir, &declared));
                args.push(arg);
            }
        }
    }

    // Not a path — an internal directory *name* inside the epub
    // container, passed through verbatim.
    if let Some(subdir) = meta
        .get("epub-subdirectory")
        .and_then(|v| v.as_plain_text())
    {
        args.push(OsString::from(format!("--epub-subdirectory={subdir}")));
    }

    Ok(args)
}

pub struct PandocWriteStage;

impl PandocWriteStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PandocWriteStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for PandocWriteStage {
    fn name(&self) -> &str {
        "pandoc-write"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::RenderedOutput
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        // Findings 3/4 (final review): resolve the binary through the
        // runtime (honouring `QUARTO_PANDOC`) and enforce the version
        // floor before spawning anything.
        let pandoc_bin = resolve_and_gate_pandoc(self.name(), ctx.runtime.as_ref())?;

        // `LanguageResolveStage` (earlier in the shared prefix) populates
        // `quarto.language`; the `unwrap_or_else` fallback only matters for
        // a caller that assembles a bare stage list without it.
        let language = LanguageTerms::from_meta(&doc.ast.meta)
            .unwrap_or_else(|| crate::language::resolve_language("en", &[]));

        // T9.5: the serialized JSON lives inside the per-render temp
        // directory, not beside the output file or the process cwd.
        let temp_dir = ctx.temp_dir()?.to_path_buf();
        let results_file = temp_dir.join("pandoc-results.json");

        let params_blob = FilterParamsBuilder::new(
            &ctx.format,
            &ctx.project,
            ctx.ref_type_registry.as_ref(),
            &language,
            results_file,
        )
        .build()
        .to_string();

        // T9.1: the Pandoc-superset shape (`raw: false`), never pampa's
        // native `raw-json` envelope.
        let mut json_buf = Vec::new();
        pampa::writers::json::write_with_config(
            &doc.ast,
            &doc.ast_context,
            &mut json_buf,
            &pampa::writers::json::JsonConfig {
                raw: false,
                ..Default::default()
            },
        )
        .map_err(|diags| {
            PipelineError::stage_error(
                self.name(),
                format!(
                    "failed to serialize AST to Pandoc JSON ({} diagnostics)",
                    diags.len()
                ),
            )
        })?;

        let json_path = temp_dir.join("pandoc-input.json");
        std::fs::write(&json_path, &json_buf).map_err(|e| {
            PipelineError::stage_error(self.name(), format!("failed to write temp JSON: {e}"))
        })?;

        // Finding 5 (final review): extracted into the per-render temp
        // dir rather than a process-global cache, so cleanup rides on
        // `ctx.temp_dir()`'s existing lifecycle instead of leaking.
        let share = temp_dir.join("pandoc-share");
        std::fs::create_dir_all(&share).map_err(|e| {
            PipelineError::stage_error(
                self.name(),
                format!("failed to create pandoc share directory: {e}"),
            )
        })?;
        extract_share_tree(&share).map_err(|e| {
            PipelineError::stage_error(
                self.name(),
                format!("failed to materialize vendored pandoc filter tree: {e}"),
            )
        })?;

        let output_path = ctx.output_path();
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                PipelineError::stage_error(
                    self.name(),
                    format!(
                        "failed to create output directory {}: {e}",
                        parent.display()
                    ),
                )
            })?;
        }

        // `init.lua`'s dependenciesFile() only needs the path to exist and
        // be readable/writable, not carry pre-existing content -- but it
        // does need to exist: `QUARTO_FILTER_DEPENDENCY_FILE` names a file
        // for the Lua side to open, not to create.
        let deps_file = temp_dir.join("pandoc-filter-deps.txt");
        std::fs::write(&deps_file, "").map_err(|e| {
            PipelineError::stage_error(
                self.name(),
                format!("failed to create filter dependency file: {e}"),
            )
        })?;
        let to_format = &ctx.format.output_extension;

        let format_extra_args = if ctx.format.identifier == FormatIdentifier::Epub {
            epub_extra_args(&temp_dir, &doc.path, &doc.ast.meta)?
        } else {
            Vec::new()
        };

        // T9.6: `-f json -t <to_format> --data-dir <share>/pandoc/datadir
        // -L <share>/filters/main.lua -o <output>`.
        let output = Command::new(&pandoc_bin)
            .arg("-f")
            .arg("json")
            .arg("-t")
            .arg(to_format)
            .arg("--data-dir")
            .arg(share.join("pandoc").join("datadir"))
            .arg("-L")
            .arg(share.join("filters").join("main.lua"))
            .args(&format_extra_args)
            .arg("-o")
            .arg(&output_path)
            .arg(&json_path)
            .env("QUARTO_SHARE_PATH", share)
            .env("QUARTO_FILTER_PARAMS", encode_params_blob(&params_blob))
            .env("QUARTO_FILTER_DEPENDENCY_FILE", &deps_file)
            .output()
            .map_err(|e| {
                PipelineError::stage_error(self.name(), format!("failed to execute pandoc: {e}"))
            })?;

        // T10.1/T10.5: stderr is classified unconditionally, regardless of
        // exit status — not only on failure. T10.3: the temp JSON is
        // retained on failure (for debugging) and removed on success.
        let stderr = String::from_utf8_lossy(&output.stderr);
        let status_desc = output.status.to_string();
        let warnings = classify_pandoc_completion(
            self.name(),
            output.status.success(),
            &status_desc,
            &stderr,
            &json_path,
        )?;
        retain_temp_json_unless_success(output.status.success(), &json_path);
        ctx.add_diagnostics(warnings);

        // T9.2: no binary bytes travel through `PipelineData` — pandoc
        // already wrote `output_path` directly.
        Ok(PipelineData::RenderedOutput(RenderedOutput {
            input_path: doc.path,
            output_path,
            format: ctx.format.clone(),
            content: String::new(),
            is_intermediate: false,
            supporting_files: vec![],
            metadata: doc.ast.meta,
            source_context: doc.source_context,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_error_reporting::DiagnosticKind;

    /// T10.1: a zero-exit render whose stderr carries a `[WARNING]` line
    /// surfaces a corresponding diagnostic — the success-case
    /// discriminator this whole task exists for (commit `0b295831e`
    /// changed the policy from stderr-on-nonzero-exit-only to
    /// unconditional; a test asserting only "stderr appears on failure"
    /// would pass under both the old and new behaviour).
    ///
    /// Revert hunk: reverting `classify_pandoc_completion` to
    /// `if !success { classify_pandoc_stderr(stderr) } else { Ok(vec![]) }`
    /// (i.e. only classifying on failure) makes this RED: `success` is
    /// `true` here, so the reverted branch would return `Ok(vec![])`
    /// instead of the one warning diagnostic.
    #[test]
    fn test_warning_on_successful_render_is_surfaced() {
        let result = classify_pandoc_completion(
            "pandoc-write",
            true,
            "exit status: 0 (success)",
            "[WARNING] Could not fetch resource img.png: replacing image with description\n",
            Path::new("/tmp/does-not-matter.json"),
        );

        let diags = result.expect("a successful render must not produce an Err");
        assert_eq!(diags.len(), 1, "expected exactly one diagnostic");
        assert_eq!(diags[0].kind, DiagnosticKind::Warning);
        assert!(
            diags[0].title.contains("Could not fetch resource"),
            "expected the warning line verbatim, got: {}",
            diags[0].title
        );
    }

    /// A failing render surfaces the `Q-20-3` error instead of any
    /// diagnostics — `classify_pandoc_completion`'s other branch.
    #[test]
    fn test_failing_render_surfaces_error_not_diagnostics() {
        let result = classify_pandoc_completion(
            "pandoc-write",
            false,
            "exit status: 83",
            "lua: boom\n",
            Path::new("/tmp/does-not-matter.json"),
        );
        let err = result.expect_err("a failing render must produce an Err");
        assert!(err.to_string().contains("boom"));
    }

    /// T10.3: the temp-JSON retention policy — retained (still exists) on
    /// failure, removed on success.
    ///
    /// Revert hunk: replacing the `if success { remove_file(...) }` guard
    /// in `retain_temp_json_unless_success` with an unconditional
    /// `remove_file` call makes the failure half of this test RED (the
    /// file would be gone after a failed render, when it should be kept
    /// for debugging).
    #[test]
    fn test_temp_json_retained_on_failure_removed_on_success() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");

        let failure_json = dir.path().join("failure.json");
        std::fs::write(&failure_json, b"{}").expect("failed to write fixture JSON");
        retain_temp_json_unless_success(false, &failure_json);
        assert!(
            failure_json.exists(),
            "temp JSON should be retained after a failed render"
        );

        let success_json = dir.path().join("success.json");
        std::fs::write(&success_json, b"{}").expect("failed to write fixture JSON");
        retain_temp_json_unless_success(true, &success_json);
        assert!(
            !success_json.exists(),
            "temp JSON should be removed after a successful render"
        );

        // The `Q-20-3` error itself also names the retained path, so a
        // caller reading only the error text knows where to look.
        let err = classify_pandoc_completion(
            "pandoc-write",
            false,
            "exit status: 83",
            "lua: boom\n",
            &failure_json,
        )
        .expect_err("a failing render must produce an Err");
        assert!(
            err.to_string()
                .contains(&failure_json.display().to_string()),
            "expected the error to name the retained JSON path, got: {err}"
        );
    }

    /// Finding 4 (final review): `version::gate` had a well-tested unit but
    /// no production caller — `Q-20-1`/`Q-20-2` could never actually be
    /// emitted. Mirrors `xtask::verify`'s
    /// `pandoc_preflight_is_wired_into_run` wiring-reachability pattern: a
    /// source-grep that fails if the call site disappears, since a passing
    /// unit test for `gate` alone cannot detect that regression.
    #[test]
    fn test_version_gate_is_wired_into_run() {
        let source = include_str!("pandoc_write.rs");
        assert!(
            source.contains("version::gate(version_str.as_deref())"),
            "`resolve_and_gate_pandoc` no longer calls `version::gate` — the pandoc \
             version floor is untested by any wiring-reachability check if this call \
             is removed"
        );
        assert!(
            source.contains("resolve_and_gate_pandoc(self.name(), ctx.runtime.as_ref())?"),
            "`PandocWriteStage::run` no longer calls `resolve_and_gate_pandoc` — the \
             pandoc binary would be resolved via a bare `Command::new(\"pandoc\")` again, \
             silently ignoring `QUARTO_PANDOC` and the version floor"
        );
    }
}
