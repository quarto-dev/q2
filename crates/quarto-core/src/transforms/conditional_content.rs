/*
 * conditional_content.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Conditional content: .content-visible / .content-hidden
 * (bd-fu16z22k, Phase 4).
 */

//! Conditional content — the Q2 port of Quarto 1's
//! `content-hidden.lua` custom-node filter.
//!
//! Divs, Spans, and CodeBlocks carrying `.content-visible` or
//! `.content-hidden` are kept or removed based on `when-format` /
//! `unless-format`, `when-profile` / `unless-profile` (project
//! profiles, bd-fu16z22k), and `when-meta` / `unless-meta`
//! attributes:
//!
//! - condition kinds **AND** together (`when-format="html"
//!   when-profile="prod"` needs both);
//! - comma/space-separated values within one condition **OR** — a q2
//!   extension; Q1 matches the attribute value literally, so
//!   `when-profile="a,b"` silently never matched there;
//! - `unless-*` negates its kind;
//! - `.content-visible` with no conditions is always visible,
//!   `.content-hidden` with no conditions always hidden;
//! - surviving elements lose the marker class *and* the condition
//!   attributes (Q1's `clearHiddenVisibleAttributes`,
//!   `customnodes/content-hidden.lua:211`).
//!
//! ## Resolving the wrapper
//!
//! A conditional `Div` is scaffolding: it exists only to carry the
//! condition. Q1 resolves a visible one by returning its *content*
//! (`customnodes/content-hidden.lua:66`, `return el.content`), so the
//! wrapper disappears from the output. Spans and CodeBlocks keep their
//! element -- "this is only called on spans and codeblocks, so here we
//! keep the scaffolding element, as opposed to in the Div where we
//! return the inlined content" (`:154`).
//!
//! We unwrap a `Div` only when the wrapper carries nothing of its own
//! after stripping (see [`is_bare_wrapper`]). Q1 unwraps
//! unconditionally, discarding any `#id` or extra classes the author
//! wrote; keeping those costs nothing and loses no parity, because an
//! empty-id wrapper is absorbed into its section by `sectionize_blocks`
//! regardless.
//!
//! Leaving the wrapper in place was a real defect, not a cosmetic one:
//! `collect_toc_entries` walks only the section tree, and a surviving
//! Div terminates that walk, so every heading inside a conditional
//! block dropped out of the table of contents
//! (bd-tabset-headings-in-toc-t04ie7f7, plan
//! `claude-notes/plans/2026-08-18-tabset-headings-in-toc.md`).
//!
//! Semantics notes:
//! - `when-format` uses the same alias table as Lua's
//!   `quarto.doc.is_format` ([`pampa::lua::quarto_doc::is_format_match`]),
//!   matched against the canonical Pandoc format
//!   ([`crate::format::lua_format_for`]) so preview pseudo-formats
//!   behave like render.
//! - `when-meta` resolves a dotted path in the document's **merged**
//!   metadata (the transform runs after `MetadataMergeStage`, so
//!   profile overlays are visible) with Q1 truthiness: present and
//!   not `false` ⇒ true.
//! - The transform runs first in the Normalization phase — before
//!   shortcode resolution, so hidden content cannot emit spurious
//!   shortcode warnings, and long before crossref numbering, so a
//!   hidden float never consumes a number. (Engine cells inside
//!   hidden blocks still *execute* — engines run in an earlier
//!   pipeline stage; Q1 behaves the same way.)
//!
//! Strictness (divergence from Q1, which is silent): unknown
//! `when-*` / `unless-*` attributes on a marker element, and elements
//! carrying *both* marker classes (treated as hidden), warn with
//! **Q-2-42**.
//!
//! ## The llms view (bd-llms-txt-unimplemented-oih6z6j7)
//!
//! When `website.llms-txt: true` is active for an html render
//! ([`crate::transforms::llms::llms_view_active`]), visibility is
//! evaluated for **two** views: the html target, and the llms
//! markdown companion. The llms view's format check matches the
//! literal `llms` token *or* anything the html target matches — the
//! companion mirrors the html page, so `when-format="html"` content
//! stays in it; only explicit `llms` conditions differentiate the
//! views. Content visible in exactly one view is kept and tagged
//! (`.quarto-llms-omit` / `.quarto-llms-keep`) instead of resolved;
//! `LlmsCaptureTransform` at the end of the Finalization phase is
//! the sole consumer of the markers and removes them from both
//! views. Caveat (shared with Q1's marker approach): llms-only
//! content stays in the AST through the Navigation phase, so a
//! *heading* inside an llms-only div can surface in the html TOC,
//! and a crossref float inside one is not numbered. Prefer prose in
//! llms-conditional blocks.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{Attr, Block, ConfigValue, Inline};

