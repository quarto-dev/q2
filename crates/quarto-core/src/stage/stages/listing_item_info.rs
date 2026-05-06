/*
 * stage/stages/listing_item_info.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Pre-checkpoint stage that auto-fills `meta.listing-item.*` for the
 * listings feature surface (`bd-izqh`).
 */

//! Pre-checkpoint stage that enriches `meta.listing-item` with
//! values derived from the post-include AST when the author has
//! not supplied them. Runs between [`IncludeExpansionStage`] and
//! [`DocumentProfileStage`]; the latter then reads the enriched
//! map via `extract_listing_item` (L0).
//!
//! Author values always win — the stage strictly fills holes.
//!
//! See `claude-notes/plans/2026-05-05-listings-L1-autofill-stage.md`
//! for the full design, the §"Decisions log" (D1–D14), and the
//! follow-up issues spawned during implementation:
//!
//! - `bd-8h9o` — shortcode-bearing image `src` filtering (D13).
//! - `bd-zzke` — plain-text helper consolidation across the six
//!   in-tree variants (D10).
//! - `bd-a3we` — Automerge VFS mtime; lets WASM populate
//!   `date_modified` once it lands.
//!
//! [`IncludeExpansionStage`]: crate::stage::stages::IncludeExpansionStage
//! [`DocumentProfileStage`]: crate::stage::stages::DocumentProfileStage

use std::path::Path;

use async_trait::async_trait;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::config_value::ConfigValue;
use quarto_pandoc_types::inline::Inline;
use quarto_source_map::SourceInfo;
use quarto_system_runtime::SystemRuntime;
use yaml_rust2::Yaml;

use crate::stage::data::DocumentAst;
use crate::stage::{PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext};
use crate::transforms::inlines_to_plain_text;

/// Reading-time constant: words per minute. Matches Q1's
/// `estimateReadingTimeMinutes`. Per D3 in the L1 sub-plan.
const WORDS_PER_MINUTE: u32 = 200;

/// Pipeline stage that auto-fills `meta.listing-item.*`.
///
/// Input kind: [`PipelineDataKind::DocumentAst`].
/// Output kind: [`PipelineDataKind::DocumentAst`].
pub struct ListingItemInfoStage;

impl ListingItemInfoStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ListingItemInfoStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for ListingItemInfoStage {
    fn name(&self) -> &str {
        "listing-item-info"
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
        autofill_listing_item(&mut doc, ctx);
        Ok(PipelineData::DocumentAst(doc))
    }
}

/// Walk the AST and fill any unset standard fields on
/// `meta.listing-item`. Author values are never overwritten.
fn autofill_listing_item(doc: &mut DocumentAst, ctx: &StageContext) {
    let cand_description = compute_description(&doc.ast.blocks);
    let cand_image = first_image_src(&doc.ast.blocks);
    let cand_word_count = word_count(&doc.ast.blocks);
    let cand_reading = cand_word_count.map(|w| div_ceil_u32(w, WORDS_PER_MINUTE));
    let cand_date_modified = mtime_iso(ctx.runtime.as_ref(), &doc.path);

    fill_string_if_absent(&mut doc.ast.meta, "description", cand_description);
    fill_string_if_absent(&mut doc.ast.meta, "image", cand_image);
    fill_u32_if_absent(&mut doc.ast.meta, "word-count", cand_word_count);
    fill_u32_if_absent(&mut doc.ast.meta, "reading-time-minutes", cand_reading);
    fill_string_if_absent(&mut doc.ast.meta, "date-modified", cand_date_modified);
}

/// Set `meta.listing-item.<key> = <value>` only if the key is not
/// already present. Author values always win — the stage strictly
/// fills holes.
fn fill_string_if_absent(meta: &mut ConfigValue, key: &str, value: Option<String>) {
    if meta.contains_path(&["listing-item", key]) {
        return;
    }
    let Some(v) = value else { return };
    meta.insert_path(
        &["listing-item", key],
        ConfigValue::new_string(v, SourceInfo::default()),
    );
}

