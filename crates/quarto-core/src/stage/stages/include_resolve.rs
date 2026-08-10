/*
 * stage/stages/include_resolve.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Pipeline stage that resolves `include-in-header` /
 * `include-before-body` / `include-after-body` document-metadata
 * keys into a canonical `rendered.includes.*` location.
 */

//! Resolve user-authored include slots into `rendered.includes.*`.
//!
//! Reads the three Q1-compatible document-metadata keys
//! (`include-in-header`, `include-before-body`, `include-after-body`)
//! plus the legacy Pandoc inline-content keys (`header-includes`,
//! `include-before`, `include-after`), reads any file paths via
//! [`SystemRuntime::file_read`], and writes the resulting flat
//! literal-text lists to `meta.rendered.includes.{header,
//! before-body, after-body}` for the template (and user filters) to
//! consume.
//!
//! Each authored entry can be:
//!
//! - a bare string path (`include-in-header: foo.html`),
//! - a smart-include `{file: <path>}` object,
//! - a smart-include `{text: <literal>}` object,
//! - or an array of any of the above.
//!
//! Engine-contributed [`PandocIncludes`](super::super::PandocIncludes)
//! (from `StageContext.includes`) are folded in too. The stage
//! drains that channel — engines write through it, Quarto resolves
//! once, downstream code reads `rendered.includes.*`.
//!
//! Ordering follows Q1 (`pandoc.ts:874-929`): for the header and
//! before-body slots, contributed entries (engine output) come
//! before user-authored entries; for the after-body slot, user
//! entries come first.
//!
//! Path resolution for the first cut is **document-relative** — the
//! plan (claude-notes/plans/2026-05-04-includes-feature.md §Resolved
//! questions #4) calls for `!path`-aware resolution once that YAML
//! tag exists; until then bare string paths join the document's
//! directory.
//!
//! Pipeline placement: between `IncludeExpansionStage` and
//! `DocumentProfileStage`. File-slot includes are authored YAML —
//! fully knowable at the profile checkpoint — so resolving them
//! pre-checkpoint lets the profile's `includes: Vec<IncludeEntry>`
//! capture both shortcode-form `{{< include … >}}` children
//! (recorded by `IncludeExpansionStage`) and the file-slot
//! dependencies recorded here, in one set, for `bd-r82e` cache
//! invalidation.
//!
//! Engine-contributed `PandocIncludes` (`StageContext.includes`) are
//! folded into `rendered.includes.*` later, by
//! [`ApplyTemplateStage::run`](super::ApplyTemplateStage). That late
//! drain also covers shortcode resolution and user-filter
//! contributions made via `quarto.doc.include_text()`.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::config_value::{ConfigMapEntry, ConfigValueKind};
use quarto_source_map::SourceInfo;
use quarto_system_runtime::SystemRuntime;

use crate::document_profile::IncludeEntry;
use crate::stage::data::PandocIncludes;
use crate::stage::{PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext};

/// Authored YAML key for the file-path-based "in-header" slot.
const KEY_INCLUDE_IN_HEADER: &str = "include-in-header";
/// Authored YAML key for the file-path-based "before-body" slot.
const KEY_INCLUDE_BEFORE_BODY: &str = "include-before-body";
/// Authored YAML key for the file-path-based "after-body" slot.
const KEY_INCLUDE_AFTER_BODY: &str = "include-after-body";
/// Authored YAML key for the Pandoc-native inline "header-includes" slot.
const KEY_HEADER_INCLUDES: &str = "header-includes";
/// Authored YAML key for the Pandoc-native inline "include-before" slot.
const KEY_INCLUDE_BEFORE: &str = "include-before";
/// Authored YAML key for the Pandoc-native inline "include-after" slot.
const KEY_INCLUDE_AFTER: &str = "include-after";

/// Resolve include slots into `rendered.includes.*`.
///
/// See module docs for the full contract.
pub struct IncludeResolveStage;