use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::llms;

const VISIBLE_CLASS: &str = "content-visible";
const HIDDEN_CLASS: &str = "content-hidden";

const CONDITION_KEYS: [&str; 6] = [
    "when-format",
    "unless-format",
    "when-profile",
    "unless-profile",
    "when-meta",
    "unless-meta",
];

/// See the module docs. Registered first in the Normalization phase
/// of `build_transform_pipeline`.
pub struct ConditionalContentTransform;

impl ConditionalContentTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ConditionalContentTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for ConditionalContentTransform {
    fn name(&self) -> &str {
        "conditional-content"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> crate::Result<()> {
        let lua_format = crate::format::lua_format_for(&ctx.format.target_format).to_string();
        // When the llms view is active (website + llms-txt: true +
        // html target), conditions are evaluated for *both* views and
        // view-specific content is tagged with a marker class instead
        // of being resolved here; `LlmsCaptureTransform` (end of the
        // Finalization phase) is the single consumer of the markers
        // and guarantees they never reach a writer. Same-predicate
        // contract: see `transforms::llms::llms_view_active`.
        let llms_view = crate::transforms::llms::llms_view_active(&ast.meta, ctx);
        let active: Vec<&str> = ctx
            .project
            .config
            .active_config_profiles
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        let mut diagnostics = Vec::new();
        {
            let env = ConditionEnv {
                format: &lua_format,
                llms_view,
                active_profiles: &active,
                meta: &ast.meta,
                diagnostics: &mut diagnostics,
            };
            let mut walker = Walker { env };
            walker.filter_blocks(&mut ast.blocks);
        }
        ctx.diagnostics.extend(diagnostics);
        Ok(())
    }
}

/// Everything a condition evaluation can see.
struct ConditionEnv<'a> {
    /// Canonical Pandoc format (`lua_format_for`), for the alias table.
    format: &'a str,
    /// When true, also evaluate visibility for the `llms` view and
    /// tag view-specific content instead of resolving it here.
    llms_view: bool,
    /// Active project-profile names, activation order.
    active_profiles: &'a [&'a str],
    /// The document's merged metadata.
    meta: &'a ConfigValue,
    diagnostics: &'a mut Vec<DiagnosticMessage>,
}

struct Walker<'a> {
    env: ConditionEnv<'a>,
}

/// What to do with a marker element.
enum Verdict {
    /// Not a conditional element — leave untouched.
    NotConditional,
    /// Keep it, after stripping the condition attributes.
    Keep,
    /// Remove it entirely.
    Remove,
    /// Visible in the target format but not the llms view: keep,
    /// strip conditions, tag `.quarto-llms-omit` so the capture
    /// clone drops it. Only issued when `llms_view` is active.
    KeepTargetOnly,
    /// Hidden in the target format but visible in the llms view:
    /// keep for the capture, strip conditions, tag
    /// `.quarto-llms-keep`; `LlmsCaptureTransform` removes it from
    /// the main AST after cloning. Only issued when `llms_view` is
    /// active.
    KeepLlmsOnly,
}

/// What [`Walker::keep_block`] decided about a block.
enum BlockAction {
    /// Drop the block.
    Drop,
    /// Keep the block where it is.
    Keep,
    /// Replace the block with its own content — a resolved conditional
    /// `Div` whose wrapper carried nothing of its own. See
    /// [`is_bare_wrapper`].
    Splice,
}

