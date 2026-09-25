# Monaco syntax highlighting for `.qmd` via tree-sitter

GitHub issue: [quarto-dev/q2#10](https://github.com/quarto-dev/q2/issues/10)

## Progress log

Braid epic: **bd-t4ezufyg**. Sub-strands: Phase 0 `bd-mawltv3x`, Phase 1
`bd-7dbvncn1`, Phase 2 `bd-fswu57n2`, Phase 3 `bd-ttny2hfr`, Phases 4–7
`bd-aau3i3qr`, Phase 8 `bd-jx0nmbf3`.

- [x] **Phase 0 — shared `highlight_captures` + `flatten_spans` resolver.**
  Added `captures.rs` (`captures_from_tree` node-exact `Query.captures()` walk +
  innermost-wins `flatten_spans` with documented later-in-stream tie-break);
  `highlight_captures` public fn; switched `Registry::highlight` **and**
  `UserGrammars::highlight` off `collect_spans` onto the new pair (removed
  `collect_spans` entirely). 13 lib unit tests green incl. the equal-extent
  tie-break (winner = `property`, confirmed empirically) and
  `builtin_configs_have_no_injection_or_locals`. **Exactly 4 of 15 span-encoding
  goldens changed** (`bash`, `julia`, `python`, `user_grammar_toml`) — all
  nested→flat reshapes, per-byte visible colour preserved for the pure-nesting
  three; `toml` shows the bd-98k6 `type` fix (`[0,14]`→`[0,4]`, gap bytes flip to
  `property`). `render_to_html_user_grammars` `.contains()` asserts hand-rechecked
  (unaffected — `StubProvider` is a single span). clippy clean. **Fixes bd-98k6.**
  *Fixture note:* the `user-grammar-equal-extent` wasm was renamed
  `equalextent.wasm`→`toml.wasm` (the loader resolves `tree_sitter_<stem>`; the
  binary exports `tree_sitter_toml`), README corrected.
- [x] **Phase 1 — expand `highlights.scm`.** Replaced the minimal query with
  dotted `markup.*`/`punctuation.*`/`attribute.*` captures: headings (per-level,
  whole-node + marker children), emphasis/strong/strikethrough, inline code,
  links (`pandoc_span` *with* `target`), images (`![` fused opener +
  `markup.image.*`), `attribute.specifier`, shortcodes, math, raw HTML
  (`html_element`), fence delimiter + info string, lists, block quotes. **Removed**
  the old `(pandoc_code_block) @text.literal` / `(code_fence_content) @none` so
  interiors are left to zones 2/3. Validated via `tree-sitter query` on two
  fixtures: no capture lands inside `code_fence_content` or `metadata`.
  `tree-sitter test` green (529/529, grammar unchanged). *Grammar notes:* no
  `setext_heading` node exists (atx only); block + inline raw HTML are both
  `html_element`; `metadata` is **opaque** (no child nodes for the `---` fences),
  so `punctuation.delimiter.frontmatter` is **synthesized in the extractor**
  (Phase 3), not a query capture.
- [x] **Phase 2 — `quarto-lsp-core::tokens` structural extractor.** New
  `tokens.rs`: `get_semantic_tokens(doc)`, structural zone-1 extractor (compiles
  `tree_sitter_qmd::HIGHLIGHT_QUERY` once via `OnceLock`, drops unknown captures,
  excludes captures inside `metadata`/`code_fence_content` at the source,
  flattens via the shared `quarto_highlight::flatten_spans`), `Utf16LineIndex`
  (byte→UTF-16 + per-line split of cross-line spans, trims `\n`/`\r`).
  `types.rs`: `SemanticToken`, `SemanticTokensJson`, the 46-entry
  `QMD_TOKEN_LEGEND` (22 structural + 24 `code.*` roots, all `qmd.`-prefixed),
  and `capture_to_token_type(capture, embedded)` longest-dotted-prefix
  translator. New deps: `tree-sitter`, `tree-sitter-qmd`, `quarto-highlight`. 35
  lib tests green incl. `every_query_capture_maps_to_a_legend_entry`,
  `structural_captures_never_enter_interiors`, `tokens_never_span_a_line`,
  `utf16_offsets_account_for_multibyte`, translator-disjointness, and the
  `structural_corpus` insta snapshot (verified: display-math splits into 3
  per-line tokens; attr-span content correctly *not* link.label). clippy clean.
- [x] **Phase 3 — embedded-language layer.** `tokens.rs` zones 2/3:
  `embedded_tokens` walks the CST for `metadata` + `pandoc_code_block`. Zone 2
  (`frontmatter_tokens`) synthesizes `---`/`...` fence delimiters by line (the
  `metadata` node is opaque) and highlights the YAML body via
  `highlight_captures("yaml", …)`. Zone 3 (`code_cell_tokens`) resolves the
  language from `info_string` (first word) or `attribute_specifier →
  language_specifier` (strip leading `.`), validates with `is_language_supported`,
  and highlights `code_fence_content`; all embedded captures translate with
  `embedded=true` (→ `qmd.code.*`). 41 lib tests green incl.
  `tokens_for_code_cell_r`, `tokens_for_fenced_python`,
  `tokens_for_multiline_code_string` (per-line split of a triple-quoted string),
  `tokens_for_frontmatter_yaml`, `tokens_for_unknown_code_language`, and
  `code_cell_parity_with_render` (editor decode == render spans). Corpus snapshot
  extended with frontmatter + r + python cells, verified by hand. clippy clean.
- [x] **Phases 4–7 — hub-client integration (WASM round-trip verified in Phase 8).**
  - *Phase 4:* `lsp_get_semantic_tokens(path)` + `lsp_get_token_legend()` WASM
    exports (`LspSemanticTokensResponse`); declared in
    `wasm-quarto-hub-client.d.ts`. Native build of `quarto-lsp-core` re-exports
    confirm the types; the bare `cargo check --target wasm32` fails at
    tree-sitter's C compile (needs npm's wasm-shim sysroot) — real WASM build is
    `npm run build:wasm` in Phase 8.
  - *Phase 5:* `getSemanticTokens(path)` + checked-in `QMD_TOKEN_LEGEND` mirror
    in `intelligenceService.ts`; `SemanticToken`/`LspSemanticTokensResponse`
    types in `preview-renderer/types/intelligence.ts`. Graceful → `[]`.
  - *Phase 6:* `registerDocumentSemanticTokensProvider('qmd', …)` with pure
    `encodeSemanticTokens` delta-encoder, `CancellationToken` + `getVersionId`
    staleness guard (null on cancel/stale/throw, empty `Uint32Array` on valid
    empty), `getLegend()` from the TS constant; symbol/folding re-bound
    `markdown`→`qmd`. `Editor.tsx`: `.qmd`→`'qmd'`, `registerQmdLanguage`,
    `semanticHighlighting.enabled`, `quarto-{light,dark}` theme prop.
  - *Phase 7:* `quartoTheme.ts` — `'qmd'` language registration, markdown-derived
    Monarch base (tier 1: `nextEmbedded` routing of `{r}`/`{python}`/frontmatter
    to stock tokenizers), exported `quartoThemeRules` (all `qmd.`-prefixed,
    code.\* mirroring the `hl-*` palette), `quarto-{light,dark}` themes with
    `semanticHighlighting: true`. Tier-2 palette-alignment deferred (gate on
    observed flicker at the Phase-8 check).
  - **Tests:** Rust `code_legend_covers_render_css` green (24 roots match).
    14 pure vitest tests green (`quartoTheme.test.ts` namespace invariant,
    `monacoProviders.test.ts` delta-encoder + cancel/stale/empty return shapes,
    `intelligenceService.test.ts` graceful-`[]`). `semanticTokens.wasm.test.ts`
    (real WASM round-trip + legend drift guard) written; runs in Phase 8.
    `npm run typecheck` + eslint clean (added `argsIgnorePattern: '^_'` to the
    eslint config — fixes 5 pre-existing `_model`/`_token` errors).
- [x] **Phase 8 — end-to-end verification.** `cargo xtask verify` green on every
  feature-relevant leg: Rust workspace build + tests, WASM rebuild (new
  `lsp_get_semantic_tokens`/`lsp_get_token_legend` exports), hub-client
  `build:all`, and `test:ci` (`test` 42 files, `test:integration` 8, `test:wasm`
  18 — including the new `semanticTokens.wasm.test.ts` real round-trip + legend
  drift guard and the now-passing `userGrammarParity.wasm.test.ts`), plus
  preview-renderer/preview-runtime suites. e2e `q2 render` of an `r` cell emits
  correct flat non-nested `hl-variable`/`hl-operator`/`hl-number`/`hl-function`
  spans (Phase-0 render path). **Discovered + fixed during verify:** the JS-side
  user-grammar highlighter (`ts-packages/preview-runtime/.../Highlight.ts`) had
  to gain the same innermost-wins flatten as the native path, or browser and
  native render user-grammar code blocks differently — now true parity (bd-98k6
  fixed on the browser render path too).
  - **Manual browser check — PASSED (2026-06-16).** Confirmed in a real
    hub-client + local hub server session: `.qmd` editor shows the tree-sitter
    semantic highlighting, "definitely an improvement" over the old markdown
    mapping. (Gotcha hit en route: the **browser** was serving a cached old WASM
    — a hard refresh / cache clear after `npm run build:wasm` was needed, on top
    of `npm run dev:fresh`.) Minor visual **tweaks** noted by the user as
    follow-up (palette / specific scopes), not blockers.
  - **Tweak (2026-06-16): link/image brackets uniform.** The opening `[` was
    coloured grey (`punctuation.bracket`) but the closing `]` was not (it is
    fused into the anonymous `](` token inside `target`, unreachable by the
    query), so the brackets clashed. Dropped the `[`/`)` bracket captures from
    `highlights.scm`; all link/image bracket punctuation now renders in the
    uniform default foreground. (`qmd.punctuation.bracket` stays a reserved
    legend entry / theme rule for when the grammar can split `](`.) Requires a
    WASM rebuild + browser hard-refresh.
  - **Pre-existing, unrelated failure:** `@quarto/hub-mcp`\'s `hub-mcp.test.ts`
    (19/22) fails in this sandbox — `live:` tests needing an automerge server +
    MCP child-process spawn. Untouched package; orthogonal to this work.

## Overview

Hub-client currently maps `.qmd` files to Monaco's built-in `'markdown'`
language (`hub-client/src/components/Editor.tsx:64` — `getLanguageForFile`).
That grammar is wrong for the editor in three ways:

1. **qmd / markdown structure** — link bracketed text and the URL get the
   same hue, image syntax is indistinguishable from a plain link, and
   Pandoc/Quarto constructs (attribute specifiers, shortcodes, math, raw
   HTML, crossrefs, callouts) get no treatment at all. This is the
   proximate symptom in GH#10.
2. **YAML frontmatter** — Monaco's markdown grammar does not treat the
   `---` fenced block as YAML; it reads the fences as a horizontal rule
   and leaves the body un-highlighted.
3. **Embedded code cells** — Monaco's markdown `nextEmbedded` fence
   support keys off a language regex that does **not** match Quarto's
   executable `{r}` / `{python}` brace syntax, so executable cells fall
   through to plain text.

**Tree-sitter everywhere.** Drive Monaco's highlighting from a
single Rust-side semantic-token extractor in `quarto-lsp-core`:

- **qmd structure** from the `tree-sitter-qmd` grammar already compiled
  into the WASM bundle (authoritative, Quarto-aware).
