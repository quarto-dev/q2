/*
 * transforms/lightbox.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Lightbox images (Quarto 1's built-in `lightbox` feature, GLightbox).
 */

//! Lightbox images and galleries (bd-kcpkgftw).
//!
//! Port of Quarto 1's built-in lightbox
//! (`src/resources/filters/layout/lightbox.lua` and
//! `filters/modules/lightbox.lua`). A lightboxed image is wrapped in a
//! link to itself, `<a href="img.png" class="lightbox"
//! data-gallery="…">`, and GLightbox (`resources/js/glightbox/`, the same
//! 3.1.0 build Q1 ships) opens that link as a zoomed overlay. Links that
//! share a `data-gallery` page together.
//!
//! ## The `lightbox` option
//!
//! | value | mode |
//! |---|---|
//! | absent | explicit: only images with the `.lightbox` class |
//! | `true`, `auto`, `{match: auto}` | automatic: every figure and every image alone in its paragraph, unless `.nolightbox` |
//! | `false` | off: nothing, not even `.lightbox` images (Q1 skips the filter) |
//!
//! The object form also takes `effect`, `desc-position`, `loop` and
//! `css-class`, which feed the GLightbox constructor. Q1 hard-errors on
//! an unknown `effect`; here every invalid value warns `Q-19-5` at its
//! source location and falls back to the default (porting lever 1).
//! `loop` defaults to `false`, which is what Q1 does (its docs say `true`).
//!
//! ## Which images
//!
//! - inside a `Figure` or the `content` of a `FloatRefTarget`, and an
//!   image alone in a paragraph: automatic;
//! - anywhere else (inline among text, in captions): explicit class only;
//! - inside a `Link`: never — the image is already a link target.
//!
//! The link's title (GLightbox's slide title) is the figure caption; for a
//! cross-referenceable float it carries the number, "Figure 1: …", like the
//! figcaption. Without a caption it is the image's alt text.
//!
//! Per-image attributes: `group` names the gallery (default: one gallery
//! per image); `description` is markdown, rendered into a hidden
//! `div.glightbox-desc` at the end of the body that the link points at
//! through `data-glightbox`; `desc-position`, `type`, `effect`, `zoomable`
//! and `draggable` move to the link as `data-*` attributes, which
//! GLightbox reads. A `title` attribute overrides the title. (Q1 sets the
//! bare names and lets Pandoc's HTML writer add `data-`; Q2 puts the
//! prefix in the AST so the q2-preview `Link`, which only passes `data-*`
//! through, renders the same attributes.)
//!
//! Code cells: knitr forwards an unknown chunk option to the cell `Div` as
//! a JSON-encoded attribute, so `#| lightbox: {group: g}` arrives as
//! `lightbox='{"group":"g"}'` on `div.cell`. The cell pass pushes it down
//! to the cell's images (a list value is indexed per plot) before the main
//! walk, as Q1's `Div` handler does.
//!
//! ## Ordering and scope
//!
//! Runs in Finalization right before `crossref-render`, so floats are
//! still `FloatRefTarget` nodes with their caption and number in hand —
//! the same shape the q2-preview pipeline (which never runs
//! `crossref-render`) sees, so both produce the same links. The crossref
//! renderer and the HTML writer already look through `Link[Image]` for a
//! figure's image. `hephaestus-render` rewrites a `.hep` image's `src`
//! later on and calls [`sync_lightbox_hrefs`] to keep the link in step.
//!
//! Bootstrap-themed `HtmlRender` and `HtmlPreview`. Only the render ships
//! GLightbox (`js:glightbox`, `css:glightbox`) and its init script, and
//! only when at least one image was wrapped; the preview gets the links
//! for DOM parity but no overlay. Revealjs and dashboards (which Q1 also
//! covers) are follow-up work.

use hashlink::LinkedHashMap;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::block::{Block, Div, Paragraph};
use quarto_pandoc_types::config_value::{
    ConfigValue, ConfigValueKind, blocks_to_plain_text, inlines_to_plain_text,
};
use quarto_pandoc_types::custom::CustomNode;
use quarto_pandoc_types::inline::{Image, Inline, Inlines, Link, Str};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{AttrSourceInfo, Slot, TargetSourceInfo};
use quarto_source_map::SourceInfo;

use super::crossref_render::format_crossref_number;
use super::image_walk::for_each_image_mut;
use crate::Result;
use crate::artifact::{Artifact, ArtifactScope};
use crate::ast_walk::{ListVisitor, block_children_mut, inline_children_mut};
use crate::crossref::FLOAT_REF_TARGET;
use crate::format::{PipelineProfile, is_minimal_html};
use crate::render::{ChapterSeed, RenderContext};
use crate::transform::{AstTransform, TransformPhase};
use crate::transforms::append_with_sentinel;

/// The vendored GLightbox 3.1.0. Version contract: `resources/js/README.md`
/// § `glightbox/`.
const GLIGHTBOX_JS: &[u8] = include_bytes!("../../../../resources/js/glightbox/glightbox.min.js");
const GLIGHTBOX_CSS: &[u8] = include_bytes!("../../../../resources/js/glightbox/glightbox.min.css");

const JS_KEY: &str = "js:glightbox";
const CSS_KEY: &str = "css:glightbox";
const JS_REL_PATH: &str = "glightbox/glightbox.min.js";
const CSS_REL_PATH: &str = "glightbox/glightbox.min.css";

/// Marks a link for GLightbox (its `selector`) and, on an image, asks for
/// the treatment explicitly.
const LIGHTBOX: &str = "lightbox";
/// Opts an image out of automatic mode.
const NO_LIGHTBOX: &str = "nolightbox";
const GALLERY_PREFIX: &str = "quarto-lightbox-gallery-";
const DESCRIPTION: &str = "description";
const TITLE: &str = "title";
const GROUP: &str = "group";

