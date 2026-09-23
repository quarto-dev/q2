/*
 * stage/stages/typst_compile.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * pandoc-hybrid-typst Phase 2: compiles the intermediate `.typ` file
 * `PandocWriteStage` produced into the final PDF via a real `typst
 * compile` subprocess. See
 * `claude-notes/plans/2026-09-18-pandoc-hybrid-typst.md` Phase 2.
 */

//! `TypstCompileStage` — the typst-only tail of the Pandoc-hybrid render
//! leg, appended after [`super::PandocWriteStage`] only for
//! `FormatIdentifier::Typst` (see `build_pandoc_pipeline_stages` in
//! `crate::pipeline`).
//!
//! Native-only: shells out to a real `typst` binary via
//! `std::process::Command`. This is Q2's **first PDF-producing code
//! path** — no tinytex/latexmk/wkhtmltopdf/weasyprint integration exists
//! anywhere else in the workspace.

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use async_trait::async_trait;
use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_pandoc_types::ConfigValue;

use crate::pandoc_filters::bundle::extract_typst_packages;
use crate::stage::{
    PipelineData, PipelineDataKind, PipelineError, PipelineStage, RenderedOutput, StageContext,
};

/// The minimum typst version this stage requires (`validateRequiredTypstVersion`,
/// `core/typst.ts:204`). Unlike Q1 — which only validates when `QUARTO_TYPST`
/// overrides its own bundled known-good binary — Q2 has no bundled typst at
/// all, so this floor is checked unconditionally, every render.
const TYPST_VERSION_FLOOR: (u32, u32) = (0, 8);

/// PDF standards Typst's `--pdf-standard` flag actually accepts (`core/
/// typst.ts`'s `kTypstSupportedStandards`). An entry outside this set is
/// dropped with a warning rather than forwarded verbatim — a typo'd
/// standard should not silently become a confusing `typst compile` error.
const TYPST_SUPPORTED_PDF_STANDARDS: &[&str] = &[
    "1.4", "1.5", "1.6", "1.7", "2.0", "a-1b", "a-1a", "a-2b", "a-2u", "a-2a", "a-3b", "a-3u",
    "a-3a", "a-4", "a-4f", "a-4e", "ua-1",
];

pub struct TypstCompileStage;