impl Walker<'_> {
    /// Evaluate an element's marker classes + condition attributes.
    fn verdict(&mut self, attr: &Attr, source_info: &quarto_source_map::SourceInfo) -> Verdict {
        let visible_marker = attr.1.iter().any(|c| c == VISIBLE_CLASS);
        let hidden_marker = attr.1.iter().any(|c| c == HIDDEN_CLASS);
        if !visible_marker && !hidden_marker {
            return Verdict::NotConditional;
        }
        if visible_marker && hidden_marker {
            self.env.diagnostics.push(
                DiagnosticMessageBuilder::warning(
                    "Element is both `.content-visible` and `.content-hidden`",
                )
                .with_code("Q-2-42")
                .problem(
                    "An element cannot carry both marker classes; it is treated as \
                     `.content-hidden`.",
                )
                .with_location(source_info.clone())
                .build(),
            );
        }

        // Unknown `when-*` / `unless-*` spellings are probably typos.
        for key in attr.2.keys() {
            if (key.starts_with("when-") || key.starts_with("unless-"))
                && !CONDITION_KEYS.contains(&key.as_str())
            {
                self.env.diagnostics.push(
                    DiagnosticMessageBuilder::warning(format!(
                        "Unknown conditional-content attribute `{key}`"
                    ))
                    .with_code("Q-2-42")
                    .problem(format!(
                        "`{key}` is not a recognized condition and is ignored. Supported \
                         conditions: `when-format`, `unless-format`, `when-profile`, \
                         `unless-profile`, `when-meta`, `unless-meta`."
                    ))
                    .with_location(source_info.clone())
                    .build(),
                );
            }
        }

        // Both markers present ⇒ hidden semantics (the safe reading).
        let visibility = |conditions_match: bool| {
            if hidden_marker {
                !conditions_match
            } else {
                conditions_match
            }
        };
        let visible_target = visibility(self.conditions_match(attr, false));
        if !self.env.llms_view {
            return if visible_target {
                Verdict::Keep
            } else {
                Verdict::Remove
            };
        }
        let visible_llms = visibility(self.conditions_match(attr, true));
        match (visible_target, visible_llms) {
            (true, true) => Verdict::Keep,
            (false, false) => Verdict::Remove,
            (true, false) => Verdict::KeepTargetOnly,
            (false, true) => Verdict::KeepLlmsOnly,
        }
    }

    /// AND across condition kinds; OR across comma/space-separated
    /// values within one condition; `unless-*` negates. No condition
    /// attributes ⇒ vacuously true.
    /// `llms_view_eval`: evaluate for the llms view instead of the
    /// target format. The companion is a *mirror of the html page*,
    /// so its format check matches the literal `llms` token **or**
    /// anything the html target matches — `when-format="html"`
    /// content stays in the companion; only the explicit
    /// `llms`-token conditions differentiate the two views.
    fn conditions_match(&self, attr: &Attr, llms_view_eval: bool) -> bool {
        #[derive(Clone, Copy)]
        enum Kind {
            Format,
            Profile,
            Meta,
        }
        let mut result = true;
        for (key, value) in attr.2.iter() {
            let (invert, kind) = match key.as_str() {
                "when-format" => (false, Kind::Format),
                "unless-format" => (true, Kind::Format),
                "when-profile" => (false, Kind::Profile),
                "unless-profile" => (true, Kind::Profile),
                "when-meta" => (false, Kind::Meta),
                "unless-meta" => (true, Kind::Meta),
                _ => continue,
            };
            let any = value
                .split([',', ' '])
                .filter(|v| !v.is_empty())
                .any(|v| match kind {
                    Kind::Format => self.check_format(v, llms_view_eval),
                    Kind::Profile => self.check_profile(v),
                    Kind::Meta => self.check_meta(v),
                });
            result = result && (invert != any);
        }
        result
    }

    fn check_format(&self, query: &str, llms_view_eval: bool) -> bool {
        if llms_view_eval {
            return pampa::lua::quarto_doc::is_format_match(
                crate::transforms::llms::LLMS_FORMAT,
                query,
            ) || pampa::lua::quarto_doc::is_format_match(self.env.format, query);
        }
        pampa::lua::quarto_doc::is_format_match(self.env.format, query)
    }

    fn check_profile(&self, name: &str) -> bool {
        self.env.active_profiles.contains(&name)
    }

    /// Q1's `check_meta`: dotted-path lookup in the merged metadata;
    /// truthy = present and not `false` (null counts as absent).
    fn check_meta(&self, path: &str) -> bool {
        let parts: Vec<&str> = path.split('.').collect();
        match self.env.meta.get_path(&parts) {
            None => false,
            Some(value) => {
                if value.is_null() {
                    return false;
                }
                value.as_bool().unwrap_or(true)
            }
        }
    }

    // ── recursion ───────────────────────────────────────────────────

    fn filter_blocks(&mut self, blocks: &mut Vec<Block>) {
        let taken = std::mem::take(blocks);
        blocks.reserve(taken.len());
        for mut block in taken {
            match self.keep_block(&mut block) {
                BlockAction::Drop => {}
                BlockAction::Keep => blocks.push(block),
                BlockAction::Splice => match block {
                    Block::Div(div) => blocks.extend(div.content),
                    // `Splice` is only ever issued for a `Div`.
                    other => blocks.push(other),
                },
            }
        }
    }

    /// Decide what happens to `block`; recurse into whatever content
    /// it keeps.
    fn keep_block(&mut self, block: &mut Block) -> BlockAction {
        match block {
            Block::Div(div) => {
                let mut splice = false;
                match self.verdict(&div.attr, &div.source_info) {
                    Verdict::Remove => return BlockAction::Drop,
                    Verdict::Keep => {
                        strip_condition_attrs(&mut div.attr, &mut div.attr_source);
                        // The wrapper existed only to carry the
                        // condition; with the condition resolved and
                        // nothing else on it, Q1 returns its content.
                        splice = is_bare_wrapper(&div.attr);
                    }
                    Verdict::KeepTargetOnly => {
                        strip_condition_attrs(&mut div.attr, &mut div.attr_source);
                        llms::add_marker_class(
                            &mut div.attr,
                            &mut div.attr_source,
                            llms::LLMS_OMIT_CLASS,
                        );
                    }
                    Verdict::KeepLlmsOnly => {
                        strip_condition_attrs(&mut div.attr, &mut div.attr_source);
                        llms::add_marker_class(
                            &mut div.attr,
                            &mut div.attr_source,
                            llms::LLMS_KEEP_CLASS,
                        );
                    }
                    Verdict::NotConditional => {}
                }
                self.filter_blocks(&mut div.content);
                if splice {
                    return BlockAction::Splice;
                }
            }
            Block::CodeBlock(cb) => match self.verdict(&cb.attr, &cb.source_info) {
                Verdict::Remove => return BlockAction::Drop,
                Verdict::Keep => strip_condition_attrs(&mut cb.attr, &mut cb.attr_source),
                Verdict::KeepTargetOnly => {
                    strip_condition_attrs(&mut cb.attr, &mut cb.attr_source);
                    llms::add_marker_class(
                        &mut cb.attr,
                        &mut cb.attr_source,
                        llms::LLMS_OMIT_CLASS,
                    );
                }
                Verdict::KeepLlmsOnly => {
                    strip_condition_attrs(&mut cb.attr, &mut cb.attr_source);
                    llms::add_marker_class(
                        &mut cb.attr,
                        &mut cb.attr_source,
                        llms::LLMS_KEEP_CLASS,
                    );
                }
                Verdict::NotConditional => {}
            },
            Block::Plain(b) => self.filter_inlines(&mut b.content),
            Block::Paragraph(b) => self.filter_inlines(&mut b.content),
            Block::Header(h) => self.filter_inlines(&mut h.content),
            Block::LineBlock(lb) => {
                for line in &mut lb.content {
                    self.filter_inlines(line);
                }
            }
            Block::BlockQuote(bq) => self.filter_blocks(&mut bq.content),
            Block::OrderedList(ol) => {
                for item in &mut ol.content {
                    self.filter_blocks(item);
                }
            }
            Block::BulletList(bl) => {
                for item in &mut bl.content {
                    self.filter_blocks(item);
                }
            }
            Block::DefinitionList(dl) => {
                for (term, defs) in &mut dl.content {
                    self.filter_inlines(term);
                    for def in defs {
                        self.filter_blocks(def);
                    }
                }
            }
            Block::Figure(fig) => {
                self.filter_blocks(&mut fig.content);
                if let Some(short) = &mut fig.caption.short {
                    self.filter_inlines(short);
                }
                if let Some(long) = &mut fig.caption.long {
                    self.filter_blocks(long);
                }
            }
            Block::Table(table) => {
                if let Some(short) = &mut table.caption.short {
                    self.filter_inlines(short);
                }
                if let Some(long) = &mut table.caption.long {
                    self.filter_blocks(long);
                }
                for row in &mut table.head.rows {
                    for cell in &mut row.cells {
                        self.filter_blocks(&mut cell.content);
                    }
                }
                for body in &mut table.bodies {
                    for row in &mut body.body {
                        for cell in &mut row.cells {
                            self.filter_blocks(&mut cell.content);
                        }
                    }
                }
                for row in &mut table.foot.rows {
                    for cell in &mut row.cells {
                        self.filter_blocks(&mut cell.content);
                    }
                }
            }
            Block::Custom(custom) => {
                for (_name, slot) in &mut custom.slots {
                    use quarto_pandoc_types::custom::Slot;
                    match slot {
                        Slot::Block(b) => {
                            // A slot holds exactly one block; neither
                            // removal nor splicing is representable,
                            // so only recurse.
                            let _ = self.keep_block(b);
                        }
                        Slot::Blocks(bs) => self.filter_blocks(bs),
                        Slot::Inline(i) => {
                            let _ = self.keep_inline(i);
                        }
                        Slot::Inlines(is) => self.filter_inlines(is),
                    }
                }
            }
            _ => {}
        }
        BlockAction::Keep
    }

    fn filter_inlines(&mut self, inlines: &mut Vec<Inline>) {
        inlines.retain_mut(|inline| self.keep_inline(inline));
    }

    fn keep_inline(&mut self, inline: &mut Inline) -> bool {
        match inline {
            Inline::Span(span) => {
                match self.verdict(&span.attr, &span.source_info) {
                    Verdict::Remove => return false,
                    Verdict::Keep => strip_condition_attrs(&mut span.attr, &mut span.attr_source),
                    Verdict::KeepTargetOnly => {
                        strip_condition_attrs(&mut span.attr, &mut span.attr_source);
                        llms::add_marker_class(
                            &mut span.attr,
                            &mut span.attr_source,
                            llms::LLMS_OMIT_CLASS,
                        );
                    }
                    Verdict::KeepLlmsOnly => {
                        strip_condition_attrs(&mut span.attr, &mut span.attr_source);
                        llms::add_marker_class(
                            &mut span.attr,
                            &mut span.attr_source,
                            llms::LLMS_KEEP_CLASS,
                        );
                    }
                    Verdict::NotConditional => {}
                }
                self.filter_inlines(&mut span.content);
            }
            Inline::Emph(i) => self.filter_inlines(&mut i.content),
            Inline::Underline(i) => self.filter_inlines(&mut i.content),
            Inline::Strong(i) => self.filter_inlines(&mut i.content),
            Inline::Strikeout(i) => self.filter_inlines(&mut i.content),
            Inline::Superscript(i) => self.filter_inlines(&mut i.content),
            Inline::Subscript(i) => self.filter_inlines(&mut i.content),
            Inline::SmallCaps(i) => self.filter_inlines(&mut i.content),
            Inline::Quoted(i) => self.filter_inlines(&mut i.content),
            Inline::Cite(i) => self.filter_inlines(&mut i.content),
            Inline::Link(i) => self.filter_inlines(&mut i.content),
            Inline::Image(i) => self.filter_inlines(&mut i.content),
            Inline::Note(note) => self.filter_blocks(&mut note.content),
            _ => {}
        }
        true
    }
}