/// Image attributes moved to the link (Q1 `kForwardedAttr`, less `title`
/// and `description`, which are handled on their own).
const FORWARDED: [&str; 5] = ["desc-position", "type", "effect", "zoomable", "draggable"];

/// Attributes a code cell's `lightbox` option may set on its images.
const CELL_FORWARDED: [&str; 7] = [
    TITLE,
    DESCRIPTION,
    "desc-position",
    "type",
    "effect",
    "zoomable",
    "draggable",
];

/// Sentinel making a re-run append the init only once.
const INIT_SENTINEL: &str = "<!-- quarto-lightbox -->";

/// Catalog code for an invalid `lightbox` option value.
const CODE_INVALID_OPTION: &str = "Q-19-5";

/// See module docs.
pub struct LightboxTransform;

impl LightboxTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LightboxTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for LightboxTransform {
    fn name(&self) -> &str {
        "lightbox"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let render = ctx.pipeline_profile == PipelineProfile::HtmlRender;
        let preview = ctx.pipeline_profile == PipelineProfile::HtmlPreview;
        if !(render || preview) || is_minimal_html(&ast.meta) {
            return Ok(());
        }

        let config = LightboxConfig::from_meta(&ast.meta, &mut ctx.diagnostics);
        if config.mode == Mode::Off {
            return Ok(());
        }
        let auto = config.mode == Mode::Auto;

        let mut cells = CellPass {
            auto,
            diagnostics: &mut ctx.diagnostics,
        };
        cells.blocks(&mut ast.blocks);

        let mut walker = Walker {
            auto,
            count: 0,
            descriptions: Vec::new(),
            scope: Scope::default(),
            chapter_seed: ctx.chapter_seed,
            diagnostics: &mut ctx.diagnostics,
        };
        walker.blocks(&mut ast.blocks);
        let Walker {
            count,
            descriptions,
            ..
        } = walker;
        if count == 0 {
            return Ok(());
        }

        if !descriptions.is_empty() {
            let mut kvs = LinkedHashMap::new();
            kvs.insert("aria-hidden".to_string(), "true".to_string());
            ast.blocks.push(Block::Div(Div {
                attr: (String::new(), vec!["hidden".to_string()], kvs),
                content: descriptions,
                // The container is lightbox chrome; the descriptions inside
                // keep their images' source info.
                source_info: SourceInfo::generated(quarto_source_map::By {
                    kind: "lightbox".to_string(),
                    data: serde_json::Value::Null,
                }),
                attr_source: AttrSourceInfo::empty(),
            }));
        }

        // The runtime is render-only: the preview has no
        // `ApplyTemplateStage` to turn artifacts into tags. It still gets
        // the links, so its DOM matches the render's.
        if render {
            ctx.artifacts.store(
                JS_KEY,
                Artifact::from_bytes(GLIGHTBOX_JS.to_vec(), "text/javascript")
                    .with_path(JS_REL_PATH)
                    .with_scope(ArtifactScope::Project),
            );
            ctx.artifacts.store(
                CSS_KEY,
                Artifact::from_bytes(GLIGHTBOX_CSS.to_vec(), "text/css")
                    .with_path(CSS_REL_PATH)
                    .with_scope(ArtifactScope::Project),
            );
            append_with_sentinel(
                &mut ast.meta,
                "after-body",
                INIT_SENTINEL,
                init_script(&config),
            );
        }
        Ok(())
    }
}

/// After something rewrote a lightboxed image's `src` (hephaestus swaps a
/// `.hep` for the SVG it rendered), point the wrapping link at the new
/// target too.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn sync_lightbox_hrefs(blocks: &mut [Block]) {
    crate::ast_walk::for_each_inline_mut(blocks, &mut |inline| {
        let Inline::Link(link) = inline else {
            return;
        };
        if !link.attr.1.iter().any(|c| c == LIGHTBOX) {
            return;
        }
        if let [Inline::Image(img)] = link.content.as_slice()
            && link.target.0 != img.target.0
        {
            link.target.0 = img.target.0.clone();
        }
    });
}

// ---------------------------------------------------------------------------
// Options

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Off,
    Explicit,
    Auto,
}

#[derive(Debug, Clone, PartialEq)]
struct LightboxConfig {
    mode: Mode,
    effect: String,
    desc_position: String,
    loop_: bool,
    css_class: Option<String>,
}

impl Default for LightboxConfig {
    fn default() -> Self {
        Self {
            mode: Mode::Explicit,
            effect: "zoom".to_string(),
            desc_position: "bottom".to_string(),
            loop_: false,
            css_class: None,
        }
    }
}

const EFFECTS: [&str; 3] = ["fade", "zoom", "none"];
const DESC_POSITIONS: [&str; 4] = ["top", "bottom", "left", "right"];

