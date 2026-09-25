/*
 * format.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Output format types and resolution.
 */

//! Output format specification and resolution.
//!
//! Formats determine how documents are rendered. The format includes:
//! - The format identifier (html, pdf, docx, etc.)
//! - Whether to use the native Rust pipeline or Pandoc
//! - Format-specific options

use std::path::PathBuf;

use quarto_pandoc_types::ConfigValue;

use crate::extension::discover::parse_format_descriptor;

/// Format identifier enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FormatIdentifier {
    /// HTML output (native Rust pipeline)
    Html,
    /// PDF output (requires Pandoc + LaTeX)
    Pdf,
    /// Word document (requires Pandoc)
    Docx,
    /// PowerPoint presentation (requires Pandoc)
    Pptx,
    /// EPUB (requires Pandoc)
    Epub,
    /// Typst (requires typst binary)
    Typst,
    /// RevealJS slides (native Rust pipeline)
    Revealjs,
    /// GitHub-flavored Markdown
    Gfm,
    /// CommonMark
    CommonMark,
    /// OpenOffice text document (long-tail Phase 2; Q1
    /// `createWordprocessorFormat("OpenOffice", "odt")`)
    Odt,
    /// OpenDocument text — same ODF zip as Odt, `.xml` extension (Q1
    /// `createWordprocessorFormat("OpenDocument", "xml")`)
    Opendocument,
    /// Rich Text Format (Q1 `rtfFormat()`: wordprocessor + standalone)
    Rtf,
    /// FictionBook 2 ebook (Q1 `createEbookFormat("FictionBook", "fb2")`)
    Fb2,
    /// Plain text (Q1 `plaintextFormat("Text", "txt")`)
    Plain,
    /// reStructuredText
    Rst,
    /// Org mode
    Org,
    /// Muse
    Muse,
    /// Groff ms (Q1 `plaintextFormat("Groff Manuscript", "ms")`)
    Ms,
    /// Groff man page
    Man,
    /// GNU TexInfo
    Texinfo,
    /// TEI Simple
    Tei,
    /// Zim Wiki (`.zim` extension)
    Zimwiki,
    /// DokuWiki
    Dokuwiki,
    /// Haddock markup
    Haddock,
    /// Pandoc JSON (debug aid — the AST pandoc itself would consume, D6)
    Json,
    /// Pandoc native Haskell AST (debug aid, D6)
    Native,
    /// Adobe InDesign ICML
    Icml,
    /// Jira wiki markup
    Jira,
    /// MediaWiki
    Mediawiki,
    /// XWiki
    Xwiki,
    /// Textile (Q1 has the `texttile` typo, so this is a fresh baseline,
    /// not a Q1 parity port)
    Textile,
    /// DocBook (plaintext family — `.xml` extension)
    Docbook,
    /// DocBook 4
    Docbook4,
    /// DocBook 5
    Docbook5,
    /// Pandoc markdown (long-tail Phase 3, Tier B; Q1
    /// `pandocMarkdownFormat()` — plaintext base with **no** output-divs
    /// override, unlike every other markdown flavor)
    Markdown,
    /// Strict markdown (Q1 `markdownFormat("Strict Markdown")`)
    MarkdownStrict,
    /// PHP Markdown Extra (Q1 `markdownFormat("PHP Markdown Extra")`)
    MarkdownPhpExtra,
    /// GitHub-flavored markdown via pandoc's `markdown_github` writer
    /// (Q1 `markdownFormat("GitHub-Flavored Markdown")` with
    /// `pandoc: { to: "markdown_github" }`; distinct from the Phase 1
    /// `Gfm` variant, whose writer is `gfm`)
    MarkdownGithub,
    /// MultiMarkdown (Q1 `markdownFormat("MultiMarkdown")`)
    MarkdownMmd,
    /// Markua (Q1 `markdownFormat("Markua")`)
    Markua,
    /// CommonMark-X (Q1 `markdownFormat("CommonMark (Extended)")` via
    /// `pandoc: { to: "commonmark_x" }`; distinct from the Phase 1
    /// `CommonMark` variant)
    CommonmarkX,
    /// Djot markup (long-tail Phase 4, Tier C; pandoc `Format.hs`
    /// extension `dj`, not Q1's blanket `txt`)
    Djot,
    /// txt2tags
    T2t,
    /// Pandoc XML (native-AST XML serialization)
    Xml,
    /// ANSI terminal escape output (kept per Gordon, 2026-09-24; ext
    /// `txt` — no file convention)
    Ansi,
    /// Vim help files (`doc/*.txt` by hard convention, ext `txt`)
    Vimdoc,
    /// BBCode (forum markup; Q1 supports none of the six bbcode
    /// flavors — all fall to `unknownFormat` — so zero parity risk, D3)
    Bbcode,
    /// BBCode, Steam flavor
    BbcodeSteam,
    /// BBCode, phpBB flavor
    BbcodePhpbb,
    /// BBCode, FluxBB flavor
    BbcodeFluxbb,
    /// BBCode, Hubzilla flavor
    BbcodeHubzilla,
    /// BBCode, XenForo flavor
    BbcodeXenforo,
    /// Pandoc chunked HTML — the writer emits a **zip archive** of
    /// chapter files plus `index.html` (embeds images; a missing image
    /// is a hard exit 99). Vendored Lua treats it as non-HTML, so
    /// FloatRefTargets degrade to placeholders (Q1 parity).
    Chunkedhtml,
    /// S5 slide decks (long-tail Phase 5, Tier D; Q1
    /// `createHtmlPresentationFormat` — standalone deck, bundled
    /// `s5/default/` assets)
    S5,
    /// DZSlides slide decks (self-contained inline shim, no external
    /// assets)
    Dzslides,
    /// Slidy slide decks (W3C CDN assets — requires network at view
    /// time, pandoc's stock template behavior)
    Slidy,
    /// Slideous slide decks (bundled `slideous/` assets)
    Slideous,
}

impl FormatIdentifier {
    /// Get the format name as a string
    pub fn as_str(&self) -> &'static str {
        match self {
            FormatIdentifier::Html => "html",
            FormatIdentifier::Pdf => "pdf",
            FormatIdentifier::Docx => "docx",
            FormatIdentifier::Pptx => "pptx",
            FormatIdentifier::Epub => "epub",
            FormatIdentifier::Typst => "typst",
            FormatIdentifier::Revealjs => "revealjs",
            FormatIdentifier::Gfm => "gfm",
            FormatIdentifier::CommonMark => "commonmark",
            FormatIdentifier::Odt => "odt",
            FormatIdentifier::Opendocument => "opendocument",
            FormatIdentifier::Rtf => "rtf",
            FormatIdentifier::Fb2 => "fb2",
            FormatIdentifier::Plain => "plain",
            FormatIdentifier::Rst => "rst",
            FormatIdentifier::Org => "org",
            FormatIdentifier::Muse => "muse",
            FormatIdentifier::Ms => "ms",
            FormatIdentifier::Man => "man",
            FormatIdentifier::Texinfo => "texinfo",
            FormatIdentifier::Tei => "tei",
            FormatIdentifier::Zimwiki => "zimwiki",
            FormatIdentifier::Dokuwiki => "dokuwiki",
            FormatIdentifier::Haddock => "haddock",
            FormatIdentifier::Json => "json",
            FormatIdentifier::Native => "native",
            FormatIdentifier::Icml => "icml",
            FormatIdentifier::Jira => "jira",
            FormatIdentifier::Mediawiki => "mediawiki",
            FormatIdentifier::Xwiki => "xwiki",
            FormatIdentifier::Textile => "textile",
            FormatIdentifier::Docbook => "docbook",
            FormatIdentifier::Docbook4 => "docbook4",
            FormatIdentifier::Docbook5 => "docbook5",
            // Long-tail Phase 3 (Tier B) — Q1 `isMarkdownOutput`'s nine
            // flavors; canonical names are pandoc's `-t` writer names.
            FormatIdentifier::Markdown => "markdown",
            FormatIdentifier::MarkdownStrict => "markdown_strict",
            FormatIdentifier::MarkdownPhpExtra => "markdown_phpextra",
            FormatIdentifier::MarkdownGithub => "markdown_github",
            FormatIdentifier::MarkdownMmd => "markdown_mmd",
            FormatIdentifier::Markua => "markua",
            FormatIdentifier::CommonmarkX => "commonmark_x",
            // Long-tail Phase 4 (Tier C) — canonical names are pandoc's
            // `-t` writer names.
            FormatIdentifier::Djot => "djot",
            FormatIdentifier::T2t => "t2t",
            FormatIdentifier::Xml => "xml",
            FormatIdentifier::Ansi => "ansi",
            FormatIdentifier::Vimdoc => "vimdoc",
            FormatIdentifier::Bbcode => "bbcode",
            FormatIdentifier::BbcodeSteam => "bbcode_steam",
            FormatIdentifier::BbcodePhpbb => "bbcode_phpbb",
            FormatIdentifier::BbcodeFluxbb => "bbcode_fluxbb",
            FormatIdentifier::BbcodeHubzilla => "bbcode_hubzilla",
            FormatIdentifier::BbcodeXenforo => "bbcode_xenforo",
            FormatIdentifier::Chunkedhtml => "chunkedhtml",
            // Long-tail Phase 5 (Tier D) — canonical names are pandoc's
            // `-t` writer names.
            FormatIdentifier::S5 => "s5",
            FormatIdentifier::Dzslides => "dzslides",
            FormatIdentifier::Slidy => "slidy",
            FormatIdentifier::Slideous => "slideous",
        }
    }

    /// Check if this format uses the native Rust pipeline
    pub fn is_native(&self) -> bool {
        matches!(self, FormatIdentifier::Html | FormatIdentifier::Revealjs)
    }

    /// Check if this format renders through the pandoc-hybrid path
    /// ([`crate::stage::stages::PandocWriteStage`] with a real `pandoc`
    /// `-t <writer>` invocation), as opposed to the native Rust pipeline.
    ///
    /// Every variant listed here **must** have an explicit arm in
    /// [`pandoc_writer_name_for`] naming the pandoc writer Q1 uses —
    /// admitting a variant through the render gate (render.rs's
    /// `is_pandoc_hybrid()` check) without a writer arm would send pandoc
    /// the output *extension* as `-t` instead, or fail to reach pandoc at
    /// all. `Pdf` is deliberately absent: the latex/beamer epic owns it.
    pub fn is_pandoc_hybrid(&self) -> bool {
        matches!(
            self,
            FormatIdentifier::Docx
                | FormatIdentifier::Pptx
                | FormatIdentifier::Epub
                | FormatIdentifier::Typst
                | FormatIdentifier::Gfm
                | FormatIdentifier::CommonMark
                // Long-tail Phase 2 (Tier A): wordprocessor, ebook, and
                // plaintext families — all plain pandoc-writer targets.
                | FormatIdentifier::Odt
                | FormatIdentifier::Opendocument
                | FormatIdentifier::Rtf
                | FormatIdentifier::Fb2
                | FormatIdentifier::Plain
                | FormatIdentifier::Rst
                | FormatIdentifier::Org
                | FormatIdentifier::Muse
                | FormatIdentifier::Ms
                | FormatIdentifier::Man
                | FormatIdentifier::Texinfo
                | FormatIdentifier::Tei
                | FormatIdentifier::Zimwiki
                | FormatIdentifier::Dokuwiki
                | FormatIdentifier::Haddock
                | FormatIdentifier::Json
                | FormatIdentifier::Native
                | FormatIdentifier::Icml
                | FormatIdentifier::Jira
                | FormatIdentifier::Mediawiki
                | FormatIdentifier::Xwiki
                | FormatIdentifier::Textile
                | FormatIdentifier::Docbook
                | FormatIdentifier::Docbook4
                | FormatIdentifier::Docbook5
                // Long-tail Phase 3 (Tier B): the markdown family — all
                // plain pandoc-writer targets like Tier A.
                | FormatIdentifier::Markdown
                | FormatIdentifier::MarkdownStrict
                | FormatIdentifier::MarkdownPhpExtra
                | FormatIdentifier::MarkdownGithub
                | FormatIdentifier::MarkdownMmd
                | FormatIdentifier::Markua
                | FormatIdentifier::CommonmarkX
                // Long-tail Phase 4 (Tier C): the stretch tail — bare
                // pandoc-writer targets (no defaults rows anywhere).
                | FormatIdentifier::Djot
                | FormatIdentifier::T2t
                | FormatIdentifier::Xml
                | FormatIdentifier::Ansi
                | FormatIdentifier::Vimdoc
                | FormatIdentifier::Bbcode
                | FormatIdentifier::BbcodeSteam
                | FormatIdentifier::BbcodePhpbb
                | FormatIdentifier::BbcodeFluxbb
                | FormatIdentifier::BbcodeHubzilla
                | FormatIdentifier::BbcodeXenforo
                | FormatIdentifier::Chunkedhtml
                // Long-tail Phase 5 (Tier D): the JS slide family —
                // standalone pandoc decks.
                | FormatIdentifier::S5
                | FormatIdentifier::Dzslides
                | FormatIdentifier::Slidy
                | FormatIdentifier::Slideous
        )
    }

    /// Whether this is one of Q1's `isMarkdownOutput` flavors
    /// (`config/format.ts:169-180`): the markdown family whose pandoc
    /// writers re-escape shortcode braces, so written output needs the
    /// shortcode-unescape postprocessor
    /// ([`crate::stage::stages::pandoc_write`], mirroring Q1's
    /// `format-markdown.ts:21`).
    pub fn is_markdown_output(&self) -> bool {
        matches!(
            self,
            FormatIdentifier::Markdown
                | FormatIdentifier::MarkdownStrict
                | FormatIdentifier::MarkdownPhpExtra
                | FormatIdentifier::MarkdownGithub
                | FormatIdentifier::MarkdownMmd
                | FormatIdentifier::Markua
                | FormatIdentifier::CommonmarkX
                | FormatIdentifier::Gfm
                | FormatIdentifier::CommonMark
        )
    }

    /// The canonical format *name*: what `format-identifier.base-format`
    /// filter params carry, what `KNOWN_BASE_FORMATS` (extension
    /// discovery) spells, and what `TryFrom<&str>` accepts. This is the
    /// contract seam that must hold for every variant — **never** the
    /// output *extension* (`output_extension`), which diverges for Typst
    /// (`"typst"` vs `.pdf`) and the markdown writers (`"gfm"`/`"commonmark"`
    /// vs `.md`). Long-tail Phase 1 wrinkle 2: the params builder used to
    /// send the extension here, so a Typst render advertised base-format
    /// `"pdf"` and downstream `format:` scoping in extensions could never
    /// match it.
    pub fn canonical_name(&self) -> &'static str {
        self.as_str()
    }

    /// Check if this is an HTML-based format
    pub fn is_html_based(&self) -> bool {
        matches!(self, FormatIdentifier::Html | FormatIdentifier::Revealjs)
    }

    /// Check if this format produces multiple output files (e.g., HTML website chapters)
    pub fn is_multi_file(&self) -> bool {
        // HTML is multi-file in project context (each chapter gets a file)
        // PDF, DOCX, EPUB are single-file
        matches!(self, FormatIdentifier::Html | FormatIdentifier::Revealjs)
    }
}