/// Remove the condition attributes from a surviving element, keeping
/// classes (including the marker class) — Q1's
/// `clearHiddenVisibleAttributes`. The parallel `AttrSourceInfo`
/// entries are removed in lockstep to preserve the
/// positional-alignment invariant (see `attr.rs`); on a preexisting
/// misalignment the source entries are cleared rather than guessed.
fn strip_condition_attrs(attr: &mut Attr, attr_source: &mut quarto_pandoc_types::AttrSourceInfo) {
    let aligned = attr.2.len() == attr_source.attributes.len();
    if aligned {
        let keep: Vec<bool> = attr
            .2
            .keys()
            .map(|k| !CONDITION_KEYS.contains(&k.as_str()))
            .collect();
        let mut it = keep.iter();
        attr_source.attributes.retain(|_| *it.next().unwrap());
    } else {
        attr_source.attributes.clear();
    }
    attr.2.retain(|k, _| !CONDITION_KEYS.contains(&k.as_str()));

    // Q1's `clearHiddenVisibleAttributes` drops the marker classes too
    // (`customnodes/content-hidden.lua:216`). They have done their job
    // once the condition is resolved, and leaving them behind puts a
    // `<div class="content-visible">` in the output that Q1 never emits.
    let classes_aligned = attr.1.len() == attr_source.classes.len();
    if classes_aligned {
        let keep: Vec<bool> = attr
            .1
            .iter()
            .map(|c| c != VISIBLE_CLASS && c != HIDDEN_CLASS)
            .collect();
        let mut it = keep.iter();
        attr_source.classes.retain(|_| *it.next().unwrap());
    } else {
        attr_source.classes.clear();
    }
    attr.1.retain(|c| c != VISIBLE_CLASS && c != HIDDEN_CLASS);
}