impl LightboxConfig {
    fn from_meta(meta: &ConfigValue, diagnostics: &mut Vec<DiagnosticMessage>) -> Self {
        let mut config = Self::default();
        let Some(value) = meta.get(LIGHTBOX) else {
            return config;
        };
        if let Some(b) = value.as_bool() {
            config.mode = if b { Mode::Auto } else { Mode::Off };
            return config;
        }
        if let Some(entries) = value.as_map_entries() {
            for entry in entries {
                let v = &entry.value;
                match entry.key.as_str() {
                    "match" => match v.as_plain_text().as_deref() {
                        Some("auto") => config.mode = Mode::Auto,
                        _ => invalid(diagnostics, v, "match", "`auto`"),
                    },
                    "effect" => match v.as_plain_text() {
                        Some(s) if EFFECTS.contains(&s.as_str()) => config.effect = s,
                        _ => invalid(diagnostics, v, "effect", "`fade`, `zoom` or `none`"),
                    },
                    "desc-position" => match v.as_plain_text() {
                        Some(s) if DESC_POSITIONS.contains(&s.as_str()) => {
                            config.desc_position = s
                        }
                        _ => invalid(
                            diagnostics,
                            v,
                            "desc-position",
                            "`top`, `bottom`, `left` or `right`",
                        ),
                    },
                    "loop" => match v.as_bool() {
                        Some(b) => config.loop_ = b,
                        None => invalid(diagnostics, v, "loop", "`true` or `false`"),
                    },
                    "css-class" => match v.as_plain_text() {
                        Some(s) if !s.trim().is_empty() => config.css_class = Some(s),
                        _ => invalid(diagnostics, v, "css-class", "a class name"),
                    },
                    other => diagnostics.push(
                        DiagnosticMessageBuilder::warning("Invalid lightbox option")
                            .with_code(CODE_INVALID_OPTION)
                            .with_location(entry.key_source.clone())
                            .problem(format!("`lightbox` has no option `{other}`, so it is ignored."))
                            .add_hint(
                                "The options are `match`, `effect`, `desc-position`, `loop` and `css-class`.",
                            )
                            .build(),
                    ),
                }
            }
            return config;
        }
        match value.as_plain_text().as_deref() {
            Some("auto") => config.mode = Mode::Auto,
            _ => diagnostics.push(
                DiagnosticMessageBuilder::warning("Invalid lightbox option")
                    .with_code(CODE_INVALID_OPTION)
                    .with_location(value.source_info.clone())
                    .problem("`lightbox` must be `true`, `false`, `auto` or a map of options, so it is ignored.")
                    .build(),
            ),
        }
        config
    }
}

fn invalid(
    diagnostics: &mut Vec<DiagnosticMessage>,
    value: &ConfigValue,
    key: &str,
    expected: &str,
) {
    let shown = value
        .as_plain_text()
        .map_or_else(|| "this value".to_string(), |s| format!("`{s}`"));
    diagnostics.push(
        DiagnosticMessageBuilder::warning("Invalid lightbox option")
            .with_code(CODE_INVALID_OPTION)
            .with_location(value.source_info.clone())
            .problem(format!(
                "{shown} is not a valid `lightbox.{key}`, so the default is used."
            ))
            .add_hint(format!("Use {expected}."))
            .build(),
    );
}

/// Q1's init script, with the GLightbox options in Q1's stable key order
/// (#11124). The `window.onload` chaining (#9727) and both slide hooks
/// (`data:` URIs, math typesetting) are kept verbatim.
fn init_script(config: &LightboxConfig) -> String {
    let mut options = format!(
        "{{\"selector\":\".{LIGHTBOX}\",\"closeEffect\":{effect},\"openEffect\":{effect},\"descPosition\":{desc},\"loop\":{loop_}",
        effect = json_string(&config.effect),
        desc = json_string(&config.desc_position),
        loop_ = config.loop_,
    );
    if let Some(skin) = &config.css_class {
        options.push_str(&format!(",\"skin\":{}", json_string(skin)));
    }
    options.push('}');
    format!(
        "{INIT_SENTINEL}\n<script>var lightboxQuarto = GLightbox({options});\n{INIT_BODY}</script>"
    )
}

fn json_string(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

const INIT_BODY: &str = r#"(function() {
  let previousOnload = window.onload;
  window.onload = () => {
    if (previousOnload) {
      previousOnload();
    }
    lightboxQuarto.on('slide_before_load', (data) => {
      const { slideIndex, slideNode, slideConfig, player, trigger } = data;
      const href = trigger.getAttribute('href');
      if (href !== null) {
        const imgEl = window.document.querySelector(`a[href="${href}"] img`);
        if (imgEl !== null) {
          const srcAttr = imgEl.getAttribute("src");
          if (srcAttr && srcAttr.startsWith("data:")) {
            slideConfig.href = srcAttr;
          }
        }
      }
    });

    lightboxQuarto.on('slide_after_load', (data) => {
      const { slideIndex, slideNode, slideConfig, player, trigger } = data;
      if (window.Quarto?.typesetMath) {
        window.Quarto.typesetMath(slideNode);
      }
    });

  };

})();
"#;

// ---------------------------------------------------------------------------
// Code cells

/// Pushes a cell's `lightbox` attribute down to its images.
struct CellPass<'a> {
    auto: bool,
    diagnostics: &'a mut Vec<DiagnosticMessage>,
}

impl ListVisitor for CellPass<'_> {
    fn blocks(&mut self, list: &mut Vec<Block>) {
        for block in list.iter_mut() {
            if let Block::Div(div) = block
                && div.attr.1.iter().any(|c| c == "cell")
                && let Some(raw) = div.attr.2.remove(LIGHTBOX)
            {
                self.apply(div, &raw);
            }
            block_children_mut(block, self);
        }
    }

    fn inlines(&mut self, list: &mut Inlines) {
        for inline in list.iter_mut() {
            inline_children_mut(inline, self);
        }
    }
}