impl IncludeResolveStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for IncludeResolveStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for IncludeResolveStage {
    fn name(&self) -> &str {
        "include-resolve"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(mut doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        let doc_dir = doc
            .path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);

        // Engine PandocIncludes haven't been produced yet at this
        // point in the pipeline (engine runs after the profile
        // checkpoint, this stage runs before it). They're folded
        // into `rendered.includes.*` later by
        // `ApplyTemplateStage::run` via `append_pandoc_includes`.
        let no_engine_includes = PandocIncludes::default();

        let recorded = resolve_includes(
            &mut doc.ast.meta,
            ctx.runtime.as_ref(),
            &doc_dir,
            &no_engine_includes,
            &mut ctx.diagnostics,
        );

        // Record file-slot includes alongside any pre-existing
        // shortcode-form `recorded_includes` so the immediately
        // following `DocumentProfileStage` drains both into
        // `profile.includes`. `bd-r82e` (Phase-8) cache invalidation
        // then sees the file dependencies and rebuilds when any
        // included file changes. Step 4 of the plan extends
        // `IncludeEntry` with a kind tag; until then both kinds
        // share the vec without a discriminator.
        for entry in recorded {
            if !doc
                .recorded_includes
                .iter()
                .any(|existing| existing.path == entry.path)
            {
                doc.recorded_includes.push(entry);
            }
        }

        Ok(PipelineData::DocumentAst(doc))
    }
}