/// Counterpart to [`fill_string_if_absent`] for `u32` numeric fields.
fn fill_u32_if_absent(meta: &mut ConfigValue, key: &str, value: Option<u32>) {
    if meta.contains_path(&["listing-item", key]) {
        return;
    }
    let Some(n) = value else { return };
    meta.insert_path(
        &["listing-item", key],
        ConfigValue::new_scalar(Yaml::Integer(n as i64), SourceInfo::default()),
    );
}

/// First plain-text paragraph from the post-include AST, untruncated
/// (per D11). Returns `None` if no `Para` / `Plain` block has any
/// non-whitespace content.
fn compute_description(blocks: &[Block]) -> Option<String> {
    for block in blocks {
        let inlines = match block {
            Block::Plain(p) => &p.content,
            Block::Paragraph(p) => &p.content,
            _ => continue,
        };
        let text = inlines_to_plain_text(inlines);
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// First inline image's URL in document order. Recurses into block
/// containers and inline containers (Link, Emph, etc.) so an image
/// nested inside formatting still surfaces. Skips images whose
/// target URL is empty (returns the next non-empty one).
///
/// Does **not** filter unresolved-shortcode targets like
/// `{{< meta thumb >}}.png` — see `bd-8h9o`.
fn first_image_src(blocks: &[Block]) -> Option<String> {
    blocks.iter().find_map(first_image_src_in_block)
}

fn first_image_src_in_block(block: &Block) -> Option<String> {
    match block {
        Block::Plain(p) => first_image_src_in_inlines(&p.content),
        Block::Paragraph(p) => first_image_src_in_inlines(&p.content),
        Block::Header(h) => first_image_src_in_inlines(&h.content),
        Block::BlockQuote(q) => first_image_src(&q.content),
        Block::Div(d) => first_image_src(&d.content),
        Block::BulletList(l) => l.content.iter().find_map(|items| first_image_src(items)),
        Block::OrderedList(l) => l.content.iter().find_map(|items| first_image_src(items)),
        Block::DefinitionList(dl) => {
            for (term, defs) in &dl.content {
                if let Some(s) = first_image_src_in_inlines(term) {
                    return Some(s);
                }
                for blocks in defs {
                    if let Some(s) = first_image_src(blocks) {
                        return Some(s);
                    }
                }
            }
            None
        }
        Block::Figure(f) => first_image_src(&f.content),
        Block::LineBlock(lb) => lb
            .content
            .iter()
            .find_map(|line| first_image_src_in_inlines(line)),
        Block::CaptionBlock(c) => first_image_src_in_inlines(&c.content),
        // CodeBlock, RawBlock, Table, HorizontalRule, BlockMetadata,
        // NoteDefinition*, Custom: no inline-image children we model here.
        _ => None,
    }
}

fn first_image_src_in_inlines(inlines: &[Inline]) -> Option<String> {
    inlines.iter().find_map(first_image_src_in_inline)
}

fn first_image_src_in_inline(inline: &Inline) -> Option<String> {
    match inline {
        Inline::Image(img) => {
            if img.target.0.is_empty() {
                None
            } else {
                Some(img.target.0.clone())
            }
        }
        Inline::Emph(e) => first_image_src_in_inlines(&e.content),
        Inline::Underline(u) => first_image_src_in_inlines(&u.content),
        Inline::Strong(s) => first_image_src_in_inlines(&s.content),
        Inline::Strikeout(s) => first_image_src_in_inlines(&s.content),
        Inline::Superscript(s) => first_image_src_in_inlines(&s.content),
        Inline::Subscript(s) => first_image_src_in_inlines(&s.content),
        Inline::SmallCaps(s) => first_image_src_in_inlines(&s.content),
        Inline::Quoted(q) => first_image_src_in_inlines(&q.content),
        Inline::Cite(c) => first_image_src_in_inlines(&c.content),
        Inline::Link(l) => first_image_src_in_inlines(&l.content),
        Inline::Span(s) => first_image_src_in_inlines(&s.content),
        Inline::Note(n) => first_image_src(&n.content),
        Inline::Insert(i) => first_image_src_in_inlines(&i.content),
        Inline::Highlight(h) => first_image_src_in_inlines(&h.content),
        _ => None,
    }
}

/// Word count over the post-include AST. Tokenizes on whitespace runs.
/// **Excludes `Inline::Note` content** (footnote prose) for Q1 parity:
/// a reader's eye doesn't fall on footnote text, so it shouldn't pad
/// reading time.
///
/// Returns `None` for empty documents (D6 — avoids "0-minute read").
fn word_count(blocks: &[Block]) -> Option<u32> {
    let mut text = String::new();
    append_block_text_skip_notes(blocks, &mut text);
    let n = text.split_whitespace().count();
    if n == 0 { None } else { u32::try_from(n).ok() }
}

/// Block walker for word-count: produces a flat string of word-bearing
/// text, skipping footnote bodies. Mirrors the structural coverage of
/// [`first_image_src`] but for prose text.
fn append_block_text_skip_notes(blocks: &[Block], out: &mut String) {
    for block in blocks {
        match block {
            Block::Plain(p) => {
                append_inline_text_skip_notes(&p.content, out);
                out.push(' ');
            }
            Block::Paragraph(p) => {
                append_inline_text_skip_notes(&p.content, out);
                out.push(' ');
            }
            Block::Header(h) => {
                append_inline_text_skip_notes(&h.content, out);
                out.push(' ');
            }
            Block::BlockQuote(q) => append_block_text_skip_notes(&q.content, out),
            Block::Div(d) => append_block_text_skip_notes(&d.content, out),
            Block::BulletList(l) => {
                for items in &l.content {
                    append_block_text_skip_notes(items, out);
                }
            }
            Block::OrderedList(l) => {
                for items in &l.content {
                    append_block_text_skip_notes(items, out);
                }
            }
            Block::DefinitionList(dl) => {
                for (term, defs) in &dl.content {
                    append_inline_text_skip_notes(term, out);
                    out.push(' ');
                    for blocks in defs {
                        append_block_text_skip_notes(blocks, out);
                    }
                }
            }
            Block::Figure(f) => append_block_text_skip_notes(&f.content, out),
            Block::LineBlock(lb) => {
                for line in &lb.content {
                    append_inline_text_skip_notes(line, out);
                    out.push(' ');
                }
            }
            Block::CaptionBlock(c) => {
                append_inline_text_skip_notes(&c.content, out);
                out.push(' ');
            }
            // CodeBlock, RawBlock, Table, HorizontalRule, BlockMetadata,
            // NoteDefinition*, Custom: not counted toward reading-time.
            _ => {}
        }
    }
}

fn append_inline_text_skip_notes(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Str(s) => out.push_str(&s.text),
            Inline::Space(_) | Inline::SoftBreak(_) | Inline::LineBreak(_) => out.push(' '),
            Inline::Emph(e) => append_inline_text_skip_notes(&e.content, out),
            Inline::Underline(u) => append_inline_text_skip_notes(&u.content, out),
            Inline::Strong(s) => append_inline_text_skip_notes(&s.content, out),
            Inline::Strikeout(s) => append_inline_text_skip_notes(&s.content, out),
            Inline::Superscript(s) => append_inline_text_skip_notes(&s.content, out),
            Inline::Subscript(s) => append_inline_text_skip_notes(&s.content, out),
            Inline::SmallCaps(s) => append_inline_text_skip_notes(&s.content, out),
            Inline::Quoted(q) => append_inline_text_skip_notes(&q.content, out),
            Inline::Cite(c) => append_inline_text_skip_notes(&c.content, out),
            Inline::Code(c) => out.push_str(&c.text),
            Inline::Math(m) => out.push_str(&m.text),
            Inline::Link(l) => append_inline_text_skip_notes(&l.content, out),
            Inline::Span(s) => append_inline_text_skip_notes(&s.content, out),
            Inline::Insert(i) => append_inline_text_skip_notes(&i.content, out),
            Inline::Highlight(h) => append_inline_text_skip_notes(&h.content, out),
            // Footnote text excluded for Q1-parity reading-time.
            Inline::Note(_) => {}
            // Image alt text not counted; raw inlines / shortcodes /
            // attribute nodes / edit comments / delete / customs:
            // not word-bearing prose.
            _ => {}
        }
    }
}