- **YAML frontmatter and code-cell interiors** from `crates/quarto-highlight`
  — the *same* tree-sitter grammar set the render pipeline uses (R, Python,
  JS, TS, Bash, SQL, HTML, CSS, JSON, YAML, Julia, Lua), already WASM-built
  and already a dependency of `wasm-quarto-hub-client`.

The payoff over the structure-only draft: **one token source, Quarto-awareness
in every zone, and code-cell colour parity with the rendered output by
construction** (the editor and the render/HTML path share the *same* resolver
— `highlight_captures` + `flatten_spans` in `quarto-highlight` — so the same
code text decodes to the same per-byte capture in both). We reuse grammars
already in the bundle rather than bundling a second highlighter for the
*authoritative* colours.

**Two-layer highlighting (best-practice, not semantic-only).** Monaco is
designed around a fast synchronous **base tokenizer** (Monarch) refined by an
async **semantic** layer that overrides where it has an opinion. We use both:
- *Adopted — "Hybrid-A":* the full semantic provider is the **authoritative**
  colour source (all three zones); a synchronous **Monarch base** paints
  instantly while the async round-trip is in flight and fills any byte semantic
  leaves uncaptured. This *inverts* the usual Monaco reliability roles (normally
  the synchronous layer is the complete/correct one and semantic only *refines* a
  few ranges). The inversion buys **clean precedence** — semantic wins wherever
  it has an opinion, base fills gaps, no two-sources-must-agree negotiation — but
  it shifts baseline correctness onto the async layer, with three consequences
  the base design must respect (none of which make the base "throwaway"):
  - **The base is the *permanent* colour for every uncaptured byte**, not just a
    transient flash-cover. Wherever semantic never emits a token (the fused `](`,
    punctuation the query skips, anything an author forgets) the base colour is
    the final steady-state colour. So the base palette must be *visually
    coordinated* with the semantic palette, not arbitrary.
  - **Recolour-on-settle is potentially whole-document, not a few tokens.**
    Because semantic colours *everything*, every token can flip base→semantic on
    every (debounced) edit. To keep that transition invisible rather than a
    per-keystroke flicker, the base should **colour-align with the semantic
    palette on the high-frequency tokens** (headings, emphasis, code keywords),
    even though it needn't be structurally complete. This is the *conditional*
    consequence — it bites only if the base is visible long enough to flicker — so
    Phase 6 makes it **tier 2: deferred, gated on an observed flicker** at the
    Phase 8 manual check, not a pre-emptive requirement (the warm-WASM window is
    brief; see Phase 6's base-sizing tiers).
  - **The base must be *competent in zones 2–3*, the GH#10 zones.** A plain
    markdown base is broken exactly for frontmatter and `{r}` cells (overview
    bugs #2/#3), so whenever the base shows, the two zones the feature exists to
    fix look wrong. The base therefore routes embedded regions correctly (see
    Phase 6: `nextEmbedded` with a Quarto-brace-aware info-string regex →
    Monaco's stock YAML / r / python tokenizers) so the async-window and gap-fill
    states already look right. This stays Hybrid-A, **not** Hybrid-B: semantic
    always covers the full code-cell/frontmatter interior and overrides the base
    there in steady state, so the competent base only ever shows in flight — no
    two-authoritative-sources conflict. What we still do **not** do is hand-write
    a *Quarto-aware* Monarch grammar — correctness/parity remains semantic's job.
- *Rejected — "Hybrid-B":* Monarch authoritative for structure with semantic
  adding *only* Quarto constructs. That runs two *authoritative* sources that
  must agree where they visually meet, and code-cell colours would not match the
  rendered output. The earlier "two engines fight over ranges" worry applies to
  B, **not** to A — in A precedence is well-defined (semantic wins where present,
  base fills the gap).

**Scope of "parity."** Parity is only *definable* for **code cells (zone 3)** —
the sole place the same source text appears in both the editor and the rendered
HTML (as `<div class="sourceCode">` with `hl-*` spans). Markdown structure
(zone 1) renders as real HTML elements (`<h1>`, `<a>`, …), and frontmatter
(zone 2) is consumed as metadata and never displayed — neither has a rendered
counterpart to match. So "parity" below always means *code-cell* parity; zones
1–2 are editor-only. Parity is also a **steady-state** property: it is delivered
by the semantic layer (wired to the shared resolver), which the Monarch base
cannot reach, so a code cell only attains parity *once semantic settles* — never
in the brief base-only window on open / mid-edit.

### Architecture decisions (confirmed / carried over)

1. **Parser location:** Rust. A new `quarto-lsp-core::tokens` module parses
   with `tree-sitter-qmd` and calls `quarto-highlight` for embedded regions.
   Exposed through a new `lsp_get_semantic_tokens` WASM export, mirroring the
   existing `lsp_get_symbols` / `lsp_get_folding_ranges` pattern at
   `crates/wasm-quarto-hub-client/src/lib.rs:2205,2248`.
2. **Monaco API:** `monaco.languages.registerDocumentSemanticTokensProvider`.
   The whole-document, delta-encoded LSP-style API maps naturally onto a flat
   token list, and theme rules target our custom type names directly.
3. **Coverage + base layer:** the semantic provider covers all three zones (qmd
   structure + frontmatter YAML + code-cell interiors) and is authoritative. The
   `'qmd'` language id gets a **markdown-derived Monarch base** that also routes
   embedded regions to Monaco's stock tokenizers (Hybrid-A, see Overview) as the
   instant paint layer **and** the permanent gap-fill colour. This *relaxes* the
   old "semantic-only" constraint: anything we don't emit semantically falls back
   to a reasonable colour, not plain default-foreground — so partial coverage
   degrades gracefully rather than looking broken. We still aim for full semantic
   coverage (Quarto-awareness + parity), but it is no longer load-bearing for
   "doesn't look broken." Note the base is **not** disposable: it is the
   steady-state colour for every uncaptured byte and must colour-align with the
   semantic palette to avoid per-keystroke flicker (see the three Hybrid-A
   consequences in the Overview).
4. **Language id:** register a dedicated `'qmd'` id (**confirmed 2026-06-16** —
   keep qmd as its own language, *not* folded into `'markdown'`). `.md` keeps
   Monaco's default markdown. Considered and **rejected**: reuse `'markdown'` for
   `.qmd` and gate the provider by extension — that would drop the
   language-registration + Monarch-base work and give an instant base for free,
   but it forfeits a distinct qmd identity (status-bar label, qmd-specific
   language configuration, qmd-only LSP features keyed on the id) and couples
   qmd's editor behaviour to markdown's. The dedicated id is what *requires* the
   Hybrid-A Monarch base (Phase 6) to avoid an unstyled-on-open flash.
5. **Single grammar set *and* single resolver:** the editor and the render
   pipeline share `quarto-highlight` — not just the grammars, but the exact
   `highlight_captures` + `flatten_spans` pair that decides which capture wins
   each byte. The render producer (`Registry::highlight`, consumed by
   `annotate_pandoc`) switches off the lossy `collect_spans` event stream onto
   this shared pair (Phase 0), so a code cell is coloured identically in the
   editor and the rendered HTML *by construction*, and bd-98k6 (the `collect_spans`
   over-wrapping bug) is fixed everywhere rather than only sidestepped on the
   editor path.

## The three zones and their token sources

```
┌───────────────────────────── a .qmd buffer ─────────────────────────────┐
│ ---                          ◀── frontmatter `metadata` node            │
│ title: "x"                   ◀── ZONE 2: quarto-highlight("yaml", body) │
│ ---                                                                      │
│                                                                          │
│ # Heading [link](u){#sec}    ◀── ZONE 1: tree-sitter-qmd highlights.scm │
│                                                                          │
│ ```{r}                       ◀── fence delim + info: ZONE 1             │
│ x <- 1                       ◀── ZONE 3: quarto-highlight("r", body)    │
│ ```                                                                      │
└──────────────────────────────────────────────────────────────────────────┘
```

- **Zone 1 (qmd structure):** a `tree_sitter::Query` over `highlights.scm`,
  run on the `tree-sitter-qmd` CST. Emits tokens for markdown/Pandoc
  punctuation and text — but **not** for the interiors of `metadata`
  (frontmatter) or `code_fence_content`, which are left to zones 2/3.
- **Zone 2 (frontmatter):** locate the `metadata` node (grammar aliases
  `minus_metadata → metadata`, `grammar.js:150`), slice its inner YAML
  (strip the `---` fence lines), call `quarto_highlight::highlight_captures("yaml", body)`,
  offset spans by the body's start byte.
- **Zone 3 (code cells):** for each `pandoc_code_block` node
  (`grammar.js:903`), extract the language from its `info_string` child
  (```` ```python ````) or its `attribute_specifier` → `language_specifier`
  child (```` ```{r} ````, `grammar.js:530-537`), slice the
  `code_fence_content`, call `quarto_highlight::highlight_captures(lang, body)`,
  offset spans.

## Unified token model (the central new contract)

This is the main design work this plan adds over the structure-only draft:
**two families of tree-sitter capture names must collapse into one Monaco
legend.**

- **Structural captures** (from qmd `highlights.scm`) are dotted tree-sitter
  names under the `markup.*` / `punctuation.*` roots — `markup.link.url`,
  `markup.heading.1`, `punctuation.special`, … — and **are themselves the legend
  entries** (modulo dotted-prefix collapse, e.g. `markup.heading.1..6` →
  `markup.heading`). There is no separate structural vocabulary to rename.
- **Code captures** (from `quarto-highlight`\'s per-language `highlights.scm`,
  including yaml) are the standard tree-sitter programming names: `keyword`,
  `function`, `function.builtin`, `string`, `string.escape`, `comment`,
  `number`, `constant`, `variable`, `type`, `operator`, `property`,
  `punctuation.bracket`, `punctuation.delimiter`, `tag`, `attribute`, … — mapped
  under the `code.` namespace (zones 2/3 prepend `code.` before lookup).

### Legend (`QMD_TOKEN_LEGEND`)

A single ordered array of Monaco token-type names — the contract the theme
targets. **Every entry carries a `qmd.` sentinel super-prefix** *on top of* its
family namespace — `qmd.markup.*` / `qmd.punctuation.*` (structural) or
`qmd.code.*` (embedded) — so that no theme rule keyed on a legend entry can
prefix-match a scope another language emits: no other grammar emits anything
under `qmd.`, so the guarantee needs no survey of what TS/JS/JSON/CSS/HTML emit
(see Defence 1 in Phase 7). **The entries are listed below as family stems for
brevity; the canonical names prepend `qmd.`** (e.g. `qmd.markup.heading`,
`qmd.code.keyword`). The `.scm` capture names stay unprefixed; the translator
adds `qmd.` when mapping a capture to its legend index. Two groups:

```
// qmd-structural — dotted tree-sitter names; these ARE the capture names
// (Quarto-specific colours per GH#10; longest-prefix only collapses suffixes)
markup.heading, markup.emphasis, markup.strong, markup.strikethrough,
markup.link.label, markup.link.url, markup.link.title,
markup.image.label, markup.image.url,
markup.raw.inline, markup.raw, markup.raw.info, markup.math, markup.shortcode,
markup.list, markup.quote,
punctuation.special, punctuation.special.image, punctuation.bracket,
punctuation.delimiter.fence, punctuation.delimiter.frontmatter,
attribute.specifier,

// embedded-code (shared; tree-sitter standard names, namespaced under `code.`)
// MUST cover every root the render CSS colours via `hl-<root>` — see the
// parity-coverage invariant in Phase 7. These 24 roots == the `.hl-*` selectors
// in `resources/scss/html/templates/highlight.scss`:
code.attribute, code.boolean, code.character, code.comment, code.constant,
code.constructor, code.embedded, code.error, code.escape, code.function,
code.keyword, code.label, code.markup, code.module, code.namespace,
code.number, code.operator, code.property, code.punctuation, code.special,
code.string, code.tag, code.type, code.variable
```

A few entries earn their keep against collisions: `punctuation.special`
(shared by heading markers, emphasis/strong markers, math delimiters and
code-span backticks — none need a distinct colour) is separate from
`punctuation.special.image` (the `![` accent, matched first by longest-prefix);
and `punctuation.delimiter.fence` vs `punctuation.delimiter.frontmatter` are
**distinct** because the query emits distinct capture names for them — the
plan wants those two coloured differently.

### Translator (`capture_name → legend index`)

One **uniform** function maps **both** families through the same rule: prepend
the `qmd.` sentinel (always) and `code.` (embedded only — zones 2/3), then apply
**longest-dotted-prefix** against the legend (try the full name; drop trailing
components until a legend entry matches). Because the structural capture names
*are* the legend entries modulo that `qmd.` prefix, there is **no structural
rename table** — the prefix walk does all the work for both families:

- `markup.heading.3` (structural) → `qmd.markup.heading` (the level suffix drops).
- `markup.link.url` (structural) → `qmd.markup.link.url` (exact match, no collapse).
- `function.builtin` (embedded) → `qmd.code.function.builtin` → `qmd.code.function`.
- `string.escape` (embedded) → `qmd.code.string.escape` → `qmd.code.string`.
- `punctuation.bracket` (embedded, from a code grammar) → `qmd.code.punctuation.bracket`
  → `qmd.code.punctuation` — it lands in the `qmd.code.*` namespace and never
  collides with the *structural* `qmd.punctuation.bracket` legend entry.
- Two families (`markup.*`/`punctuation.*` structural, `code.*` embedded), all
  under the `qmd.` super-prefix, keep the families disjoint **and** keep every
  rule safe under the global theme: a bare `keyword`/`string`/`tag` rule would
  recolour other languages (see Phase 7, Defence 1).
- Unknown capture → **skipped** (optionally a one-off `debug!`) — never panics,
  never emits a garbage index.

The query author controls capture names directly, so the translator can only
route what the query **distinguishes**: a construct that needs its own colour
must get its own capture name (and legend row); a construct that may share a
colour reuses an existing name. This is why frontmatter `---` and a fenced-code
delimiter are different legend entries — the query emits
`@punctuation.delimiter.frontmatter` vs `@punctuation.delimiter.fence`, not one
shared `@punctuation.delimiter` (a name-keyed translator could never split one
name two ways). Conversely, heading markers and math delimiters deliberately
**share** `@punctuation.special`.

Adding a capture to any `.scm` requires either an existing prefix match or a new
legend row; the translator is the single point of truth, matching the draft's
"Rust translator owns the table" principle.

## Token extraction, flatten, merge, and invariants

### Extraction strategy: `Query.captures()`, not the highlight event stream

**Decision (settled empirically 2026-06-14).** All three zones extract tokens
via `tree_sitter::Query` captures — node-exact byte ranges — **not** via
`tree-sitter-highlight`'s `HighlightEvent` stream. The editor therefore must
**not** reuse `quarto-highlight`'s nested `collect_spans` output.

Why: Monaco's semantic-tokens API is structurally flat (delta-encoded
5-tuples, no nesting), but tree-sitter captures nest. *Something* must
flatten — and a flatten is only correct if its input ranges are correct. The
`tree-sitter-highlight` event stream is **lossy for same-start nested
captures**: it omits the inner capture's end boundary entirely. Dumping the
raw events for the bd-98k6 fixture `name = "value"` (`(bare_key) @type` nested
in `(pair (bare_key)) @property`, both opening at byte 0):

```
Start(type)@0 · Start(property)@0 · Source 0..5 · … · End · End · End @14
```

The first `Source` jumps straight to `0..5` — **no boundary at byte 4** (the
bare_key's true end), and all three `End`s fire at cursor 14. No consumption
strategy recovers `type`=0–4, because byte 4 is not in the stream: a
top-of-stack flatten emits the *outer* `property` over the whole region and
**drops `type`**; `collect_spans` records `[0,14,type]` (this *is* bd-98k6 —
its golden `integration__golden__user_grammar_toml.snap` shows `[0,14,"type"]`).
`Query.captures()` returns node-exact ranges (`type`=0–4, `property`=0–14)
directly — the only extraction that carries the boundary.

This makes "fix bd-98k6 for the editor" and "feed Monaco" the *same* work
(bd-98k6 fix-path (a)) — **and the render/HTML path converges onto it too.**
Rather than maintain two resolvers (one for the editor, the lossy
`collect_spans` for render), Phase 0 switches the render producer
(`Registry::highlight`, which `quarto-highlight`\'s `annotate_pandoc` at
`crates/quarto-highlight/src/annotate.rs:222` consumes — driven by the
`code_highlight` render stage) onto the same
`highlight_captures` + `flatten_spans`. Consequences:

- **Code-cell parity is by construction**, not by a drift-prone second
  algorithm: both consumers decode the same code text through the same two
  functions.
- **bd-98k6 is fixed on the render path**, so it can be closed (not left open).
- **The HTML writer is unchanged.** `write_highlighted_body` (`html.rs:671`)
  already walks arbitrary spans via open/close events and flushes gaps; fed flat
  disjoint spans it simply emits non-nested `<span>`s.
- **Cost (smaller and more localised than it first appears):** the rendered
  `hl-*` HTML is **not** captured in any `.snap` — the only insta snapshots that
  could change are the 15 **span-encoding goldens** in
  `crates/quarto-highlight/tests/integration/snapshots/`
  (`integration__golden__*.snap`), which store the encoded
  `(start, end, capture)` list directly. **Of those 15, only \~4 actually change**
  — `bash`, `julia`, `python`, and `user_grammar_toml`, the only goldens whose
  current output nests; the other 11 are already fully disjoint and stay
  byte-identical under the new resolver (confirm on regen). (Verified 2026-06-16
  by scanning every golden for nested and equal-extent captures.) Review the
  changed ones per the CLAUDE.md snapshot
  policy (report counts + diffs, flag surprises), reading them as *span-shape*
  diffs (nested → flat), reading the new flat span list as the per-byte winner
  directly: for the pure-nesting goldens (`bash`/`julia`/`python`) that winner
  equals the old nested-DOM innermost, so visible colour is unchanged; for
  `user_grammar_toml` the gap-byte recolour is the intended bd-98k6 fix (see the
  tie-break note below). The one place
  rendered `hl-` HTML is checked is a hand-written `.contains()` assertion, not a
  snapshot — `crates/quarto-core/tests/integration/render_to_html_user_grammars.rs:142,147`
  (`contains("<span class=\"hl-marker\"")`, `!contains("data-hl-spans=")`) — so
  it will **not** auto-regenerate; re-check it by hand after the switch. The
  **CSS audit is already moot**: `resources/scss/html/templates/highlight.scss`
  has no descendant (`.hl-x .hl-y`) selectors, so flat spans cannot break a
  nesting-dependent rule. A **tie-break** for *genuine* equal-extent captures
  (two patterns on one node) is still required defensively — but none occur in
  the corpus (the `user_grammar_toml` `[0,14]`/`[0,14]` pair is the over-wrap
  bug, not a real tie), so it is pinned by a new synthetic fixture (see Flatten
  below), not by these goldens.

All built-in language configs pass **empty** injection/locals queries
(`langs/mod.rs` `build_for`, all 13 langs), so `tree-sitter-highlight` was only
ever doing the highlights query + its overlap resolution — there is no
injection/locals behaviour for the captures walk to lose.

### Flatten: innermost-wins (`quarto_highlight::flatten_spans`)

`Query.captures()` produce nested/overlapping triples `(start, end, capture)`.
One shared pass collapses them to a flat, non-overlapping run. **This lives in
`quarto-highlight` as `pub fn flatten_spans(Vec<HighlightSpan>) -> Vec<HighlightSpan>`,
not in `quarto-lsp-core`** — both the editor (`tokens.rs`, all zones) and the
render producer (`Registry::highlight`) call it, and `pampa` cannot depend on
the LSP crate. Rules:

- **Innermost (narrowest) span wins** each byte; the narrower capture paints
  over the wider one, splitting the wider span around it. This is well-defined
  because captures from one `Query` over one tree are **nested-or-disjoint** —
  CST node ranges never partially overlap — so for any byte the set of covering
  captures is a strict nesting chain and "narrowest" is unambiguous. The *only*
  residual ambiguity is two captures at **equal extent** (same start *and* end,
  e.g. two patterns matching the same node), handled by the tie-break.
- **Tie-break** genuine equal-extent captures deterministically. **No corpus
  golden actually contains one.** The `user_grammar_toml` golden *looks* like it
  does — `[0,14,"property"]` and `[0,14,"type"]` are both present — but that is
  the bd-98k6 over-wrap **bug**, not a real tie: the fixture's `highlights.scm`
  captures `(bare_key) @type` *nested inside* `(pair (bare_key)) @property`
  (true extents `type`=0–4 ⊂ `property`=0–14, confirmed against the grammar
  query), and legacy `collect_spans` merely stretched `type` to the pair's end.
  Node-exact extraction recovers `type`=0–4, and innermost-wins resolves it with
  **no tie**. So the corpus exercises the *nesting* path, never the equal-extent
  path. Consequences for verification:
  - A genuine equal-extent collision still *can* arise (two patterns matching one
    node), so the tie-break is a real defensive requirement — it just needs a
    purpose-built fixture, which this plan adds:
    `crates/quarto-highlight/tests/fixtures/user-grammar-equal-extent/` — the
    TOML grammar binary paired with a `highlights.scm` that captures `(bare_key)`
    under **both** `@type` and `@property`, so `Query.captures()` emits two
    captures at the identical range and drives the tie-break through the real
    `highlight_captures` path.
  - **A span-list assertion is now sufficient** — the draft's "rendered effective
    colour" indirection is no longer needed. That indirection existed only
    because `collect_spans` kept *both* overlapping spans and the writer nested
    them, leaving the winner implicit in the DOM. `flatten_spans` instead
    **resolves the tie to a single surviving span**, so its output names the
    winner directly: assert which capture survives over the collision range.
  - The exact winner is **determined empirically** when Phase 0 first runs (TDD
    red→green) against the synthetic fixture, and **documented** in
    `flatten_spans`. Note that `flatten_spans` sees only `(start, end, capture)`,
    no pattern index — determinism comes from `highlight_captures` preserving a
    stable capture-stream order plus a stable flatten. Don't assume earlier- vs
    later-pattern wins.
- Implementation: sweep captures sorted by `(start, -length)` — with a **stable**
  sort so equal-extent captures keep their input (capture-stream) order, which is
  what makes the tie-break deterministic — or a byte→winning-capture paint buffer
  then run-length-encode. One function, used by zone 1 and zones 2/3 alike, and by
  the render path.

### Merge across zones

The three zones cover **disjoint** byte regions by construction (structural
excludes code/YAML interiors), so cross-zone merge is concatenate-and-sort
*after* each zone is independently flattened:

1. Flatten zone-1 (structural) captures; flatten each zone-2/zone-3 (embedded)
   region's captures.
2. **Embedded wins** as a belt-and-braces guard: if a structural span still
   intersects an embedded region (query over-capture), drop/clip it.
3. Concatenate; sort by `start_byte`.
4. **Split any span that crosses a line boundary into one span per line**
   (LSP requires single-line tokens — see Position mapping (b)). Editor-only,
   applied here at the byte→position step; **never** folded into the shared
   `flatten_spans` (the render/HTML path emits multi-line spans freely and must
   not be perturbed — that would reshape the Phase-0 goldens and break parity).
5. Convert byte spans → UTF-16 `(line, character, length)` (see below).

**Invariant tests:**
- `tokens_are_non_overlapping_and_sorted` — over a *deliberately nested*
  fixture (e.g. `# heading with [**bold** link](url)` plus a code cell with a
  `(pair (key))`-style nesting) the final `Vec<SemanticToken>` is strictly
  sorted and non-overlapping. With the flatten in place this holds *by
  construction*; the fixture must actually nest, or the test gives false
  confidence.
- `tokens_never_span_a_line` — over a fixture whose CST nodes cross a newline
  (display math `$$…$$`, a raw HTML block, and a code cell with a multi-line
  string/comment) assert **no** emitted token's range extends past the end of
  its own line. Non-overlap does **not** catch this — a multi-line span is
  well-formed in byte space but illegal in the LSP model — so it is a separate
  invariant.

### Position mapping (two correctness gotchas)

**(a) UTF-16, not bytes.** `tree-sitter` byte offsets and `Point.column` are
**byte-based**; Monaco semantic tokens are **UTF-16 code units**
(`crates/quarto-lsp-core/src/types.rs:14` already documents this for
`Position`). There is **no** reusable byte→UTF-16 index in the tree today, so
`tokens.rs` builds its own `Utf16LineIndex` from the document string once per
call: line-start byte offsets + per-line UTF-16 counting. ASCII docs are
unaffected; non-ASCII lines (emoji, accented text) would mis-highlight without
this. A dedicated test exercises a fixture with a multi-byte character before a
token.

**(b) One token per line — the LSP model forbids multi-line tokens.** A
Monaco/LSP semantic token is `(line, character, length)` where `length` is in
UTF-16 units **on that one line**; the delta encoding has no way to represent a
token spanning a newline. But a single tree-sitter capture routinely *does*
cross lines — display math (`$$…$$`), raw HTML blocks, block quotes (zone 1),
multi-line strings / block comments (zone 3), YAML block scalars (zone 2). So
the byte→position step must **split a cross-line span into one token per line it
touches**, each clipped to its line and with any trailing newline trimmed (a
token must not include the `\n` or run to EOL+1). The `Utf16LineIndex` already
holds the line-start table this needs, so the split is a fold over the line
boundaries between the span's start and end. This is **editor-only**:
`flatten_spans` (shared with render, Phase 0) stays multi-line — only the LSP
conversion in `tokens.rs` splits — so the render goldens and code-cell parity
are untouched. `tokens_never_span_a_line` (above) pins it.

## What's already in place (do not rebuild)

- **tree-sitter-qmd unified grammar** with link/image/attr nodes. **Links have
  no dedicated node:** `[text](url)` parses to `pandoc_span` with a `target`
  child (`pandoc_span` is overloaded — see Phase 1); `pandoc_image` is
  `![…](target)` reusing the same `target`. `target` holds aliased `url` /
  `title` children. Attribute specifiers surface as `attribute_specifier`
  (an alias of the hidden `_pandoc_attr_specifier`). Plus `metadata`,
  `pandoc_code_block`, `code_fence_content`, `info_string`,
  `language_specifier`. Exposes `tree_sitter_qmd::LANGUAGE`
  (`bindings/rust/lib.rs:29`) **and** the query as a compile-time constant
  `tree_sitter_qmd::HIGHLIGHT_QUERY` (`:32`) — Phase 2 compiles *that*, not an
  `include_str!` across the crate boundary. (`tree_sitter_qmd::INJECTION_QUERY`
  (`:35`) also exists; the editor ignores it — zones 2/3 do language injection
  themselves by walking `pandoc_code_block` / `metadata`, so embedded
  highlighting is never double-applied.)
- **CST + `tree_sitter::Query` precedent** to copy: `crates/quarto-csl/src/parser.rs`,
  `crates/quarto-parse-errors/src/lib.rs` (both build a `Parser`, run a
  `QueryCursor`).
- **`quarto-highlight`** — `highlight(class, source) -> Result<Option<String>, HighlightError>`
  (JSON encoding) built on an internal `collect_spans` that walks
  `tree-sitter-highlight`\'s event stream (native `user_grammar.rs:274`, wasm
  `registry.rs:111`). `collect_spans` is **lossy for same-start nested
  captures** (drops the inner end boundary — bd-98k6), so Phase 0 **replaces it
  as the resolver** with a `Query.captures()`-based `highlight_captures` +
  `flatten_spans` (see the extraction decision above) and re-points
  `Registry::highlight` (the producer behind `annotate_pandoc`) at the new pair.
  `collect_spans` is then retired from the production path (kept only if a test
  still references the old event-stream behaviour). What Phase 0 reuses
  unchanged: class-alias resolution (`sh→bash`, `js→javascript`, `yml→yaml`, …)
  in the registry (`BUILTIN_ALIASES`, `crates/quarto-highlight/src/registry.rs:154`),
  the `is_language_supported` wrapper (`lib.rs:70`), and the builtin/user-grammar
  `HighlightConfiguration` lookup (which already holds the compiled `Query` +
  `Language` the captures walk needs). WASM-safe (the wasmtime user-grammar
  loader is `#[cfg(not(target_arch="wasm32"))]`).
- **HTML span emitter** `write_highlighted_body` (`crates/pampa/src/writers/html.rs:671`)
  — already span-shape-agnostic: it sorts open/close events and flushes gaps, so
  flat disjoint spans emit non-nested `<span class="hl-…">`. No writer change is
  needed when the producer starts emitting flattened spans; the rendered HTML is
  not snapshotted, so only the `quarto-highlight` span-encoding goldens change
  (see Phase 0).
- **`HighlightSpan`** wire type (`crates/quarto-highlight-encoding/src/lib.rs:27`).
- **WASM `lsp_*` export pattern** (`crates/wasm-quarto-hub-client/src/lib.rs:2205`):
  read VFS → UTF-8 check → `Document::new` → call → serialize.
- **intelligenceService + monacoProviders plumbing**
  (`hub-client/src/services/intelligenceService.ts`,
  `hub-client/src/services/monacoProviders.ts:162-222`) — the exact provider
  registration shape to extend.
- **`Document::new(path, content)`** (`crates/quarto-lsp-core/src/document.rs`)
  — text-only, WASM-safe; `tokens.rs` parses independently of the heavy
  `analyze_document` pipeline.

## Implementation phases (TDD)

Per CLAUDE.md: tests precede implementation in each phase; a phase closes
only when its test layer is green.

### Phase 0 — `quarto-highlight`: shared `highlight_captures` + `flatten_spans` resolver (editor **and** render)

Phase 0 establishes the **one resolver both consumers share**: node-exact
`Query.captures()` extraction + the innermost-wins flatten. Both the editor
(zones 1–3) and the render producer call this pair, which is what gives
code-cell parity by construction (and fixes bd-98k6 everywhere). It replaces
`collect_spans` on the production path.

**Tests first** (`crates/quarto-highlight/src/...` unit tests + render snapshots):
- `highlight_captures_for_r` — `x <- 1` as `"r"` yields captures with sensible
  names (`operator`/`variable`/etc.) and byte offsets that slice valid
  substrings of the source.
- `highlight_captures_unsupported_language` — unknown class → `Ok(None)`.
- `highlight_captures_yaml` — `title: x` as `"yaml"` yields a `property`-ish
  capture over `title`.
- `highlight_captures_are_node_exact` — a same-start nested case yields the
  inner capture at its *own* end (not the outer's): proves we use
  `Query.captures()`, not the event stream (guards against a bd-98k6
  regression).
- `flatten_spans_*` — innermost-wins over a nested fixture. For the **equal-extent
  tie-break**, load the synthetic `user-grammar-equal-extent` fixture (the only
  source of a genuine same-start-*and*-end collision — the corpus has none, see
  Flatten), run `highlight_captures` + `flatten_spans`, and assert the collision
  range collapses to **exactly one** surviving span (flanking disjoint spans
  untouched). Pin the winning capture to whatever the implementation
  deterministically produces (determine empirically, TDD red→green) and document
  the rule.
- `flatten_is_idempotent` — `flatten_spans(flatten_spans(x)) == flatten_spans(x)`.
- `builtin_configs_have_no_injection_or_locals` — the captures-only switch is
  lossless **only because** every built-in `HighlightConfiguration` passes empty
  injection + locals queries (verified today: all `build_for` callers pass
  `"", ""`). Nothing else guards that. Build every supported language's config
  and assert both queries are empty, so the day a future grammar ships a
  non-empty injection query — whose injected highlighting the `Query.captures()`
  walk would silently drop — CI fails loudly instead of the editor (and render)
  quietly losing it.
- bd-98k6 golden: `integration__golden__user_grammar_toml.snap` (a **span-list**
  golden, not rendered HTML) currently records the inner `type` as `[0,14]`
  alongside `[0,14,"property"]`; after the switch the node-exact walk yields
  `type`=0–4 with `property` split around it. This is **nesting**, not a tie (see
  Flatten) — the **render-path** fix that lets bd-98k6 close. The per-byte
  *visible* colour does change on the gap bytes the over-wrap bug previously
  mis-painted (e.g. the spaces around `=` flip from `type` to `property`); that
  recolour **is** the bd-98k6 fix — expected, **not** something to hold constant.
  For the pure-nesting goldens (`bash`/`julia`/`python`) innermost-wins
  reproduces the same per-byte winner the nested DOM showed, so their visible
  colour is preserved and only the span *encoding* reshapes nested→flat.

**Implementation:**
- Add `pub fn highlight_captures(class, source) -> Result<Option<Vec<HighlightSpan>>, HighlightError>`
  to `crates/quarto-highlight/src/lib.rs`: resolve the class to its
  `HighlightConfiguration` (reusing the existing alias resolution + builtin/
  user-grammar dispatch), run a `QueryCursor` over the config's compiled
  `Query`, return one `HighlightSpan` per capture — node-exact
  `(start, end, capture_name)`, **unflattened**.
- Add `pub fn flatten_spans(Vec<HighlightSpan>) -> Vec<HighlightSpan>` — the
  shared innermost-wins flatten (see Flatten). Lives here so both `pampa` and
  `quarto-lsp-core` can call it without an LSP-crate dependency.
- **Switch the render producer.** Re-point `Registry::highlight` (consumed by
  `quarto-highlight`\'s `annotate_pandoc`,
  `crates/quarto-highlight/src/annotate.rs:222`, via the `code_highlight` render
  stage) from `collect_spans` to
  `flatten_spans(highlight_captures(...))` → `encode`. The producer now emits
  flattened, non-overlapping spans; the HTML writer (`write_highlighted_body`)
  consumes them unchanged. **Switch the user-grammar sibling too** —
  `UserGrammars::highlight` (`user_grammar.rs`) shares `collect_spans`; converge
  it onto the same `flatten_spans` (extracting captures from the user grammar's
  own `HighlightConfiguration` query) so built-in and user-grammar code cells
  flatten identically, not half-and-half. (User grammars remain render-path
  only; editor user-grammar support stays the deferred follow-up.)
- **Regenerate the span-encoding goldens** in
  `crates/quarto-highlight/tests/integration/snapshots/` — expect **\~4 of the 15
  `integration__golden__*.snap` to change** (`bash`, `julia`, `python`,
  `user_grammar_toml`; the other 11 stay byte-identical — rendered `hl-*` HTML is
  not snapshotted) and
  report per the CLAUDE.md snapshot policy (count modified, summarise the
  nested→flat change, flag any surprise — a "disjoint" golden that unexpectedly
  moves means the query double-captures a node). Read each diff as a *span-shape*
  change; per-byte visible colour is preserved for the pure-nesting goldens
  (`bash`/`julia`/`python`) by innermost-wins, while `user_grammar_toml`\'s
  gap-byte recolour is the intended bd-98k6 fix (see Flatten). **Re-check by
  hand** the
  `.contains()` assertion at
  `crates/quarto-core/tests/integration/render_to_html_user_grammars.rs:142,147`
  — it is not a snapshot and will not auto-regenerate. The CSS audit is already
  confirmed moot (`highlight.scss` has no `.hl-x .hl-y` descendant selectors).
- No cfg seam is needed — the captures walk over a resolved `Query` is
  target-independent (the wasmtime user-grammar loader stays `cfg(not(wasm32))`).

### Phase 1 — expand `highlights.scm` to cover qmd inline + block

The structural extractor (Phase 2) consumes this query, so the query must
exist first.

**Tests first** (the query's real coverage is Phase 2's extractor, not the
tree-sitter corpus):
- `highlights.scm` is a *query*, not the grammar — editing it needs **no**
  `tree-sitter generate` (that regenerates `parser.c` from `grammar.js` and only
  adds churn), and `tree-sitter test` exercises *corpus parse* tests, which
  cannot catch a wrong capture.
- Confirm every node we query has a parse regression test under
  `crates/tree-sitter-qmd/tree-sitter-markdown/test/` (most exist; add any
  missing) and that `tree-sitter test` stays green. **Only touch tests you add.**
- The query is actually exercised by the Phase-2 extractor tests + corpus
  snapshot, which compile `tree_sitter_qmd::HIGHLIGHT_QUERY` and assert on the
  emitted tokens.

**Implementation** — extend `crates/tree-sitter-qmd/tree-sitter-markdown/queries/highlights.scm`
with dotted `markup.*` / `punctuation.*` capture names that **are** the legend
entries (the translator only collapses dotted suffixes — see Unified token model;
emit a distinct name wherever two constructs need distinct colours):
- Headings: ATX/setext markers → `@punctuation.special`; text →
  `@markup.heading.{1..6}`.
- Emphasis / strong / strikethrough: text → `@markup.emphasis` / `@markup.strong`
  / `@markup.strikethrough`; markers → `@punctuation.special`.
- Code spans (`code_span_delimiter` + content) → `@punctuation.special`
  (backticks share the marker accent), `@markup.raw.inline`.
- **Links are `pandoc_span` with a `target` child** — there is no
  `commonmark_link` node, and `pandoc_span` is overloaded: `[text](url)`
  (link), `[text]{.cls}` (attr span) and bare `[text]` (plain span) all parse
  to `pandoc_span`. A span is a *link* only when it has a `target` child, so
  match `(pandoc_span (target …))` for link coloring and leave attr/plain
  spans to other rules. Inside: `content` (aliased from `_inlines`) →
  `@markup.link.label`; the `target`'s aliased `url` → `@markup.link.url`; the
  `target`'s aliased `title` → `@markup.link.title` (its own legend row — **not**
  a bare `@string`, which would have no structural legend home and, themed, would
  recolour every code/JSON string in the editor).
- **Bracket punctuation is uneven.** The opening `[` is a queryable string
  literal (`"["`), as is the closing `)` in `target` (`")"`), but the `](`
  between label and url is one **anonymous regex token** inside `target`
  (`/[ \t]*[\]][(]/`) that a query cannot match by literal. Color the queryable
  brackets `@punctuation.bracket`; the fused `](` inherits default foreground
  (acceptable — splitting that token in the grammar is out of scope here).
- `pandoc_image` (`![content](target)`) reuses the same `target` — there is no
  separate image-url node. The `![` opener is one fused token →
  `@punctuation.special.image`; `content` → `@markup.image.label`; the
  `target`\'s `url` → `@markup.image.url`. (Distinct `markup.image.*` names — not
  `markup.link.*.image`, which would collapse into the link entries under
  longest-prefix — so the theme *can* mirror link colours yet keep the option to
  diverge.)
- `attribute_specifier` → `@attribute.specifier` (query the **alias**; the
  underlying `_pandoc_attr_specifier` is hidden and never queryable by that name
  — it is always `alias($._pandoc_attr_specifier, $.attribute_specifier)`). The
  dotted name (not bare `@attribute`) keeps the theme rule from prefix-matching
  HTML's `attribute.name`/`attribute.value` under the global theme (see Phase 7).
  `shortcode` → `@markup.shortcode` (kept in the safe `markup.*` namespace, not a
  bare `@function.macro`).
- Inline/display math delimiters → `@punctuation.special`; contents →
  `@markup.math`.
- Raw HTML inline/block → `@markup.raw` (not a bare `@tag`, which would
  prefix-match HTML/XML `tag` tokens editor-wide).
- Fenced code **delimiter** → `@punctuation.delimiter.fence`; **info string** →
  `@markup.raw.info`. Frontmatter `---` → `@punctuation.delimiter.frontmatter`
  (distinct from the fence delimiter — separate legend rows, so the query must
  give them separate names; a shared `@punctuation.delimiter` could not be split
  back apart by the translator).
- **Critically:** do **not** emit a token spanning the whole
  `pandoc_code_block` or the `metadata` interior. Replace the current
  `(pandoc_code_block) @text.literal` / `(code_fence_content) @none` so the
  interiors are left to the embedded layer (zone 2/3). The corpus snapshot
  (structural in Phase 2, extended with a code cell + frontmatter in Phase 3)
  is the durable check that no structural span lands inside a fenced body.

### Phase 2 — `quarto-lsp-core::tokens`: structural extractor + model

The `highlights.scm` from Phase 1 is now rich enough for these tests to pass.

**Tests first** (`crates/quarto-lsp-core/src/tokens.rs` mod tests):
- `tokens_for_atx_heading` — `# Hello` → marker token (`punctuation.special`) +
  heading-text token (`markup.heading`).
- `tokens_for_link` — `[label](https://example.com)` → distinct spans for
  `label` (`markup.link.label`), `https://example.com` (`markup.link.url`), and
  the queryable brackets (`punctuation.bracket`). (Mechanically a `pandoc_span`
  with a `target` child — see Phase 1; the fused `](` token is not independently
  colorable.)
- `tokens_for_image` — `![alt](image.png)` → `punctuation.special.image` over the
  `![` opener (a single fused token, not just `!`) plus `markup.image.label` /
  `markup.image.url`.
- `tokens_for_attribute_specifier` — `{#fig-1 width="400px"}` →
  `attribute.specifier` token over the specifier.
- `tokens_are_non_overlapping_and_sorted` — invariant over a *deliberately
  nested* fixture (e.g. a link inside a heading), so it actually exercises the
  innermost-wins flatten.
- `tokens_never_span_a_line` — over a fixture with a cross-line construct
  (display math, raw HTML block) assert no token extends past its line's end.
  The per-line split (Position mapping (b)) is editor-only and is **not** caught
  by the non-overlap invariant, so it needs its own test.
- `every_query_capture_maps_to_a_legend_entry` — iterate the qmd query's
  `capture_names()` and assert each resolves via `capture_to_token_type` (or sits
  on an explicit allowlist of intentionally-unstyled captures). The translator
  silently skips unknown captures, so without this a Phase-1 query that adds (say)
  `@markup.crossref` without a matching legend row would leave the construct
  uncoloured **with no error**. This pins the Phase-1 query ↔ legend contract —
  the inbound dual of Phase 7's namespace-invariant guard (which pins the legend ↔
  theme outbound side).