/// Resolve user-authored include slots, fold engine contributions,
/// and write flat lists to `meta.rendered.includes.{header,
/// before-body, after-body}`.
///
/// Returns the file-slot [`IncludeEntry`]s the caller should record
/// for cache-key invalidation. Smart-include `{text: …}` entries do
/// not produce file-slot entries (no file dependency). Missing files
/// produce a warning diagnostic and contribute no content.
///
/// **Ordering** (mirrors Q1 `pandoc.ts:874-929`):
///
/// - **header** and **before-body**: engine contributions first,
///   then legacy inline keys, then file-slot keys (in the order they
///   appear in the array).
/// - **after-body**: legacy inline keys first, then file-slot keys,
///   then engine contributions last (Q1 puts engine/extras after
///   user content for the after-body slot).
pub fn resolve_includes(
    meta: &mut ConfigValue,
    runtime: &dyn SystemRuntime,
    doc_dir: &Path,
    pandoc_includes: &PandocIncludes,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<IncludeEntry> {
    let mut recorded: Vec<IncludeEntry> = Vec::new();

    // === header ===
    let mut header: Vec<String> = Vec::new();
    header.extend(pandoc_includes.header_includes.iter().cloned());
    extend_from_inline_key(&mut header, meta, KEY_HEADER_INCLUDES);
    extend_from_file_slot_key(
        &mut header,
        &mut recorded,
        meta,
        KEY_INCLUDE_IN_HEADER,
        runtime,
        doc_dir,
        diagnostics,
    );

    // === before-body ===
    let mut before_body: Vec<String> = Vec::new();
    before_body.extend(pandoc_includes.include_before.iter().cloned());
    extend_from_inline_key(&mut before_body, meta, KEY_INCLUDE_BEFORE);
    extend_from_file_slot_key(
        &mut before_body,
        &mut recorded,
        meta,
        KEY_INCLUDE_BEFORE_BODY,
        runtime,
        doc_dir,
        diagnostics,
    );

    // === after-body ===
    let mut after_body: Vec<String> = Vec::new();
    extend_from_inline_key(&mut after_body, meta, KEY_INCLUDE_AFTER);
    extend_from_file_slot_key(
        &mut after_body,
        &mut recorded,
        meta,
        KEY_INCLUDE_AFTER_BODY,
        runtime,
        doc_dir,
        diagnostics,
    );
    after_body.extend(pandoc_includes.include_after.iter().cloned());

    write_rendered_lists(meta, header, before_body, after_body);

    recorded
}

/// Append a [`PandocIncludes`] payload to the existing
/// `rendered.includes.{header, before-body, after-body}` arrays.
///
/// Used by [`ApplyTemplateStage`](super::ApplyTemplateStage) to
/// catch late additions to `StageContext.includes` made AFTER
/// `IncludeResolveStage` ran — e.g. shortcode resolution
/// (`ShortcodeResolveTransform`) and Lua filters via
/// `quarto.doc.include_text()`. If the canonical location does not
/// yet exist (the resolve stage was skipped, e.g. unit-test
/// pipelines that don't include it), this function creates it.
///
/// **Ordering**: late entries are appended to the end of the
/// existing arrays. For `header` and `before-body` the resolve
/// stage put engine output first, so late additions land after
/// authored content — fine for shortcode/Lua additions which
/// conceptually come after authored YAML. For `after-body` the
/// resolve stage puts engine output last; late additions land
/// after that, which is also fine.
pub fn append_pandoc_includes(meta: &mut ConfigValue, pandoc: &PandocIncludes) {
    if pandoc.header_includes.is_empty()
        && pandoc.include_before.is_empty()
        && pandoc.include_after.is_empty()
    {
        return;
    }

    // Ensure the canonical container exists so an empty pre-state
    // still receives the late additions.
    if !meta.contains_path(&["rendered", "includes"]) {
        write_rendered_lists(meta, Vec::new(), Vec::new(), Vec::new());
    }

    append_to_rendered_slot(meta, "header", &pandoc.header_includes);
    append_to_rendered_slot(meta, "before-body", &pandoc.include_before);
    append_to_rendered_slot(meta, "after-body", &pandoc.include_after);
}

fn append_to_rendered_slot(meta: &mut ConfigValue, slot: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    let si = meta.source_info.clone();
    if let Some(slot_value) = meta.get_path_mut(&["rendered", "includes", slot])
        && let ConfigValueKind::Array(existing) = &mut slot_value.value
    {
        for s in items {
            existing.push(ConfigValue::new_string(s.clone(), si.clone()));
        }
    }
}

/// Append literal-text entries from a Pandoc-native inline-content
/// key (e.g. `header-includes`) to `out`. Accepts a single string,
/// a `PandocInlines` value, or an array of either.
///
/// Quarto's YAML reader parses scalar strings under these keys as
/// markdown, so a value like `<meta name="x" content="y">` arrives
/// as a `PandocInlines` containing a `RawInline { format: "html",
/// text: "<meta…>" }`. To preserve the user's literal HTML we walk
/// the inlines via [`literal_html_text`] rather than
/// [`ConfigValue::as_plain_text`] (which would drop the
/// `RawInline` content).
fn extend_from_inline_key(out: &mut Vec<String>, meta: &ConfigValue, key: &str) {
    let Some(value) = meta.get(key) else {
        return;
    };
    extend_with_inline_value(out, value);
}

fn extend_with_inline_value(out: &mut Vec<String>, value: &ConfigValue) {
    match &value.value {
        ConfigValueKind::Array(items) => {
            for item in items {
                extend_with_inline_value(out, item);
            }
        }
        _ => {
            if let Some(s) = literal_html_text(value) {
                out.push(s);
            }
        }
    }
}

/// Convert a `ConfigValue` into the literal-text form needed for an
/// HTML include slot.
///
/// - `Scalar(String)` / `Path` / `Glob` / `Expr` → the underlying
///   string verbatim.
/// - `PandocInlines` → walk the inlines, preserving `RawInline`
///   text, `Code` text, and original quote characters on `Quoted`
///   nodes. Without this `<meta>` / `<script>` etc. would be dropped
///   (since `inlines_to_plain_text` skips raw inlines), turning a
///   user's literal HTML include into empty/garbled output.
/// - Anything else → `None`.
fn literal_html_text(value: &ConfigValue) -> Option<String> {
    use quarto_pandoc_types::config_value::ConfigValueKind as K;
    use yaml_rust2::Yaml;

    match &value.value {
        K::Scalar(Yaml::String(s)) => Some(s.clone()),
        K::Path(s) | K::Glob(s) | K::Expr(s) => Some(s.clone()),
        K::PandocInlines(inlines) => Some(inlines_to_html_literal(inlines)),
        _ => None,
    }
}

/// Walk Pandoc inlines preserving raw HTML markup and original
/// quote characters. Mirrors `inlines_to_plain_text` for plain
/// inlines but emits `RawInline` text rather than skipping it.
fn inlines_to_html_literal(inlines: &[quarto_pandoc_types::inline::Inline]) -> String {
    use quarto_pandoc_types::inline::{Inline, QuoteType};

    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Str(s) => out.push_str(&s.text),
            Inline::Space(_) | Inline::SoftBreak(_) => out.push(' '),
            Inline::LineBreak(_) => out.push('\n'),
            Inline::RawInline(r) => out.push_str(&r.text),
            Inline::Code(c) => out.push_str(&c.text),
            Inline::Math(m) => out.push_str(&m.text),
            Inline::Emph(e) => out.push_str(&inlines_to_html_literal(&e.content)),
            Inline::Strong(s) => out.push_str(&inlines_to_html_literal(&s.content)),
            Inline::Underline(u) => out.push_str(&inlines_to_html_literal(&u.content)),
            Inline::Strikeout(s) => out.push_str(&inlines_to_html_literal(&s.content)),
            Inline::Superscript(s) => out.push_str(&inlines_to_html_literal(&s.content)),
            Inline::Subscript(s) => out.push_str(&inlines_to_html_literal(&s.content)),
            Inline::SmallCaps(s) => out.push_str(&inlines_to_html_literal(&s.content)),
            Inline::Quoted(q) => {
                let ch = match q.quote_type {
                    QuoteType::SingleQuote => '\'',
                    QuoteType::DoubleQuote => '"',
                };
                out.push(ch);
                out.push_str(&inlines_to_html_literal(&q.content));
                out.push(ch);
            }
            Inline::Link(l) => out.push_str(&inlines_to_html_literal(&l.content)),
            Inline::Image(i) => out.push_str(&inlines_to_html_literal(&i.content)),
            Inline::Span(s) => out.push_str(&inlines_to_html_literal(&s.content)),
            Inline::Cite(c) => out.push_str(&inlines_to_html_literal(&c.content)),
            // Reconstruct shortcode source text (escaped ones keep
            // their triple braces) so ShortcodeResolveTransform's
            // text-level pass over `rendered.includes.*` can expand
            // them later. Previously dropped silently
            // (bd-shortcodes-in-metadata-bp06aub8).
            Inline::Shortcode(sc) => {
                out.push_str(&pampa::writers::qmd::shortcode_source_text(sc));
            }
            _ => {}
        }
    }
    out
}