impl CellPass<'_> {
    fn apply(&mut self, div: &mut Div, raw: &str) {
        let Ok(option) = serde_json::from_str::<serde_json::Value>(raw) else {
            self.diagnostics.push(
                DiagnosticMessageBuilder::warning("Invalid lightbox option")
                    .with_code(CODE_INVALID_OPTION)
                    .with_location(div.source_info.clone())
                    .problem(format!(
                        "The cell's `lightbox` option `{raw}` could not be read, so it is ignored."
                    ))
                    .add_hint("Use `true`, `false`, or a map such as `{group: my-gallery}`.")
                    .build(),
            );
            return;
        };
        let object = option.as_object();
        let opted_out = option == serde_json::Value::Bool(false)
            || object.is_some_and(|o| o.get(NO_LIGHTBOX) == Some(&serde_json::Value::Bool(true)));
        let explicit = !self.auto && (option == serde_json::Value::Bool(true) || object.is_some());

        let mut ordinal = 0usize;
        let mut overflow: Vec<&'static str> = Vec::new();
        for_each_image_mut(&mut div.content, &mut |img| {
            ordinal += 1;
            if opted_out {
                add_class(&mut img.attr.1, NO_LIGHTBOX);
                return;
            }
            if explicit {
                add_class(&mut img.attr.1, LIGHTBOX);
            }
            let Some(object) = object else {
                return;
            };
            if let Some(group) = object.get(GROUP) {
                img.attr.2.insert(GROUP.to_string(), json_to_attr(group));
            }
            for key in CELL_FORWARDED {
                let Some(value) = object.get(key) else {
                    continue;
                };
                let value = match value {
                    serde_json::Value::Array(items) if items.len() > 1 => {
                        match items.get(ordinal - 1) {
                            Some(item) => item,
                            None => {
                                if !overflow.contains(&key) {
                                    overflow.push(key);
                                }
                                continue;
                            }
                        }
                    }
                    serde_json::Value::Array(items) if items.len() == 1 => &items[0],
                    other => other,
                };
                img.attr.2.insert(key.to_string(), json_to_attr(value));
            }
        });
        for key in overflow {
            self.diagnostics.push(
                DiagnosticMessageBuilder::warning("Invalid lightbox option")
                    .with_code(CODE_INVALID_OPTION)
                    .with_location(div.source_info.clone())
                    .problem(format!(
                        "The cell has more plots than entries in its `lightbox.{key}` list, so the extra plots get none."
                    ))
                    .build(),
            );
        }
    }
}

fn json_to_attr(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn add_class(classes: &mut Vec<String>, class: &str) {
    if !classes.iter().any(|c| c == class) {
        classes.push(class.to_string());
    }
}

// ---------------------------------------------------------------------------
// The walk

/// Where the walk is: what an image found here gets.
#[derive(Clone, Default)]
struct Scope {
    /// Inside a figure's (or float's) content: automatic mode applies.
    in_figure: bool,
    /// The figure's caption as a lightbox title.
    title: Option<String>,
    /// The enclosing float's id, which a nested float's images use as
    /// their gallery (Q1's subfloat galleries).
    float_id: Option<String>,
    /// Gallery for images in this scope that name none.
    gallery: Option<String>,
}

struct Walker<'a> {
    auto: bool,
    /// Images wrapped so far; numbers galleries and descriptions.
    count: usize,
    descriptions: Vec<Block>,
    scope: Scope,
    chapter_seed: Option<ChapterSeed>,
    diagnostics: &'a mut Vec<DiagnosticMessage>,
}

impl ListVisitor for Walker<'_> {
    fn blocks(&mut self, list: &mut Vec<Block>) {
        for block in list.iter_mut() {
            self.block(block);
        }
    }

    fn inlines(&mut self, list: &mut Inlines) {
        for inline in list.iter_mut() {
            self.inline(inline, self.scope.in_figure);
        }
    }
}