/// True when a resolved conditional `Div` carries nothing of its own —
/// no id, no classes, no attributes — once the marker class and the
/// condition attributes have been stripped.
///
/// Q1 unwraps a visible conditional Div unconditionally
/// (`customnodes/content-hidden.lua:66`, `return el.content`), which
/// also discards any `#id` or extra classes the author wrote. We unwrap
/// only what the feature itself contributed, so author attributes
/// survive. The two rules coincide whenever the Div is a bare marker,
/// which is every real use we have measured; and when they differ, an
/// empty-id wrapper is absorbed into its section by `sectionize_blocks`
/// anyway, so the TOC outcome is identical either way.
fn is_bare_wrapper(attr: &Attr) -> bool {
    attr.0.is_empty() && attr.1.is_empty() && attr.2.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::AttrSourceInfo;
    use quarto_pandoc_types::block::Div;
    use quarto_source_map::{By, SourceInfo};

    fn attr(classes: &[&str], kvs: &[(&str, &str)]) -> Attr {
        (
            String::new(),
            classes.iter().map(|c| c.to_string()).collect(),
            kvs.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    fn plain(text: &str) -> Block {
        Block::Plain(quarto_pandoc_types::block::Plain {
            content: vec![Inline::Str(quarto_pandoc_types::inline::Str {
                text: text.to_string(),
                source_info: SourceInfo::generated(By::unknown()),
            })],
            source_info: SourceInfo::generated(By::unknown()),
        })
    }

    fn div(classes: &[&str], kvs: &[(&str, &str)], text: &str) -> Block {
        Block::Div(Div {
            attr: attr(classes, kvs),
            content: vec![Block::Plain(quarto_pandoc_types::block::Plain {
                content: vec![Inline::Str(quarto_pandoc_types::inline::Str {
                    text: text.to_string(),
                    source_info: SourceInfo::generated(By::unknown()),
                })],
                source_info: SourceInfo::generated(By::unknown()),
            })],
            source_info: SourceInfo::generated(By::unknown()),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn run(
        blocks: &mut Vec<Block>,
        format: &str,
        active: &[&str],
        meta: &ConfigValue,
    ) -> Vec<DiagnosticMessage> {
        run_with_llms(blocks, format, active, meta, false)
    }

    fn run_with_llms(
        blocks: &mut Vec<Block>,
        format: &str,
        active: &[&str],
        meta: &ConfigValue,
        llms_view: bool,
    ) -> Vec<DiagnosticMessage> {
        let mut diagnostics = Vec::new();
        let env = ConditionEnv {
            format,
            llms_view,
            active_profiles: active,
            meta,
            diagnostics: &mut diagnostics,
        };
        let mut walker = Walker { env };
        walker.filter_blocks(blocks);
        diagnostics
    }

    fn empty_meta() -> ConfigValue {
        ConfigValue::new_map(vec![], SourceInfo::generated(By::unknown()))
    }

    fn texts(blocks: &[Block]) -> String {
        // Debug-format is a lazy but reliable way to see which
        // Str contents survived.
        format!("{blocks:?}")
    }

    #[test]
    fn visible_kept_when_profile_active_removed_otherwise() {
        let meta = empty_meta();
        let mut blocks = vec![div(&["content-visible"], &[("when-profile", "adv")], "X")];
        run(&mut blocks, "html", &["adv"], &meta);
        assert_eq!(blocks.len(), 1);

        let mut blocks = vec![div(&["content-visible"], &[("when-profile", "adv")], "X")];
        run(&mut blocks, "html", &[], &meta);
        assert!(blocks.is_empty());
    }

    #[test]
    fn hidden_inverts() {
        let meta = empty_meta();
        let mut blocks = vec![div(&["content-hidden"], &[("when-profile", "adv")], "X")];
        run(&mut blocks, "html", &["adv"], &meta);
        assert!(blocks.is_empty());

        let mut blocks = vec![div(&["content-hidden"], &[("when-profile", "adv")], "X")];
        run(&mut blocks, "html", &[], &meta);
        assert_eq!(blocks.len(), 1);
    }

    #[test]
    fn bare_markers() {
        let meta = empty_meta();
        let mut blocks = vec![
            div(&["content-hidden"], &[], "H"),
            div(&["content-visible"], &[], "V"),
        ];
        run(&mut blocks, "html", &[], &meta);
        assert_eq!(blocks.len(), 1);
        assert!(texts(&blocks).contains('V'));
    }

    #[test]
    fn format_alias_matching() {
        let meta = empty_meta();
        // revealjs is html-family: when-format="html" matches.
        let mut blocks = vec![div(&["content-visible"], &[("when-format", "html")], "X")];
        run(&mut blocks, "revealjs", &[], &meta);
        assert_eq!(blocks.len(), 1, "revealjs is an html alias");

        let mut blocks = vec![div(&["content-visible"], &[("when-format", "pdf")], "X")];
        run(&mut blocks, "html", &[], &meta);
        assert!(blocks.is_empty());
    }

    #[test]
    fn kinds_and_together_values_or_within() {
        let meta = empty_meta();
        let kvs = [("when-format", "html"), ("when-profile", "a,b")];
        let mut blocks = vec![div(&["content-visible"], &kvs, "X")];
        run(&mut blocks, "html", &["b"], &meta);
        assert_eq!(blocks.len(), 1, "html AND (a OR b) holds");

        let mut blocks = vec![div(&["content-visible"], &kvs, "X")];
        run(&mut blocks, "latex", &["b"], &meta);
        assert!(blocks.is_empty(), "format leg fails");

        let mut blocks = vec![div(&["content-visible"], &kvs, "X")];
        run(&mut blocks, "html", &["c"], &meta);
        assert!(blocks.is_empty(), "profile leg fails");
    }

    #[test]
    fn meta_truthiness() {
        let meta = {
            use pampa::pandoc::yaml_to_config_value;
            use pampa::utils::diagnostic_collector::DiagnosticCollector;
            use quarto_config::InterpretationContext;
            let parsed =
                quarto_yaml::parse_file("features:\n  beta: true\n  off: false\n", "_quarto.yml")
                    .expect("valid yaml");
            let mut diagnostics = DiagnosticCollector::new();
            yaml_to_config_value(
                parsed,
                InterpretationContext::ProjectConfig,
                &mut diagnostics,
            )
        };
        let case = |path: &str, meta: &ConfigValue| {
            let mut blocks = vec![div(&["content-visible"], &[("when-meta", path)], "X")];
            run(&mut blocks, "html", &[], meta);
            !blocks.is_empty()
        };
        assert!(case("features.beta", &meta));
        assert!(!case("features.off", &meta), "explicit false is falsy");
        assert!(!case("features.missing", &meta));
        assert!(case("features", &meta), "a map is truthy");
    }

    #[test]
    fn surviving_element_loses_marker_class_and_conditions_keeps_the_rest() {
        let meta = empty_meta();
        let mut blocks = vec![div(
            &["content-visible", "keep-me"],
            &[("when-profile", "adv"), ("data-x", "1")],
            "X",
        )];
        run(&mut blocks, "html", &["adv"], &meta);
        // Author attributes remain, so the wrapper is not bare and the
        // Div stays -- only what the feature contributed is removed.
        let Block::Div(d) = &blocks[0] else {
            panic!("div carrying author attrs survives")
        };
        assert!(
            !d.attr.1.contains(&"content-visible".to_string()),
            "marker class stripped (Q1's clearHiddenVisibleAttributes)"
        );
        assert!(d.attr.1.contains(&"keep-me".to_string()));
        assert!(!d.attr.2.contains_key("when-profile"), "stripped");
        assert!(d.attr.2.contains_key("data-x"), "unrelated attrs kept");
    }

    #[test]
    fn bare_wrapper_is_unwrapped() {
        let meta = empty_meta();
        let mut blocks = vec![div(&["content-visible"], &[("when-format", "html")], "X")];
        run(&mut blocks, "html", &[], &meta);
        assert_eq!(blocks.len(), 1, "content survives: {}", texts(&blocks));
        assert!(
            !matches!(blocks[0], Block::Div(_)),
            "a wrapper carrying nothing of its own is spliced away"
        );
        assert!(texts(&blocks).contains('X'), "{}", texts(&blocks));
    }

    #[test]
    fn wrapper_with_an_id_is_kept() {
        let meta = empty_meta();
        let mut blocks = vec![div(&["content-visible"], &[("when-format", "html")], "X")];
        let Block::Div(d) = &mut blocks[0] else {
            panic!("div")
        };
        d.attr.0 = "anchor".to_string();
        run(&mut blocks, "html", &[], &meta);
        let Block::Div(d) = &blocks[0] else {
            panic!("a wrapper with an author id is not bare, so it survives")
        };
        assert_eq!(d.attr.0, "anchor", "author id preserved");
        assert!(d.attr.1.is_empty(), "marker class still stripped");
    }

    #[test]
    fn both_markers_warn_and_hide() {
        let meta = empty_meta();
        let mut blocks = vec![div(&["content-visible", "content-hidden"], &[], "X")];
        let diags = run(&mut blocks, "html", &[], &meta);
        assert!(blocks.is_empty(), "hidden wins");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-2-42"));
    }

    #[test]
    fn unknown_condition_attr_warns_once() {
        let meta = empty_meta();
        let mut blocks = vec![div(&["content-visible"], &[("when-profil", "x")], "X")];
        let diags = run(&mut blocks, "html", &[], &meta);
        assert_eq!(blocks.len(), 1, "unknown condition doesn't hide");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-2-42"));
        assert!(diags[0].to_text(None).contains("when-profil"));
    }

    #[test]
    fn nested_conditionals() {
        let meta = empty_meta();
        // `adv` is not active, so the inner block is removed; the outer
        // condition holds, so the outer's *other* content survives.
        let inner = div(&["content-visible"], &[("when-profile", "adv")], "INNER");
        let outer = Block::Div(Div {
            attr: attr(&["content-visible"], &[("when-format", "html")]),
            content: vec![inner, plain("OUTER")],
            source_info: SourceInfo::generated(By::unknown()),
            attr_source: AttrSourceInfo::empty(),
        });
        let mut blocks = vec![outer];
        run(&mut blocks, "html", &[], &meta);
        assert!(!texts(&blocks).contains("INNER"), "inner removed");
        assert!(
            texts(&blocks).contains("OUTER"),
            "outer content survives: {}",
            texts(&blocks)
        );
        assert!(
            !blocks.iter().any(|b| matches!(b, Block::Div(_))),
            "both bare wrappers are spliced away"
        );
    }

    #[test]
    fn error_code_is_registered_in_catalog() {
        assert!(quarto_error_catalog::ERROR_CATALOG.get("Q-2-42").is_some());
    }

    /// Four-quadrant llms-view semantics
    /// (bd-llms-txt-unimplemented-oih6z6j7): with the llms view
    /// active, view-specific content is tagged instead of resolved.
    #[test]
    fn llms_view_tags_view_specific_content() {
        use crate::transforms::llms::{LLMS_KEEP_CLASS, LLMS_OMIT_CLASS};

        let mut blocks = vec![
            // Visible in html, hidden in llms → tagged omit.
            div(&["content-hidden"], &[("when-format", "llms")], "HTMLONLY"),
            // Hidden in html, visible in llms → tagged keep.
            div(&["content-visible"], &[("when-format", "llms")], "LLMSONLY"),
            // Visible in both → plain keep, no marker.
            div(&["content-visible"], &[("when-format", "html")], "BOTH"),
            // Hidden in both → removed.
            div(&["content-hidden"], &[("unless-format", "pdf")], "NEITHER"),
        ];
        let meta = empty_meta();
        let diags = run_with_llms(&mut blocks, "html", &[], &meta, true);
        assert!(diags.is_empty(), "no diagnostics expected: {diags:?}");

        assert_eq!(blocks.len(), 3, "NEITHER removed: {}", texts(&blocks));
        let classes_of = |b: &Block| -> Vec<String> {
            let Block::Div(d) = b else { panic!("div") };
            d.attr.1.clone()
        };
        assert!(
            classes_of(&blocks[0]).iter().any(|c| c == LLMS_OMIT_CLASS),
            "HTMLONLY tagged omit"
        );
        assert!(
            classes_of(&blocks[1]).iter().any(|c| c == LLMS_KEEP_CLASS),
            "LLMSONLY tagged keep"
        );
        // BOTH is fully resolved and its wrapper carried nothing of
        // its own, so it is spliced away entirely -- no Div, no marker.
        assert!(
            !matches!(blocks[2], Block::Div(_)),
            "BOTH's wrapper is unwrapped once both views agree"
        );
        assert!(texts(&blocks).contains("BOTH"), "BOTH's content survives");

        // The two tagged survivors keep their Div (the marker class
        // makes them non-bare) and lose their condition attributes.
        for b in &blocks[..2] {
            let Block::Div(d) = b else {
                panic!("tagged survivors stay wrapped")
            };
            assert!(
                d.attr
                    .2
                    .keys()
                    .all(|k| !k.starts_with("when-") && !k.starts_with("unless-")),
                "condition attrs stripped"
            );
            assert!(
                !d.attr
                    .1
                    .iter()
                    .any(|c| c == "content-visible" || c == "content-hidden"),
                "marker classes stripped"
            );
        }
    }

    /// Without the llms view, `when-format="llms"` behaves like any
    /// non-matching format: content-visible removed, content-hidden
    /// kept, no markers.
    #[test]
    fn llms_conditions_inert_without_llms_view() {
        let mut blocks = vec![
            div(&["content-visible"], &[("when-format", "llms")], "LLMSONLY"),
            div(&["content-hidden"], &[("when-format", "llms")], "HTMLONLY"),
        ];
        let meta = empty_meta();
        run(&mut blocks, "html", &[], &meta);
        let text = texts(&blocks);
        assert!(!text.contains("LLMSONLY"), "visible-when-llms removed");
        assert!(text.contains("HTMLONLY"), "hidden-when-llms kept");
        assert!(
            !text.contains("quarto-llms-"),
            "no markers without the llms view"
        );
    }
}