/// Append literal-text entries (after file reads / smart-include
/// resolution) for one of the file-path slots to `out`. File reads
/// that fail produce a warning diagnostic and contribute nothing.
fn extend_from_file_slot_key(
    out: &mut Vec<String>,
    recorded: &mut Vec<IncludeEntry>,
    meta: &ConfigValue,
    key: &str,
    runtime: &dyn SystemRuntime,
    doc_dir: &Path,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    let Some(value) = meta.get(key) else {
        return;
    };
    extend_with_smart_include_value(out, recorded, value, key, runtime, doc_dir, diagnostics);
}

fn extend_with_smart_include_value(
    out: &mut Vec<String>,
    recorded: &mut Vec<IncludeEntry>,
    value: &ConfigValue,
    key: &str,
    runtime: &dyn SystemRuntime,
    doc_dir: &Path,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    match &value.value {
        ConfigValueKind::Array(items) => {
            for item in items {
                extend_with_smart_include_value(
                    out,
                    recorded,
                    item,
                    key,
                    runtime,
                    doc_dir,
                    diagnostics,
                );
            }
        }
        ConfigValueKind::Map(_) => {
            // Smart-include object: prefer `text:` over `file:` if
            // somehow both are set — no schema basis to disambiguate
            // otherwise. `text:` values reach us as PandocInlines
            // because Quarto's YAML reader parses string scalars as
            // markdown; `literal_html_text` rebuilds the literal
            // HTML by preserving RawInline content and original
            // quote characters. File paths do not need that
            // treatment — they are paths.
            if let Some(text_value) = value.get("text") {
                if let Some(text) = literal_html_text(text_value) {
                    out.push(text);
                } else {
                    push_invalid_form_warning(diagnostics, key, value);
                }
            } else if let Some(file) = value.get("file").and_then(|v| v.as_plain_text()) {
                read_include_file(
                    out,
                    recorded,
                    &file,
                    key,
                    runtime,
                    doc_dir,
                    diagnostics,
                    value,
                );
            } else {
                push_invalid_form_warning(diagnostics, key, value);
            }
        }
        _ => {
            if let Some(s) = value.as_plain_text() {
                read_include_file(out, recorded, &s, key, runtime, doc_dir, diagnostics, value);
            } else {
                push_invalid_form_warning(diagnostics, key, value);
            }
        }
    }
}

fn read_include_file(
    out: &mut Vec<String>,
    recorded: &mut Vec<IncludeEntry>,
    rel_path: &str,
    key: &str,
    runtime: &dyn SystemRuntime,
    doc_dir: &Path,
    diagnostics: &mut Vec<DiagnosticMessage>,
    location: &ConfigValue,
) {
    let resolved = doc_dir.join(rel_path);
    let bytes = match runtime.file_read(&resolved) {
        Ok(b) => b,
        Err(e) => {
            diagnostics.push(
                DiagnosticMessageBuilder::warning("Include file not found")
                    .with_code("Q-5-4")
                    .with_location(location.source_info.clone())
                    .problem(format!(
                        "Could not read include file '{}' for `{}`: {}",
                        resolved.display(),
                        key,
                        e
                    ))
                    .build(),
            );
            return;
        }
    };

    // Canonicalize for cache-key stability where possible. Failure to
    // canonicalize (path doesn't exist on disk yet, VFS-only, etc.)
    // falls back to the joined path — still stable for hashing across
    // repeat runs of the same project layout.
    let canonical = runtime.canonicalize(&resolved).unwrap_or(resolved.clone());

    let content = String::from_utf8_lossy(&bytes).into_owned();
    out.push(content);
    recorded.push(IncludeEntry::new(canonical, &bytes));
}