impl TypstCompileStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TypstCompileStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for TypstCompileStage {
    fn name(&self) -> &str {
        "typst-compile"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::RenderedOutput
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::RenderedOutput
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::RenderedOutput(rendered) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        let typ_input = rendered.output_path.clone();
        let pdf_output = ctx.output_path();

        // Phase 1's pipeline-level tests deliberately configure a `.typ`
        // output path (via `RenderToFileOptions.output_path`) to inspect
        // pandoc's raw `.typ` text directly, sidestepping the CLI's real
        // `.pdf` deliverable — see `pandoc_typst_writer.rs`'s module docs.
        // `PandocWriteStage`'s intermediate-path computation
        // (`ctx.output_path().with_extension("typ")`) is then a no-op, so
        // `typ_input` already equals `pdf_output`: there is no distinct
        // intermediate to compile, so pass the `.typ` output through
        // unchanged rather than trying to compile a file into itself.
        if typ_input == pdf_output {
            return Ok(PipelineData::RenderedOutput(rendered));
        }

        let typst_bin = resolve_and_gate_typst(self.name(), ctx.runtime.as_ref())?;

        let temp_dir = ctx.temp_dir()?.to_path_buf();
        let packages_dir = temp_dir.join("typst-packages");
        std::fs::create_dir_all(&packages_dir).map_err(|e| {
            PipelineError::stage_error(
                self.name(),
                format!("failed to create typst packages directory: {e}"),
            )
        })?;
        extract_typst_packages(&packages_dir).map_err(|e| {
            PipelineError::stage_error(
                self.name(),
                format!("failed to materialize vendored typst packages: {e}"),
            )
        })?;

        // The vendored packages above (and anything typst-gather stages
        // next) both live under `packages_dir/packages/` — Typst's own
        // package-cache convention is `<cache-root>/preview/<name>/
        // <version>/`, with no extra nesting, so the value passed to
        // `--package-cache-path` must be this joined path, not `packages_dir`
        // itself (a prior version of this stage passed `packages_dir`
        // directly, which silently defeated hermetic staging: typst found
        // nothing at `packages_dir/preview/...`, fell through to its
        // ordinary network-fetch fallback, and the mismatch went unnoticed
        // because every render tried here so far happened to avoid the one
        // vendored package — `marginalia` — that's gated behind
        // `$if(margin-geometry)$` in `definitions.typ`, i.e. behind margin
        // notes/citations, not present in a plain document).
        let package_cache_dir = packages_dir.join("packages");

        // typst-gather integration (pandoc-hybrid-typst Phase 2): stage any
        // `@preview` package the document references beyond the 5 vendored
        // above. `gather_packages` skips a package already present at
        // `package_cache_dir` without touching the network
        // (`cache_preview_with_deps`'s `cached_path.exists()` check), so a
        // document using only the vendored packages costs nothing beyond a
        // local file scan. `typ_input` — the actual, final compiled
        // document — is scanned directly rather than the ephemeral
        // `--template` staging directory `PandocWriteStage` used: Pandoc's
        // own `$if(...)$`-gated partial inclusion means an import only
        // appears in `typ_input` when the feature that needs it is actually
        // active, and that's the complete, authoritative set of imports the
        // compile is about to need (verified empirically: a `.column-margin`
        // div's compiled `.typ` contains a literal
        // `#import "@preview/marginalia:0.3.1"` line; a plain document's
        // does not). Quarto doesn't support user-authored `@local` typst
        // packages, so `configured_local` is always empty; any `@local`
        // import found is surfaced as a warning below rather than staged.
        let configured_local: HashSet<String> = HashSet::new();
        match typst_gather::gather_packages(
            &package_cache_dir,
            Vec::new(),
            std::slice::from_ref(&typ_input),
            &configured_local,
        ) {
            Ok(result) => {
                if !result.unconfigured_local.is_empty() {
                    let names: Vec<String> = result
                        .unconfigured_local
                        .iter()
                        .map(|(name, source)| format!("@local/{name} (referenced in {source})"))
                        .collect();
                    ctx.add_diagnostics(vec![
                        DiagnosticMessageBuilder::warning("Unsupported @local typst package")
                            .problem(format!(
                                "This document references @local typst package(s), which \
                                 Quarto does not support: {}. The compile will likely fail.",
                                names.join(", ")
                            ))
                            .build(),
                    ]);
                }
            }
            Err(e) => {
                return Err(PipelineError::stage_error(
                    self.name(),
                    format!("failed to gather typst packages: {e}"),
                ));
            }
        }

        let (pdf_standards, rejected_standards) =
            normalize_pdf_standards(&string_array(rendered.metadata.get("pdf-standard")));
        if !rejected_standards.is_empty() {
            ctx.add_diagnostics(vec![
                DiagnosticMessageBuilder::warning("Unsupported PDF standard")
                    .problem(format!(
                        "typst does not support PDF standard(s) {}; they were ignored.",
                        rejected_standards.join(", ")
                    ))
                    .build(),
            ]);
        }

        let mut cmd = Command::new(&typst_bin);
        cmd.arg("compile");
        cmd.arg("--root").arg(&ctx.project.dir);
        cmd.args(font_path_args(&packages_dir));
        if package_cache_dir.join("preview").is_dir() {
            cmd.arg("--package-cache-path").arg(&package_cache_dir);
        }
        if !pdf_standards.is_empty() {
            cmd.arg("--pdf-standard").arg(pdf_standards.join(","));
        }
        cmd.arg(&typ_input).arg(&pdf_output);

        let output = cmd.output().map_err(|e| {
            PipelineError::stage_error(self.name(), format!("failed to execute typst: {e}"))
        })?;

        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if !output.status.success() {
            return Err(nonzero_exit_error(
                self.name(),
                &output.status.to_string(),
                &stderr,
            ));
        }
        if !stderr.is_empty() {
            ctx.add_diagnostics(vec![
                DiagnosticMessageBuilder::warning("typst compile diagnostic")
                    .problem(stderr)
                    .build(),
            ]);
        }

        // pandoc-hybrid-typst Phase 2's `keep-typ` bullet: ported directly
        // from Q1's `kKeepTyp` default (`config/constants.ts:88`) — discard
        // the intermediate `.typ` unless the document opts in. Q1 also
        // forces this on in debug mode; Q2 has no equivalent debug-mode
        // concept wired to this stage yet, so that half is not ported.
        let keep_typ = rendered
            .metadata
            .get("keep-typ")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !keep_typ {
            let _ = std::fs::remove_file(&typ_input);
        }

        Ok(PipelineData::RenderedOutput(RenderedOutput {
            input_path: rendered.input_path,
            output_path: pdf_output,
            format: rendered.format,
            content: String::new(),
            is_intermediate: false,
            supporting_files: rendered.supporting_files,
            metadata: rendered.metadata,
            source_context: rendered.source_context,
        }))
    }
}