impl Walker<'_> {
    fn with_scope(&mut self, scope: Scope, f: impl FnOnce(&mut Self)) {
        let saved = std::mem::replace(&mut self.scope, scope);
        f(self);
        self.scope = saved;
    }

    fn block(&mut self, block: &mut Block) {
        match block {
            // An image alone in its paragraph: a block image without a
            // caption (Q1 `discoverFigure`).
            Block::Paragraph(p) if matches!(p.content.as_slice(), [Inline::Image(_)]) => {
                self.inline(&mut p.content[0], true);
            }
            Block::Figure(fig) => {
                let title = fig
                    .caption
                    .long
                    .as_ref()
                    .map(|blocks| blocks_to_plain_text(blocks))
                    .filter(|t| !t.trim().is_empty());
                let inner = Scope {
                    in_figure: true,
                    title,
                    ..self.scope.clone()
                };
                self.with_scope(inner, |w| w.blocks(&mut fig.content));
                let caption = Scope {
                    in_figure: false,
                    title: None,
                    ..self.scope.clone()
                };
                self.with_scope(caption, |w| {
                    if let Some(short) = fig.caption.short.as_mut() {
                        w.inlines(short);
                    }
                    if let Some(long) = fig.caption.long.as_mut() {
                        w.blocks(long);
                    }
                });
            }
            Block::Custom(node) if node.type_name == FLOAT_REF_TARGET => self.float(node),
            _ => block_children_mut(block, self),
        }
    }

    fn float(&mut self, node: &mut CustomNode) {
        let title = float_title(node, self.chapter_seed.as_ref());
        let id = node.attr.0.clone();
        let gallery = self.scope.float_id.clone().or(self.scope.gallery.clone());
        for (name, slot) in node.slots.iter_mut() {
            let scope = if name == "content" {
                Scope {
                    in_figure: true,
                    title: title.clone(),
                    float_id: Some(id.clone()).filter(|id| !id.is_empty()),
                    gallery: gallery.clone(),
                }
            } else {
                Scope::default()
            };
            self.with_scope(scope, |w| match slot {
                Slot::Block(b) => w.block(b),
                Slot::Blocks(bs) => w.blocks(bs),
                Slot::Inline(i) => w.inline(i, w.scope.in_figure),
                Slot::Inlines(is) => w.inlines(is),
            });
        }
    }

    fn inline(&mut self, inline: &mut Inline, auto_eligible: bool) {
        match inline {
            // Already a link: leave it, image and all.
            Inline::Link(_) => {}
            Inline::Image(img) => {
                let explicit = img.attr.1.iter().any(|c| c == LIGHTBOX);
                let opted_out = img.attr.1.iter().any(|c| c == NO_LIGHTBOX);
                if explicit || (self.auto && auto_eligible && !opted_out) {
                    let placeholder = Inline::Str(Str {
                        text: String::new(),
                        source_info: img.source_info.clone(),
                    });
                    let Inline::Image(img) = std::mem::replace(inline, placeholder) else {
                        unreachable!("matched an Image above");
                    };
                    *inline = Inline::Link(self.wrap(img));
                }
            }
            // A footnote's body is not part of the figure (but an image
            // alone in one of its paragraphs is still a block image).
            Inline::Note(_) => {
                let scope = Scope {
                    in_figure: false,
                    title: None,
                    ..self.scope.clone()
                };
                self.with_scope(scope, |w| inline_children_mut(inline, w));
            }
            _ => inline_children_mut(inline, self),
        }
    }

    /// Q1 `lightboxImage`: wrap `img` in its lightbox link.
    fn wrap(&mut self, mut img: Image) -> Link {
        self.count += 1;
        let n = self.count;
        img.attr.1.retain(|c| c != LIGHTBOX);

        let title = img
            .attr
            .2
            .remove(TITLE)
            .or_else(|| self.scope.title.clone())
            .or_else(|| Some(inlines_to_plain_text(&img.content)))
            .filter(|t| !t.trim().is_empty());

        let mut kvs = LinkedHashMap::new();
        let gallery = img
            .attr
            .2
            .remove(GROUP)
            .or_else(|| self.scope.gallery.clone())
            .unwrap_or_else(|| format!("{GALLERY_PREFIX}{n}"));
        kvs.insert("data-gallery".to_string(), gallery);

        if let Some(description) = img.attr.2.remove(DESCRIPTION) {
            let desc_class = format!("lightbox-desc-{n}");
            let content = self.parse_description(&description, &img.source_info);
            self.descriptions.push(Block::Div(Div {
                attr: (
                    String::new(),
                    vec!["glightbox-desc".to_string(), desc_class.clone()],
                    LinkedHashMap::new(),
                ),
                content,
                source_info: img.source_info.clone(),
                attr_source: AttrSourceInfo::empty(),
            }));
            let mut config = format!("description: .{desc_class};");
            if let Some(title) = &title {
                config.push_str(&format!("title:{title};"));
            }
            kvs.insert("data-glightbox".to_string(), config);
        }

        for key in FORWARDED {
            if let Some(value) = img.attr.2.remove(key) {
                kvs.insert(format!("data-{key}"), value);
            }
        }

        let source_info = img.source_info.clone();
        Link {
            attr: (String::new(), vec![LIGHTBOX.to_string()], kvs),
            target: (img.target.0.clone(), title.unwrap_or_default()),
            content: vec![Inline::Image(img)],
            source_info,
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        }
    }

    fn parse_description(&mut self, text: &str, source_info: &SourceInfo) -> Vec<Block> {
        let kind = pampa::pandoc::meta::parse_config_string_as_markdown(
            text,
            source_info,
            self.diagnostics,
        );
        let para = |content: Inlines| {
            Block::Paragraph(Paragraph {
                content,
                source_info: source_info.clone(),
            })
        };
        match kind {
            ConfigValueKind::PandocBlocks(blocks) => blocks,
            ConfigValueKind::PandocInlines(inlines) => vec![para(inlines)],
            _ => vec![para(vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: source_info.clone(),
            })])],
        }
    }
}