fn push_invalid_form_warning(
    diagnostics: &mut Vec<DiagnosticMessage>,
    key: &str,
    value: &ConfigValue,
) {
    diagnostics.push(
        DiagnosticMessageBuilder::warning("Invalid include form")
            .with_code("Q-5-5")
            .with_location(value.source_info.clone())
            .problem(format!(
                "`{}` entry must be a string path, `{{file: <path>}}`, or `{{text: <literal>}}`",
                key
            ))
            .build(),
    );
}

/// Write the three lists into `meta.rendered.includes.{header,
/// before-body, after-body}`. Each list becomes a `ConfigValue`
/// array of strings; empty lists are still written so consumers can
/// branch on `contains_path` reliably.
fn write_rendered_lists(
    meta: &mut ConfigValue,
    header: Vec<String>,
    before_body: Vec<String>,
    after_body: Vec<String>,
) {
    let si = meta.source_info.clone();
    let to_array = |items: Vec<String>, si: SourceInfo| {
        let entries = items
            .into_iter()
            .map(|s| ConfigValue::new_string(s, si.clone()))
            .collect();
        ConfigValue::new_array(entries, si)
    };

    let includes_map = ConfigValue::new_map(
        vec![
            ConfigMapEntry {
                key: "header".to_string(),
                key_source: si.clone(),
                value: to_array(header, si.clone()),
            },
            ConfigMapEntry {
                key: "before-body".to_string(),
                key_source: si.clone(),
                value: to_array(before_body, si.clone()),
            },
            ConfigMapEntry {
                key: "after-body".to_string(),
                key_source: si.clone(),
                value: to_array(after_body, si.clone()),
            },
        ],
        si,
    );

    meta.insert_path(&["rendered", "includes"], includes_map);
}

#[cfg(test)]
mod tests {
    //! Step-0 TDD tests. These are written before the production
    //! code in `resolve_includes` is filled in; they will pass once
    //! Step 1 is complete.

    use super::*;
    use async_trait::async_trait;
    use quarto_pandoc_types::config_value::ConfigMapEntry;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    // --- ConfigValue helpers ---