impl std::fmt::Display for FormatIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl TryFrom<&str> for FormatIdentifier {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s.to_lowercase().as_str() {
            "html" => Ok(FormatIdentifier::Html),
            "pdf" => Ok(FormatIdentifier::Pdf),
            "docx" => Ok(FormatIdentifier::Docx),
            "pptx" => Ok(FormatIdentifier::Pptx),
            "epub" => Ok(FormatIdentifier::Epub),
            "typst" => Ok(FormatIdentifier::Typst),
            "revealjs" => Ok(FormatIdentifier::Revealjs),
            "gfm" => Ok(FormatIdentifier::Gfm),
            "commonmark" => Ok(FormatIdentifier::CommonMark),
            "odt" => Ok(FormatIdentifier::Odt),
            "opendocument" => Ok(FormatIdentifier::Opendocument),
            "rtf" => Ok(FormatIdentifier::Rtf),
            "fb2" => Ok(FormatIdentifier::Fb2),
            "plain" => Ok(FormatIdentifier::Plain),
            "rst" => Ok(FormatIdentifier::Rst),
            "org" => Ok(FormatIdentifier::Org),
            "muse" => Ok(FormatIdentifier::Muse),
            "ms" => Ok(FormatIdentifier::Ms),
            "man" => Ok(FormatIdentifier::Man),
            "texinfo" => Ok(FormatIdentifier::Texinfo),
            "tei" => Ok(FormatIdentifier::Tei),
            "zimwiki" => Ok(FormatIdentifier::Zimwiki),
            "dokuwiki" => Ok(FormatIdentifier::Dokuwiki),
            "haddock" => Ok(FormatIdentifier::Haddock),
            "json" => Ok(FormatIdentifier::Json),
            "native" => Ok(FormatIdentifier::Native),
            "icml" => Ok(FormatIdentifier::Icml),
            "jira" => Ok(FormatIdentifier::Jira),
            "mediawiki" => Ok(FormatIdentifier::Mediawiki),
            "xwiki" => Ok(FormatIdentifier::Xwiki),
            "textile" => Ok(FormatIdentifier::Textile),
            "docbook" => Ok(FormatIdentifier::Docbook),
            "docbook4" => Ok(FormatIdentifier::Docbook4),
            "docbook5" => Ok(FormatIdentifier::Docbook5),
            "markdown" => Ok(FormatIdentifier::Markdown),
            "markdown_strict" => Ok(FormatIdentifier::MarkdownStrict),
            "markdown_phpextra" => Ok(FormatIdentifier::MarkdownPhpExtra),
            "markdown_github" => Ok(FormatIdentifier::MarkdownGithub),
            "markdown_mmd" => Ok(FormatIdentifier::MarkdownMmd),
            "markua" => Ok(FormatIdentifier::Markua),
            "commonmark_x" => Ok(FormatIdentifier::CommonmarkX),
            "djot" => Ok(FormatIdentifier::Djot),
            "t2t" => Ok(FormatIdentifier::T2t),
            "xml" => Ok(FormatIdentifier::Xml),
            "ansi" => Ok(FormatIdentifier::Ansi),
            "vimdoc" => Ok(FormatIdentifier::Vimdoc),
            "bbcode" => Ok(FormatIdentifier::Bbcode),
            "bbcode_steam" => Ok(FormatIdentifier::BbcodeSteam),
            "bbcode_phpbb" => Ok(FormatIdentifier::BbcodePhpbb),
            "bbcode_fluxbb" => Ok(FormatIdentifier::BbcodeFluxbb),
            "bbcode_hubzilla" => Ok(FormatIdentifier::BbcodeHubzilla),
            "bbcode_xenforo" => Ok(FormatIdentifier::BbcodeXenforo),
            "chunkedhtml" => Ok(FormatIdentifier::Chunkedhtml),
            // Long-tail Phase 5 (Tier D)
            "s5" => Ok(FormatIdentifier::S5),
            "dzslides" => Ok(FormatIdentifier::Dzslides),
            "slidy" => Ok(FormatIdentifier::Slidy),
            "slideous" => Ok(FormatIdentifier::Slideous),
            _ => Err(format!("Unknown format: {}", s)),
        }
    }
}

/// Builtin pseudo-formats that map to a known base format and an
/// optional pipeline-kind selector.
///
/// `pipeline_kind` is the structured replacement for string-literal
/// `target_format` matches in pipeline-dispatching code. Per Plan 1's
/// §"Multi-plan contract: cleanup owed to Plan 7", `q2-preview` maps
/// to `Some("preview")` so `AstTransformsStage` (and the JS-side
/// data-source switch, eventually) can dispatch on a typed selector
/// instead of grepping on `target_format == "q2-preview"`.
///
/// Temporary bridge until the extensions system provides this
/// mapping. Each entry should become an extension when that system
/// lands.
fn builtin_pseudo_format(name: &str) -> Option<(&'static str, Option<&'static str>)> {
    match name {
        // `q2-slides` is the preview pseudo-format for `format: revealjs`
        // (analogous to `q2-preview` for `html`). It uses the same AST/preview
        // pipeline_kind so `render_page_for_preview` returns AST JSON; the
        // reveal slide construction fires because `build_transform_pipeline`
        // treats `target_format == "q2-slides"` as revealjs (see
        // `is_revealjs_target`). The SPA renders the section AST with a reveal
        // shell. See claude-notes/plans/2026-06-08-revealjs-presentations.md
        // Phase 1P.
        "q2-slides" => Some(("html", Some("preview"))),
        "q2-debug" => Some(("html", None)),
        "q2-preview" => Some(("html", Some("preview"))),
        // Sandboxed-preview port (bd-jgpz4hfq): same preview pipeline as
        // q2-preview — the sandboxed renderer needs the post-pipeline AST
        // (highlight spans, chrome metadata, theme fingerprint).
        "q2-sandboxed-preview" => Some(("html", Some("preview"))),
        // `q2-html-render` is hub-client's explicit opt-out from the
        // q2-preview default (bd-kltzdhle): the document previews in the
        // full-DOM iframe renderer instead of the React AST renderer. The
        // pipeline treats it exactly like `q2-debug` — a plain HTML render
        // — so `q2 render` writes an ordinary HTML file for it; only the
        // hub-client router (`getQ2Format.ts`) reads the name. See
        // claude-notes/plans/2026-09-09-hub-client-default-q2-preview.md.
        "q2-html-render" => Some(("html", None)),
        _ => None,
    }
}

/// Whether a `target_format` string should build the reveal.js slide tree
/// (`RevealSlidesTransform` replacing the generic title-block + sectionize).
/// True for the native render format (`revealjs`) and its preview
/// pseudo-format (`q2-slides`).
pub fn is_revealjs_target(target_format: &str) -> bool {
    matches!(target_format, "revealjs" | "q2-slides")
}

/// The pipeline-composition axis: which family of transforms
/// (HTML-scaffolding, revealjs-scaffolding, or none) a render runs, and
/// whether it's the full render or the render/preview kind.
///
/// This is the single derivation point replacing two previously-independent
/// ad-hoc checks: the inline `is_revealjs` family check in
/// `build_transform_pipeline`, and the `pipeline_kind: Option<&'static str>`
/// kind check in `AstTransformsStage::run()`. `RevealjsRender` is the native
/// `revealjs` render; `RevealjsPreview` is `q2-slides` — the two must stay
/// distinct cells (not collapsed into one "reveal" variant) because their
/// surviving transform lists differ once a preview exclude-list applies.
/// `Pandoc(fmt)` carries the raw `target_format` string (e.g. `"docx"`,
/// `"pptx"`) since Pandoc has no bounded enum of destination formats here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineProfile {
    /// Native HTML render (`html`, extension-style HTML formats, `q2-debug`).
    HtmlRender,
    /// HTML preview (`q2-preview`, `q2-sandboxed-preview`).
    HtmlPreview,
    /// Native revealjs render (`revealjs`).
    RevealjsRender,
    /// Revealjs preview (`q2-slides`).
    RevealjsPreview,
    /// A Pandoc-writer output format, carrying its raw format string
    /// (e.g. `"docx"`, `"pptx"`, `"gfm"`).
    Pandoc(String),
}

impl PipelineProfile {
    /// Derive the pipeline profile from a `target_format` string.
    ///
    /// Reproduces [`is_revealjs_target`] and [`builtin_pseudo_format`]
    /// exactly, deriving family from the raw string (never from a resolved
    /// [`Format::identifier`]) — `q2-slides`'s *output writer* base is
    /// `"html"`, so deriving family from the identifier would misclassify it
    /// as `HtmlPreview`, losing its reveal-ness.
    pub fn from_format(target_format: &str) -> PipelineProfile {
        // Reveal family is resolved by the string itself, not by any
        // downstream identifier: covers "revealjs" (Render) and
        // "q2-slides" (Preview).
        if is_revealjs_target(target_format) {
            return if target_format == "q2-slides" {
                PipelineProfile::RevealjsPreview
            } else {
                PipelineProfile::RevealjsRender
            };
        }

        // Builtin pseudo-formats resolve to an HTML base with an optional
        // "preview" pipeline_kind (q2-preview, q2-debug, q2-sandboxed-preview).
        if let Some((_, pipeline_kind)) = builtin_pseudo_format(target_format) {
            return if pipeline_kind == Some("preview") {
                PipelineProfile::HtmlPreview
            } else {
                PipelineProfile::HtmlRender
            };
        }

        // Known base format, e.g. "html", "docx", "pptx", "gfm".
        if let Ok(identifier) = FormatIdentifier::try_from(target_format) {
            return match identifier {
                FormatIdentifier::Html => PipelineProfile::HtmlRender,
                FormatIdentifier::Revealjs => PipelineProfile::RevealjsRender,
                _ => PipelineProfile::Pandoc(target_format.to_string()),
            };
        }

        // Extension-style formats, e.g. "acm-html" -> base "html".
        let desc = parse_format_descriptor(target_format);
        if let Ok(identifier) = FormatIdentifier::try_from(desc.base_format.as_str()) {
            return match identifier {
                FormatIdentifier::Html => PipelineProfile::HtmlRender,
                FormatIdentifier::Revealjs => PipelineProfile::RevealjsRender,
                _ => PipelineProfile::Pandoc(desc.base_format),
            };
        }

        // Unknown format string: treat as a Pandoc writer name verbatim.
        // `Format::from_format_string` is the authority on whether the
        // string actually resolves; this function is total.
        PipelineProfile::Pandoc(target_format.to_string())
    }
}

/// The canonical Pandoc output format a Lua filter or shortcode should see as
/// its `FORMAT` global, given the pipeline's `target_format`.
///
/// The preview pipeline runs with a *pseudo-format* `target_format`
/// (`q2-preview`, `q2-slides`, …) so q2-core can branch on it for AST-vs-HTML
/// output and reveal-tree construction (see [`builtin_pseudo_format`] /
/// [`is_revealjs_target`]). But Lua extensions don't know about those
/// pseudo-formats: their `quarto.doc.is_format("html:js")` /
/// `is_format("revealjs")` checks (e.g. the built-in `video` shortcode) only
/// recognize real Pandoc formats. If we hand the pseudo-format straight to Lua,
/// those checks fail and format-gated shortcodes/filters silently degrade
/// (`{{< video >}}` collapsed to a plain link in preview — bd-5b21rbaq).
///
/// This maps each preview pseudo-format back to the real format it emulates so
/// preview Lua behaves identically to render:
/// - `q2-preview` / `q2-debug` / `q2-sandboxed-preview` / `q2-html-render` → `html`
/// - `q2-slides` → `revealjs` (so `is_format("revealjs")` is true, matching
///   what [`is_revealjs_target`] already does for pipeline decisions; note this
///   intentionally differs from [`builtin_pseudo_format`], which reports the
///   *output writer* base `html`).
///
/// Any non-pseudo `target_format` (`html`, `revealjs`, `latex`, …) passes
/// through unchanged.
pub fn lua_format_for(target_format: &str) -> &str {
    match target_format {
        "q2-preview" | "q2-debug" | "q2-sandboxed-preview" | "q2-html-render" => "html",
        "q2-slides" => "revealjs",
        other => other,
    }
}