/// Resolves the `typst` binary (`QUARTO_TYPST` env var, then `PATH`) and
/// enforces [`TYPST_VERSION_FLOOR`] unconditionally — mirrors
/// `pandoc_write.rs`'s `resolve_and_gate_pandoc`, but always validates
/// (Q1 only validates when its own bundled default is overridden; Q2 has
/// no bundled default to skip validation for).
fn resolve_and_gate_typst(
    stage_name: &str,
    runtime: &dyn quarto_system_runtime::SystemRuntime,
) -> Result<std::path::PathBuf, PipelineError> {
    let typst_path = runtime.find_binary("typst", "QUARTO_TYPST");

    let Some(path) = typst_path else {
        return Err(PipelineError::stage_error_with_diagnostics(
            stage_name,
            vec![
                DiagnosticMessageBuilder::error("Typst Not Found")
                    .with_code("Q-21-1")
                    .problem(
                        "The typst binary could not be found on PATH, and QUARTO_TYPST is not set.",
                    )
                    .build(),
            ],
        ));
    };

    let version_output = Command::new(&path).arg("--version").output().ok();
    let version_str = version_output
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();

    if !at_least(&version_str, TYPST_VERSION_FLOOR) {
        return Err(PipelineError::stage_error_with_diagnostics(
            stage_name,
            vec![
                DiagnosticMessageBuilder::error("Typst Version Too Old")
                    .with_code("Q-21-2")
                    .problem(format!(
                        "typst reported version {version_str:?}, which is older than the \
                         minimum required version {}.{}.",
                        TYPST_VERSION_FLOOR.0, TYPST_VERSION_FLOOR.1
                    ))
                    .build(),
            ],
        ));
    }

    Ok(path)
}