- `structural_captures_never_enter_interiors` — run the **zone-1 extractor
  alone** (before the Phase-3 merge) over a fixture with a code cell and
  frontmatter, and assert no structural span lands inside a `metadata` /
  `code_fence_content` range. Phase 3's "embedded wins" clip would *mask* a
  Phase-1 over-capture, so this checks exclusion at the source rather than relying
  on the belt-and-braces clip that hides the bug.
- `utf16_offsets_account_for_multibyte` — a line containing a multi-byte
  char before a `[link]` produces the correct UTF-16 `character`.
- `insta` snapshot over `tests/fixtures/highlight-corpus.qmd` (every structural
  construct we colour) — the durable structural contract.

**Implementation:**
- New deps in `crates/quarto-lsp-core/Cargo.toml`: `tree-sitter`,
  `tree-sitter-qmd`, `quarto-highlight` (which re-exports `HighlightSpan`) —
  all already WASM-safe and in the workspace.
- `crates/quarto-lsp-core/src/tokens.rs`:
  - `pub fn get_semantic_tokens(doc: &Document) -> Vec<SemanticToken>`.
  - `static LANG_AND_QUERY: OnceLock<(Language, Query)>` — compile
    `tree_sitter_qmd::HIGHLIGHT_QUERY` against `tree_sitter_qmd::LANGUAGE` once
    per process (use the exposed constant — do **not** `include_str!` the `.scm`
    across the crate boundary); `Parser` per call (cheap; cache on `Document`
    is a profiling-gated follow-up).
  - Run `QueryCursor` over the root for node-exact captures, skip those inside
    `metadata` / `code_fence_content` regions, translate
    `(start_byte, end_byte, capture_name)` via the translator, then apply
    `quarto_highlight::flatten_spans` (the **same** innermost-wins flatten the
    render producer uses — zone-1 captures nest, e.g. a link inside a heading,
    so flattening is required, not optional).
  - `Utf16LineIndex` built from `doc.content()`; converts each (already
    flattened, merged) byte span to one-or-more LSP tokens, **splitting any span
    that crosses a newline into per-line tokens** (trailing `\n` trimmed) before
    the `byte → (line, char, length)` map. This split lives here, not in
    `flatten_spans` (which stays multi-line for the render path).