/// Extract the chosen output format from a document's leading YAML
/// front-matter: the `format:` scalar, or the first key when `format:` is a
/// map (e.g. `format: {revealjs: {...}}`). Returns `None` when there is no
/// front matter or no `format:` key.
///
/// This is the lightweight, pre-pipeline peek used to resolve a per-document
/// format (e.g. a `format: revealjs` deck inside an otherwise-HTML project)
/// before the render context — and thus the transform pipeline / output
/// extension — is chosen. The CLI's single-input detection delegates here so
/// the two paths agree.
pub fn format_key_from_frontmatter(content: &str) -> Option<String> {
    let yaml = extract_yaml_frontmatter(content)?;
    let value: serde_yaml::Value = serde_yaml::from_str(&yaml).ok()?;
    match value.get("format")? {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Mapping(m) => m.keys().find_map(|k| k.as_str().map(str::to_string)),
        _ => None,
    }
}

/// Extract every key a document's leading YAML front-matter `format:`
/// declares, in declaration order: the scalar wrapped as a single-element
/// vec, or every key when it is a map (e.g. `format: {docx: default, html:
/// default}` → `["docx", "html"]`). Returns an empty vec when there is no
/// front matter or no `format:` key.
///
/// The counterpart to [`format_key_from_frontmatter`], which returns only
/// the single key that reduction picks; this returns the full declaration so
/// [`multi_format_diagnostics`] can name what else was skipped.
pub fn format_keys_from_frontmatter(content: &str) -> Vec<String> {
    let Some(yaml) = extract_yaml_frontmatter(content) else {
        return Vec::new();
    };
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&yaml) else {
        return Vec::new();
    };
    match value.get("format") {
        Some(serde_yaml::Value::String(s)) => vec![s.clone()],
        Some(serde_yaml::Value::Mapping(m)) => m
            .keys()
            .filter_map(|k| k.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// Warn when a document's `format:` declares more than one key but only one
/// is actually rendered — a signal `render.rs`'s blanket non-native-format
/// refusal used to provide as a side effect before P7-foundation relaxed it
/// to admit `Docx`/`Pptx` (design doc §14). Returns an empty vec when
/// `all_keys` has zero or one entries, or when every declared key besides
/// `used_key` has already been accounted for.
///
/// Modeled on [`crate::project::project_kind_diagnostics`]'s shape: a pure
/// function over already-resolved data, returning `Vec<DiagnosticMessage>`
/// for the caller to print.
pub fn multi_format_diagnostics(
    all_keys: &[String],
    used_key: &str,
) -> Vec<quarto_error_reporting::DiagnosticMessage> {
    use quarto_error_reporting::DiagnosticMessageBuilder;

    if all_keys.len() <= 1 {
        return Vec::new();
    }
    let skipped: Vec<&str> = all_keys
        .iter()
        .map(String::as_str)
        .filter(|k| *k != used_key)
        .collect();
    if skipped.is_empty() {
        return Vec::new();
    }
    vec![
        DiagnosticMessageBuilder::warning(
            "`format:` declares more than one format; only one is rendered",
        )
        .with_code("Q-20-8")
        .problem(format!(
            "rendered `{used_key}`; skipped `{}`.",
            skipped.join("`, `")
        ))
        .build(),
    ]
}

/// Return the text of the leading YAML front-matter block (between the opening
/// `---` and the closing `---` / `...` line), if present.
pub fn extract_yaml_frontmatter(content: &str) -> Option<String> {
    let s = content.strip_prefix('\u{feff}').unwrap_or(content);
    let after_open = s
        .strip_prefix("---\n")
        .or_else(|| s.strip_prefix("---\r\n"))?;
    for terminator in ["\n---", "\n..."] {
        if let Some(idx) = after_open.find(terminator) {
            return Some(after_open[..idx].to_string());
        }
    }
    None
}

/// Extract the format key from a `format:` [`ConfigValue`]: the scalar string,
/// or the first key when it is a map (`format: {revealjs: {...}}`).
pub fn format_key_from_config_value(value: &ConfigValue) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    value
        .as_map_entries()
        .and_then(|entries| entries.first().map(|e| e.key.clone()))
}

/// Resolve a document's effective output format key by prefer-merging the
/// `format:` declarations, lowest precedence to highest:
///
/// ```text
///   project config   →   document front matter   →   `--to` override
/// ```
///
/// The `--to` override is modeled as a synthesized `format: !prefer <to>`
/// layer merged on top — uniform with a document that had written that
/// instruction itself — so an explicit `--to` wins over every in-document
/// declaration, while in its absence a per-file or project-level `format:` is
/// honored (the per-file one winning over the project default). Returns
/// `default` when no layer declares a format.
///
/// This runs *before* the render pipeline is built because the format key
/// selects both the transform pipeline (reveal vs. generic) and the output
/// file extension; the full format-specific config flattening still happens
/// later in `MetadataMergeStage`.
pub fn resolve_format_key(
    project_format: Option<&str>,
    document_format: Option<&str>,
    cli_to: Option<&str>,
    default: &str,
) -> String {
    use quarto_config::MergedConfig;
    use quarto_pandoc_types::{ConfigMapEntry, MergeOp};
    use quarto_source_map::{By, SourceInfo};

    // One `{format: !prefer <key>}` layer. All layers use `Prefer` (last
    // wins); ordering project → document → `--to` encodes the precedence.
    let format_layer = |key: &str| -> ConfigValue {
        let si = SourceInfo::generated(By::unknown());
        ConfigValue::new_map(
            vec![ConfigMapEntry {
                key: "format".to_string(),
                key_source: si.clone(),
                value: ConfigValue::new_string(key, si.clone()).with_merge_op(MergeOp::Prefer),
            }],
            si,
        )
    };

    let owned: Vec<ConfigValue> = [project_format, document_format, cli_to]
        .into_iter()
        .flatten()
        .map(format_layer)
        .collect();
    if owned.is_empty() {
        return default.to_string();
    }
    let layers: Vec<&ConfigValue> = owned.iter().collect();
    MergedConfig::new(layers)
        .materialize()
        .ok()
        .as_ref()
        .and_then(|merged| merged.get("format"))
        .and_then(format_key_from_config_value)
        .unwrap_or_else(|| default.to_string())
}

/// Map a FormatIdentifier to its output file extension.
fn output_extension_for(id: FormatIdentifier) -> String {
    match id {
        FormatIdentifier::Html => "html",
        FormatIdentifier::Pdf => "pdf",
        FormatIdentifier::Docx => "docx",
        FormatIdentifier::Pptx => "pptx",
        FormatIdentifier::Epub => "epub",
        FormatIdentifier::Typst => "pdf",
        FormatIdentifier::Revealjs => "html",
        FormatIdentifier::Gfm => "md",
        FormatIdentifier::CommonMark => "md",
        // Long-tail Phase 2 (Tier A) — Q1 `createFormat` extension choices.
        // Three formats share `xml` (opendocument, docbook, docbook4/5);
        // zimwiki's extension is `zim`.
        FormatIdentifier::Odt => "odt",
        FormatIdentifier::Opendocument => "xml",
        FormatIdentifier::Rtf => "rtf",
        FormatIdentifier::Fb2 => "fb2",
        FormatIdentifier::Plain => "txt",
        FormatIdentifier::Rst => "rst",
        FormatIdentifier::Org => "org",
        FormatIdentifier::Muse => "muse",
        FormatIdentifier::Ms => "ms",
        FormatIdentifier::Man => "man",
        FormatIdentifier::Texinfo => "texinfo",
        FormatIdentifier::Tei => "tei",
        FormatIdentifier::Zimwiki => "zim",
        FormatIdentifier::Dokuwiki => "dokuwiki",
        FormatIdentifier::Haddock => "haddock",
        FormatIdentifier::Json => "json",
        FormatIdentifier::Native => "native",
        FormatIdentifier::Icml => "icml",
        FormatIdentifier::Jira => "jira",
        FormatIdentifier::Mediawiki => "mediawiki",
        FormatIdentifier::Xwiki => "xwiki",
        FormatIdentifier::Textile => "textile",
        FormatIdentifier::Docbook => "xml",
        FormatIdentifier::Docbook4 => "xml",
        FormatIdentifier::Docbook5 => "xml",
        // Long-tail Phase 3 (Tier B) — the markdown family all writes `.md`,
        // Q1 `markdownFormat`'s extension choice for every flavor.
        FormatIdentifier::Markdown => "md",
        FormatIdentifier::MarkdownStrict => "md",
        FormatIdentifier::MarkdownPhpExtra => "md",
        FormatIdentifier::MarkdownGithub => "md",
        FormatIdentifier::MarkdownMmd => "md",
        FormatIdentifier::Markua => "md",
        FormatIdentifier::CommonmarkX => "md",
        // Long-tail Phase 4 (Tier C) — pandoc `Format.hs` extension
        // conventions where one exists (`djot`→`dj`), a deliberate
        // filename improvement over Q1's blanket `txt` (documented for
        // Q1 migrants in Phase 6); `chunkedhtml` writes a zip archive.
        FormatIdentifier::Djot => "dj",
        FormatIdentifier::T2t => "t2t",
        FormatIdentifier::Xml => "xml",
        FormatIdentifier::Ansi => "txt",
        FormatIdentifier::Vimdoc => "txt",
        FormatIdentifier::Bbcode => "txt",
        FormatIdentifier::BbcodeSteam => "txt",
        FormatIdentifier::BbcodePhpbb => "txt",
        FormatIdentifier::BbcodeFluxbb => "txt",
        FormatIdentifier::BbcodeHubzilla => "txt",
        FormatIdentifier::BbcodeXenforo => "txt",
        FormatIdentifier::Chunkedhtml => "zip",
        // Long-tail Phase 5 (Tier D) — all four write HTML decks.
        FormatIdentifier::S5
        | FormatIdentifier::Dzslides
        | FormatIdentifier::Slidy
        | FormatIdentifier::Slideous => "html",
    }
    .to_string()
}

/// The pandoc **writer** name to pass as `-t` for a `PipelineProfile::Pandoc`
/// render — distinct from [`output_extension_for`], which names the file
/// extension of the *final* user-facing artifact.
///
/// The two coincide for docx/pptx/epub, which is why nothing needed this
/// distinction before. It diverges for every format whose pandoc writer
/// name is not its file extension:
///
/// - **Typst** — pandoc's typst *writer* is invoked with `-t typst`, but
///   the user-facing output is a compiled PDF (`output_extension_for`
///   correctly says `"pdf"`) — there is no direct `-t pdf` path through
///   pandoc's typst writer, and passing the final extension here would
///   skip the writer, the vendored Lua filters, and the template entirely.
///   Compiling the `.typ` pandoc produces into that PDF is
///   `TypstCompileStage`'s job (pandoc-hybrid Phase 2), not this stage's.
/// - **Gfm / CommonMark** (long-tail Phase 1 wrinkle 4) — pandoc's writers
///   are `-t gfm` / `-t commonmark`, but the output file is `.md`. Passing
///   the extension (`-t md`) would silently select pandoc's *plain
///   markdown* writer instead — wrong syntax (no GFM tables/task lists),
///   wrong thing entirely.
/// - **Tier A** (long-tail Phase 2) — every one of the 25 gets an explicit
///   arm even where writer == extension, because four of them *diverge*
///   (opendocument: writer `opendocument`, extension `xml`; zimwiki:
///   `zimwiki`/`zim`; plain: `plain`/`txt`; docbook×3: `docbook…`/`xml`)
///   and a shared `-t xml` would be no pandoc writer at all.
///
/// Every `FormatIdentifier` variant for which [`FormatIdentifier::is_pandoc_hybrid`]
/// is true must have an explicit arm here.
fn pandoc_writer_name_for(id: FormatIdentifier) -> String {
    match id {
        FormatIdentifier::Typst => "typst".to_string(),
        FormatIdentifier::Gfm => "gfm".to_string(),
        FormatIdentifier::CommonMark => "commonmark".to_string(),
        // Long-tail Phase 2 (Tier A) — explicit arms for all 25, equal to
        // the canonical name (which is also pandoc's `-t` writer name for
        // each of these writers).
        FormatIdentifier::Odt => "odt".to_string(),
        FormatIdentifier::Opendocument => "opendocument".to_string(),
        FormatIdentifier::Rtf => "rtf".to_string(),
        FormatIdentifier::Fb2 => "fb2".to_string(),
        FormatIdentifier::Plain => "plain".to_string(),
        FormatIdentifier::Rst => "rst".to_string(),
        FormatIdentifier::Org => "org".to_string(),
        FormatIdentifier::Muse => "muse".to_string(),
        FormatIdentifier::Ms => "ms".to_string(),
        FormatIdentifier::Man => "man".to_string(),
        FormatIdentifier::Texinfo => "texinfo".to_string(),
        FormatIdentifier::Tei => "tei".to_string(),
        FormatIdentifier::Zimwiki => "zimwiki".to_string(),
        FormatIdentifier::Dokuwiki => "dokuwiki".to_string(),
        FormatIdentifier::Haddock => "haddock".to_string(),
        FormatIdentifier::Json => "json".to_string(),
        FormatIdentifier::Native => "native".to_string(),
        FormatIdentifier::Icml => "icml".to_string(),
        FormatIdentifier::Jira => "jira".to_string(),
        FormatIdentifier::Mediawiki => "mediawiki".to_string(),
        FormatIdentifier::Xwiki => "xwiki".to_string(),
        FormatIdentifier::Textile => "textile".to_string(),
        FormatIdentifier::Docbook => "docbook".to_string(),
        FormatIdentifier::Docbook4 => "docbook4".to_string(),
        FormatIdentifier::Docbook5 => "docbook5".to_string(),
        // Long-tail Phase 3 (Tier B) — explicit arms for all seven new
        // flavors (writer = canonical name): the fall-through would send
        // `-t md` (their shared *extension*), which silently selects
        // pandoc's plain markdown writer for every one of them.
        FormatIdentifier::Markdown => "markdown".to_string(),
        FormatIdentifier::MarkdownStrict => "markdown_strict".to_string(),
        FormatIdentifier::MarkdownPhpExtra => "markdown_phpextra".to_string(),
        FormatIdentifier::MarkdownGithub => "markdown_github".to_string(),
        FormatIdentifier::MarkdownMmd => "markdown_mmd".to_string(),
        FormatIdentifier::Markua => "markua".to_string(),
        FormatIdentifier::CommonmarkX => "commonmark_x".to_string(),
        // Long-tail Phase 4 (Tier C) — explicit arms for all twelve
        // (writer = canonical name): the fall-through would send the
        // *extension* (`-t txt` for ansi/vimdoc/bbcode×6, `-t dj` for
        // djot, `-t zip` for chunkedhtml), none of which is a pandoc
        // writer.
        FormatIdentifier::Djot => "djot".to_string(),
        FormatIdentifier::T2t => "t2t".to_string(),
        FormatIdentifier::Xml => "xml".to_string(),
        FormatIdentifier::Ansi => "ansi".to_string(),
        FormatIdentifier::Vimdoc => "vimdoc".to_string(),
        FormatIdentifier::Bbcode => "bbcode".to_string(),
        FormatIdentifier::BbcodeSteam => "bbcode_steam".to_string(),
        FormatIdentifier::BbcodePhpbb => "bbcode_phpbb".to_string(),
        FormatIdentifier::BbcodeFluxbb => "bbcode_fluxbb".to_string(),
        FormatIdentifier::BbcodeHubzilla => "bbcode_hubzilla".to_string(),
        FormatIdentifier::BbcodeXenforo => "bbcode_xenforo".to_string(),
        FormatIdentifier::Chunkedhtml => "chunkedhtml".to_string(),
        // Long-tail Phase 5 (Tier D) — the deck writers are the format
        // names; an extension fall-through would send `-t html`, a plain
        // non-deck document.
        FormatIdentifier::S5 => "s5".to_string(),
        FormatIdentifier::Dzslides => "dzslides".to_string(),
        FormatIdentifier::Slidy => "slidy".to_string(),
        FormatIdentifier::Slideous => "slideous".to_string(),
        other => output_extension_for(other),
    }
}

/// Extra pandoc CLI flags `PandocWriteStage` should append for a
/// `PipelineProfile::Pandoc` render, beyond the shared `-f json -t
/// <writer> -o <output>` invocation every format gets.
///
/// Only typst needs any (`format-typst.ts`'s `pandoc.standalone = true`,
/// `wrap: none`, `default-image-extension: svg`) — every other
/// Pandoc-hybrid format keeps the pre-existing bare invocation unchanged.
/// `citeproc: false` from the same registration needs no entry here: Q2
/// never passes `--citeproc` to pandoc for *any* Pandoc-hybrid format
/// (citations are resolved upstream of this stage), so "false" is the
/// already-existing default, not a flag to add. The opt-in `-citations`
/// variant (Q1 auto-adds a target-format variant when the user asks for
/// pandoc's native citeproc) is deferred — there is no existing
/// pseudo-format-variant seam to model it on (see
/// `builtin_pseudo_format` above, which only maps whole-format aliases,
/// not opt-in suffixes on a base format), so it needs its own design
/// decision rather than a guess here.
fn pandoc_invocation_args_for(id: FormatIdentifier) -> Vec<String> {
    match id {
        FormatIdentifier::Typst => vec![
            "--standalone".to_string(),
            "--wrap".to_string(),
            "none".to_string(),
            "--default-image-extension".to_string(),
            "svg".to_string(),
        ],
        // Long-tail Phase 2: Q1's `plaintextFormat` sets
        // `pandoc: standalone: true` for the whole plaintext family, and
        // `rtfFormat()` adds standalone on top of its wordprocessor base.
        // Odt/opendocument need no flag (pandoc's zip writers imply
        // standalone, as shipped docx already shows) and fb2's
        // `createEbookFormat` sets none. `--default-image-extension` is
        // NOT repeated here — decision D2 gives it the single sink in
        // `format_defaults::build_forwarded_args`.
        FormatIdentifier::Rtf
        | FormatIdentifier::Plain
        | FormatIdentifier::Rst
        | FormatIdentifier::Org
        | FormatIdentifier::Muse
        | FormatIdentifier::Ms
        | FormatIdentifier::Man
        | FormatIdentifier::Texinfo
        | FormatIdentifier::Tei
        | FormatIdentifier::Zimwiki
        | FormatIdentifier::Dokuwiki
        | FormatIdentifier::Haddock
        | FormatIdentifier::Json
        | FormatIdentifier::Native
        | FormatIdentifier::Icml
        | FormatIdentifier::Jira
        | FormatIdentifier::Mediawiki
        | FormatIdentifier::Xwiki
        | FormatIdentifier::Textile
        | FormatIdentifier::Docbook
        | FormatIdentifier::Docbook4
        | FormatIdentifier::Docbook5 => vec!["--standalone".to_string()],
        // Long-tail Phase 5 (Tier D): Q1's `createHtmlPresentationFormat`
        // always renders standalone with `wrap: none` (the decks rely on
        // their own CSS/JS for layout; pandoc's soft wrapping would
        // corrupt attribute-heavy slide markup).
        FormatIdentifier::S5
        | FormatIdentifier::Dzslides
        | FormatIdentifier::Slidy
        | FormatIdentifier::Slideous => vec![
            "--standalone".to_string(),
            "--wrap".to_string(),
            "none".to_string(),
        ],
        _ => Vec::new(),
    }
}

/// A complete format specification
#[derive(Debug, Clone)]
pub struct Format {
    /// Format identifier (the base format enum, e.g., Html, Pdf)
    pub identifier: FormatIdentifier,

    /// The original format string (e.g., "q2-slides", "acm-html", "html")
    pub target_format: String,

    /// Extension name, if this is an extension format (e.g., Some("acm") for "acm-html")
    pub extension_name: Option<String>,

    /// Human-readable display name (e.g., "HTML", "q2-slides")
    pub display_name: String,

    /// Output file extension (without leading dot)
    pub output_extension: String,

    /// Whether this format uses the native Rust pipeline
    pub native_pipeline: bool,

    /// Structured pipeline selector. `Some("preview")` for the
    /// `q2-preview` pseudo-format; `None` for everything else
    /// today. Pipeline-dispatching stages (e.g.
    /// `AstTransformsStage::run()`) read this instead of
    /// string-matching on `target_format`.
    pub pipeline_kind: Option<&'static str>,
}

impl Format {
    /// Create an HTML format
    pub fn html() -> Self {
        Self {
            identifier: FormatIdentifier::Html,
            target_format: "html".to_string(),
            extension_name: None,
            display_name: "HTML".to_string(),
            output_extension: "html".to_string(),
            native_pipeline: true,
            pipeline_kind: None,
        }
    }

    /// Create a PDF format
    pub fn pdf() -> Self {
        Self {
            identifier: FormatIdentifier::Pdf,
            target_format: "pdf".to_string(),
            extension_name: None,
            display_name: "PDF".to_string(),
            output_extension: "pdf".to_string(),
            native_pipeline: false,
            pipeline_kind: None,
        }
    }

    /// Create a DOCX format
    pub fn docx() -> Self {
        Self {
            identifier: FormatIdentifier::Docx,
            target_format: "docx".to_string(),
            extension_name: None,
            display_name: "DOCX".to_string(),
            output_extension: "docx".to_string(),
            native_pipeline: false,
            pipeline_kind: None,
        }
    }

    /// The canonical Pandoc output format a Lua filter or shortcode should see
    /// as its `FORMAT` global.
    ///
    /// Lua extensions branch on real Pandoc formats
    /// (`quarto.doc.is_format("html:js")`, `is_format("revealjs")`, …), not on
    /// q2's preview pseudo-formats or extension-style format strings. This
    /// resolves a `Format` to the base format it emulates:
    /// - reveal targets (`revealjs` render **and** `q2-slides` preview) →
    ///   `"revealjs"`, so `is_format("revealjs")` fires in preview too. The
    ///   bare [`identifier`](Self::identifier) would otherwise collapse
    ///   `q2-slides` to `Html` (its *output writer* is HTML), losing
    ///   reveal-ness — the reason a user filter in a reveal preview saw
    ///   `"html"` (bd-5b21rbaq).
    /// - everything else → the identifier base (`html`, `pdf`, …), which
    ///   already canonicalizes extension formats (`acm-pdf` → `pdf`) and the
    ///   HTML preview pseudo-formats (`q2-preview`/`q2-debug`/`q2-sandboxed-preview`/`q2-html-render` → `html`).
    ///
    /// This is the [`Format`]-aware companion to the string-only
    /// [`lua_format_for`] (used where only a `target_format` string is in hand,
    /// e.g. the transform-pipeline builder); the two agree on the pseudo-format
    /// and base cases.
    pub fn lua_format(&self) -> &str {
        if is_revealjs_target(&self.target_format) {
            "revealjs"
        } else {
            self.identifier.as_str()
        }
    }

    /// Parse a format string into a Format.
    ///
    /// Accepts:
    /// - Known base formats: "html", "pdf", "docx", etc.
    /// - Extension-style formats: "acm-html", "my-journal-pdf"
    /// - Builtin pseudo-formats: "q2-slides", "q2-debug", "q2-preview"
    ///   (temporary, until extensions)
    ///
    /// Returns Err for unrecognized format strings.
    pub fn from_format_string(format_str: &str) -> Result<Self, String> {
        // 1. Try as a known base format directly
        if let Ok(identifier) = FormatIdentifier::try_from(format_str) {
            return Ok(Self {
                identifier,
                target_format: format_str.to_string(),
                extension_name: None,
                display_name: identifier.as_str().to_uppercase(),
                output_extension: output_extension_for(identifier),
                native_pipeline: identifier.is_native(),
                pipeline_kind: None,
            });
        }

        // 2. Try as extension-style: "acm-html" -> base "html", extension "acm"
        // Note: For pseudo-formats like "q2-slides", parse_format_descriptor returns
        // base_format="q2-slides" (no known suffix), so the try_from below fails
        // just like step 1. This is intentional — step 3 handles pseudo-formats.
        let desc = parse_format_descriptor(format_str);
        if let Ok(identifier) = FormatIdentifier::try_from(desc.base_format.as_str()) {
            return Ok(Self {
                identifier,
                target_format: format_str.to_string(),
                extension_name: desc.extension_name,
                display_name: format_str.to_string(),
                output_extension: output_extension_for(identifier),
                native_pipeline: identifier.is_native(),
                pipeline_kind: None,
            });
        }

        // 3. Try as a builtin pseudo-format: "q2-preview" -> base "html"
        // with `pipeline_kind = Some("preview")`. The (base, kind)
        // pair is the single source of truth; pipeline-dispatching
        // stages read `pipeline_kind` instead of string-matching.
        if let Some((base, pipeline_kind)) = builtin_pseudo_format(format_str) {
            let identifier = FormatIdentifier::try_from(base).unwrap();
            return Ok(Self {
                identifier,
                target_format: format_str.to_string(),
                extension_name: None,
                display_name: format_str.to_string(),
                output_extension: output_extension_for(identifier),
                native_pipeline: identifier.is_native(),
                pipeline_kind,
            });
        }

        // 4. Unknown format
        Err(format!("Unknown format: {}", format_str))
    }

    /// The pandoc writer name `PandocWriteStage` should pass as `-t` for a
    /// `PipelineProfile::Pandoc` render. See [`pandoc_writer_name_for`] for
    /// why this differs from [`Self::output_extension`] (typst, gfm,
    /// commonmark).
    pub fn pandoc_writer_name(&self) -> String {
        pandoc_writer_name_for(self.identifier)
    }

    /// Extra pandoc CLI flags this format needs beyond the shared
    /// invocation. See [`pandoc_invocation_args_for`] for why only typst
    /// needs any today.
    pub fn pandoc_invocation_args(&self) -> Vec<String> {
        pandoc_invocation_args_for(self.identifier)
    }

    /// Check if this format is HTML-based
    pub fn is_html(&self) -> bool {
        self.identifier.is_html_based()
    }

    /// Check if this format produces multiple files
    pub fn is_multi_file(&self) -> bool {
        self.identifier.is_multi_file()
    }

    /// Get the output file path for an input file
    pub fn output_path(&self, input: &std::path::Path) -> PathBuf {
        let mut output = input.to_path_buf();
        output.set_extension(&self.output_extension);
        output
    }
}

impl Default for Format {
    fn default() -> Self {
        Self::html()
    }
}

/// Check if minimal HTML mode should be used, based on merged metadata.
///
/// This is the `ConfigValue`-based equivalent of `Format::use_minimal_html()`.
/// It reads `minimal` and `theme` from the fully merged document metadata
/// (`doc.ast.meta`) instead of from `Format.metadata`.
///
/// Returns true when:
/// - `minimal: true` is set
/// - `theme: none` is set
/// - `theme: pandoc` is set
///
/// When `minimal: true` is set, it takes precedence regardless of theme.
pub fn is_minimal_html(meta: &ConfigValue) -> bool {
    if let Some(true) = meta.get("minimal").and_then(|v| v.as_bool()) {
        return true;
    }

    // `as_plain_text` (not `as_str`): a bare `theme: none` / `theme: pandoc`
    // front-matter string is stored as `ConfigValueKind::PandocInlines`, for
    // which `as_str` returns `None`, so minimal mode was silently not applied.
    // A theme *list* or *map* still yields `None` (correct — not "none"/"pandoc").
    // (bd-y89ihf0i)
    if let Some(theme) = meta.get("theme").and_then(|v| v.as_plain_text())
        && (theme == "none" || theme == "pandoc")
    {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // === FormatIdentifier tests ===

    #[test]
    fn test_format_identifier_from_string() {
        assert_eq!(
            FormatIdentifier::try_from("html").unwrap(),
            FormatIdentifier::Html
        );
        assert_eq!(
            FormatIdentifier::try_from("HTML").unwrap(),
            FormatIdentifier::Html
        );
        assert_eq!(
            FormatIdentifier::try_from("pdf").unwrap(),
            FormatIdentifier::Pdf
        );
        assert!(FormatIdentifier::try_from("unknown").is_err());
    }

    #[test]
    fn test_format_identifier_from_string_all_formats() {
        assert_eq!(
            FormatIdentifier::try_from("html").unwrap(),
            FormatIdentifier::Html
        );
        assert_eq!(
            FormatIdentifier::try_from("pdf").unwrap(),
            FormatIdentifier::Pdf
        );
        assert_eq!(
            FormatIdentifier::try_from("docx").unwrap(),
            FormatIdentifier::Docx
        );
        assert_eq!(
            FormatIdentifier::try_from("epub").unwrap(),
            FormatIdentifier::Epub
        );
        assert_eq!(
            FormatIdentifier::try_from("typst").unwrap(),
            FormatIdentifier::Typst
        );
        assert_eq!(
            FormatIdentifier::try_from("revealjs").unwrap(),
            FormatIdentifier::Revealjs
        );
        assert_eq!(
            FormatIdentifier::try_from("gfm").unwrap(),
            FormatIdentifier::Gfm
        );
        assert_eq!(
            FormatIdentifier::try_from("commonmark").unwrap(),
            FormatIdentifier::CommonMark
        );
    }

    #[test]
    fn test_format_identifier_from_string_case_insensitive() {
        assert_eq!(
            FormatIdentifier::try_from("DOCX").unwrap(),
            FormatIdentifier::Docx
        );
        assert_eq!(
            FormatIdentifier::try_from("Epub").unwrap(),
            FormatIdentifier::Epub
        );
        assert_eq!(
            FormatIdentifier::try_from("TYPST").unwrap(),
            FormatIdentifier::Typst
        );
        assert_eq!(
            FormatIdentifier::try_from("RevealJS").unwrap(),
            FormatIdentifier::Revealjs
        );
        assert_eq!(
            FormatIdentifier::try_from("GFM").unwrap(),
            FormatIdentifier::Gfm
        );
        assert_eq!(
            FormatIdentifier::try_from("CommonMark").unwrap(),
            FormatIdentifier::CommonMark
        );
    }

    #[test]
    fn test_format_identifier_from_string_error() {
        let result = FormatIdentifier::try_from("invalid_format");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Unknown format"));
        assert!(err.contains("invalid_format"));
    }

    #[test]
    fn test_format_identifier_as_str() {
        assert_eq!(FormatIdentifier::Html.as_str(), "html");
        assert_eq!(FormatIdentifier::Pdf.as_str(), "pdf");
        assert_eq!(FormatIdentifier::Docx.as_str(), "docx");
        assert_eq!(FormatIdentifier::Epub.as_str(), "epub");
        assert_eq!(FormatIdentifier::Typst.as_str(), "typst");
        assert_eq!(FormatIdentifier::Revealjs.as_str(), "revealjs");
        assert_eq!(FormatIdentifier::Gfm.as_str(), "gfm");
        assert_eq!(FormatIdentifier::CommonMark.as_str(), "commonmark");
    }

    #[test]
    fn test_format_identifier_properties() {
        assert!(FormatIdentifier::Html.is_native());
        assert!(!FormatIdentifier::Pdf.is_native());

        assert!(FormatIdentifier::Html.is_html_based());
        assert!(FormatIdentifier::Revealjs.is_html_based());
        assert!(!FormatIdentifier::Pdf.is_html_based());
    }

    #[test]
    fn test_format_identifier_is_native_all() {
        // Native formats
        assert!(FormatIdentifier::Html.is_native());
        assert!(FormatIdentifier::Revealjs.is_native());

        // Non-native formats
        assert!(!FormatIdentifier::Pdf.is_native());
        assert!(!FormatIdentifier::Docx.is_native());
        assert!(!FormatIdentifier::Epub.is_native());
        assert!(!FormatIdentifier::Typst.is_native());
        assert!(!FormatIdentifier::Gfm.is_native());
        assert!(!FormatIdentifier::CommonMark.is_native());
    }

    #[test]
    fn test_format_identifier_is_html_based_all() {
        // HTML-based formats
        assert!(FormatIdentifier::Html.is_html_based());
        assert!(FormatIdentifier::Revealjs.is_html_based());

        // Non-HTML formats
        assert!(!FormatIdentifier::Pdf.is_html_based());
        assert!(!FormatIdentifier::Docx.is_html_based());
        // T2.6 (P7-foundation Task 2): `Pptx` must read as non-HTML too.
        // `is_html_based()` and `is_native()` return the same value for
        // every variant that existed before P1 added `Pptx`, so a future
        // regression that routes this gate through `is_native()` instead
        // would be invisible to any test that only checks outcomes —
        // this pins the predicate's own table.
        assert!(!FormatIdentifier::Pptx.is_html_based());
        assert!(!FormatIdentifier::Epub.is_html_based());
        assert!(!FormatIdentifier::Typst.is_html_based());
        assert!(!FormatIdentifier::Gfm.is_html_based());
        assert!(!FormatIdentifier::CommonMark.is_html_based());
    }

    #[test]
    fn test_format_identifier_is_multi_file() {
        // Multi-file formats
        assert!(FormatIdentifier::Html.is_multi_file());
        assert!(FormatIdentifier::Revealjs.is_multi_file());

        // Single-file formats
        assert!(!FormatIdentifier::Pdf.is_multi_file());
        assert!(!FormatIdentifier::Docx.is_multi_file());
        assert!(!FormatIdentifier::Epub.is_multi_file());
        assert!(!FormatIdentifier::Typst.is_multi_file());
        assert!(!FormatIdentifier::Gfm.is_multi_file());
        assert!(!FormatIdentifier::CommonMark.is_multi_file());
    }

    #[test]
    fn test_format_identifier_display() {
        assert_eq!(format!("{}", FormatIdentifier::Html), "html");
        assert_eq!(format!("{}", FormatIdentifier::Pdf), "pdf");
    }

    #[test]
    fn test_format_identifier_clone_copy() {
        let original = FormatIdentifier::Html;
        let cloned = original;
        let copied = original; // Copy trait

        assert_eq!(original, cloned);
        assert_eq!(original, copied);
    }

    #[test]
    fn test_format_identifier_hash() {
        let mut set = HashSet::new();
        set.insert(FormatIdentifier::Html);
        set.insert(FormatIdentifier::Pdf);
        set.insert(FormatIdentifier::Html); // Duplicate

        assert_eq!(set.len(), 2);
        assert!(set.contains(&FormatIdentifier::Html));
        assert!(set.contains(&FormatIdentifier::Pdf));
    }

    // === Format tests ===

    #[test]
    fn test_format_html() {
        let format = Format::html();

        assert_eq!(format.identifier, FormatIdentifier::Html);
        assert_eq!(format.target_format, "html");
        assert!(format.extension_name.is_none());
        assert_eq!(format.extension_name, None);
        assert_eq!(format.display_name, "HTML");
        assert_eq!(format.output_extension, "html");
        assert!(format.native_pipeline);
    }

    #[test]
    fn test_format_pdf() {
        let format = Format::pdf();

        assert_eq!(format.identifier, FormatIdentifier::Pdf);
        assert_eq!(format.target_format, "pdf");
        assert!(format.extension_name.is_none());
        assert_eq!(format.extension_name, None);
        assert_eq!(format.display_name, "PDF");
        assert_eq!(format.output_extension, "pdf");
        assert!(!format.native_pipeline);
    }

    #[test]
    fn test_format_docx() {
        let format = Format::docx();

        assert_eq!(format.identifier, FormatIdentifier::Docx);
        assert_eq!(format.target_format, "docx");
        assert!(format.extension_name.is_none());
        assert_eq!(format.extension_name, None);
        assert_eq!(format.display_name, "DOCX");
        assert_eq!(format.output_extension, "docx");
        assert!(!format.native_pipeline);
    }

    #[test]
    fn test_format_is_html() {
        assert!(Format::html().is_html());
        assert!(!Format::pdf().is_html());
        assert!(!Format::docx().is_html());
    }

    #[test]
    fn test_format_is_multi_file() {
        assert!(Format::html().is_multi_file());
        assert!(!Format::pdf().is_multi_file());
        assert!(!Format::docx().is_multi_file());
    }

    #[test]
    fn test_format_output_path() {
        let format = Format::html();
        let input = std::path::Path::new("/path/to/document.qmd");
        let output = format.output_path(input);
        assert_eq!(output, std::path::PathBuf::from("/path/to/document.html"));
    }

    #[test]
    fn test_format_output_path_pdf() {
        let format = Format::pdf();
        let input = std::path::Path::new("/path/to/document.qmd");
        let output = format.output_path(input);
        assert_eq!(output, std::path::PathBuf::from("/path/to/document.pdf"));
    }

    #[test]
    fn test_format_output_path_docx() {
        let format = Format::docx();
        let input = std::path::Path::new("/path/to/report.qmd");
        let output = format.output_path(input);
        assert_eq!(output, std::path::PathBuf::from("/path/to/report.docx"));
    }

    #[test]
    fn test_format_output_path_no_extension() {
        let format = Format::html();
        let input = std::path::Path::new("/path/to/README");
        let output = format.output_path(input);
        assert_eq!(output, std::path::PathBuf::from("/path/to/README.html"));
    }

    #[test]
    fn test_format_default() {
        let format = Format::default();

        assert_eq!(format.identifier, FormatIdentifier::Html);
        assert_eq!(format.target_format, "html");
        assert!(format.extension_name.is_none());
        assert_eq!(format.extension_name, None);
        assert_eq!(format.display_name, "HTML");
        assert_eq!(format.output_extension, "html");
        assert!(format.native_pipeline);
    }

    #[test]
    fn test_format_clone() {
        let original = Format::html();
        let cloned = original.clone();

        assert_eq!(original.identifier, cloned.identifier);
        assert_eq!(original.target_format, cloned.target_format);
        assert_eq!(original.extension_name, cloned.extension_name);
        assert_eq!(original.display_name, cloned.display_name);
        assert_eq!(original.output_extension, cloned.output_extension);
        assert_eq!(original.native_pipeline, cloned.native_pipeline);
    }

    // === from_format_string tests ===

    #[test]
    fn test_from_format_string_known_base() {
        let f = Format::from_format_string("html").unwrap();
        assert_eq!(f.identifier, FormatIdentifier::Html);
        assert_eq!(f.target_format, "html");
        assert_eq!(f.extension_name, None);
        assert_eq!(f.display_name, "HTML");
        assert_eq!(f.output_extension, "html");
    }

    #[test]
    fn test_from_format_string_extension() {
        let f = Format::from_format_string("acm-pdf").unwrap();
        assert_eq!(f.identifier, FormatIdentifier::Pdf);
        assert_eq!(f.target_format, "acm-pdf");
        assert_eq!(f.extension_name, Some("acm".to_string()));
        assert_eq!(f.output_extension, "pdf");
    }

    #[test]
    fn test_from_format_string_pseudo_format() {
        let f = Format::from_format_string("q2-slides").unwrap();
        assert_eq!(f.identifier, FormatIdentifier::Html);
        assert_eq!(f.target_format, "q2-slides");
        assert_eq!(f.extension_name, None);
        assert_eq!(f.output_extension, "html");
        assert!(f.native_pipeline);
        // revealjs epic Phase 1P: q2-slides is now the preview
        // pseudo-format for `format: revealjs` — it uses the q2-preview
        // pipeline_kind (AST path) so the SPA renders the reveal-section
        // AST with a reveal shell. The reveal slide construction fires
        // because `is_revealjs_target("q2-slides")` is true.
        assert_eq!(f.pipeline_kind, Some("preview"));
    }

    #[test]
    fn test_from_format_string_q2_preview() {
        let f = Format::from_format_string("q2-preview").unwrap();
        // q2-preview is HTML-based; the pseudo-format mapping
        // gives it `identifier: Html` while preserving the original
        // string in `target_format`.
        assert_eq!(f.identifier, FormatIdentifier::Html);
        assert_eq!(f.target_format, "q2-preview");
        assert_eq!(f.extension_name, None);
        assert_eq!(f.output_extension, "html");
        assert!(f.native_pipeline);
        // The structured selector pipeline-dispatching code reads.
        // `AstTransformsStage::run()` will branch on this in a
        // later commit (Plan 7 cleanup retires the temporary
        // `target_format == "q2-preview"` string match).
        assert_eq!(f.pipeline_kind, Some("preview"));
    }

    #[test]
    fn test_from_format_string_q2_html_render() {
        // `q2-html-render` (bd-kltzdhle) is the hub-client opt-out from
        // the q2-preview default: the full-DOM iframe renderer. In the
        // pipeline it is indistinguishable from `q2-debug` — an HTML
        // render with no preview pipeline_kind — so `q2 render` writes a
        // normal HTML file for it.
        let f = Format::from_format_string("q2-html-render").unwrap();
        assert_eq!(f.identifier, FormatIdentifier::Html);
        assert_eq!(f.target_format, "q2-html-render");
        assert_eq!(f.extension_name, None);
        assert_eq!(f.display_name, "q2-html-render");
        assert_eq!(f.output_extension, "html");
        assert!(f.native_pipeline);
        assert_eq!(f.pipeline_kind, None);
    }

    #[test]
    fn test_from_format_string_q2_sandboxed_preview() {
        let f = Format::from_format_string("q2-sandboxed-preview").unwrap();
        assert_eq!(f.identifier, FormatIdentifier::Html);
        assert_eq!(f.target_format, "q2-sandboxed-preview");
        assert_eq!(f.extension_name, None);
        assert_eq!(f.output_extension, "html");
        assert!(f.native_pipeline);
        // Sandboxed-preview port (bd-jgpz4hfq): the sandboxed renderer
        // consumes the same post-pipeline AST as q2-preview (highlight
        // spans, chrome metadata, theme fingerprint), so it uses the
        // preview pipeline_kind rather than the raw parse-only path.
        assert_eq!(f.pipeline_kind, Some("preview"));
    }

    #[test]
    fn test_lua_format_for_maps_preview_pseudo_formats() {
        // HTML-emulating pseudo-formats resolve to `html` so
        // `is_format("html:js")` fires in preview (bd-5b21rbaq).
        assert_eq!(lua_format_for("q2-preview"), "html");
        assert_eq!(lua_format_for("q2-debug"), "html");
        assert_eq!(lua_format_for("q2-sandboxed-preview"), "html");
        assert_eq!(lua_format_for("q2-html-render"), "html");
        // The reveal preview pseudo-format resolves to `revealjs` so
        // `is_format("revealjs")` fires — distinct from
        // `builtin_pseudo_format`, which reports the output-writer base `html`.
        assert_eq!(lua_format_for("q2-slides"), "revealjs");
    }

    #[test]
    fn test_lua_format_for_passes_through_real_formats() {
        for f in [
            "html", "revealjs", "latex", "pdf", "gfm", "typst", "docx", "pptx", "beamer",
        ] {
            assert_eq!(lua_format_for(f), f, "real format {f} must pass through");
        }
    }

    #[test]
    fn test_format_lua_format_canonicalizes() {
        // Real formats: identifier base.
        assert_eq!(
            Format::from_format_string("html").unwrap().lua_format(),
            "html"
        );
        assert_eq!(
            Format::from_format_string("revealjs").unwrap().lua_format(),
            "revealjs"
        );
        // Extension formats canonicalize to their base (the bug `identifier`
        // already handled; `lua_format` must preserve it).
        assert_eq!(
            Format::from_format_string("acm-pdf").unwrap().lua_format(),
            "pdf"
        );
        // HTML preview pseudo-formats → html.
        assert_eq!(
            Format::from_format_string("q2-preview")
                .unwrap()
                .lua_format(),
            "html"
        );
        assert_eq!(
            Format::from_format_string("q2-debug").unwrap().lua_format(),
            "html"
        );
        assert_eq!(
            Format::from_format_string("q2-html-render")
                .unwrap()
                .lua_format(),
            "html"
        );
        // Reveal preview pseudo-format → revealjs (NOT its html output base) —
        // the parity fix for user filters in reveal preview.
        assert_eq!(
            Format::from_format_string("q2-slides")
                .unwrap()
                .lua_format(),
            "revealjs"
        );
    }

    /// `Format::lua_format` and the string-only `lua_format_for` must agree on
    /// the pseudo-format + base cases both handle, so the shortcode pipeline
    /// (string-only) and user-filter stage (Format-aware) don't drift.
    #[test]
    fn test_lua_format_helpers_agree_on_shared_cases() {
        for fmt in [
            "html",
            "revealjs",
            "q2-preview",
            "q2-slides",
            "q2-debug",
            "q2-html-render",
        ] {
            let f = Format::from_format_string(fmt).unwrap();
            assert_eq!(
                f.lua_format(),
                lua_format_for(&f.target_format),
                "lua_format helpers disagree for {fmt}"
            );
        }
    }

    #[test]
    fn test_from_format_string_typst_extension() {
        let f = Format::from_format_string("typst").unwrap();
        assert_eq!(f.identifier, FormatIdentifier::Typst);
        assert_eq!(f.output_extension, "pdf");
    }

    /// The pandoc writer name (`-t` argument) must stay `"typst"` even
    /// though the final user-facing `output_extension` is `"pdf"` — see
    /// [`pandoc_writer_name_for`]'s doc comment.
    #[test]
    fn test_typst_pandoc_writer_name_differs_from_output_extension() {
        let f = Format::from_format_string("typst").unwrap();
        assert_eq!(f.output_extension, "pdf");
        assert_eq!(f.pandoc_writer_name(), "typst");
    }

    /// Phase 1 (long-tail formats) wrinkle 4: gfm/commonmark need explicit
    /// writer-name arms because the CLI gate admits them from Phase 1
    /// onward — the fall-through default would send `-t md` (the output
    /// extension), invoking pandoc's generic markdown writer instead of
    /// gfm/commonmark.
    #[test]
    fn test_gfm_commonmark_writer_names_are_not_md() {
        assert_eq!(
            Format::from_format_string("gfm")
                .unwrap()
                .pandoc_writer_name(),
            "gfm"
        );
        assert_eq!(
            Format::from_format_string("commonmark")
                .unwrap()
                .pandoc_writer_name(),
            "commonmark"
        );
    }

    /// Phase 1 wrinkle 3: the CLI gate's predicate. True exactly for the
    /// formats routed through `PandocWriteStage`; every variant listed here
    /// must have an explicit `pandoc_writer_name_for` arm (see
    /// `test_gfm_commonmark_writer_names_are_not_md`). Pdf stays refused
    /// (latex/beamer epic); Html/Revealjs are native.
    #[test]
    fn test_is_pandoc_hybrid_predicate() {
        use FormatIdentifier as F;
        for id in [F::Docx, F::Pptx, F::Epub, F::Typst, F::Gfm, F::CommonMark] {
            assert!(id.is_pandoc_hybrid(), "{id} must be pandoc-hybrid");
        }
        for id in [F::Html, F::Pdf, F::Revealjs] {
            assert!(!id.is_pandoc_hybrid(), "{id} must not be pandoc-hybrid");
        }
    }

    /// Phase 1 wrinkle 2: `canonical_name()` is the format key vendored Q1
    /// Lua reads as `format-identifier.base-format` — never the output
    /// extension. Typst's extension is `pdf` but its canonical name is
    /// `typst` (the latent bug this plan fixes); gfm's is `gfm`, not `md`.
    #[test]
    fn test_canonical_name_is_not_the_extension() {
        assert_eq!(FormatIdentifier::Typst.canonical_name(), "typst");
        assert_ne!(
            FormatIdentifier::Typst.canonical_name(),
            output_extension_for(FormatIdentifier::Typst),
            "typst's canonical name must not be its output extension"
        );
        assert_eq!(FormatIdentifier::Gfm.canonical_name(), "gfm");
        assert_eq!(FormatIdentifier::Docx.canonical_name(), "docx");
    }

    /// For every Pandoc-routed format without a dedicated writer arm, the
    /// writer name and the output extension still coincide (no behavior
    /// change for docx/pptx/epub). Gfm/CommonMark now have dedicated arms
    /// (`test_gfm_commonmark_writer_names_are_not_md`) and typst diverges
    /// (`test_typst_pandoc_writer_name_differs_from_output_extension`).
    #[test]
    fn test_pandoc_writer_name_matches_output_extension_for_non_typst() {
        for fmt in ["docx", "pptx", "epub"] {
            let f = Format::from_format_string(fmt).unwrap();
            assert_eq!(
                f.pandoc_writer_name(),
                f.output_extension,
                "writer name should match output_extension for {fmt}"
            );
        }
    }

    /// pandoc-hybrid-typst Phase 1 invocation builder: typst needs
    /// `--standalone`, `--wrap none`, and `--default-image-extension svg`
    /// (`format-typst.ts`'s `pandoc.standalone = true` / `wrap: none` /
    /// `default-image-extension: svg`) — none of which any other
    /// Pandoc-hybrid format passes today.
    ///
    /// Revert hunk: emptying `pandoc_invocation_args_for`'s `Typst` arm
    /// makes this RED (the expected flags go missing).
    #[test]
    fn test_typst_invocation_args_include_standalone_wrap_and_image_extension() {
        let f = Format::from_format_string("typst").unwrap();
        let args = f.pandoc_invocation_args();
        assert!(
            args.iter().any(|a| a == "--standalone"),
            "expected --standalone, got {args:?}"
        );
        assert_eq!(
            windowed_pair(&args, "--wrap"),
            Some("none".to_string()),
            "expected --wrap none, got {args:?}"
        );
        assert_eq!(
            windowed_pair(&args, "--default-image-extension"),
            Some("svg".to_string()),
            "expected --default-image-extension svg, got {args:?}"
        );
    }

    /// `citeproc: false` in Q1's typst registration means "don't ask
    /// pandoc's own citeproc to run" — Q2 never asks pandoc for citeproc on
    /// any Pandoc-hybrid format (citations are resolved upstream), so this
    /// is a non-emission, not a flag. Confirms the invocation builder
    /// doesn't regress that default by accidentally emitting `--citeproc`.
    #[test]
    fn test_typst_invocation_args_omit_citeproc_by_default() {
        let f = Format::from_format_string("typst").unwrap();
        let args = f.pandoc_invocation_args();
        assert!(
            !args.iter().any(|a| a == "--citeproc"),
            "typst must not pass --citeproc by default, got {args:?}"
        );
    }

    /// Every other Pandoc-hybrid format gets no extra invocation args —
    /// this bullet is typst-only (no behavior change for docx/pptx/epub).
    #[test]
    fn test_non_typst_formats_get_no_extra_invocation_args() {
        for fmt in ["docx", "pptx", "epub", "gfm", "commonmark"] {
            let f = Format::from_format_string(fmt).unwrap();
            assert!(
                f.pandoc_invocation_args().is_empty(),
                "expected no extra invocation args for {fmt}, got {:?}",
                f.pandoc_invocation_args()
            );
        }
    }

    /// Test helper: returns the value immediately following `flag` in
    /// `args`, if `flag` appears. Used to assert `--wrap none`-shaped
    /// two-token pairs without depending on exact adjacent indices.
    fn windowed_pair(args: &[String], flag: &str) -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    }

    #[test]
    fn test_from_format_string_revealjs() {
        let f = Format::from_format_string("revealjs").unwrap();
        assert_eq!(f.output_extension, "html");
    }

    #[test]
    fn test_from_format_string_gfm() {
        let f = Format::from_format_string("gfm").unwrap();
        assert_eq!(f.output_extension, "md");
    }

    #[test]
    fn test_from_format_string_unknown_errors() {
        assert!(Format::from_format_string("unknown").is_err());
        assert!(Format::from_format_string("htlm").is_err());
    }

    #[test]
    fn test_from_format_string_error_message() {
        let err = Format::from_format_string("htlm").unwrap_err();
        assert!(
            err.contains("htlm"),
            "error should include the bad format name"
        );
    }

    #[test]
    fn test_from_format_string_empty_string() {
        assert!(Format::from_format_string("").is_err());
    }

    #[test]
    fn test_from_format_string_hyphen_only() {
        assert!(Format::from_format_string("-").is_err());
    }

    #[test]
    fn test_from_format_string_trailing_hyphen() {
        assert!(Format::from_format_string("html-").is_err());
    }

    #[test]
    fn test_from_format_string_leading_hyphen() {
        assert!(Format::from_format_string("-html").is_err());
    }

    // === pptx resolvability (Task 1 seam, T1.2) ===

    #[test]
    fn test_from_format_string_pptx() {
        let f = Format::from_format_string("pptx").unwrap();
        assert_eq!(f.identifier, FormatIdentifier::Pptx);
        assert_eq!(f.output_extension, "pptx");
        assert!(!f.native_pipeline);
    }

    // === PipelineProfile tests (Task 1 seam, T1.1) ===

    #[test]
    fn test_pipeline_profile_from_format() {
        assert_eq!(
            PipelineProfile::from_format("html"),
            PipelineProfile::HtmlRender
        );
        assert_eq!(
            PipelineProfile::from_format("q2-debug"),
            PipelineProfile::HtmlRender
        );
        assert_eq!(
            PipelineProfile::from_format("acm-html"),
            PipelineProfile::HtmlRender
        );
        assert_eq!(
            PipelineProfile::from_format("q2-preview"),
            PipelineProfile::HtmlPreview
        );
        assert_eq!(
            PipelineProfile::from_format("q2-sandboxed-preview"),
            PipelineProfile::HtmlPreview
        );
        assert_eq!(
            PipelineProfile::from_format("revealjs"),
            PipelineProfile::RevealjsRender
        );
        assert_eq!(
            PipelineProfile::from_format("q2-slides"),
            PipelineProfile::RevealjsPreview
        );
        assert_eq!(
            PipelineProfile::from_format("docx"),
            PipelineProfile::Pandoc("docx".to_string())
        );
        assert_eq!(
            PipelineProfile::from_format("pptx"),
            PipelineProfile::Pandoc("pptx".to_string())
        );
        assert_eq!(
            PipelineProfile::from_format("gfm"),
            PipelineProfile::Pandoc("gfm".to_string())
        );
    }

    /// Refactor-induced-vacuity guard: a four-variant `PipelineProfile` (no
    /// slot for reveal+preview) would map `q2-slides` to `RevealjsRender`,
    /// silently running the full reveal-render pipeline for the preview leg.
    /// This asserts the `RevealjsPreview` cell by name AND by distinctness
    /// from `RevealjsRender` — the only surface the two differ on.
    #[test]
    fn test_pipeline_profile_q2_slides_is_revealjs_preview_not_render() {
        assert_ne!(
            PipelineProfile::from_format("q2-slides"),
            PipelineProfile::RevealjsRender
        );
        assert_eq!(
            PipelineProfile::from_format("q2-slides"),
            PipelineProfile::RevealjsPreview
        );
    }

    // === is_minimal_html tests ===

    use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
    use quarto_source_map::SourceInfo;

    fn si() -> SourceInfo {
        SourceInfo::for_test()
    }

    fn meta_with(entries: Vec<ConfigMapEntry>) -> ConfigValue {
        ConfigValue::new_map(entries, si())
    }

    fn entry(key: &str, value: ConfigValue) -> ConfigMapEntry {
        ConfigMapEntry {
            key: key.to_string(),
            key_source: si(),
            value,
        }
    }

    #[test]
    fn test_is_minimal_html_default() {
        let meta = meta_with(vec![]);
        assert!(!is_minimal_html(&meta));
    }

    #[test]
    fn test_is_minimal_html_minimal_true() {
        let meta = meta_with(vec![entry("minimal", ConfigValue::new_bool(true, si()))]);
        assert!(is_minimal_html(&meta));
    }

    #[test]
    fn test_is_minimal_html_minimal_false() {
        let meta = meta_with(vec![entry("minimal", ConfigValue::new_bool(false, si()))]);
        assert!(!is_minimal_html(&meta));
    }

    #[test]
    fn test_is_minimal_html_theme_none() {
        let meta = meta_with(vec![entry("theme", ConfigValue::new_string("none", si()))]);
        assert!(is_minimal_html(&meta));
    }

    #[test]
    fn test_is_minimal_html_theme_pandoc() {
        let meta = meta_with(vec![entry(
            "theme",
            ConfigValue::new_string("pandoc", si()),
        )]);
        assert!(is_minimal_html(&meta));
    }

    #[test]
    fn test_is_minimal_html_theme_bootstrap() {
        let meta = meta_with(vec![entry("theme", ConfigValue::new_string("cosmo", si()))]);
        assert!(!is_minimal_html(&meta));
    }

    /// bd-y89ihf0i: a bare `theme: none` front-matter string is stored as
    /// `PandocInlines`, not `Scalar(String)`. With `as_str()` this resolved to
    /// `None`, so minimal mode was silently NOT applied; `as_plain_text` fixes
    /// it. (Surfaced by the `metadata-as-str` lint, not the original seed.)
    #[test]
    fn test_is_minimal_html_theme_none_as_inlines() {
        use quarto_pandoc_types::inline::{Inline, Str};
        let theme = ConfigValue::new_inlines(
            vec![Inline::Str(Str {
                text: "none".to_string(),
                source_info: si(),
            })],
            si(),
        );
        let meta = meta_with(vec![entry("theme", theme)]);
        assert!(is_minimal_html(&meta));
    }

    #[test]
    fn test_is_minimal_html_minimal_overrides_theme() {
        let meta = meta_with(vec![
            entry("minimal", ConfigValue::new_bool(true, si())),
            entry("theme", ConfigValue::new_string("cosmo", si())),
        ]);
        assert!(is_minimal_html(&meta));
    }

    // === multi_format_diagnostics tests (P7-foundation Task 1, T1.1-T1.3) ===

    fn keys(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    /// T1.1: a 2-key `format:` map names the used key and the skipped key in
    /// their own clauses — not a whole-message equality assertion, which
    /// would be brittle against wording edits and non-discriminating about
    /// which key landed in which clause.
    #[test]
    fn test_multi_format_names_skipped_key() {
        let diags = multi_format_diagnostics(&keys(&["docx", "html"]), "docx");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-20-8"));
        let text = diags[0].to_text(None);
        assert!(
            text.contains("docx") && !text.split("skipped").next().unwrap().contains("html"),
            "used clause must name docx, not html: {text}"
        );
        assert!(
            text.contains("html"),
            "skipped clause must name html: {text}"
        );
    }

    /// T1.2: the skipped list preserves declaration order. Pinned to a
    /// non-alphabetical declaration (`docx, pptx, html`) so a regression to a
    /// sorted/`BTreeSet` collection reddens this test instead of passing by
    /// accident (an alphabetical fixture would make sorted and
    /// declaration-order output indistinguishable).
    #[test]
    fn test_multi_format_skipped_order() {
        let diags = multi_format_diagnostics(&keys(&["docx", "pptx", "html"]), "docx");
        assert_eq!(diags.len(), 1);
        let text = diags[0].to_text(None);
        let pptx_pos = text.find("pptx").expect("pptx named");
        let html_pos = text.find("html").expect("html named");
        assert!(
            pptx_pos < html_pos,
            "skipped keys must appear in declaration order (pptx before html): {text}"
        );
    }

    /// T1.3: a single-key map, a scalar `format:`, and no declared keys at
    /// all all produce no warning.
    #[test]
    fn test_single_format_no_warning() {
        assert!(multi_format_diagnostics(&keys(&["html"]), "html").is_empty());
        assert!(multi_format_diagnostics(&[], "html").is_empty());
    }

    #[test]
    fn test_format_keys_from_frontmatter_map_preserves_order() {
        let content =
            "---\nformat:\n  docx: default\n  pptx: default\n  html: default\n---\nbody\n";
        assert_eq!(
            format_keys_from_frontmatter(content),
            vec!["docx", "pptx", "html"]
        );
    }

    #[test]
    fn test_format_keys_from_frontmatter_scalar() {
        let content = "---\nformat: html\n---\nbody\n";
        assert_eq!(format_keys_from_frontmatter(content), vec!["html"]);
    }

    #[test]
    fn test_format_keys_from_frontmatter_absent() {
        assert!(format_keys_from_frontmatter("no front matter here").is_empty());
    }

    // === long-tail Phase 2: Tier A bulk tail (25 variants) ===

    /// The 25 Tier A variants with their Q1 output extensions
    /// (`formats.ts`/`formats-shared.ts` at the pinned tag). Three formats
    /// share the `xml` extension (opendocument, docbook, docbook4/5), and
    /// zimwiki's extension (`zim`) diverges from its name.
    const TIER_A_EXTENSIONS: &[(&str, &str)] = &[
        ("odt", "odt"),
        ("opendocument", "xml"),
        ("rtf", "rtf"),
        ("fb2", "fb2"),
        ("plain", "txt"),
        ("rst", "rst"),
        ("org", "org"),
        ("muse", "muse"),
        ("ms", "ms"),
        ("man", "man"),
        ("texinfo", "texinfo"),
        ("tei", "tei"),
        ("zimwiki", "zim"),
        ("dokuwiki", "dokuwiki"),
        ("haddock", "haddock"),
        ("json", "json"),
        ("native", "native"),
        ("icml", "icml"),
        ("jira", "jira"),
        ("mediawiki", "mediawiki"),
        ("xwiki", "xwiki"),
        ("textile", "textile"),
        ("docbook", "xml"),
        ("docbook4", "xml"),
        ("docbook5", "xml"),
    ];

    /// Q1 `plaintextFormat` family — everything in Tier A except the
    /// wordprocessor trio (odt/opendocument/rtf) and the ebook format
    /// (fb2). All of these get pandoc `--standalone` (verified in
    /// `formats-shared.ts`'s `plaintextFormat`).
    const TIER_A_PLAINTEXT: &[&str] = &[
        "plain",
        "rst",
        "org",
        "muse",
        "ms",
        "man",
        "texinfo",
        "tei",
        "zimwiki",
        "dokuwiki",
        "haddock",
        "json",
        "native",
        "icml",
        "jira",
        "mediawiki",
        "xwiki",
        "textile",
        "docbook",
        "docbook4",
        "docbook5",
    ];

    /// Every Tier A name parses from its canonical name and round-trips
    /// through `as_str`/`canonical_name` — `canonical_name` is what
    /// `format-identifier.base-format` carries, so a wrong mapping here
    /// would misclassify the render for every vendored Q1 Lua format check.
    #[test]
    fn test_tier_a_try_from_and_canonical_name() {
        for (name, _) in TIER_A_EXTENSIONS {
            let id = FormatIdentifier::try_from(*name)
                .unwrap_or_else(|e| panic!("{name} must parse: {e}"));
            assert_eq!(id.as_str(), *name);
            assert_eq!(id.canonical_name(), *name);
        }
    }

    /// Case-insensitive parse spot checks, one per family, including the
    /// two with internal capitals a naive `eq_ignore_ascii_case` might get
    /// wrong (`zimwiki` has none; `mediawiki`/`docbook5` are the tricky
    /// spellings).
    #[test]
    fn test_tier_a_try_from_case_insensitive() {
        for mixed in ["ODT", "Fb2", "ZimWiki", "DocBook5", "MediaWiki", "TeXinfo"] {
            let id = FormatIdentifier::try_from(mixed)
                .unwrap_or_else(|e| panic!("{mixed} must parse case-insensitively: {e}"));
            assert_eq!(id.as_str(), mixed.to_ascii_lowercase());
        }
    }

    /// Output extensions — the inventory table's third column verbatim,
    /// including the three-way `xml` sharing and zimwiki's `zim`.
    #[test]
    fn test_tier_a_output_extensions() {
        for (name, ext) in TIER_A_EXTENSIONS {
            let f = Format::from_format_string(name)
                .unwrap_or_else(|e| panic!("{name} must parse: {e}"));
            assert_eq!(f.output_extension, *ext, "output extension for {name}");
        }
    }

    /// Wrinkle 4: every Tier A variant has an explicit writer-name arm —
    /// the writer equals the canonical name, which diverges from the
    /// extension for opendocument/zimwiki/plain/docbook (fall-through would
    /// send `-t xml`/`-t zim`/`-t txt`, none of which is a pandoc writer).
    #[test]
    fn test_tier_a_pandoc_writer_names_are_explicit_arms() {
        for (name, ext) in TIER_A_EXTENSIONS {
            let f = Format::from_format_string(name).unwrap();
            assert_eq!(
                f.pandoc_writer_name(),
                *name,
                "writer name for {name} must be its canonical name, not its extension {ext}"
            );
        }
        // The divergences, explicitly — these are the rows that fail if a
        // future edit deletes the explicit arms and restores the
        // `other => output_extension_for(other)` fall-through.
        for name in ["opendocument", "zimwiki", "plain", "docbook"] {
            let f = Format::from_format_string(name).unwrap();
            assert_ne!(
                f.pandoc_writer_name(),
                f.output_extension,
                "{name}'s writer name must diverge from its output extension"
            );
        }
    }

    /// All 25 are pandoc-hybrid (self-widening the CLI gate at
    /// `render.rs`'s single `is_pandoc_hybrid` check) and none are native.
    #[test]
    fn test_tier_a_is_pandoc_hybrid() {
        for (name, _) in TIER_A_EXTENSIONS {
            let id = FormatIdentifier::try_from(*name).unwrap();
            assert!(id.is_pandoc_hybrid(), "{name} must be pandoc-hybrid");
            assert!(!id.is_native(), "{name} must not be native");
        }
    }

    /// D2/Q1-parity invocation args: `--standalone` for the plaintext
    /// family and rtf only (`plaintextFormat`'s `pandoc: standalone: true`
    /// + `rtfFormat`'s wordprocessor-plus-standalone override).
    /// Odt/opendocument need no flag (pandoc's zip writers imply
    /// standalone, matching shipped docx behavior) and fb2's
    /// `createEbookFormat` sets none. `--default-image-extension` is
    /// deliberately absent from this matrix — its single sink is
    /// `format_defaults::build_forwarded_args` (D2).
    #[test]
    fn test_tier_a_invocation_args_standalone_matrix() {
        for name in TIER_A_PLAINTEXT.iter().copied().chain(["rtf"]) {
            let args = Format::from_format_string(name)
                .unwrap()
                .pandoc_invocation_args();
            assert_eq!(
                args,
                vec!["--standalone".to_string()],
                "invocation args for {name}"
            );
        }
        for name in ["odt", "opendocument", "fb2"] {
            let args = Format::from_format_string(name)
                .unwrap()
                .pandoc_invocation_args();
            assert!(
                args.is_empty(),
                "{name} must get no invocation args, got {args:?}"
            );
        }
    }

    // === long-tail Phase 3: Tier B markdown family (9 flavors) ===

    /// Q1 `isMarkdownOutput`'s nine flavors (`config/format.ts:169-180`),
    /// with their shared `md` output extension. `gfm`/`commonmark` are the
    /// Phase 1 variants; the other seven are the Phase 3 additions.
    const TIER_B: &[&str] = &[
        "markdown",
        "markdown_strict",
        "markdown_phpextra",
        "markdown_github",
        "markdown_mmd",
        "markua",
        "commonmark_x",
        "gfm",
        "commonmark",
    ];

    /// Every Tier B name parses from its canonical name and round-trips
    /// through `as_str`/`canonical_name` — same contract seam as Tier A
    /// (`format-identifier.base-format` carries this string).
    #[test]
    fn test_tier_b_try_from_and_canonical_name() {
        for name in TIER_B {
            let id = FormatIdentifier::try_from(*name)
                .unwrap_or_else(|e| panic!("{name} must parse: {e}"));
            assert_eq!(id.as_str(), *name);
            assert_eq!(id.canonical_name(), *name);
        }
    }

    /// Case-insensitive parse spot checks over the awkward spellings.
    #[test]
    fn test_tier_b_try_from_case_insensitive() {
        for mixed in [
            "Markdown",
            "MARKDOWN_STRICT",
            "Markdown_PhpExtra",
            "Markdown_GitHub",
            "Markdown_MMD",
            "Markua",
            "CommonMark_X",
        ] {
            let id = FormatIdentifier::try_from(mixed)
                .unwrap_or_else(|e| panic!("{mixed} must parse case-insensitively: {e}"));
            assert_eq!(id.as_str(), mixed.to_ascii_lowercase());
        }
    }

    /// All nine produce `.md` — including the seven new flavors, so the
    /// `output_extension_for` arms must spell `md` explicitly (a
    /// fall-through to the format name would give `markdown_strict` files).
    #[test]
    fn test_tier_b_output_extensions() {
        for name in TIER_B {
            let f = Format::from_format_string(name)
                .unwrap_or_else(|e| panic!("{name} must parse: {e}"));
            assert_eq!(f.output_extension, "md", "output extension for {name}");
        }
    }

    /// Writer name equals canonical name for all nine — pandoc's writer
    /// flags are exactly `-t markdown`, `-t markdown_strict`, …
    /// `-t commonmark_x`, diverging from the shared `md` extension.
    #[test]
    fn test_tier_b_pandoc_writer_names() {
        for name in TIER_B {
            let f = Format::from_format_string(name).unwrap();
            assert_eq!(
                f.pandoc_writer_name(),
                *name,
                "writer name for {name} must be its canonical name, not the md extension"
            );
            assert_ne!(
                f.pandoc_writer_name(),
                f.output_extension,
                "{name}'s writer name must diverge from its output extension"
            );
        }
    }

    /// All nine are pandoc-hybrid, none native, and all nine are
    /// markdown-output — the shortcode-unescape postprocessor's gate
    /// (Phase 3a) must cover the whole family, not just the Phase 1 pair.
    #[test]
    fn test_tier_b_is_pandoc_hybrid_and_markdown_output() {
        for name in TIER_B {
            let id = FormatIdentifier::try_from(*name).unwrap();
            assert!(id.is_pandoc_hybrid(), "{name} must be pandoc-hybrid");
            assert!(!id.is_native(), "{name} must not be native");
            assert!(
                id.is_markdown_output(),
                "{name} must be markdown-output (shortcode unescape gate)"
            );
        }
    }

    /// The shortcode-unescape gate must not swallow non-markdown formats.
    #[test]
    fn test_is_markdown_output_negatives() {
        for name in [
            "html", "revealjs", "docx", "pptx", "odt", "plain", "typst", "epub",
        ] {
            let id = FormatIdentifier::try_from(name).unwrap();
            assert!(
                !id.is_markdown_output(),
                "{name} must not be markdown-output"
            );
        }
    }

    /// Tier B gets no invocation flags: Q1's `markdownFormat`/
    /// `pandocMarkdownFormat` set no `pandoc:` overrides beyond
    /// `output-divs` (which rides the filter-params blob, not the CLI).
    #[test]
    fn test_tier_b_invocation_args_empty() {
        for name in TIER_B {
            let args = Format::from_format_string(name)
                .unwrap()
                .pandoc_invocation_args();
            assert!(
                args.is_empty(),
                "{name} must get no invocation args, got {args:?}"
            );
        }
    }

    // === long-tail Phase 4: Tier C stretch (12 variants) ===

    /// The 12 Tier C formats. Q1 gives all of them *no* format object at
    /// all (`unknownFormat("txt")`, `formats.ts:341-343`), so the
    /// Q1-parity invocation is **bare** — and `--standalone` is not a
    /// no-op here: pandoc 3.11 ships default templates for
    /// ansi/djot/t2t/bbcode/vimdoc, so standalone would wrap output in
    /// template chrome Q1 never produced (measured: `pandoc -t djot
    /// --standalone` prepends `# <title>`).
    const TIER_C: &[(&str, &str)] = &[
        ("djot", "dj"),
        ("t2t", "t2t"),
        ("xml", "xml"),
        ("ansi", "txt"),
        ("vimdoc", "txt"),
        ("bbcode", "txt"),
        ("bbcode_steam", "txt"),
        ("bbcode_phpbb", "txt"),
        ("bbcode_fluxbb", "txt"),
        ("bbcode_hubzilla", "txt"),
        ("bbcode_xenforo", "txt"),
        ("chunkedhtml", "zip"),
    ];

    /// Every Tier C name parses from its canonical name and round-trips
    /// through `as_str`/`canonical_name`.
    #[test]
    fn test_tier_c_try_from_and_canonical_name() {
        for (name, _) in TIER_C {
            let id = FormatIdentifier::try_from(*name)
                .unwrap_or_else(|e| panic!("{name} must parse: {e}"));
            assert_eq!(id.as_str(), *name);
            assert_eq!(id.canonical_name(), *name);
        }
    }

    /// Extensions follow pandoc's `Format.hs` conventions where one
    /// exists (`djot`→`dj`), not Q1's blanket `txt` (a deliberate
    /// filename improvement for Q1 migrants, documented in Phase 6);
    /// `chunkedhtml` writes a zip archive.
    #[test]
    fn test_tier_c_output_extensions() {
        for (name, ext) in TIER_C {
            let f = Format::from_format_string(name)
                .unwrap_or_else(|e| panic!("{name} must parse: {e}"));
            assert_eq!(f.output_extension, *ext, "output extension for {name}");
        }
    }

    /// Writer name equals canonical name for all twelve — an explicit
    /// arm each, because the extension fall-through would send `-t txt`
    /// (ansi/vimdoc/bbcode×6) or `-t zip`/`-t dj` (chunkedhtml/djot),
    /// none of which is a pandoc writer. (`t2t` and `xml` legitimately
    /// coincide with their extensions; the extension check pins the two
    /// where divergence is the point.)
    #[test]
    fn test_tier_c_pandoc_writer_names() {
        for (name, _ext) in TIER_C {
            let f = Format::from_format_string(name).unwrap();
            assert_eq!(
                f.pandoc_writer_name(),
                *name,
                "writer name for {name} must be its canonical name"
            );
        }
        assert_ne!(
            Format::from_format_string("djot")
                .unwrap()
                .pandoc_writer_name(),
            Format::from_format_string("djot").unwrap().output_extension
        );
        assert_ne!(
            Format::from_format_string("chunkedhtml")
                .unwrap()
                .pandoc_writer_name(),
            Format::from_format_string("chunkedhtml")
                .unwrap()
                .output_extension
        );
    }

    /// All twelve are pandoc-hybrid, none native, and none is
    /// markdown-output — Tier C writers don't re-escape shortcode
    /// braces, so the Phase 3a unescape postprocessor must stay off.
    #[test]
    fn test_tier_c_is_pandoc_hybrid_and_not_markdown_output() {
        for (name, _) in TIER_C {
            let id = FormatIdentifier::try_from(*name).unwrap();
            assert!(id.is_pandoc_hybrid(), "{name} must be pandoc-hybrid");
            assert!(!id.is_native(), "{name} must not be native");
            assert!(
                !id.is_markdown_output(),
                "{name} must not be markdown-output (no shortcode unescape)"
            );
        }
    }

    /// Bare invocation: no CLI flags at all. The `--standalone` flags
    /// every other family gets would activate pandoc's default templates
    /// for these writers and change output vs Q1.
    #[test]
    fn test_tier_c_invocation_args_empty() {
        for (name, _) in TIER_C {
            let args = Format::from_format_string(name)
                .unwrap()
                .pandoc_invocation_args();
            assert!(
                args.is_empty(),
                "{name} must get no invocation args, got {args:?}"
            );
        }
    }

    /// No defaults rows: `format_pandoc_defaults` must return the no-op
    /// default for every Tier C format (no page-width, no output-divs
    /// override, no `--default-image-extension`), and the params blob
    /// builder must not grow format-specific keys for them. This pins
    /// the "bare invocation" decision at the defaults *sink* — an
    /// accidental row added later fails here.
    #[test]
    fn test_tier_c_pandoc_defaults_noop() {
        use crate::pandoc_filters::format_defaults::{
            FormatPandocDefaults, format_pandoc_defaults,
        };
        for (name, _) in TIER_C {
            let id = FormatIdentifier::try_from(*name).unwrap();
            assert_eq!(
                format_pandoc_defaults(id),
                FormatPandocDefaults::default(),
                "{name} must get no pandoc defaults (Q1 unknownFormat parity)"
            );
        }
    }

    // === Phase 5: Tier D — JS slide formats (Q1 createHtmlPresentationFormat) ===
    //
    // Q1 gives these four `createHtmlPresentationFormat` treatment:
    // standalone HTML slide decks with fig 9.5×6.5, echo/warning false,
    // `--standalone --wrap none --default-image-extension png`. Measured
    // against real pandoc 3.11 (`pandoc -f markdown -t <fmt> --standalone
    // --wrap none`, deck fixture with `# One`/`# Two`):
    //   s5       → `class="slide section level1"`, assets `href="s5/default/…"`
    //   dzslides → `class="slide level1"`, self-contained inline shim
    //              (no external JS asset; the literal `dzslides` marker
    //              appears in the inlined template)
    //   slidy    → `class="slide titlepage"` + `slide section`,
    //              CDN `https://www.w3.org/Talks/Tools/Slidy2/.../slidy.js`
    //   slideous → `class="slide titlepage"`, `src="slideous/slideous.js"`
    const TIER_D: &[(&str, &str)] = &[
        ("s5", "html"),
        ("dzslides", "html"),
        ("slidy", "html"),
        ("slideous", "html"),
    ];

    /// Every Tier D name parses from its canonical name and round-trips
    /// through `as_str`/`canonical_name`.
    #[test]
    fn test_tier_d_try_from_and_canonical_name() {
        for (name, _) in TIER_D {
            let id = FormatIdentifier::try_from(*name)
                .unwrap_or_else(|e| panic!("{name} must parse: {e}"));
            assert_eq!(id.as_str(), *name);
            assert_eq!(id.canonical_name(), *name);
        }
    }

    /// All four write HTML decks — the same extension as html, but a
    /// distinct writer target.
    #[test]
    fn test_tier_d_output_extensions_html() {
        for (name, ext) in TIER_D {
            let f = Format::from_format_string(name)
                .unwrap_or_else(|e| panic!("{name} must parse: {e}"));
            assert_eq!(f.output_extension, *ext, "output extension for {name}");
        }
    }

    /// Writer name equals canonical name for all four — an explicit arm
    /// each, because the extension fall-through would send `-t html`,
    /// which renders a plain (non-deck) HTML document.
    #[test]
    fn test_tier_d_pandoc_writer_names() {
        for (name, _) in TIER_D {
            let f = Format::from_format_string(name).unwrap();
            assert_eq!(
                f.pandoc_writer_name(),
                *name,
                "writer name for {name} must be its canonical name"
            );
        }
    }

    /// All four are pandoc-hybrid, none native, none markdown-output.
    #[test]
    fn test_tier_d_is_pandoc_hybrid_and_not_markdown_output() {
        for (name, _) in TIER_D {
            let id = FormatIdentifier::try_from(*name).unwrap();
            assert!(id.is_pandoc_hybrid(), "{name} must be pandoc-hybrid");
            assert!(!id.is_native(), "{name} must not be native");
            assert!(
                !id.is_markdown_output(),
                "{name} must not be markdown-output (no shortcode unescape)"
            );
        }
    }

    /// Q1's `createHtmlPresentationFormat` always renders standalone with
    /// no wrapping — the exact flag set, pinned as a vector so an added
    /// or dropped flag is caught.
    #[test]
    fn test_tier_d_invocation_args_standalone_wrap_none() {
        for (name, _) in TIER_D {
            let args = Format::from_format_string(name)
                .unwrap()
                .pandoc_invocation_args();
            assert_eq!(
                args,
                vec![
                    "--standalone".to_string(),
                    "--wrap".to_string(),
                    "none".to_string()
                ],
                "invocation args for {name}"
            );
        }
    }

    /// The presentation family's `--default-image-extension png` row
    /// (Q1 `format-typst.ts` analog: `createHtmlPresentationFormat`'s
    /// fig-format) — the only `format_pandoc_defaults` row this tier
    /// gets; page-width and output-divs stay untouched.
    #[test]
    fn test_tier_d_pandoc_defaults_png() {
        use crate::pandoc_filters::format_defaults::format_pandoc_defaults;
        for (name, _) in TIER_D {
            let id = FormatIdentifier::try_from(*name).unwrap();
            let defaults = format_pandoc_defaults(id);
            assert_eq!(
                defaults.default_image_extension,
                Some("png"),
                "{name} must default images to png"
            );
            assert_eq!(defaults.page_width, None, "{name} must not set page-width");
            assert_eq!(
                defaults.output_divs, None,
                "{name} must not override output-divs"
            );
        }
    }
}