/// Parses the first `major.minor` numeric run out of `typst --version`'s
/// stdout (`"typst 0.14.2 (...)"`) and compares numerically against
/// `floor` — never lexicographically (mirrors `pandoc_filters::version`'s
/// `at_least`, duplicated rather than shared: typst's floor/output shape
/// is a distinct, unrelated binary from pandoc's).
fn at_least(version_str: &str, floor: (u32, u32)) -> bool {
    let Some(start) = version_str.find(|c: char| c.is_ascii_digit()) else {
        return false;
    };
    let rest = &version_str[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    let mut parts = rest[..end].split('.');
    let major: Option<u32> = parts.next().and_then(|s| s.parse().ok());
    let minor: Option<u32> = parts.next().and_then(|s| s.parse().ok());
    match (major, minor) {
        (Some(major), Some(minor)) => (major, minor) >= floor,
        (Some(major), None) => (major, 0) >= floor,
        _ => false,
    }
}

/// `core/typst.ts:25-38`'s `fontPathsArgs`: Quarto's own vendored fonts
/// (staged at `packages_dir/fonts`) must come first — Typst's font
/// resolution takes the first path that provides a given font family, so
/// ordering is load-bearing for the vendored template's own font
/// references, not cosmetic.
pub(crate) fn font_path_args(packages_dir: &Path) -> Vec<String> {
    vec![
        "--font-path".to_string(),
        packages_dir.join("fonts").to_string_lossy().into_owned(),
    ]
}

/// `core/typst.ts:49-119`'s `getAvailableTypstFonts`, minus its cross-render
/// disk/memory cache (see below): runs `typst fonts` with the same
/// `--font-path` args the real compile will use, feeding the
/// `typst-available-fonts` filter param the vendored `typst_css.lua`'s
/// issue-#12556 font-fallback-filtering workaround reads (Phase 1
/// deliberately left this param unset — the Lua consumer fails open,
/// treating no param as "don't filter"). Called from `PandocWriteStage`,
/// before pandoc runs, since the filter param must exist when Pandoc's Lua
/// filters run, not after.
///
/// Fails open (`None`) on any error — a missing/too-old `typst` binary is
/// `TypstCompileStage`'s error to report (with a real `Q-19-*` code) later
/// in the same render; this best-effort discovery must not preempt that
/// with a different, less clear failure.
///
/// Not cached across renders, unlike Q1's in-memory + on-disk cache: each
/// render's `--font-path` points at that render's own temp directory
/// (`ctx.temp_dir()`), so a literal-path cache key would never hit across
/// separate `render_document_to_file` calls anyway — the cache would only
/// ever help within a single render, where `typst fonts` already runs at
/// most once. Revisit if project-mode multi-document builds show repeated
/// `typst fonts` invocations are a measurable cost.
pub(crate) fn discover_available_typst_fonts(
    typst_path: Option<&Path>,
    font_path_args: &[String],
) -> Option<Vec<String>> {
    let typst_path = typst_path?;
    let mut cmd = Command::new(typst_path);
    cmd.arg("fonts");
    cmd.args(font_path_args);
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(parse_typst_fonts_output(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

/// `core/typst.ts`'s `parseTypstFontsOutput`: one font family name per
/// line, lowercased and trimmed, blank lines dropped.
fn parse_typst_fonts_output(output: &str) -> Vec<String> {
    output
        .lines()
        .map(|line| line.trim().to_lowercase())
        .filter(|line| !line.is_empty())
        .collect()
}

/// Reads a metadata value that may be a bare scalar or an array of
/// scalars into owned strings (`pdf-standard`/`font-paths`-shaped keys).
/// Non-string entries are skipped rather than erroring — this is
/// best-effort metadata reading, not schema validation.
fn string_array(value: Option<&ConfigValue>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    match &value.value {
        quarto_pandoc_types::config_value::ConfigValueKind::Array(items) => items
            .iter()
            .filter_map(|item| item.as_plain_text())
            .collect(),
        _ => value.as_plain_text().into_iter().collect(),
    }
}

/// `core/typst.ts`'s `normalizePdfStandardForTypst`: lowercases, strips an
/// optional leading `pdf`/`pdf-`/`pdf/` prefix, and keeps only values
/// Typst actually supports. Returns `(accepted, rejected)` — rejected
/// entries are the caller's to warn about, not silently dropped.
fn normalize_pdf_standards(raw: &[String]) -> (Vec<String>, Vec<String>) {
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    for s in raw {
        let lower = s.to_lowercase();
        let normalized = match lower.strip_prefix("pdf") {
            Some(rest) => rest
                .strip_prefix('/')
                .or_else(|| rest.strip_prefix('-'))
                .unwrap_or(rest),
            None => &lower,
        };
        if TYPST_SUPPORTED_PDF_STANDARDS.contains(&normalized) {
            accepted.push(normalized.to_string());
        } else {
            rejected.push(s.clone());
        }
    }
    (accepted, rejected)
}

/// The `Q-21-3` error for a nonzero-exit `typst compile` — mirrors
/// `pandoc_filters::diagnostics::nonzero_exit_error`'s shape.
fn nonzero_exit_error(stage_name: &str, status_desc: &str, stderr: &str) -> PipelineError {
    let message = DiagnosticMessageBuilder::error(format!(
        "typst compile exited with {status_desc}:\n{stderr}"
    ))
    .with_code("Q-21-3")
    .build();
    PipelineError::stage_error_with_diagnostics(stage_name, vec![message])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T1: numeric, not lexicographic — `0.9` must compare above `0.10`
    /// is false lexicographically but true numerically is NOT the claim
    /// here (0.9 < 0.10 numerically too); the real regression this guards
    /// is `0.8` vs `0.10`: `"0.8" > "0.10"` as strings, `0.8 < 0.10`
    /// numerically.
    #[test]
    fn test_at_least_is_numeric_not_lexicographic() {
        assert!(at_least("typst 0.14.2 (b33de9de)", (0, 8)));
        assert!(!at_least("typst 0.7.9", (0, 8)));
        assert!(at_least("typst 0.10.0", (0, 8)));
        assert!(!at_least("", (0, 8)));
    }

    #[test]
    fn test_normalize_pdf_standards_strips_prefix_and_filters() {
        let (accepted, rejected) = normalize_pdf_standards(&[
            "PDF/A-2b".to_string(),
            "pdf-a-3u".to_string(),
            "2.0".to_string(),
            "not-a-standard".to_string(),
        ]);
        assert_eq!(accepted, vec!["a-2b", "a-3u", "2.0"]);
        assert_eq!(rejected, vec!["not-a-standard"]);
    }

    #[test]
    fn test_string_array_handles_scalar_and_array() {
        use quarto_source_map::SourceInfo;

        let scalar = ConfigValue::new_string("a-2b", SourceInfo::for_test());
        assert_eq!(string_array(Some(&scalar)), vec!["a-2b"]);

        let array = ConfigValue::new_array(
            vec![
                ConfigValue::new_string("a-2b", SourceInfo::for_test()),
                ConfigValue::new_string("a-3u", SourceInfo::for_test()),
            ],
            SourceInfo::for_test(),
        );
        assert_eq!(string_array(Some(&array)), vec!["a-2b", "a-3u"]);

        assert_eq!(string_array(None), Vec::<String>::new());
    }

    /// Regression test for the `--package-cache-path` bug this session
    /// found and fixed: `gather_packages`'s `dest` argument must be the
    /// `preview/`-containing directory (`package_cache_dir`, i.e.
    /// `packages_dir.join("packages")`), not `packages_dir` itself — an
    /// arbitrary `@preview` package already staged there (standing in for
    /// one of the 5 Quarto vendors, or a document-specific one
    /// `typst-gather` fetched on a previous render) must be recognized as
    /// cached with **zero network activity** (`stats.downloaded == 0`,
    /// `stats.skipped == 1`), scanning only the final compiled `.typ` file
    /// (`typ_input`-equivalent), exactly as `TypstCompileStage::run` does.
    #[test]
    fn test_gather_packages_recognizes_already_staged_package_without_network() {
        let temp = tempfile::TempDir::new().unwrap();

        // A `.typ` file standing in for `typ_input`: the actual, final
        // compiled document, which is all `TypstCompileStage` scans.
        let typ_file = temp.path().join("doc.typ");
        std::fs::write(
            &typ_file,
            "#import \"@preview/fake-pkg:1.0.0\": whatever\n\nHello.\n",
        )
        .unwrap();

        // Pre-seed the exact cache layout `gather_packages` checks for —
        // `cache_preview_with_deps` only checks `Path::exists()`, not
        // manifest validity, so an empty marker file is enough to prove
        // the "already cached, skip network" branch is reached.
        let package_cache_dir = temp.path().join("package-cache");
        let pkg_dir = package_cache_dir
            .join("preview")
            .join("fake-pkg")
            .join("1.0.0");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(pkg_dir.join("typst.toml"), "").unwrap();

        let result = typst_gather::gather_packages(
            &package_cache_dir,
            Vec::new(),
            std::slice::from_ref(&typ_file),
            &HashSet::new(),
        )
        .expect("gather_packages should succeed for an already-cached package");

        assert_eq!(result.stats.downloaded, 0, "must not touch the network");
        assert_eq!(result.stats.skipped, 1, "must recognize the cached package");
        assert_eq!(result.stats.failed, 0);
        assert!(result.unconfigured_local.is_empty());
    }

    #[test]
    fn test_parse_typst_fonts_output_lowercases_trims_and_drops_blanks() {
        let output = "  Libertinus Serif\nNew Computer Modern\n\n  \nDejaVu Sans Mono\n";
        assert_eq!(
            parse_typst_fonts_output(output),
            vec![
                "libertinus serif",
                "new computer modern",
                "dejavu sans mono"
            ]
        );
    }

    #[test]
    fn test_parse_typst_fonts_output_empty_input_is_empty() {
        assert_eq!(parse_typst_fonts_output(""), Vec::<String>::new());
    }

    /// Fails open (`None`), not an error, when no binary path is given —
    /// this is best-effort discovery, and a genuinely missing `typst`
    /// binary is `TypstCompileStage`'s error to report later in the same
    /// render.
    #[test]
    fn test_discover_available_typst_fonts_none_when_binary_missing() {
        assert_eq!(discover_available_typst_fonts(None, &[]), None);
    }

    /// End-to-end against a real `typst` binary + the real vendored Font
    /// Awesome fonts (`resources/typst-packages/fonts/`): confirms the
    /// whole chain — binary invocation, `--font-path` wiring, stdout
    /// parsing — reports a real, known family name, not just that *some*
    /// list came back. Every other test in this suite already assumes a
    /// real `pandoc`/`typst` on `PATH` (see `pandoc_typst_compile.rs`'s
    /// module docs), so this follows the same convention rather than
    /// skipping when the binary is absent.
    #[test]
    fn test_discover_available_typst_fonts_finds_vendored_font_awesome() {
        let temp = tempfile::TempDir::new().unwrap();
        let packages_dir = temp.path().join("typst-packages");
        std::fs::create_dir_all(&packages_dir).unwrap();
        crate::pandoc_filters::bundle::extract_typst_packages(&packages_dir).unwrap();

        use quarto_system_runtime::SystemRuntime;
        let typst_path = quarto_system_runtime::NativeRuntime::new()
            .find_binary("typst", "QUARTO_TYPST")
            .expect("typst must be on PATH to run this test suite");
        let args = font_path_args(&packages_dir);

        let fonts = discover_available_typst_fonts(Some(&typst_path), &args)
            .expect("a real typst binary + real font-path should report fonts");
        assert!(
            fonts.iter().any(|f| f.contains("font awesome")),
            "expected a Font Awesome family in {fonts:?}"
        );
    }
}