- `crates/quarto-lsp-core/src/types.rs`:
  - `pub struct SemanticToken { line: u32, character: u32, length: u32, token_type: u32, modifiers: u32 }`.
  - `pub struct SemanticTokensJson { tokens: Vec<SemanticToken> }` — the legend
    is **not** shipped per-response (it is a compile-time constant on both
    sides; see the legend handling in Phases 4–6).
  - `QMD_TOKEN_LEGEND: &[&str]` and `capture_to_token_type(&str) -> Option<u32>`
    (the longest-dotted-prefix translator; the single point both families
    map through).
- `crates/quarto-lsp-core/src/lib.rs`: re-export `tokens::get_semantic_tokens`,
  `types::{SemanticToken, QMD_TOKEN_LEGEND}`.

### Phase 3 — embedded-language layer (frontmatter YAML + code cells)

**Tests first** (`tokens.rs`):
- `tokens_for_code_cell_r` — ```` ```{r}\nx <- 1\n``` ```` → tokens inside the
  body carry code legend types (`variable`/`operator`/…), positioned at the
  right lines, and **no** structural span overlaps them.
- `tokens_for_fenced_python` — ```` ```python\nimport os\n``` ```` → `keyword`
  over `import`.
- `tokens_for_multiline_code_string` — a `python` cell whose body contains a
  triple-quoted string spanning ≥2 lines yields one `string` token **per line**
  (none spanning the newline), confirming the per-line split (Position mapping
  (b)) applies to embedded zones, not just zone 1.