    fn s(value: &str) -> ConfigValue {
        ConfigValue::new_string(value.to_string(), SourceInfo::for_test())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let entries = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::for_test(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(entries, SourceInfo::for_test())
    }

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::for_test())
    }

    fn rendered_strings(meta: &ConfigValue, slot: &str) -> Vec<String> {
        let arr = meta
            .get_path(&["rendered", "includes", slot])
            .and_then(|v| v.as_array())
            .unwrap_or(&[]);
        arr.iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect()
    }

    // --- Mock runtime ---

    struct MockFileRuntime {
        files: HashMap<PathBuf, Vec<u8>>,
    }

    impl MockFileRuntime {
        fn new(files: Vec<(&str, &str)>) -> Self {
            Self {
                files: files
                    .into_iter()
                    .map(|(p, c)| (PathBuf::from(p), c.as_bytes().to_vec()))
                    .collect(),
            }
        }
    }

    macro_rules! mock_runtime_stubs {
        () => {
            fn file_write(
                &self,
                _path: &std::path::Path,
                _contents: &[u8],
            ) -> quarto_system_runtime::RuntimeResult<()> {
                Ok(())
            }
            fn file_copy(
                &self,
                _src: &std::path::Path,
                _dst: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<()> {
                Ok(())
            }
            fn path_rename(
                &self,
                _old: &std::path::Path,
                _new: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<()> {
                Ok(())
            }
            fn file_remove(
                &self,
                _path: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<()> {
                Ok(())
            }
            fn path_metadata(
                &self,
                _path: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata> {
                unimplemented!()
            }
            fn dir_create(
                &self,
                _path: &std::path::Path,
                _recursive: bool,
            ) -> quarto_system_runtime::RuntimeResult<()> {
                Ok(())
            }
            fn dir_remove(
                &self,
                _path: &std::path::Path,
                _recursive: bool,
            ) -> quarto_system_runtime::RuntimeResult<()> {
                Ok(())
            }
            fn dir_list(
                &self,
                _path: &std::path::Path,
            ) -> quarto_system_runtime::RuntimeResult<Vec<PathBuf>> {
                Ok(vec![])
            }
            fn cwd(&self) -> quarto_system_runtime::RuntimeResult<PathBuf> {
                Ok(PathBuf::from("/"))
            }
            fn temp_dir(
                &self,
                _template: &str,
            ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::TempDir> {
                Ok(quarto_system_runtime::TempDir::new(PathBuf::from(
                    "/tmp/test",
                )))
            }
            fn exec_pipe(
                &self,
                _command: &str,
                _args: &[&str],
                _stdin: &[u8],
            ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
                Ok(vec![])
            }
            fn exec_command(
                &self,
                _command: &str,
                _args: &[&str],
                _stdin: Option<&[u8]>,
            ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput> {
                Ok(quarto_system_runtime::CommandOutput {
                    code: 0,
                    stdout: vec![],
                    stderr: vec![],
                })
            }
            fn env_get(&self, _name: &str) -> quarto_system_runtime::RuntimeResult<Option<String>> {
                Ok(None)
            }
            fn env_all(
                &self,
            ) -> quarto_system_runtime::RuntimeResult<std::collections::HashMap<String, String>> {
                Ok(std::collections::HashMap::new())
            }
            fn os_name(&self) -> &'static str {
                "mock"
            }
            fn arch(&self) -> &'static str {
                "mock"
            }
            fn cpu_time(&self) -> quarto_system_runtime::RuntimeResult<u64> {
                Ok(0)
            }
            fn xdg_dir(
                &self,
                _kind: quarto_system_runtime::XdgDirKind,
                _subpath: Option<&std::path::Path>,
            ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
                Ok(PathBuf::from("/xdg"))
            }
            fn stdout_write(&self, _data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
                Ok(())
            }
            fn stderr_write(&self, _data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
                Ok(())
            }
        };
    }

    #[async_trait]
    impl SystemRuntime for MockFileRuntime {
        fn file_read(
            &self,
            path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            self.files.get(path).cloned().ok_or_else(|| {
                quarto_system_runtime::RuntimeError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("mock: file not found: {}", path.display()),
                ))
            })
        }
        fn path_exists(
            &self,
            path: &std::path::Path,
            _kind: Option<quarto_system_runtime::PathKind>,
        ) -> quarto_system_runtime::RuntimeResult<bool> {
            Ok(self.files.contains_key(path))
        }
        fn canonicalize(
            &self,
            path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(path.to_path_buf())
        }
        async fn fetch_url(
            &self,
            _url: &str,
        ) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
            Err(quarto_system_runtime::RuntimeError::NotSupported(
                "mock".to_string(),
            ))
        }
        mock_runtime_stubs!();
    }

    fn rt(files: Vec<(&str, &str)>) -> Arc<MockFileRuntime> {
        Arc::new(MockFileRuntime::new(files))
    }

    fn empty_pandoc_includes() -> PandocIncludes {
        PandocIncludes::default()
    }

    // === Test 1: bare-string file path → file content lands in
    // `rendered.includes.header`. ===
    #[test]
    fn bare_string_path_resolves_into_header() {
        let runtime = rt(vec![("/proj/extra.html", "<style>X</style>")]);
        let mut meta = map(vec![(KEY_INCLUDE_IN_HEADER, s("extra.html"))]);

        let mut diags = Vec::new();
        let recorded = resolve_includes(
            &mut meta,
            runtime.as_ref(),
            Path::new("/proj"),
            &empty_pandoc_includes(),
            &mut diags,
        );

        assert!(diags.is_empty(), "no diagnostics expected: {:?}", diags);
        assert_eq!(rendered_strings(&meta, "header"), vec!["<style>X</style>"]);
        assert_eq!(rendered_strings(&meta, "before-body"), Vec::<String>::new());
        assert_eq!(rendered_strings(&meta, "after-body"), Vec::<String>::new());
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].path, PathBuf::from("/proj/extra.html"));
    }

    // === Test 2: `{file: ...}` smart-include form is equivalent to
    // a bare string. ===
    #[test]
    fn smart_file_form_resolves_like_bare_string() {
        let runtime = rt(vec![("/proj/extra.html", "<meta name='x' content='y'>")]);
        let mut meta = map(vec![(
            KEY_INCLUDE_IN_HEADER,
            map(vec![("file", s("extra.html"))]),
        )]);

        let mut diags = Vec::new();
        let recorded = resolve_includes(
            &mut meta,
            runtime.as_ref(),
            Path::new("/proj"),
            &empty_pandoc_includes(),
            &mut diags,
        );

        assert!(diags.is_empty());
        assert_eq!(
            rendered_strings(&meta, "header"),
            vec!["<meta name='x' content='y'>"]
        );
        assert_eq!(recorded.len(), 1);
    }

    // === Test 3: `{text: ...}` smart-include form inserts the
    // literal verbatim, no file read, no recorded entry. ===
    #[test]
    fn smart_text_form_inserts_literal_verbatim() {
        let runtime = rt(vec![]);
        let mut meta = map(vec![(
            KEY_INCLUDE_BEFORE_BODY,
            map(vec![("text", s("<header class='hi'/>"))]),
        )]);

        let mut diags = Vec::new();
        let recorded = resolve_includes(
            &mut meta,
            runtime.as_ref(),
            Path::new("/proj"),
            &empty_pandoc_includes(),
            &mut diags,
        );

        assert!(diags.is_empty());
        assert_eq!(
            rendered_strings(&meta, "before-body"),
            vec!["<header class='hi'/>"]
        );
        assert!(
            recorded.is_empty(),
            "smart-text entries should not produce file-include records"
        );
    }

    // === Test 4: array of mixed forms preserves authored order. ===
    #[test]
    fn array_of_mixed_forms_preserves_order() {
        let runtime = rt(vec![("/proj/a.html", "<a/>"), ("/proj/c.html", "<c/>")]);
        let mut meta = map(vec![(
            KEY_INCLUDE_IN_HEADER,
            arr(vec![
                s("a.html"),
                map(vec![("text", s("<b/>"))]),
                map(vec![("file", s("c.html"))]),
            ]),
        )]);

        let mut diags = Vec::new();
        let recorded = resolve_includes(
            &mut meta,
            runtime.as_ref(),
            Path::new("/proj"),
            &empty_pandoc_includes(),
            &mut diags,
        );

        assert!(diags.is_empty());
        assert_eq!(
            rendered_strings(&meta, "header"),
            vec!["<a/>", "<b/>", "<c/>"]
        );
        // Two file entries (a and c). The text entry is not recorded.
        assert_eq!(recorded.len(), 2);
    }

    // === Test 5: missing file emits a warning and other entries
    // still resolve. ===
    #[test]
    fn missing_file_emits_warning_and_continues() {
        let runtime = rt(vec![("/proj/ok.html", "<ok/>")]);
        let mut meta = map(vec![(
            KEY_INCLUDE_IN_HEADER,
            arr(vec![s("missing.html"), s("ok.html")]),
        )]);

        let mut diags = Vec::new();
        let _ = resolve_includes(
            &mut meta,
            runtime.as_ref(),
            Path::new("/proj"),
            &empty_pandoc_includes(),
            &mut diags,
        );

        assert_eq!(
            diags.len(),
            1,
            "expected exactly one missing-file warning, got: {:?}",
            diags
        );
        assert!(
            diags[0].title.to_lowercase().contains("not found"),
            "warning title should mention 'not found': {:?}",
            diags[0].title
        );
        // The good file still made it into the rendered list.
        assert_eq!(rendered_strings(&meta, "header"), vec!["<ok/>"]);
    }

    // === Test 6: legacy inline keys (`header-includes` /
    // `include-before` / `include-after`) fold into the right
    // slots. ===
    #[test]
    fn legacy_inline_keys_fold_into_rendered_lists() {
        let runtime = rt(vec![]);
        let mut meta = map(vec![
            (KEY_HEADER_INCLUDES, s("<style>L</style>")),
            (KEY_INCLUDE_BEFORE, s("<div>BEFORE</div>")),
            (KEY_INCLUDE_AFTER, s("<div>AFTER</div>")),
        ]);

        let mut diags = Vec::new();
        let _ = resolve_includes(
            &mut meta,
            runtime.as_ref(),
            Path::new("/proj"),
            &empty_pandoc_includes(),
            &mut diags,
        );

        assert!(diags.is_empty());
        assert_eq!(rendered_strings(&meta, "header"), vec!["<style>L</style>"]);
        assert_eq!(
            rendered_strings(&meta, "before-body"),
            vec!["<div>BEFORE</div>"]
        );
        assert_eq!(
            rendered_strings(&meta, "after-body"),
            vec!["<div>AFTER</div>"]
        );
    }

    // === Test 7: engine-contributed `PandocIncludes` fold in. ===
    #[test]
    fn engine_pandoc_includes_fold_in() {
        let runtime = rt(vec![]);
        let mut meta = map(vec![]);
        let pandoc = PandocIncludes {
            header_includes: vec!["<engine-h/>".to_string()],
            include_before: vec!["<engine-b/>".to_string()],
            include_after: vec!["<engine-a/>".to_string()],
        };

        let mut diags = Vec::new();
        let _ = resolve_includes(
            &mut meta,
            runtime.as_ref(),
            Path::new("/proj"),
            &pandoc,
            &mut diags,
        );

        assert!(diags.is_empty());
        assert_eq!(rendered_strings(&meta, "header"), vec!["<engine-h/>"]);
        assert_eq!(rendered_strings(&meta, "before-body"), vec!["<engine-b/>"]);
        assert_eq!(rendered_strings(&meta, "after-body"), vec!["<engine-a/>"]);
    }

    // === Test 8: ordering — for header & before-body, engine-first;
    // for after-body, user-first. Mirrors Q1's
    // `pandoc.ts:874-929`. ===
    #[test]
    fn ordering_engine_first_for_header_user_first_for_after_body() {
        let runtime = rt(vec![
            ("/proj/h.html", "<h-file/>"),
            ("/proj/b.html", "<b-file/>"),
            ("/proj/a.html", "<a-file/>"),
        ]);
        let mut meta = map(vec![
            (KEY_HEADER_INCLUDES, s("<h-inline/>")),
            (KEY_INCLUDE_BEFORE, s("<b-inline/>")),
            (KEY_INCLUDE_AFTER, s("<a-inline/>")),
            (KEY_INCLUDE_IN_HEADER, s("h.html")),
            (KEY_INCLUDE_BEFORE_BODY, s("b.html")),
            (KEY_INCLUDE_AFTER_BODY, s("a.html")),
        ]);
        let pandoc = PandocIncludes {
            header_includes: vec!["<h-engine/>".to_string()],
            include_before: vec!["<b-engine/>".to_string()],
            include_after: vec!["<a-engine/>".to_string()],
        };

        let mut diags = Vec::new();
        let _ = resolve_includes(
            &mut meta,
            runtime.as_ref(),
            Path::new("/proj"),
            &pandoc,
            &mut diags,
        );

        assert!(diags.is_empty());
        // header & before-body: engine, then inline, then file.
        assert_eq!(
            rendered_strings(&meta, "header"),
            vec!["<h-engine/>", "<h-inline/>", "<h-file/>"]
        );
        assert_eq!(
            rendered_strings(&meta, "before-body"),
            vec!["<b-engine/>", "<b-inline/>", "<b-file/>"]
        );
        // after-body: inline, then file, then engine (Q1 parity).
        assert_eq!(
            rendered_strings(&meta, "after-body"),
            vec!["<a-inline/>", "<a-file/>", "<a-engine/>"]
        );
    }

    // === Test 9: regression — `PandocInlines` values (the form the
    // YAML reader produces when scalar strings are parsed as
    // markdown) round-trip back to literal HTML, including
    // RawInline tags and original quote characters. End-to-end
    // smoke (target/q2-includes-smoke) caught this before the
    // helper existed. ===
    #[test]
    fn pandoc_inlines_value_preserves_raw_html_and_quotes() {
        use quarto_pandoc_types::config_value::ConfigValueKind;
        use quarto_pandoc_types::inline::{Inline, QuoteType, Quoted, RawInline, Str};

        // Build a PandocInlines value matching what the YAML reader
        // produces for: `<script>console.log('AFTER');</script>`
        // (RawInline + Str + Quoted + Str + RawInline).
        let si = SourceInfo::for_test();
        let inlines = vec![
            Inline::RawInline(RawInline {
                format: "html".to_string(),
                text: "<script>".to_string(),
                source_info: si.clone(),
            }),
            Inline::Str(Str {
                text: "console.log(".to_string(),
                source_info: si.clone(),
            }),
            Inline::Quoted(Quoted {
                quote_type: QuoteType::SingleQuote,
                content: vec![Inline::Str(Str {
                    text: "AFTER".to_string(),
                    source_info: si.clone(),
                })],
                source_info: si.clone(),
            }),
            Inline::Str(Str {
                text: ");".to_string(),
                source_info: si.clone(),
            }),
            Inline::RawInline(RawInline {
                format: "html".to_string(),
                text: "</script>".to_string(),
                source_info: si.clone(),
            }),
        ];
        let parsed_value = ConfigValue {
            value: ConfigValueKind::PandocInlines(inlines),
            source_info: si.clone(),
            merge_op: quarto_pandoc_types::config_value::MergeOp::Concat,
        };

        // Smart-include `text:` form holding the parsed value.
        let mut meta = map(vec![(
            KEY_INCLUDE_AFTER_BODY,
            map(vec![("text", parsed_value)]),
        )]);

        let runtime = rt(vec![]);
        let mut diags = Vec::new();
        let _ = resolve_includes(
            &mut meta,
            runtime.as_ref(),
            Path::new("/proj"),
            &empty_pandoc_includes(),
            &mut diags,
        );

        assert!(diags.is_empty(), "{:?}", diags);
        assert_eq!(
            rendered_strings(&meta, "after-body"),
            vec!["<script>console.log('AFTER');</script>"],
            "RawInline content and original single-quote chars must round-trip"
        );
    }
}