/// A float's caption as a lightbox title, numbered like its figcaption:
/// "Figure 1: caption" (crossref-render's `prefix_caption`). `None` for an
/// uncaptioned float, whose images fall back to their alt text.
fn float_title(node: &CustomNode, chapter_seed: Option<&ChapterSeed>) -> Option<String> {
    let caption = match node.slots.get("caption_long") {
        Some(Slot::Blocks(blocks)) => blocks_to_plain_text(blocks),
        _ => return None,
    };
    if caption.trim().is_empty() {
        return None;
    }
    let kind = node
        .plain_data
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if kind.is_empty() {
        return Some(caption);
    }
    let number = node
        .plain_data
        .get("order")
        .and_then(|v| v.get("order"))
        .and_then(|v| v.as_u64())
        .map(|n| format_crossref_number(n as u32, chapter_seed));
    Some(match number {
        Some(n) => format!("{kind}\u{a0}{n}: {caption}"),
        None => format!("{kind}: {caption}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast_walk::for_each_inline_mut;
    use crate::format::Format;
    use crate::project::{DocumentInfo, ProjectConfig, ProjectContext};
    use crate::render::BinaryDependencies;
    use quarto_pandoc_types::block::{Figure, Plain};
    use quarto_pandoc_types::caption::Caption;

    fn project() -> ProjectContext {
        ProjectContext {
            dir: std::path::PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: std::path::PathBuf::from("/project"),
            ..Default::default()
        }
    }

    fn parse(qmd: &str) -> Pandoc {
        let (ast, _ctx, _diags) = pampa::readers::qmd::read(
            qmd.as_bytes(),
            false,
            "doc.qmd",
            &mut std::io::sink(),
            true,
            None,
        )
        .expect("parse");
        ast
    }

    struct Outcome {
        artifacts: Vec<String>,
        diagnostics: Vec<DiagnosticMessage>,
    }

    async fn run_with(ast: &mut Pandoc, profile: PipelineProfile) -> Outcome {
        let project = project();
        let doc = DocumentInfo::from_path("/project/doc.qmd");
        let binaries = BinaryDependencies::new();
        let format = Format::html();
        let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);
        ctx.pipeline_profile = profile;
        LightboxTransform::new()
            .transform(ast, &mut ctx)
            .await
            .unwrap();
        let mut artifacts: Vec<String> = ctx.artifacts.keys().map(|k| k.to_string()).collect();
        artifacts.sort();
        Outcome {
            artifacts,
            diagnostics: ctx.diagnostics,
        }
    }

    async fn run(ast: &mut Pandoc) -> Outcome {
        run_with(ast, PipelineProfile::HtmlRender).await
    }

    /// One lightbox link: (href, title, kv attributes, image src, image classes).
    #[derive(Debug)]
    struct Found {
        href: String,
        title: String,
        kvs: Vec<(String, String)>,
        src: String,
        img_classes: Vec<String>,
    }

    fn links(blocks: &mut [Block]) -> Vec<Found> {
        let mut out = Vec::new();
        for_each_inline_mut(blocks, &mut |inline| {
            if let Inline::Link(link) = inline
                && link.attr.1.iter().any(|c| c == LIGHTBOX)
                && let [Inline::Image(img)] = link.content.as_slice()
            {
                out.push(Found {
                    href: link.target.0.clone(),
                    title: link.target.1.clone(),
                    kvs: link
                        .attr
                        .2
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                    src: img.target.0.clone(),
                    img_classes: img.attr.1.clone(),
                });
            }
        });
        out
    }

    fn kv<'a>(found: &'a Found, key: &str) -> Option<&'a str> {
        found
            .kvs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn after_body(meta: &ConfigValue) -> Vec<String> {
        meta.get_path(&["rendered", "includes", "after-body"])
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    // --- options ---------------------------------------------------------

    fn config_of(qmd_front: &str) -> (LightboxConfig, Vec<DiagnosticMessage>) {
        let ast = parse(&format!("---\n{qmd_front}\n---\n\nx\n"));
        let mut diags = Vec::new();
        let config = LightboxConfig::from_meta(&ast.meta, &mut diags);
        (config, diags)
    }

    #[test]
    fn option_forms() {
        assert_eq!(config_of("title: t").0.mode, Mode::Explicit);
        assert_eq!(config_of("lightbox: true").0.mode, Mode::Auto);
        assert_eq!(config_of("lightbox: false").0.mode, Mode::Off);
        // A bare `auto` arrives as PandocInlines (bd-y89ihf0i).
        assert_eq!(config_of("lightbox: auto").0.mode, Mode::Auto);
        let (c, d) = config_of(
            "lightbox:\n  match: auto\n  effect: fade\n  desc-position: left\n  loop: true\n  css-class: cool",
        );
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(
            c,
            LightboxConfig {
                mode: Mode::Auto,
                effect: "fade".into(),
                desc_position: "left".into(),
                loop_: true,
                css_class: Some("cool".into()),
            }
        );
        // The object form without `match` stays explicit.
        assert_eq!(
            config_of("lightbox:\n  effect: none").0.mode,
            Mode::Explicit
        );
    }

    #[test]
    fn invalid_options_warn_and_fall_back() {
        let (c, d) = config_of(
            "lightbox:\n  match: always\n  effect: wobble\n  desc-position: middle\n  loop: sometimes\n  colour: red",
        );
        assert_eq!(c.mode, Mode::Explicit);
        assert_eq!(c.effect, "zoom");
        assert_eq!(c.desc_position, "bottom");
        assert!(!c.loop_);
        assert_eq!(d.len(), 5, "{d:?}");
        assert!(
            d.iter()
                .all(|m| m.code.as_deref() == Some(CODE_INVALID_OPTION))
        );

        let (c, d) = config_of("lightbox: sometimes");
        assert_eq!(c.mode, Mode::Explicit);
        assert_eq!(d.len(), 1);
    }

    #[test]
    fn init_script_keeps_q1_key_order() {
        let config = LightboxConfig {
            css_class: Some("my \"skin\"".into()),
            ..LightboxConfig::default()
        };
        let script = init_script(&config);
        assert!(script.starts_with(INIT_SENTINEL));
        assert!(script.contains(
            r#"GLightbox({"selector":".lightbox","closeEffect":"zoom","openEffect":"zoom","descPosition":"bottom","loop":false,"skin":"my \"skin\""});"#
        ));
        assert!(script.contains("let previousOnload = window.onload;"));
        assert!(script.contains("window.Quarto.typesetMath(slideNode)"));
    }

    // --- which images ----------------------------------------------------

    #[tokio::test]
    async fn auto_mode_selects_block_images_only() {
        let mut ast = parse(
            "---\nlightbox: auto\n---\n\n\
             ![Captioned](a.png)\n\n\
             ![](b.png)\n\n\
             Inline ![c](c.png) image.\n\n\
             Explicit ![d](d.png){.lightbox} inline.\n\n\
             ![Opted out](e.png){.nolightbox}\n\n\
             [![linked](f.png)](https://example.com)\n\n\
             Note.^[![in a note](g.png)]\n",
        );
        let out = run(&mut ast).await;
        let found = links(&mut ast.blocks);
        let srcs: Vec<&str> = found.iter().map(|f| f.src.as_str()).collect();
        // An image alone in a footnote's paragraph is a block image too
        // (Q1's `Para` handler visits footnote bodies as well).
        assert_eq!(srcs, ["a.png", "b.png", "d.png", "g.png"]);
        assert_eq!(found[0].title, "Captioned");
        assert_eq!(found[0].href, "a.png");
        assert_eq!(
            kv(&found[0], "data-gallery"),
            Some("quarto-lightbox-gallery-1")
        );
        assert_eq!(found[1].title, "", "no caption, no alt: no title");
        assert_eq!(found[2].title, "d", "alt text is the fallback title");
        assert_eq!(
            kv(&found[2], "data-gallery"),
            Some("quarto-lightbox-gallery-3")
        );
        assert!(
            found
                .iter()
                .all(|f| !f.img_classes.iter().any(|c| c == LIGHTBOX)),
            "the class moves to the link"
        );
        assert_eq!(out.artifacts, [CSS_KEY, JS_KEY]);
        assert_eq!(after_body(&ast.meta).len(), 1);
    }

    #[tokio::test]
    async fn explicit_mode_is_the_default() {
        let mut ast = parse("![a](a.png)\n\n![b](b.png){.lightbox}\n");
        run(&mut ast).await;
        let found = links(&mut ast.blocks);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].src, "b.png");
    }

    #[tokio::test]
    async fn off_wraps_nothing_and_ships_nothing() {
        let mut ast = parse("---\nlightbox: false\n---\n\n![b](b.png){.lightbox}\n");
        let out = run(&mut ast).await;
        assert!(links(&mut ast.blocks).is_empty());
        assert!(out.artifacts.is_empty());
        assert!(after_body(&ast.meta).is_empty());
    }

    #[tokio::test]
    async fn nothing_qualifying_ships_nothing() {
        let mut ast = parse("![a](a.png)\n");
        let out = run(&mut ast).await;
        assert!(out.artifacts.is_empty());
        assert!(after_body(&ast.meta).is_empty());
    }

    #[tokio::test]
    async fn preview_wraps_but_ships_nothing() {
        let mut ast = parse("![b](b.png){.lightbox}\n");
        let out = run_with(&mut ast, PipelineProfile::HtmlPreview).await;
        assert_eq!(links(&mut ast.blocks).len(), 1);
        assert!(out.artifacts.is_empty());
        assert!(after_body(&ast.meta).is_empty());
    }

    #[tokio::test]
    async fn other_profiles_and_minimal_are_untouched() {
        for profile in [
            PipelineProfile::RevealjsRender,
            PipelineProfile::DashboardRender,
            PipelineProfile::Pandoc("docx".into()),
        ] {
            let mut ast = parse("![b](b.png){.lightbox}\n");
            let out = run_with(&mut ast, profile).await;
            assert!(links(&mut ast.blocks).is_empty());
            assert!(out.artifacts.is_empty());
        }
        let mut ast = parse("---\nminimal: true\n---\n\n![b](b.png){.lightbox}\n");
        run(&mut ast).await;
        assert!(links(&mut ast.blocks).is_empty());
    }

    // --- per-image attributes ---------------------------------------------

    #[tokio::test]
    async fn attributes_move_to_the_link() {
        let mut ast = parse(
            "![a](a.png){.lightbox group=g desc-position=left effect=fade zoomable=false width=50% title=Override}\n",
        );
        run(&mut ast).await;
        let found = links(&mut ast.blocks);
        assert_eq!(found[0].title, "Override");
        assert_eq!(
            found[0].kvs,
            [
                ("data-gallery".to_string(), "g".to_string()),
                ("data-desc-position".to_string(), "left".to_string()),
                ("data-effect".to_string(), "fade".to_string()),
                ("data-zoomable".to_string(), "false".to_string()),
            ]
        );
        let mut img_kvs = Vec::new();
        for_each_image_mut(&mut ast.blocks, &mut |img| {
            img_kvs = img.attr.2.keys().cloned().collect();
        });
        assert_eq!(img_kvs, ["width"], "only non-lightbox attributes stay");
    }

    #[tokio::test]
    async fn descriptions_are_collected_at_the_end() {
        let mut ast = parse(
            "---\nlightbox: auto\n---\n\n\
             ![One](a.png){description=\"Some *markdown*.\"}\n\n\
             ![Two](b.png)\n\n\
             ![Three](c.png){description=\"Plain.\"}\n",
        );
        run(&mut ast).await;
        let found = links(&mut ast.blocks);
        assert_eq!(
            kv(&found[0], "data-glightbox"),
            Some("description: .lightbox-desc-1;title:One;")
        );
        assert_eq!(kv(&found[1], "data-glightbox"), None);
        assert_eq!(
            kv(&found[2], "data-glightbox"),
            Some("description: .lightbox-desc-3;title:Three;")
        );
        let Some(Block::Div(container)) = ast.blocks.last() else {
            panic!("description container missing: {:?}", ast.blocks.last());
        };
        assert_eq!(container.attr.1, ["hidden"]);
        assert_eq!(
            container.attr.2.get("aria-hidden").map(String::as_str),
            Some("true")
        );
        let classes: Vec<Vec<String>> = container
            .content
            .iter()
            .map(|b| match b {
                Block::Div(d) => d.attr.1.clone(),
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(
            classes,
            [
                vec!["glightbox-desc".to_string(), "lightbox-desc-1".to_string()],
                vec!["glightbox-desc".to_string(), "lightbox-desc-3".to_string()],
            ]
        );
        let Block::Div(first) = &container.content[0] else {
            unreachable!()
        };
        let markdown = format!("{:?}", first.content);
        assert!(
            markdown.contains("Emph"),
            "description is markdown: {markdown}"
        );
    }

    // --- floats --------------------------------------------------------

    fn image_para(src: &str, alt: &str) -> Block {
        Block::Plain(Plain {
            content: vec![Inline::Image(Image {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                content: vec![Inline::Str(Str {
                    text: alt.to_string(),
                    source_info: SourceInfo::for_test(),
                })],
                target: (src.to_string(), String::new()),
                source_info: SourceInfo::for_test(),
                attr_source: AttrSourceInfo::empty(),
                target_source: TargetSourceInfo::empty(),
            })],
            source_info: SourceInfo::for_test(),
        })
    }

    fn float(id: &str, order: u64, caption: Option<&str>, content: Vec<Block>) -> Block {
        let mut node = CustomNode::new(
            FLOAT_REF_TARGET,
            (id.to_string(), vec![], LinkedHashMap::new()),
            SourceInfo::for_test(),
        )
        .with_slot("content", Slot::Blocks(content))
        .with_data(serde_json::json!({
            "ref_type": "fig", "kind": "Figure", "identifier": id,
            "order": { "order": order }
        }));
        if let Some(text) = caption {
            node = node.with_slot(
                "caption_long",
                Slot::Blocks(vec![Block::Plain(Plain {
                    content: vec![Inline::Str(Str {
                        text: text.to_string(),
                        source_info: SourceInfo::for_test(),
                    })],
                    source_info: SourceInfo::for_test(),
                })]),
            );
        }
        Block::Custom(node)
    }

    fn auto_meta() -> ConfigValue {
        parse("---\nlightbox: true\n---\n\nx\n").meta
    }

    #[tokio::test]
    async fn float_titles_carry_the_number() {
        let mut ast = Pandoc {
            meta: auto_meta(),
            blocks: vec![
                float(
                    "fig-a",
                    1,
                    Some("Caption."),
                    vec![image_para("a.png", "alt a")],
                ),
                float("fig-b", 2, None, vec![image_para("b.png", "alt b")]),
            ],
        };
        run(&mut ast).await;
        let found = links(&mut ast.blocks);
        assert_eq!(found[0].title, "Figure\u{a0}1: Caption.");
        assert_eq!(found[1].title, "alt b", "uncaptioned: alt text");
    }

    #[tokio::test]
    async fn nested_floats_share_the_parent_gallery() {
        let mut ast = Pandoc {
            meta: auto_meta(),
            blocks: vec![float(
                "fig-parent",
                1,
                Some("Parent."),
                vec![
                    float(
                        "fig-parent-1",
                        2,
                        Some("One."),
                        vec![image_para("a.png", "a")],
                    ),
                    float(
                        "fig-parent-2",
                        3,
                        Some("Two."),
                        vec![image_para("b.png", "b")],
                    ),
                ],
            )],
        };
        run(&mut ast).await;
        let found = links(&mut ast.blocks);
        assert_eq!(found.len(), 2);
        assert!(
            found
                .iter()
                .all(|f| kv(f, "data-gallery") == Some("fig-parent")),
            "{found:?}"
        );
    }

    #[tokio::test]
    async fn figure_caption_images_need_the_class() {
        let caption_img = image_para("cap.png", "in caption");
        let mut ast = Pandoc {
            meta: auto_meta(),
            blocks: vec![Block::Figure(Figure {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                caption: Caption {
                    short: None,
                    long: Some(vec![caption_img]),
                    source_info: SourceInfo::for_test(),
                },
                content: vec![image_para("a.png", "a")],
                source_info: SourceInfo::for_test(),
                attr_source: AttrSourceInfo::empty(),
            })],
        };
        run(&mut ast).await;
        let found = links(&mut ast.blocks);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].src, "a.png");
        assert_eq!(found[0].title, "in caption");
    }

    // --- code cells -----------------------------------------------------

    #[tokio::test]
    async fn cell_options_reach_the_images() {
        let mut ast = parse(
            "::: {.cell lightbox='{\"group\":\"g\",\"description\":[\"one\",\"two\"]}'}\n\
             Text ![p1](a.png) and ![p2](b.png) and ![p3](c.png).\n\
             :::\n\n\
             ::: {.cell lightbox='{\"nolightbox\":true}'}\n\
             ![q](d.png){.lightbox}\n\
             :::\n",
        );
        let out = run(&mut ast).await;
        let found = links(&mut ast.blocks);
        let srcs: Vec<&str> = found.iter().map(|f| f.src.as_str()).collect();
        // Explicit mode: the object form marks every image `.lightbox`,
        // inline ones included (Q1); `nolightbox` adds the opt-out class,
        // which an explicit `.lightbox` still overrides.
        assert_eq!(srcs, ["a.png", "b.png", "c.png", "d.png"]);
        assert!(
            found[..3]
                .iter()
                .all(|f| kv(f, "data-gallery") == Some("g"))
        );
        assert!(
            kv(&found[0], "data-glightbox")
                .unwrap()
                .contains("lightbox-desc-1")
        );
        assert!(
            kv(&found[1], "data-glightbox")
                .unwrap()
                .contains("lightbox-desc-2")
        );
        assert_eq!(kv(&found[2], "data-glightbox"), None, "list exhausted");
        assert!(found[3].img_classes.iter().any(|c| c == NO_LIGHTBOX));
        assert_eq!(out.diagnostics.len(), 1, "{:?}", out.diagnostics);
        assert_eq!(
            out.diagnostics[0].code.as_deref(),
            Some(CODE_INVALID_OPTION)
        );
        // The attribute itself is consumed.
        let mut leftover = false;
        crate::ast_walk::for_each_block_list_mut(&mut ast.blocks, &mut |list| {
            for b in list.iter() {
                if let Block::Div(d) = b
                    && d.attr.2.contains_key(LIGHTBOX)
                {
                    leftover = true;
                }
            }
        });
        assert!(!leftover);
    }

    #[tokio::test]
    async fn cell_false_opts_out_of_auto_mode() {
        let mut ast = parse(
            "---\nlightbox: auto\n---\n\n\
             ::: {.cell lightbox='false'}\n![a](a.png)\n:::\n\n\
             ::: {.cell lightbox='not json'}\n![b](b.png)\n:::\n",
        );
        let out = run(&mut ast).await;
        let found = links(&mut ast.blocks);
        let srcs: Vec<&str> = found.iter().map(|f| f.src.as_str()).collect();
        assert_eq!(srcs, ["b.png"]);
        assert_eq!(out.diagnostics.len(), 1, "unreadable option warns");
    }

    // --- hephaestus follow-up -------------------------------------------

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn sync_points_the_link_at_the_rewritten_image() {
        let mut ast = parse("![p](plot.hep){.lightbox}\n");
        run(&mut ast).await;
        for_each_image_mut(&mut ast.blocks, &mut |img| {
            img.target.0 = "doc_files/figure-html/plot.svg".to_string();
        });
        sync_lightbox_hrefs(&mut ast.blocks);
        let found = links(&mut ast.blocks);
        assert_eq!(found[0].href, "doc_files/figure-html/plot.svg");
    }
}