/// Ceiling division on `u32`. `1 word / 200 wpm` → 1 minute, not 0.
fn div_ceil_u32(num: u32, denom: u32) -> u32 {
    (num + denom - 1) / denom
}

/// File-modification date as `YYYY-MM-DD` (UTC), via the runtime
/// trait. Returns `None` if the runtime can't or won't supply an
/// mtime — currently the WASM Automerge VFS path (see `bd-a3we`).
/// The stage swallows runtime errors gracefully; nothing here can
/// panic.
fn mtime_iso(runtime: &dyn SystemRuntime, path: &Path) -> Option<String> {
    let metadata = runtime.path_metadata(path).ok()?;
    let modified = metadata.modified?;
    let dt = time::OffsetDateTime::from(modified);
    let fmt = time::macros::format_description!("[year]-[month]-[day]");
    dt.format(&fmt).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use async_trait::async_trait;
    use quarto_pandoc_types::block::{Header, Paragraph};
    use quarto_pandoc_types::config_value::{ConfigMapEntry, ConfigValueKind};
    use quarto_pandoc_types::inline::{Image, Link, Note, Space, Str};
    use quarto_pandoc_types::pandoc::Pandoc;
    use quarto_pandoc_types::{
        Block, ConfigValue, Inline,
        attr::{AttrSourceInfo, TargetSourceInfo, empty_attr},
    };
    use quarto_source_map::SourceInfo;
    use quarto_system_runtime::{
        CommandOutput, PathKind, PathMetadata, RuntimeError, RuntimeResult, SystemRuntime, TempDir,
        XdgDirKind,
    };
    use yaml_rust2::Yaml;

    use crate::stage::data::DocumentAst;

    // ─── Mock runtime ────────────────────────────────────────────────
    //
    // Mirrors `crate::stage::tests::MockRuntime` but lets each test
    // configure `path_metadata`'s response. All other methods are
    // safe stubs (the L1 stage doesn't call them, but `StageContext::new`
    // does — the same returns the existing in-tree mock uses).

    struct MockRuntime {
        /// Value of `PathMetadata.modified` when `path_metadata` is
        /// called. `None` simulates the WASM Automerge VFS contract
        /// (`bd-a3we`); a `Some(SystemTime)` simulates a populated
        /// filesystem mtime.
        modified: Option<SystemTime>,
        /// If true, `path_metadata` returns Err instead of Ok. Used to
        /// confirm the stage swallows runtime errors gracefully.
        metadata_err: bool,
    }

    impl MockRuntime {
        fn arc(modified: Option<SystemTime>) -> Arc<dyn SystemRuntime> {
            Arc::new(MockRuntime {
                modified,
                metadata_err: false,
            })
        }
        fn arc_err() -> Arc<dyn SystemRuntime> {
            Arc::new(MockRuntime {
                modified: None,
                metadata_err: true,
            })
        }
    }

    #[async_trait]
    impl SystemRuntime for MockRuntime {
        fn file_read(&self, _: &Path) -> RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn file_write(&self, _: &Path, _: &[u8]) -> RuntimeResult<()> {
            Ok(())
        }
        fn path_exists(&self, _: &Path, _: Option<PathKind>) -> RuntimeResult<bool> {
            Ok(true)
        }
        fn canonicalize(&self, p: &Path) -> RuntimeResult<PathBuf> {
            Ok(p.to_path_buf())
        }
        fn path_metadata(&self, _: &Path) -> RuntimeResult<PathMetadata> {
            if self.metadata_err {
                return Err(RuntimeError::NotSupported("mock".into()));
            }
            Ok(PathMetadata {
                kind: PathKind::File,
                size: 0,
                modified: self.modified,
                accessed: None,
                readonly: false,
            })
        }
        fn file_copy(&self, _: &Path, _: &Path) -> RuntimeResult<()> {
            Ok(())
        }
        fn path_rename(&self, _: &Path, _: &Path) -> RuntimeResult<()> {
            Ok(())
        }
        fn file_remove(&self, _: &Path) -> RuntimeResult<()> {
            Ok(())
        }
        fn dir_create(&self, _: &Path, _: bool) -> RuntimeResult<()> {
            Ok(())
        }
        fn dir_remove(&self, _: &Path, _: bool) -> RuntimeResult<()> {
            Ok(())
        }
        fn dir_list(&self, _: &Path) -> RuntimeResult<Vec<PathBuf>> {
            Ok(vec![])
        }
        fn cwd(&self) -> RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/"))
        }
        fn temp_dir(&self, _: &str) -> RuntimeResult<TempDir> {
            Ok(TempDir::new(PathBuf::from("/tmp/test")))
        }
        fn exec_pipe(&self, _: &str, _: &[&str], _: &[u8]) -> RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn exec_command(
            &self,
            _: &str,
            _: &[&str],
            _: Option<&[u8]>,
        ) -> RuntimeResult<CommandOutput> {
            Ok(CommandOutput {
                code: 0,
                stdout: vec![],
                stderr: vec![],
            })
        }
        fn env_get(&self, _: &str) -> RuntimeResult<Option<String>> {
            Ok(None)
        }
        fn env_all(&self) -> RuntimeResult<std::collections::HashMap<String, String>> {
            Ok(std::collections::HashMap::new())
        }
        async fn fetch_url(&self, _: &str) -> RuntimeResult<(Vec<u8>, String)> {
            Err(RuntimeError::NotSupported("mock".into()))
        }
        fn os_name(&self) -> &'static str {
            "mock"
        }
        fn arch(&self) -> &'static str {
            "mock"
        }
        fn cpu_time(&self) -> RuntimeResult<u64> {
            Ok(0)
        }
        fn xdg_dir(&self, _: XdgDirKind, _: Option<&Path>) -> RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/xdg"))
        }
        fn stdout_write(&self, _: &[u8]) -> RuntimeResult<()> {
            Ok(())
        }
        fn stderr_write(&self, _: &[u8]) -> RuntimeResult<()> {
            Ok(())
        }
    }

    // ─── Construction helpers ────────────────────────────────────────

    fn make_ctx(runtime: Arc<dyn SystemRuntime>) -> StageContext {
        use crate::format::Format;
        use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
        let project = ProjectContext {
            dir: PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![],
            output_dir: PathBuf::from("/project"),
        };
        let document = DocumentInfo::from_path("/project/test.qmd");
        StageContext::new(runtime, Format::html(), project, document).expect("ctx")
    }

    fn make_doc(blocks: Vec<Block>, meta: ConfigValue) -> DocumentAst {
        DocumentAst {
            path: PathBuf::from("/project/test.qmd"),
            ast: Pandoc { meta, blocks },
            ..Default::default()
        }
    }

    /// Run the stage's free function (`autofill_listing_item`) and
    /// return the mutated meta. Avoids the async dance of running the
    /// full PipelineStage; the trait wiring is exercised in tests
    /// 17/18 instead.
    fn run_autofill(
        blocks: Vec<Block>,
        meta: ConfigValue,
        runtime: Arc<dyn SystemRuntime>,
    ) -> ConfigValue {
        let mut doc = make_doc(blocks, meta);
        let ctx = make_ctx(runtime);
        autofill_listing_item(&mut doc, &ctx);
        doc.ast.meta
    }

    /// Default mock with mtime = None (mirrors WASM today).
    fn default_runtime() -> Arc<dyn SystemRuntime> {
        MockRuntime::arc(None)
    }

    fn s(text: &str) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: SourceInfo::default(),
        })
    }
    fn space() -> Inline {
        Inline::Space(Space {
            source_info: SourceInfo::default(),
        })
    }
    fn para(content: Vec<Inline>) -> Block {
        Block::Paragraph(Paragraph {
            content,
            source_info: SourceInfo::default(),
        })
    }
    fn heading(level: usize, content: Vec<Inline>) -> Block {
        Block::Header(Header {
            level,
            attr: empty_attr(),
            content,
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
        })
    }
    fn img(src: &str) -> Inline {
        Inline::Image(Image {
            attr: empty_attr(),
            content: vec![],
            target: (src.to_string(), String::new()),
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        })
    }
    fn link(content: Vec<Inline>, target: &str) -> Inline {
        Inline::Link(Link {
            attr: empty_attr(),
            content,
            target: (target.to_string(), String::new()),
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        })
    }
    fn note(content: Vec<Block>) -> Inline {
        Inline::Note(Note {
            content,
            source_info: SourceInfo::default(),
        })
    }

    fn str_at<'a>(meta: &'a ConfigValue, key: &str) -> Option<&'a str> {
        meta.get_path(&["listing-item", key])
            .and_then(|v| v.as_str())
    }
    fn int_at(meta: &ConfigValue, key: &str) -> Option<i64> {
        meta.get_path(&["listing-item", key])
            .and_then(|v| v.as_int())
    }
    fn has_li_key(meta: &ConfigValue, key: &str) -> bool {
        meta.contains_path(&["listing-item", key])
    }

    // ─── Unit tests ─────────────────────────────────────────────────

    #[test]
    fn t01_no_op_when_meta_listing_item_complete() {
        // Author has populated every curated string field; stage must
        // not touch any of them. Integer fields too.
        let mut m = ConfigValue::default();
        for (k, v) in [
            ("title", "T"),
            ("subtitle", "S"),
            ("description", "D"),
            ("image", "I"),
            ("image-alt", "IA"),
            ("date", "2025-01-01"),
            ("date-modified", "2025-02-02"),
        ] {
            m.insert_path(
                &["listing-item", k],
                ConfigValue::new_string(v, SourceInfo::default()),
            );
        }
        m.insert_path(
            &["listing-item", "word-count"],
            ConfigValue::new_scalar(Yaml::Integer(99), SourceInfo::default()),
        );
        m.insert_path(
            &["listing-item", "reading-time-minutes"],
            ConfigValue::new_scalar(Yaml::Integer(7), SourceInfo::default()),
        );

        let blocks = vec![para(vec![s("Different paragraph text")])];
        let after = run_autofill(blocks, m, default_runtime());

        assert_eq!(str_at(&after, "title"), Some("T"));
        assert_eq!(str_at(&after, "subtitle"), Some("S"));
        assert_eq!(str_at(&after, "description"), Some("D"));
        assert_eq!(str_at(&after, "image"), Some("I"));
        assert_eq!(str_at(&after, "image-alt"), Some("IA"));
        assert_eq!(str_at(&after, "date"), Some("2025-01-01"));
        assert_eq!(str_at(&after, "date-modified"), Some("2025-02-02"));
        assert_eq!(int_at(&after, "word-count"), Some(99));
        assert_eq!(int_at(&after, "reading-time-minutes"), Some(7));
    }

    #[test]
    fn t02_populate_description_full_paragraph_no_truncation() {
        // D11: store the full first-paragraph text. Use a 300-char
        // paragraph to confirm no implicit cap.
        let long: String = "a".repeat(300);
        let blocks = vec![para(vec![s(&long)])];
        let after = run_autofill(blocks, ConfigValue::default(), default_runtime());
        assert_eq!(str_at(&after, "description").map(|s| s.len()), Some(300));
        assert_eq!(str_at(&after, "description"), Some(long.as_str()));
    }

    #[test]
    fn t03_skip_description_when_no_paragraph() {
        // Heading-only document: no paragraph, no description.
        let blocks = vec![heading(1, vec![s("Title")])];
        let after = run_autofill(blocks, ConfigValue::default(), default_runtime());
        assert!(!has_li_key(&after, "description"));
    }

    #[test]
    fn t04_description_skips_empty_paragraphs() {
        // First paragraph is whitespace-only after plain-text
        // extraction; second is the real one.
        let blocks = vec![para(vec![space(), space()]), para(vec![s("Real content")])];
        let after = run_autofill(blocks, ConfigValue::default(), default_runtime());
        assert_eq!(str_at(&after, "description"), Some("Real content"));
    }

    #[test]
    fn t05_populate_image_from_first_inline_image() {
        // Paragraph carrying an inline image; first image's target.0
        // becomes listing-item.image.
        let blocks = vec![para(vec![s("see "), img("figs/cover.png")])];
        let after = run_autofill(blocks, ConfigValue::default(), default_runtime());
        assert_eq!(str_at(&after, "image"), Some("figs/cover.png"));
    }

    #[test]
    fn t06_image_walks_into_link() {
        // Image wrapped in a Link still surfaces.
        let blocks = vec![para(vec![link(vec![img("plot.png")], "/post")])];
        let after = run_autofill(blocks, ConfigValue::default(), default_runtime());
        assert_eq!(str_at(&after, "image"), Some("plot.png"));
    }

    #[test]
    fn t07_image_skips_empty_targets() {
        // First image has empty target; second has "fig.png".
        let blocks = vec![para(vec![img(""), img("fig.png")])];
        let after = run_autofill(blocks, ConfigValue::default(), default_runtime());
        assert_eq!(str_at(&after, "image"), Some("fig.png"));
    }

    #[test]
    fn t08_no_image_leaves_field_unset() {
        // Image-free document.
        let blocks = vec![para(vec![s("just text")])];
        let after = run_autofill(blocks, ConfigValue::default(), default_runtime());
        assert!(!has_li_key(&after, "image"));
    }

    #[test]
    fn t09_word_count_tokenization_simple_doc() {
        // A document with exactly seven words.
        let words = "one two three four five six seven";
        let blocks = vec![para(vec![s(words)])];
        let after = run_autofill(blocks, ConfigValue::default(), default_runtime());
        assert_eq!(int_at(&after, "word-count"), Some(7));
    }

    #[test]
    fn t10_reading_time_ceiling() {
        // 1-word doc → 1 min; 200-word → 1; 201-word → 2.
        // Probe each through three independent fixtures.
        for (n_words, expected_min) in [(1usize, 1i64), (200, 1), (201, 2)] {
            let words = vec!["w"; n_words].join(" ");
            let blocks = vec![para(vec![s(&words)])];
            let after = run_autofill(blocks, ConfigValue::default(), default_runtime());
            assert_eq!(int_at(&after, "word-count"), Some(n_words as i64));
            assert_eq!(
                int_at(&after, "reading-time-minutes"),
                Some(expected_min),
                "n_words={n_words}"
            );
        }
    }

    #[test]
    fn t11_word_count_zero_returns_none() {
        // Empty document: both word-count and reading-time stay unset
        // (D6: avoid "0-minute read" listings).
        let blocks: Vec<Block> = vec![];
        let after = run_autofill(blocks, ConfigValue::default(), default_runtime());
        assert!(!has_li_key(&after, "word-count"));
        assert!(!has_li_key(&after, "reading-time-minutes"));
    }

    #[test]
    fn t12_date_modified_via_runtime() {
        // 2024-01-15 00:00:00 UTC = 1705276800 since epoch.
        let known = UNIX_EPOCH + Duration::from_secs(1_705_276_800);
        let runtime = MockRuntime::arc(Some(known));
        let blocks = vec![para(vec![s("hi")])];
        let after = run_autofill(blocks, ConfigValue::default(), runtime);
        assert_eq!(str_at(&after, "date-modified"), Some("2024-01-15"));
    }

    #[test]
    fn t13_date_modified_skipped_when_runtime_returns_none() {
        // PathMetadata.modified = None — the WASM Automerge VFS
        // contract today (`bd-a3we` will flip this once landed).
        let runtime = MockRuntime::arc(None);
        let blocks = vec![para(vec![s("hi")])];
        let after = run_autofill(blocks, ConfigValue::default(), runtime);
        assert!(!has_li_key(&after, "date-modified"));
    }

    #[test]
    fn t13b_date_modified_skipped_when_runtime_errs() {
        // path_metadata returns Err — stage swallows gracefully and
        // leaves the field unset (no panic, no error propagation).
        let runtime = MockRuntime::arc_err();
        let blocks = vec![para(vec![s("hi")])];
        let after = run_autofill(blocks, ConfigValue::default(), runtime);
        assert!(!has_li_key(&after, "date-modified"));
    }

    #[test]
    fn t14_idempotent() {
        // Running the stage twice in a row produces a fixed point.
        let blocks = vec![para(vec![s("hello world")])];
        let m1 = run_autofill(blocks.clone(), ConfigValue::default(), default_runtime());
        let m2 = run_autofill(blocks, m1.clone(), default_runtime());
        assert_eq!(str_at(&m1, "description"), str_at(&m2, "description"));
        assert_eq!(int_at(&m1, "word-count"), int_at(&m2, "word-count"));
        assert_eq!(
            int_at(&m1, "reading-time-minutes"),
            int_at(&m2, "reading-time-minutes")
        );
    }

    #[test]
    fn t15_preserves_author_extra() {
        // Author put a free-form key in `listing-item.extra.status`;
        // L1 must not touch `extra` at all.
        let mut m = ConfigValue::default();
        m.insert_path(
            &["listing-item", "extra", "status"],
            ConfigValue::new_string("draft", SourceInfo::default()),
        );
        let blocks = vec![para(vec![s("body")])];
        let after = run_autofill(blocks, m, default_runtime());
        let extra_status = after
            .get_path(&["listing-item", "extra", "status"])
            .and_then(|v| v.as_str());
        assert_eq!(extra_status, Some("draft"));
    }

    #[test]
    fn t16_creates_listing_item_when_absent() {
        // Frontmatter has no `listing-item:` at all; after the stage
        // it exists with auto-fills.
        let blocks = vec![para(vec![s("first paragraph")])];
        let after = run_autofill(blocks, ConfigValue::default(), default_runtime());
        assert_eq!(str_at(&after, "description"), Some("first paragraph"));
        assert!(has_li_key(&after, "word-count"));
    }

    #[test]
    fn t16b_word_count_excludes_footnotes() {
        // Q1 parity (D14 in epic + plan word-count discussion):
        // text inside Inline::Note doesn't contribute to word-count.
        // Body has 2 words; footnote has 4. Total counted = 2.
        let footnote = note(vec![para(vec![s("footnote one two three")])]);
        let blocks = vec![para(vec![s("hello"), space(), s("world"), footnote])];
        let after = run_autofill(blocks, ConfigValue::default(), default_runtime());
        assert_eq!(int_at(&after, "word-count"), Some(2));
    }

    // ─── Stage trait tests ───────────────────────────────────────────

    #[test]
    fn t17_stage_advances_documentast_to_documentast() {
        let stage = ListingItemInfoStage::new();
        assert_eq!(stage.input_kind(), PipelineDataKind::DocumentAst);
        assert_eq!(stage.output_kind(), PipelineDataKind::DocumentAst);
        assert_eq!(stage.name(), "listing-item-info");
    }

    #[test]
    fn t18_stage_rejects_non_documentast_input() {
        let stage = ListingItemInfoStage::new();
        let mut ctx = make_ctx(default_runtime());
        // Construct a non-DocumentAst variant. LoadedSource is one;
        // any other variant would do — use the simplest available.
        let non_doc = PipelineData::LoadedSource(crate::stage::data::LoadedSource::new(
            PathBuf::from("/x.qmd"),
            Vec::new(),
        ));
        let result = pollster::block_on(stage.run(non_doc, &mut ctx));
        assert!(matches!(result, Err(PipelineError::UnexpectedInput { .. })));
    }

    // Silence the "unused field for ConfigMapEntry import" warning
    // if rustc complains; we rely on these symbols only via the macro
    // hierarchy when expanding helpers.
    #[allow(dead_code)]
    fn _silence_imports() -> Option<(ConfigMapEntry, ConfigValueKind)> {
        None
    }
}