- `tokens_for_frontmatter_yaml` — `---\ntitle: x\n---` → `property` over
  `title` inside the frontmatter body; the `---` lines are
  `punctuation.delimiter.frontmatter` (zone 1), not YAML.
- `tokens_for_unknown_code_language` — ```` ```fortran ```` → body has no
  embedded tokens (graceful `None`); fence delimiter/info still tokenised.
- `code_cell_parity_with_render` — for a code cell (e.g. `x <- 1` as `r`), the
  editor's zone-3 spans decode to the **same per-byte capture** as the render
  path's encoded `data-hl-spans` for the same text. Trivially true since both
  call `highlight_captures` + `flatten_spans` — the test pins that the two
  consumers stay wired to the shared resolver (a regression here means someone
  forked the resolver).
- Extend the corpus snapshot to include a code cell and frontmatter.

**Implementation** (in `tokens.rs`):
- Walk the CST for `metadata` and `pandoc_code_block` nodes.
- Frontmatter: slice the YAML body between the fence lines; call
  `quarto_highlight::highlight_captures("yaml", body)`; offset each capture's
  `start`/`end` by the body's start byte; translate; `flatten_spans`; push.
- Code cells: resolve the language from `info_string` text or
  `attribute_specifier → language_specifier` text (strip a leading `.`),
  validate with `quarto_highlight::is_language_supported`; slice
  `code_fence_content`; `highlight_captures(lang, body)`; offset; translate;
  `flatten_spans`; push. Reuse the registry's alias resolution — do **not**
  reinvent language-class normalisation. (Flatten the embedded captures *before*
  offsetting or after — equivalently, since offsetting is a constant shift; do
  it consistently.)
- Merge the (already-flattened, disjoint) zone outputs per the merge rules; the
  final `Vec<SemanticToken>` is sorted and non-overlapping by construction.

### Phase 4 — WASM export

> **Phases 4–7 are one braid strand** (hub-client integration) — they co-land
> and aren't independently verifiable. The numbered headings below are the TDD
> step sequence within that single strand (see Proposed braid strands).

**Tests first** (`hub-client/src/services/semanticTokens.wasm.test.ts`):
- Init WASM, `vfs_add_file('test.qmd', …)`, call
  `lsp_get_semantic_tokens('test.qmd')`, assert the parsed JSON contains the
  expected tokens for a `[label](url)` snippet **and** for an `{r}` cell body.
- Failure paths: missing file → `{ success:false, error }`; non-UTF-8 → same
  shape; empty doc → `tokens: []`.
- **Legend drift guard:** after `initWasm()`, assert the TS legend constant
  (Phase 5) deep-equals `JSON.parse(lsp_get_token_legend())` — the mechanical
  link that keeps the TS copy honest to the Rust source of truth.

**Implementation:**
- `lsp_get_semantic_tokens(path: &str) -> String` in
  `crates/wasm-quarto-hub-client/src/lib.rs`, modeled on `lsp_get_symbols`
  (`:2205`): read VFS, UTF-8 check, `Document::new`,
  `quarto_lsp_core::get_semantic_tokens`, serialize
  `{ success:true, tokens }` (empty doc → `tokens: []`). **Failure shape:** any
  error (missing file, non-UTF-8, internal failure) serializes
  `{ success:false, error }` — **never panic across the boundary**, never return
  a partial/garbage token list.
- `lsp_get_token_legend() -> String` — returns `QMD_TOKEN_LEGEND` as a JSON
  array. **Rust stays the source of truth, but this export is off the
  registration hot path** — it is the drift-guard *test oracle*, **not** what
  `getLegend()` calls. Why not call it at registration: `getLegend()` must be
  synchronous, but the WASM module is initialised lazily (`initWasm()` is
  awaited per-call) and the provider is registered at editor mount *before* WASM
  is ready (`Editor.tsx:562` → `handleEditorMount`, with no `await initWasm()`).
  A sync WASM legend call there would throw. Instead the TS side ships its own
  compile-time constant (Phase 5/6) and a test asserts it equals this export.
- Declare both in `hub-client/src/types/wasm-quarto-hub-client.d.ts` alongside
  the other `lsp_*` functions (the existing `lsp_analyze_document` /
  `lsp_get_symbols` / `lsp_get_folding_ranges` / `lsp_get_diagnostics` block is
  at `:55-58`; add the two new declarations immediately after).

### Phase 5 — hub-client intelligence service

**Tests first:** there is **no** existing `intelligenceService` test file in
`hub-client/src/services/` to extend (the `getSymbols` / `getFoldingRanges`
helpers are currently untested), so **create a new**
`hub-client/src/services/intelligenceService.test.ts` with a `getSemanticTokens`
case mocking the WASM call (parse + error handling). (Distinct from the Phase-4
`semanticTokens.wasm.test.ts`, which drives the real WASM round-trip.)

**Implementation** (`hub-client/src/services/intelligenceService.ts`):
- `getSemanticTokens(path): Promise<SemanticToken[]>` next to `getSymbols`,
  same JSON-decode + `isQmdFile` gate + error pattern. **Failure shape (graceful,
  matching `getSymbols`):** a non-qmd path, a `{ success:false }` envelope, or a
  JSON-decode failure all resolve to **`[]`**, never a rejection — so the
  provider needs no `try/catch` around the normal path. Error and
  legitimately-empty collapse to the same `[]`; the provider treats both as "no
  tokens → fall back to base", which is the right behaviour for a doc open in the
  editor (where a genuine read error is near-impossible — the file is in the VFS
  and UTF-8 because Monaco holds it).
- `QMD_TOKEN_LEGEND: readonly string[]` — a **checked-in TS compile-time
  constant** mirroring the Rust legend, with a JSDoc note pointing at
  `quarto-lsp-core`\'s `QMD_TOKEN_LEGEND` as the source of truth. `getLegend()`
  (Phase 6) reads it synchronously; the Phase-4 drift test guards it against the
  Rust const. Also export the `SemanticToken` **type** for callers. No async
  WASM call sits on the registration path.

### Phase 6 — Monaco language registration + semantic-tokens provider

**Tests first:**
- vitest for the delta-encoder in `monacoProviders.ts` — given a known
  `SemanticToken[]`, the resulting `Uint32Array` matches Monaco's delta-encoded
  5-tuple form `[deltaLine, deltaStartChar, length, type, mods]`. Pure function;
  no Monaco runtime.
- vitest for the provider's cancel/stale path with a stub `model` +
  `CancellationToken` and an injected WASM call (no real module): an
  already-cancelled token — or a `getVersionId()` that changes across the
  `await` — makes `provideDocumentSemanticTokens` resolve to `null` and skip
  delta-encoding. Conversely, a successful **empty** result (`[]`) resolves to
  `{ data: new Uint32Array(0), resultId: undefined }` (clears to base), **not**
  `null` — pinning that the cancel/error and empty-but-valid shapes stay
  distinct.

**Implementation:**
- `hub-client/src/components/Editor.tsx`:
  - `getLanguageForFile`: `case 'qmd': return 'qmd'` (`.md` stays
    `'markdown'`).
  - `handleBeforeMount`: `monaco.languages.register({ id:'qmd',
    extensions:['.qmd'], aliases:['Quarto','Quarto Markdown'] })` then
    `setLanguageConfiguration` (brackets/comment markers) so auto-close and
    comment-toggle work.
  - **Monarch base (Hybrid-A paint + gap-fill layer).** `setMonarchTokensProvider('qmd', …)`
    with a **markdown-derived** ruleset — seed it from monaco's basic-languages
    markdown grammar (or a \~40-line subset: headings, emphasis/strong markers,
    inline code, fenced-code regions, frontmatter block, links). This is the
    synchronous layer that paints instantly on open/while typing **and** supplies
    the permanent colour for every byte semantic leaves uncaptured. It is **not
    authoritative** (semantic overrides it where present), so it need not be
    *structurally* complete — but, per the three Hybrid-A consequences in the
    Overview, it is **not disposable** either. Beyond "avoid the unstyled flash",
    the base has two requirements — but they are **not equal-cost or equal-urgency**,
    so split them across two tiers and only commit tier 1 up front:
    - **Tier 1 — load-bearing, ship now: route embedded regions correctly** (the cheap win that fixes overview
      bugs #2/#3 at the base layer). Use Monarch `nextEmbedded` with an
      **info-string regex that matches Quarto's `{r}` / `{python}` brace syntax**
      (Monaco's stock markdown regex does not — that is bug #3) to hand fenced
      code-cell interiors to Monaco's built-in r/python/etc. tokenizers, and the
      `---…---` frontmatter block to the YAML tokenizer. This makes the base
      already syntax-highlight code cells and frontmatter, so the base→semantic
      settle is a subtle palette shift, not a plain-text→highlighted flash, in
      exactly the zones GH#10 is about. (Still Hybrid-A: semantic always covers
      those interiors and overrides in steady state — the routed base only shows
      in flight.) A base with tier 1 only is already **correct** — being
      non-authoritative, semantic overrides it in steady state regardless of how
      its in-flight colours look.
    - **Tier 2 — deferred, gate on observed flicker: colour-align with the
      semantic palette on the high-frequency tokens** (headings, emphasis/strong,
      code keywords/strings/comments) so the whole-document base→semantic recolour
      on each edit is invisible rather than a per-keystroke flicker, and so the
      permanent gap-fill colour is consistent with the semantic colours around it
      (pull the base token colours from the same `quartoThemeRules` palette in
      Phase 7 rather than inventing a second set). This is the expensive
      requirement, and its value is conditional on the **base-visible window
      actually being long enough to flicker** — which is brief in practice: WASM is
      already warm in the common editor-with-preview layout (`PreviewRouter.tsx:78`),
      and even editor-only the semantic provider's own lazy `await initWasm()` is
      the warm trigger, so the base only shows for the first cold load plus each
      debounced-edit settle. Whether that reads as a jarring flicker or an
      imperceptible shift is an **empirical** question — answer it at the Phase 8
      manual check (type inside a code cell, the worst case) before paying for
      tier 2. Deferring is low-risk: if a flicker is observed, the fix is a pure
      **data change** (map the base tokens to the existing `quartoThemeRules`
      colours), not structural rework — no grammar or provider changes. Investing
      pre-emptively would optimise an invisible window, against the "measure before
      optimising" discipline in Open items. *(Explicitly rejected: warming WASM on
      editor-open to shrink the window — it saves only the mount→first-request delta
      over the provider's own lazy init, helps only the no-preview case, and kicks
      the multi-MB WASM import off concurrently with Monaco's CDN load, the
      documented "too much recursion" module-graph race at `wasmRenderer.ts:152-168`.)*
    - What we still do **not** do: hand-write a *Quarto-aware* Monarch grammar
      (shortcodes, crossrefs, attribute specifiers, math). Correctness and parity
      remain the semantic layer's job; the base just has to look right while
      semantic is in flight and in the gaps.
  - **Enable semantic highlighting** — Monaco only *overrides* the Monarch base
    with semantic tokens when the active theme declares `semanticHighlighting:
    true` (set in Phase 7) **and/or** the editor option
    `'semanticHighlighting.enabled'` is `true`. Set both to be safe; this is the
    most common reason a semantic-tokens provider "does nothing".
- `hub-client/src/services/monacoProviders.ts:162-222`:
  - Re-bind the existing DocumentSymbol + FoldingRange registrations from
    `'markdown'` to `'qmd'`.
  - Add `registerDocumentSemanticTokensProvider('qmd', …)`:
    - `getLegend() → { tokenTypes: [...QMD_TOKEN_LEGEND], tokenModifiers: [] }`
      — synchronous, from the checked-in TS constant (Phase 5); **no WASM call**
      (the module isn't initialised at registration). Fixed for the provider
      lifetime.
    - `provideDocumentSemanticTokens(model, lastResultId, token)`: gate on
      `.qmd`; `await initWasm()` then `getSemanticTokens(path)`. **Honour the
      `CancellationToken`** — Monaco fires a fresh request on every debounced
      edit and cancels the in-flight one, so check `token.isCancellationRequested`
      right after the `await` (before delta-encoding) and return `null` if set,
      dropping a result computed against superseded content rather than applying
      it. This is the async-provider contract, not an optimisation. Also guard
      stale results directly: snapshot `model.getVersionId()` before the `await`
      and bail (`null`) if it changed afterwards (model edited or disposed mid
      WASM call).
      - **Return shape (explicit — `null` vs empty data is load-bearing in
        Monaco):**
        - *cancelled / stale* (token cancelled, or `getVersionId()` changed
          across the `await`) → **`null`**: discard the superseded result and
          leave the existing tokens in place (a fresh request is already in
          flight). Do **not** return empty data here — that would clear
          highlighting for a result we are abandoning.
        - *resolved, any count incl. zero* → delta-encode (already unit-tested)
          and return **`{ data: Uint32Array, resultId: undefined }`**. Zero
          tokens → `data: new Uint32Array(0)`, which clears semantic styling so
          the Monarch base shows — the correct "no tokens here" state. Because
          `getSemanticTokens` collapses read errors to `[]` (Phase 5), a failed
          read also lands here and degrades cleanly to the base layer.
        - *unexpected throw* (e.g. `initWasm()` rejects) → catch and return
          **`null`**; **never let the provider throw** — a thrown
          `provideDocumentSemanticTokens` makes Monaco log and can disable
          semantic tokens for the whole session.
      *(Content source — decided: tokenise the **VFS image by path**, uniform
      with the four sibling providers (`getSymbols`/`getFoldingRanges`/
      `analyzeDocument`/`getDiagnostics`), not `model.getValue()`. Safe because
      the Monaco→VFS write is **synchronous**: a local edit runs
      `applyEditorOperations` → the Automerge change callback writes
      `vfsAddFile(path, text)` (`automergeSync.ts:99`) before returning to the
      event loop, and remote edits flow Automerge → VFS → model in that same
      synchronous callback — so the VFS already matches the model when the
      debounced request fires. The only residual window (content mutating during
      the async `await initWasm()/getSemanticTokens`) is closed by the
      `getVersionId()` guard above. **This synchrony is load-bearing — pin it:**
      add a comment at the provider and a small hub-client test asserting the
      Automerge→VFS write is synchronous, so moving sync off-thread (web worker /
      batched Automerge) surfaces in CI rather than as silent mis-highlighting.
      Fallback if sync ever goes async: add a
      `lsp_get_semantic_tokens_from_text(content)` export and tokenise
      `model.getValue()` directly, gating on `model.getLanguageId() === 'qmd'`.)*
    - `releaseDocumentSemanticTokens`: no-op.
  - Track the new disposable alongside the existing two.

### Phase 7 — theme

**Tests first:** two data-only guards, no Monaco runtime —
- vitest `quartoTheme.test.ts`: assert every exported `quartoThemeRules` token
  starts with the `qmd.` sentinel (the namespace-invariant guard; Defence 2);
- Rust `code_legend_covers_render_css`: assert the `code.*` legend roots equal the
  `.hl-*` selectors in `highlight.scss` (the parity-coverage guard; Defence 3).

Visual colour correctness is still verified in Phase 8.

**Implementation** (`Editor.tsx` `handleBeforeMount`):
- `monaco.editor.defineTheme('quarto-light', { base:'vs', inherit:true,
  semanticHighlighting:true, rules:[…] })` and `'quarto-dark'`
  (`base:'vs-dark'`). `inherit:true` keeps TS/JS/JSON defaults for scopes we do
  **not** override — but it does **not** protect scopes we *do* define a rule
  for; see the theme-scoping correctness note below.
- **Theme-scoping correctness (global rules, longest-prefix match).** A Monaco
  theme applies to *every* file in the editor instance, and each rule colours
  any token whose scope **starts with** the rule's token string. So a rule for
  bare `keyword`/`string`/`tag`/`attribute` would recolour TS/JS/CSS/HTML tokens
  editor-wide — `inherit:true` does not prevent this, because the leak comes from
  rules we *add*, not from missing defaults. Two layers of defence make the leak
  **impossible by construction, then enforce it in CI:**

  **Defence 1 — `qmd.` sentinel super-prefix (structural guarantee).** Every
  legend entry — hence every theme rule token — carries a single `qmd.` prefix:
  `qmd.markup.heading`, `qmd.code.keyword`, `qmd.punctuation.bracket`, … No other
  grammar in the editor emits anything under `qmd.`, so no quarto rule can
  prefix-match a foreign scope, full stop. This is *stronger* than relying on the
  family namespaces alone: "Monaco's built-ins never emit `markup.*`/`punctuation.*`"
  is an empirical survey of grammars we don't control (true today, but fragile);
  the `qmd.` prefix needs no survey — leakage is ruled out by a namespace we own.
  The prefix is purely a theming concern: `.scm` capture names stay unprefixed
  (`markup.heading`), and the translator prepends `qmd.` (and `code.` for
  embedded) when mapping a capture to its legend entry (see Translator). The
  family structure is preserved *under* the prefix, so the render-parity colour
  mapping (`qmd.code.keyword` → the same hex as `hl-keyword`) is unaffected.
  - embedded-code rules are all `qmd.code.*` (`qmd.code.keyword`, …);
  - structural rules are all `qmd.markup.*` / `qmd.punctuation.*`;
  - the attribute specifier is `qmd.attribute.specifier` (not bare `attribute`),
    raw HTML is `qmd.markup.raw` (not bare `@tag`), link titles are
    `qmd.markup.link.title` (not bare `@string`) — the family names still earn
    their keep so a maintainer who later *drops* `qmd.` doesn't instantly
    reintroduce the leak.

  **Defence 2 — automated namespace-invariant test (CI-enforced).** Encode the
  invariant instead of leaving it to a Phase-8 eyeball. Define the `quarto-light`
  / `quarto-dark` `rules` arrays as an **exported constant** (e.g.
  `quartoThemeRules` in a small data module) and add a vitest
  (`hub-client/src/components/quartoTheme.test.ts`) asserting **every** rule's
  `token` starts with `qmd.`. Pure data, no Monaco runtime. This catches the #1
  regression mode — someone later adds a bare `keyword`/`string` rule — at CI
  time, turning manual discipline into a machine-checked invariant. The Phase-8
  manual check (open a `.ts`/`.json` file, confirm unchanged colours) stays as a
  final real-runtime sanity pass, no longer the *sole* guard.
  - *(Rejected alternative: flip the theme per active file — `quarto-*` for
    `.qmd`, `vs`/`vs-dark` otherwise. It also avoids the leak but adds a
    theme-flip flicker on tab switch; the `qmd.` prefix + invariant test is
    simpler and flicker-free.)*
- `rules` maps each `QMD_TOKEN_LEGEND` entry to a foreground colour (token keys
  below shown as family stems for brevity; the real rule keys carry the `qmd.`
  super-prefix — `qmd.punctuation.bracket`, etc. — per Defence 1 above):
  - `punctuation.bracket` → grey `#5C6370`; `markup.link.label` → `#56B6C2`;
    `markup.link.url` → `#4A90E2`; `markup.link.title` rides the string colour.
  - `punctuation.special.image` → brown `#A0522D` (chosen 2026-06-16 — a warm
    accent distinct from the cyan/blue link palette);
    `markup.image.label`/`markup.image.url` mirror the link colours, brackets
    stay grey.
  - `attribute.specifier` → mid-blue italic ("this is code" feel).
  - Embedded code types (`code.keyword`/`code.string`/`code.comment`/… ) → map
    each legend entry to the **same colour the render pipeline's `hl-<capture>`
    CSS uses** (the `hl-` class derives from the bare capture, so map
    `code.keyword`→`hl-keyword`, etc.). The resolver is already shared (Phase 0), so editor and render agree
    on *which* capture wins each byte; this step makes them agree on *what
    colour* that capture is. Parity = shared resolver (which capture) + matched
    palette (what colour); both are required, and both are code-cell-scoped
    (zones 1–2 have no rendered counterpart).
  - **Parity-coverage invariant (Defence 3 — the resolver alone does NOT give
    parity).** The shared resolver only unifies *which capture wins each byte*; it
    does **not** guarantee both surfaces assign that capture a colour, because the
    two surfaces use **different colour tables** — render keys on `hl-<root>` CSS
    classes (`highlight.scss` defines **24 roots**), the editor keys on the
    legend's `code.*` entries. A capture root present in the CSS but missing from
    the legend (originally the legend had only 14: `boolean`, `escape`,
    `constructor`, `label`, `character`, `embedded`, `error`, `markup`, `module`,
    `special` were absent) is coloured in render and **plain in the editor** — a
    silent parity break in the one zone parity is defined for. So:
    - the `code.*` legend is expanded to **all 24 CSS roots** (done in the Unified
      token model legend) — every `hl-<root>` has a `code.<root>` twin;
    - add a test (`code_legend_covers_render_css`, Rust, reading
      `resources/scss/html/templates/highlight.scss`) asserting `{`code.*` legend
      roots`} == {`.hl-*` selectors`}` — a mismatch in **either** direction fails
      (a CSS colour with no legend twin → uncoloured in editor; a legend twin with
      no CSS colour → coloured in editor only). This keeps the two colour tables
      locked together when someone later adds an `hl-foo` rule.
- Wire `theme="quarto-light"|"quarto-dark"` to the existing dark/light
  preference at `Editor.tsx:1020` (currently `vs`/`vs-dark`).

### Phase 8 — end-to-end verification

Tests passing is necessary but not sufficient (CLAUDE.md):

1. `cargo nextest run -p quarto-highlight -p quarto-lsp-core` — the Rust
   layers (Phases 0, 2, 3; Phase 1 is `tree-sitter test`).
2. `cargo nextest run --workspace` — no regressions in downstream crates
   (esp. `pampa`, which shares the tree-sitter-qmd crate). The Phase-0 producer
   switch regenerates **\~4 of the 15 span-encoding goldens** in `quarto-highlight`
   (`bash`/`julia`/`python`/`toml`; the other 11 byte-identical; rendered `hl-*`
   HTML is not snapshotted); also re-check the hand-written
   `.contains()` assertions in
   `crates/quarto-core/tests/integration/render_to_html_user_grammars.rs:142,147`.
   Review the golden diff as span-shape (nested→flat) only, bd-98k6 case now
   node-exact; the genuine equal-extent tie-break is covered separately by the
   synthetic `user-grammar-equal-extent` fixture (Phase 0), not by these goldens.
3. `cargo xtask verify` (no `--skip-hub-build`) — rebuilds the WASM so
   `lsp_get_semantic_tokens` is actually reachable from JS. **Then the
   `q2 preview` chain if validating there** (`npm run build:wasm` →
   `cargo xtask build-q2-preview-spa` → `cargo build --bin q2`), per
   CLAUDE.md's stale-WASM note.
4. **Manual browser check** (`cd hub-client && npm run dev`): open a `.qmd`
   containing frontmatter, `[hello](https://example.com)`,
   `![logo](images/logo.png)`, `![hero](hero.png){width="400px"}`, an
   `{r}` cell and a ```` ```python ```` cell. Confirm:
   - link text vs URL are different blues; image `!` is a distinct accent;
     `width="400px"` reads like code; brackets are unobtrusive grey
     (editor-only — these have no rendered counterpart);
   - frontmatter is highlighted as YAML (editor-only — the rendered doc consumes
     it as metadata, so there is nothing to compare against);
   - **both code cells match the rendered preview's colours** — this is the one
     real parity check. It holds via the shared resolver (*which* capture wins)
     **plus** the parity-coverage invariant (*both* surfaces colour that capture —
     Phase 7, Defence 3). Include a token that exercises a formerly-uncovered root
     (e.g. an R `TRUE`/`boolean` and a string `escape`) so the check actually
     covers the roots the 14-entry legend used to drop;
   - **no unstyled flash and no jarring recolour** on open and while typing — the
     Monarch base paints instantly, then semantic tokens refine. A plain-text→
     styled flash means the base layer isn't wired; a *large* base→semantic
     colour jump inside a code cell (plain → fully highlighted) on every keystroke
     means the base isn't routing `{r}`/`{python}` via `nextEmbedded` or isn't
     colour-aligned with the semantic palette (Phase 6) — the settle should be a
     subtle shift, not a flicker. Type inside a code cell specifically to exercise
     the worst case;
   - **open a `.ts` (or `.json`/`.css`) file in the same editor session and
     confirm its colours are identical to the stock `vs`/`vs-dark`** — the final
     real-runtime sanity pass for the global-theme-scoping hazard (the `qmd.`
     super-prefix + the Phase-7 `quartoTheme.test.ts` invariant test are the
     primary guards); a recoloured keyword/string/attribute means a rule token is
     prefix-matching a foreign scope (see Phase 7);
   - `.md` files in the same session still use Monaco's default markdown.
5. Screenshot in the PR — editor + rendered preview side-by-side, focused on a
   **code cell**, to show colour parity (the only zone where parity is defined).

## Critical files

| File | Change |
| --- | --- |
| `crates/quarto-highlight/src/lib.rs` | **new** shared resolver: `highlight_captures` (`Query.captures()`, node-exact) + `flatten_spans` (innermost-wins); `builtin_configs_have_no_injection_or_locals` guard (the captures-only switch is lossless only while injection/locals stay empty) |
| `crates/quarto-highlight/src/registry.rs` (+ `user_grammar.rs`) | re-point `Registry::highlight` from `collect_spans` onto `flatten_spans(highlight_captures(…))`; retire `collect_spans` from the production path |
| `crates/quarto-highlight/tests/integration/snapshots/*.snap` | regenerate the \~4 changed span-encoding goldens (`bash`/`julia`/`python`/`toml`, nested→flat); other 11 byte-identical; writer code unchanged; rendered HTML is not snapshotted |
| `crates/quarto-highlight/tests/fixtures/user-grammar-equal-extent/` | **new** synthetic fixture: TOML grammar binary + a `highlights.scm` that double-captures one node, producing a genuine equal-extent collision to pin the `flatten_spans` tie-break (the corpus has none) |
| `crates/quarto-core/tests/integration/render_to_html_user_grammars.rs` | hand-recheck the `.contains()` `hl-` assertions (`:142,147`) — not a snapshot, won't auto-regenerate |
| `resources/scss/html/templates/highlight.scss` | unchanged; its 24 `.hl-*` roots are the source of truth that `code_legend_covers_render_css` (Phase 7, Defence 3) reads to lock editor↔render colour coverage together |
| `crates/quarto-lsp-core/Cargo.toml` | add `tree-sitter`, `tree-sitter-qmd`, `quarto-highlight` (re-exports `HighlightSpan` + `flatten_spans`) |
| `crates/quarto-lsp-core/src/tokens.rs` | **new** — extractor (structural + embedded), `Utf16LineIndex` (UTF-16 map **+ editor-only per-line split of cross-line spans**), tests incl. `every_query_capture_maps_to_a_legend_entry` (translator-completeness), `structural_captures_never_enter_interiors` (zone-1 exclusion, before the merge clip), `tokens_never_span_a_line` (LSP single-line invariant) |
| `crates/quarto-lsp-core/src/types.rs` | `SemanticToken`, `SemanticTokensJson`, `QMD_TOKEN_LEGEND` (`code.*` covers all 24 `hl-*` CSS roots), `capture_to_token_type`; `code_legend_covers_render_css` parity-coverage test (reads `highlight.scss`) |
| `crates/quarto-lsp-core/src/lib.rs` | re-export tokens API |
| `crates/tree-sitter-qmd/tree-sitter-markdown/queries/highlights.scm` | expand inline/block; stop covering code/frontmatter interiors |
| `crates/wasm-quarto-hub-client/src/lib.rs` | `lsp_get_semantic_tokens` export + `lsp_get_token_legend` (drift-guard oracle only, not on the registration path) |
| `hub-client/src/types/wasm-quarto-hub-client.d.ts` | declare both exports |
| `hub-client/src/services/intelligenceService.ts` | `getSemanticTokens`; checked-in `QMD_TOKEN_LEGEND` constant + `SemanticToken` type |
| `hub-client/src/services/monacoProviders.ts` | semantic-tokens provider (honours `CancellationToken` + `getVersionId()` staleness guard; returns `null` on cancel/error, never throws); re-bind symbol/folding to `'qmd'` |
| `hub-client/src/components/Editor.tsx` | register `'qmd'`, map `.qmd`→`'qmd'`, set Monarch base (Hybrid-A: markdown-derived **+ `nextEmbedded` routing of `{r}`/`{python}`/frontmatter to stock tokenizers, colour-aligned with the semantic palette** — see Phase 6), enable semantic highlighting, define+apply `quarto-{light,dark}` (rule tokens carry the `qmd.` super-prefix; rules exported as a constant for the namespace-invariant test — Phase 7) |
| `hub-client/src/services/semanticTokens.wasm.test.ts` | **new** — wasm round-trip test + legend drift guard |
| `hub-client/src/components/quartoTheme.test.ts` | **new** — namespace-invariant test: every exported `quartoThemeRules` token starts with `qmd.` (Phase 7, Defence 2) |
| `hub-client/changelog.md` | entry (two-commit workflow, per CLAUDE.md) |

## Open items / risks (not blockers)

- **Semantic-layer latency (perceived problem already handled).** Monaco calls
  `provideDocumentSemanticTokens` after every (debounced) edit; each call = 1 qmd
  parse + N embedded parses + serialize + JS decode + delta-encode. The
  **Monarch base (Hybrid-A) removes the *perceived* cost** — text is painted
  instantly and the semantic layer refines it asynchronously — so this is a
  throughput concern, not a flash concern. **Measure before optimising**
  (`QUARTO_PERF_STATS=1` against a geometrically-scaled large fixture, per
  `claude-notes/instructions/performance-profiling.md`); only then reach for the
  levers, in rough payoff order:
  - **Range provider** (`registerDocumentRangeSemanticTokensProvider`) — colour
    the viewport first. Biggest perceived win on large docs, but note the
    tree-sitter parse is whole-doc regardless, so this mainly trims off-screen
    embedded re-parses + serialize. **Hybrid-A promotes this lever:** because the
    inversion puts baseline correctness on the async layer, a slow whole-doc
    semantic pass strands the viewport on the (non-authoritative) base for longer
    — so colouring the visible range first matters *more* here than in a
    standard-model editor where TextMate already carries baseline correctness.
    Consider it the first reach if a large-fixture profile shows open/edit
    latency, ahead of the deeper incremental-parse change.
  - **Incremental tree-sitter re-parse** — keep the `Tree` across calls and feed
    the edit delta. Largest *actual* speedup on edits, but adds per-doc state to
    the WASM module (bigger change).
  - **`(content_hash → tokens)` cache** inside the WASM module (same shape as the
    `analyze_document` cache) — helps non-edit re-requests (focus, cursor moves),
    not edits.
  - **Drop the JSON round-trip on the output** — the draft (Phase 4) serialises
    tokens to a JSON string in Rust, the provider `JSON.parse`s it and then
    delta-encodes into a `Uint32Array` in JS (Phase 6) — the `serialize + JS
    decode + delta-encode` tail above. Moving the delta-encode into Rust and
    returning the `Uint32Array` (or a flat typed array) straight from WASM skips
    the JSON string, its copy across the boundary, and the JS parse.
    **Orthogonal to the three parse-cost levers** (it attacks transcode, not
    parse) and the **lowest-risk** of the set — payoff scales with token count,
    so it helps token-heavy docs regardless of parse time. Cheap enough to take
    pre-emptively if a profile shows serialize/decode (rather than parse)
    dominating; otherwise the Phase-6 JS delta-encoder is fine to ship first.
- **UTF-16 correctness.** Covered by `Utf16LineIndex` + a multibyte test;
  flagged because tree-sitter gives byte columns and getting this wrong only
  shows up on non-ASCII lines.
- **User grammars in the editor.** The render path supports user-provided
  tree-sitter grammars via a JS provider; this plan ships **built-in
  grammars only** in the editor. Editor support for user grammars is a clean
  follow-up (thread `JsUserGrammars` into `lsp_get_semantic_tokens`).
- **Crossref / callout highlighting.** Deferred — the Phase-1 grammar query
  adds no crossref/callout capture, so there is no legend row for it yet. Add
  the capture + a legend entry + a theme rule together when needed.
- **`.md` scope.** qmd-only. To extend to plain markdown later, register the
  `'qmd'` providers for `'markdown'` too and keep the extension gate.

## Proposed braid strands

- Parent feature: "Drive Monaco highlighting from tree-sitter, ref GH#10".
  - Link `related → bd-n7x2` ("Syntax highlighting design and implementation
    for Quarto 2"): this plan reuses `quarto-highlight`, the implementation
    product of that strand, and shares its capture-name taxonomy / design doc
    (`claude-notes/plans/2026-04-19-syntax-highlighting-design.md`).
  - Link `closes → bd-98k6`: Phase 0 switches the render producer onto
    `flatten_spans(highlight_captures(…))`, which fixes the `collect_spans`
    same-start over-wrapping bug on the HTML path (bd-98k6 fix-path (a)) — the
    golden `integration__golden__user_grammar_toml.snap` flips from
    `[0,14,"type"]` to node-exact. With the render path converged, bd-98k6 is
    resolved, not merely sidestepped; close it when Phase 0 lands.
- Sub-strands per phase, blocking the parent. **Phase 0 spans the shared
  resolver *and* the render-producer switch + golden regen** (bigger than the
  editor-only draft, but the regen is small — only \~4 of the 15 goldens change,
  all nested→flat; the equal-extent tie-break is exercised by the new synthetic
  `user-grammar-equal-extent` fixture, since no corpus golden contains a genuine
  tie) — call this out in the strand so the span-encoding golden review (and the
  hand-recheck
  of the `render_to_html_user_grammars` `.contains()` assertions) is expected.
- **Phases 4–7 (WASM export → intelligence service → Monaco provider → theme)
  collapse into one hub-client-integration strand**, not four: they co-land and
  are not independently verifiable (no visible colour until provider *and* theme
  both exist), so a strand per phase is pure bookkeeping. The phase headings in
  the implementation section stay the TDD step sequence *within* that strand.
- Dependencies: Phase 1 (grammar query) blocks Phase 2 (extractor); **Phase 0
  also blocks Phase 2** — the extractor calls `quarto_highlight::flatten_spans`
  (created in Phase 0) and its `tokens_are_non_overlapping_and_sorted` test
  exercises it. Phase 0 and Phase 2 both block Phase 3 (embedded layer); Phase 3
  blocks the hub-client strand (4–7); Phase 8 (e2e) blocks on Phases 0–3 + the
  hub-client strand. Phase 0 is independent of Phase 1, so *those two* can
  proceed in parallel; Phase 2 waits on both.
