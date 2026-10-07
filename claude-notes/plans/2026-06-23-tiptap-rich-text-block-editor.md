# Rich-text (tiptap) block editor for q2-preview — feasibility plan

**Date:** 2026-06-23
**Strand:** bd-sjb4pzx8
**Branch:** `braid/bd-sjb4pzx8-tiptap-rich-text-editor`
**Status:** Phase 0 (spike) in progress — go-ahead given 2026-06-23.
**Builds on / required reading:** `claude-notes/designs/2026-06-06-block-editing-design.md`
(the master spec for the current editor), plus the boundary-splice
(`2026-06-18/19`), track-me (`2026-06-13`), and glitch (`2026-06-15`→`18`) plans.

---

## Overview

Today, activating a block in `q2 preview` / quarto-hub replaces it with a
monospaced `<textarea>` showing the **raw qmd source slice** of that block's
byte range. The user edits raw markdown; on commit the text is parsed and
spliced back into the document.

This plan explores replacing **only that textarea UI** with a **tiptap /
ProseMirror rich-text editor**, so the user edits *formatted* text instead of
raw markdown — while keeping the entire detection, identity, splice, reconcile,
and incremental-write machinery **unchanged**.

### The confirmed architecture (user's framing)

The change is deliberately minimal and lives **entirely in TypeScript**:

```
                       ┌─────────────── UNCHANGED ───────────────┐
 detect byte range  →  seed editor with markdown slice
                       └──────────────────────────────────────────┘
                                       │
                       ┌─── NEW (this work) ───┐
                       │  markdown slice         │
                       │     → ProseMirror doc   │   (parse)
                       │     → rich editing      │   (tiptap UI)
                       │     → markdown text     │   (serialize)
                       └─────────────────────────┘
                                       │
                       ┌─────────────── UNCHANGED ───────────────┐
 commitTextEdit(markdown) → parse_qmd_content → apply_node_edit →
   reconcile → incremental_write → onContentRewrite → re-render
                       └──────────────────────────────────────────┘
```

We reuse the existing **text channel** (`commitTextEdit`) verbatim. The Rust
backend, the Automerge sync, the source-map layer, and the parent↔iframe
postMessage protocol **never know the difference** — they still receive a
markdown string keyed by a `SourceInfo` byte range.

### Design refinements (locked 2026-06-23, second iteration)

These two refinements sharpen the approach above and supersede the earlier
phrasing where they conflict:

1. **Seed from the AST subtree, not from re-lexing markdown.** We already have
   the untransformed Pandoc AST (with source byte-ranges) in the iframe. So the
   **input/seed path is `AST subtree → ProseMirror doc`**, walking typed Pandoc
   nodes — *not* parsing the markdown slice with a TS markdown library (which
   would risk markdown-dialect mismatches against pampa). Chip detection thereby
   stops being a lexing problem and becomes a typed-AST walk over data we already
   trust: opaque node types (`Math`, `RawInline`, `RawBlock`, `Cite`, shortcode
   nodes) become chips whose verbatim text is sliced from source by the node's
   byte range. **The commit path is unchanged: `PM doc → markdown text →
   commitTextEdit → parse-as-qmd`.** Only the *seed* direction moved from
   markdown-text to AST.

2. **Callout/nested divs need no block chip — we "reach into" them.** A
   `::: {.callout}` (or any non-section `Div`) is not editable *as a unit*, but
   its *inner* blocks carry their own source ranges, and the existing resolution
   model already opens an editing session on an inner block (`Descendable`
   reachability). So a tiptap session operates on an inner paragraph/list, not on
   the whole div. Consequence: **block-level chips are rare; the real passthrough
   need is *inline* chips** (shortcodes, math, `@crossref`, `[@cite]`, raw inline)
   inside an otherwise-editable paragraph. The spike's chip work focuses there.

> **Why the text channel and not the subtree (AST) channel?** The architecture
> also supports `commitSubtreeEdit`, which commits an edited Pandoc AST subtree
> directly (skipping the parse). A ProseMirror editor *could* map its doc to a
> Pandoc AST and use that channel, retiring even the markdown serializer. We are
> **deliberately not doing that** for the spike: the markdown round-trip keeps
> the surface tiny (one component, no new WASM entry point, no PM⇄Pandoc-AST
> bridge in Rust) and lets us reuse the parser as the single source of truth for
> qmd semantics. The AST channel is recorded here as a **future alternative**,
> not the v1 path.

### Decisions locked with the user (2026-06-23)

| Question | Decision |
| --- | --- |
| **Fidelity target (v1)** | **Prose-rich, Quarto-opaque.** Headings, bold/italic, links, lists, blockquotes, inline/fenced code render as true rich text. Everything Quarto-specific — shortcodes `{{< … >}}`, `::: {.callout}` divs + `{attrs}`, math `$…$`/`$$…$$`, `@crossref`/`@cite`, raw HTML/inline — becomes an **opaque "chip"**: an atomic ProseMirror node that renders as a non-editable pill and re-emits its **exact source token** on serialize. |
| **Rollout** | Experimental **branch** for now (fidelity gaps are acceptable while exploring). Ship behind an **opt-in flag** (mirroring the existing `unlockNestingCursor` pattern), defaulting to the textarea. **Eventual** requirement (not this spike): fall back to the textarea when the user needs to type syntax the rich editor can't represent, or to add new blocks. |
| **This session's deliverable** | A **throwaway round-trip spike**: a minimal, isolated harness proving `markdown → ProseMirror doc → markdown` (with chips) on real qmd fixtures, *before* any integration. De-risks the one genuine unknown. |

---

## The core feasibility question

Everything reduces to one bidirectional contract, living entirely in TypeScript:

> **markdown slice ⇄ ProseMirror doc**, such that:
> 1. A block's qmd slice parses into a faithful, *editable* rich representation.
> 2. Serializing the (possibly edited) doc back to markdown produces qmd that,
>    when re-parsed by pampa, yields the intended AST.
> 3. **Opaque constructs survive verbatim** as chips — a shortcode in, the same
>    shortcode bytes out.
> 4. **An unedited open-and-close is a semantic no-op** (see the dirty-guard
>    trap below).

The spike exists to answer: *how faithfully, and at what cost, can we do this?*

### What posit-assistant tells us (reference, not reuse)

`external-sources/assistant` uses tiptap, but as a **chat-message composer**:
paragraphs + bold/italic/strike/link + custom `@file`/`/command` chip nodes.
It **disables** headings, lists, blockquotes, code blocks, tables, and
hand-writes a line-based markdown serializer (no `prosemirror-markdown`).

- **What transfers:** the *React wiring* (`useEditor`, `EditorContent`,
  imperative ref handle, `clipboardTextSerializer`), and crucially the **chip
  pattern** — atomic inline nodes carrying a stored attribute, with
  `renderText()` re-emitting the original token. That is exactly our
  shortcode/math/crossref passthrough mechanism.
- **What does NOT transfer:** their scope. They never round-trip lists,
  headings, blockquotes, or any block structure, and they never touch
  Pandoc/Quarto constructs. **The hard part of our problem — faithful
  block-level qmd round-trip — is unsolved in their code.** We are not
  inheriting a solution; we are inheriting a wiring pattern.

This is the single most important feasibility caveat: *tiptap usage in the wild
(including posit-assistant) sidesteps full-markdown round-trip by scoping the
input. We cannot sidestep it — our editor opens on arbitrary existing qmd
blocks, including whole lists and blockquotes (the "locked tile" can be a
multi-line container).*

---

## Genuine cruxes the spike must resolve

These are the things that can sink the approach. The spike is designed to hit
each one.

### C1 — Serializer fidelity (the dominant risk)

`prosemirror-markdown`\'s default serializer is CommonMark-ish and lossy
(list-marker normalization, escaping `\_`/`\*`, blank-line collapsing, ATX vs
setext headings, hard-wrap handling). We need output that **re-parses to the
right AST**, not output that is byte-identical to the input. The acceptance bar
is therefore *semantic* (AST-equivalence after re-parse), not textual.

Open sub-questions for the spike:
- Is `prosemirror-markdown` (+ custom node/mark serializer overrides) good
  enough, or do we hand-roll a serializer (as posit-assistant did)?
- How do fenced code blocks with Quarto attributes (` ```{python} `) survive?
  (Likely a chip-bearing code node, since the info string is Quarto-specific.)

### C2 — The chip / passthrough model

Opaque tokens must survive untouched. Mechanism: a custom ProseMirror **atomic
node** (inline or block) whose attrs store the verbatim source string and whose
`renderText()`/serializer emits it back unchanged; non-editable, rendered as a
visually distinct pill.

The hard part is **detection at parse time**: given a markdown slice, identify
the spans that are shortcodes / math / raw / crossrefs / cites and lift them
into chips *before or during* the markdown→PM parse, rather than letting the
markdown parser mangle them. Options to evaluate in the spike:
- Pre-tokenize with a Quarto-aware pass (we may be able to reuse pampa's own
  tokenization via WASM to *find* these spans authoritatively, rather than
  re-implementing shortcode/math lexing in TS).
- A markdown-it plugin that recognizes the tokens and emits chip nodes.

### C3 — The dirty-guard trap (no-op fidelity)

Today, "open then close without typing" is a guaranteed no-op because the
textarea value is byte-identical to the slice (`normalize(draft).trimEnd() ===
anchorSlice`). A tiptap round-trip is **not** byte-identical, so a naive
"serialize and compare to slice" would mark *every* opened block dirty and
silently reformat it on close.

**Fix (design intent):** derive dirtiness from ProseMirror's own change signal
— compare the current doc against the seeded doc (`editor.state.doc.eq(initialDoc)`
or transaction `docChanged` accumulation), **not** from serialized-text
comparison. An unedited doc ⇒ cancel, commit nothing, regardless of how the
serializer would have re-rendered it. The spike must demonstrate this cleanly.

### C4 — Multi-block container slices

The "locked tile" the editor opens on can be a whole `BulletList`,
`OrderedList`, `BlockQuote`, or `DefinitionList` — i.e. the seeded markdown is
*several* blocks with list markers / `> ` prefixes, not one paragraph. The PM
doc must model these as real structured nodes (a real `bullet_list` with
`list_item`s) and serialize them back to correct qmd. This is precisely the
scope posit-assistant skips. The spike must include at least one list and one
blockquote fixture.

### C5 — Nested / prefixed buffers

For blocks nested in prefixing containers, the existing system already feeds the
editor a **prefix-stripped clean buffer** (via `regenerate_nested_buffers` /
`write_single_block`) so the user edits at column 0. Good news: tiptap inherits
this for free — it receives the same clean buffer the textarea would. No new
work, but the spike should confirm a clean buffer parses correctly.

### C6 — What we explicitly do NOT need (simplifications)

A large class of current pain **evaporates** because PM owns its own selection
model: caret/column projection, prefix-width math, tight trailing source ranges,
per-line provenance, last-visual-line geometry. None of that is needed for the
rich editor's *internal* caret. (It remains relevant only at the
parent/iframe identity boundary, which we are not touching.)

---

## What stays unchanged (explicitly out of scope for the spike)

- `SourceInfo` byte-range identity, `captureEditTarget`, the byte-offset anchor.
- `commitTextEdit` → `apply_node_edit` → reconcile → `incremental_write` (Rust).
- Self-heal / track-me relocation, the settle-gate, concurrency handling.
- Hover/activation/roving-tabindex, the measured wrapper box.
- The cross-surface (arrow-out) cursor *for v1* — though tiptap will need its
  own key handling eventually; the spike does not address inter-block cursoring.

---

## Phase plan

> **Phase 0 is the only sanctioned work for this session.** Phases 1+ are
> sketched so we can see the whole shape, but require explicit go-ahead.

### Phase 0 — Throwaway round-trip spike (this session, after go-ahead)

**Goal:** prove `markdown → PM doc → markdown` with chips, on real qmd, with a
*semantic* (re-parse-to-same-AST) acceptance bar. Throwaway = isolated, not
wired into the preview, deleted or quarantined after we read the results.

**Spike pipeline (AST-driven, authoritative end-to-end):**

```
qmd fixture → pampa parse → AST_in → [AST→PM bridge] → PM doc
           → [PM→markdown serializer] → md_out → pampa parse → AST_out
           → assert AST_in ≅ AST_out   (semantic round-trip)
```

**Tasks (TDD — tests first):**

- [x] Pick a home for the spike: `ts-packages/preview-renderer/src/q2-preview/`
  `tiptap-roundtrip-spike/` (vitest, node env). **Oracle = native `pampa`**
  (`-t json --json-source-location full`) shelled out via `child_process` —
  gives the source-tracked untransformed AST with **no WASM init**.
- [x] Assemble a **fixture corpus** (16 fixtures, `fixtures.ts`): plain para,
  inline formatting, ATX heading, bullet/ordered/nested lists, blockquote,
  plain + `{python}` code blocks, shortcode, inline/display math, crossref,
  citation, callout div, raw HTML inline.
- [x] Write the **acceptance oracle** (`roundtrip.test.ts` + `canonical.ts`):
  buckets each fixture `exact` / `equivalent` (reformatted) / `broken` by
  comparing source-stripped ASTs (exact) and whitespace-normalized ASTs
  (equivalent). Fails the suite only on `broken`.
- [x] Define the **ProseMirror schema** (`schema.ts`): `prosemirror-markdown`\'s
  schema + an atomic inline `chip` node (attrs `{src, kind}`).
- [x] Implement **AST → PM doc** (`astToPm.ts`): walks the Pandoc AST; opaque
  node types (`Math`/`Cite`/`Span.quarto-shortcode__`/`RawInline`, plus
  block `Div`/`RawBlock`/`Table` for the spike) become chips carrying their
  verbatim source slice (C2, refinement 1).
- [x] Implement the **chip** (atomic, verbatim serialize via `state.text(src,
  false)` — no markdown escaping).
- [x] Implement **PM doc → markdown** (`pmToMarkdown.ts`): default
  `prosemirror-markdown` serializer + the chip rule. **Zero custom node/mark
  overrides were needed beyond the chip** (C1 — see verdict).
- [ ] Demonstrate the **dirty/no-op signal** off `doc.eq(initialDoc)` (C3) —
  *deferred to Phase 1*; the spike validated the static round-trip, which is
  the harder unknown. C3 is a small, well-understood addition at integration.
- [x] Produce a **findings table** + written verdict (below; `RESULTS.md` holds
  per-fixture `qmd → md_out` evidence).

**Exit criteria:** ✅ met — see verdict below.

---

## Phase 0 RESULTS (2026-06-23) — verdict: **GREEN**

Ran 16 fixtures through `qmd → pampa AST → astToPm → prosemirror-markdown
serialize → pampa AST → compare`:

```
exact:      15
equivalent:  1   (blockquote: two soft-wrapped lines join into one — benign)
broken:      0
```

Chips fired exactly where expected: shortcode (1), inline math (1), display
math (1), crossref (1), citation (1), callout div (1), raw-HTML inline (2).
Byte-exact passthrough confirmed in `RESULTS.md`, e.g.:

- `{{< video https://youtu.be/abc >}}` → identical out
- `@fig-plot` → identical out
- `::: {.callout-note}\nThis is a note.\n:::` → identical out

**What this proves.** The core unknown — *can we faithfully round-trip prose-rich
\+ Quarto-opaque qmd through a ProseMirror document?* — is answered **yes**, with
evidence. The AST-driven seed (refinement 1) means chip detection is a trivial,
authoritative typed-AST walk, and `prosemirror-markdown`\'s **stock serializer
needed no per-node overrides** beyond the one chip rule. That resolves the
post-spike "library vs hand-roll" question (open Q #2) decisively in favor of
**`prosemirror-markdown` + the chip rule** — at least for this corpus.

**Honest caveats (what the spike did NOT prove).**

1. **This validates the model, not the tiptap *UX*.** The spike is headless
   ProseMirror (`prosemirror-model` + `prosemirror-markdown`). tiptap is the
   React editing layer on top; chips-as-pills, keyboard nav, IME, focus/blur
   commit, and the measured-wrapper fit are **Phase 1** and remain unproven.
2. **C3 (dirty/no-op) was designed, not exercised.** The blockquote
   `equivalent` result is exactly the whitespace-normalization that would cause a
   spurious reformat on open-and-close *unless* dirtiness is read from
   `doc.eq(initialDoc)`. Phase 1 must implement and test that.
3. **Block-chip for `Div` is a spike convenience.** Production reaches *into*
   the div (refinement 2) and edits inner blocks; whole-div chipping was used
   here only to measure round-trip, and pleasingly it also round-trips.
4. **Corpus gaps.** Tables, figures-with-captions, definition lists, footnotes,
   images-with-attrs, nested divs, multi-line raw blocks are not in the corpus;
   the bridge maps them to verbatim block-chips (safe passthrough, not richly
   editable — consistent with v1 scope). Fidelity there is untested.

**Spike location (throwaway):**
`ts-packages/preview-renderer/src/q2-preview/tiptap-roundtrip-spike/`
(`fixtures.ts`, `pampa.ts`, `schema.ts`, `astToPm.ts`, `pmToMarkdown.ts`,
`canonical.ts`, `roundtrip.test.ts`, `RESULTS.md`). Run:
`cd ts-packages/preview-renderer && npx vitest run src/q2-preview/tiptap-roundtrip-spike/`.
Deps added (dev): `prosemirror-model`, `prosemirror-markdown`. Quarantine/delete
before any non-experimental merge; `tsc --noEmit` is clean and the full
preview-renderer suite (473 tests) passes with it present.

### Phase 1 — WYSIWYG in-place editor behind an opt-in flag (design locked 2026-06-23)

**Goal of the experiment (user's framing):** the editable view should *look like
the rendered page* — editing a paragraph shows body font, real bold, real links,
not a monospaced box. See "how close we can get".

**The key enabler (confirmed by recon).** The editor mounts in the **same iframe**
as the rendered content, and that iframe already has the **full Bootstrap + Quarto
theme CSS** loaded (`<link data-q2-theme>` in `document.head`, applied in
`q2-preview/entry.tsx`). The renderer emits **clean semantic tags** (`<p>`, `<em>`,
`<strong>`, `<h2>`, `<a>`, `<ul>/<li>`) with no proprietary classes. So a
ProseMirror editor emitting the same tags is styled by the theme **for free** — we
emit matching DOM and let the cascade do the work; we do **not** reimplement theme
styling. The existing measured-box wrapper (`renderMeasuredEdit`, the
`q2-active-edit-region` div) is **structural** (prevents reflow) and **stays**; we
swap the monospaced `<textarea>` inside it for a tiptap editor and reset
ProseMirror's default editor chrome so it doesn't fight the theme.

**Dev loop.** `q2 preview --allow-edit` (no sync server; serves `{allowEdit:true}`
read by the SPA at boot). A new `?richText=1` opt-in is plumbed exactly like
`?nestingCursor=1`: `PreviewApp` parses it → `Q2PreviewIframe` prop → `UPDATE_AST`
payload → `PreviewContext` → dispatcher branch. Iterate + screenshot via the
chrome-devtools MCP.

**tiptap now enters** (the spike was deliberately headless ProseMirror). Phase 1
uses `@tiptap/react` (`useEditor`/`EditorContent`) and **React NodeViews for
chips**. The spike's `astToPm` seed and `prosemirror-markdown` serializer port over
essentially unchanged.

**Decisions locked (2026-06-23, third iteration):**

- **Staging: paragraph-first, then widen.** First cut targets single paragraphs
  only (pure inline editing — em/strong/link/code/chips, **no** structural splits)
  to prove the "looks like the page" feel + the commit loop in the real preview
  before taking on interactive structural edits.
- **Chips: source-text pills only (v1).** Every chip (shortcode, math, crossref,
  cite, raw) renders as a small pill showing its source token; always serializes
  verbatim. (Noted future option, explicitly out of v1: render inline math via the
  already-loaded **KaTeX** / show resolved crossref text, to match the page while
  staying atomic. Deferred.)

**Sub-phases:**

- **1a — Paragraph-only WYSIWYG proof.** Wire the `richText` flag end-to-end;
      branch the `Block` dispatcher so a `Para` edit-target renders a tiptap editor
      (seeded from the `sourceNode` via `resolveSource`, i.e. AST→PM) inside the
      existing measured box; emit page-matching DOM; commit via the **unchanged**
      `commitTextEdit` path; implement the **`doc.eq(initialDoc)` dirty guard (C3)**
      and the stale-target/self-heal blur guards. Drive with chrome-devtools;
      capture before/after screenshots proving the editor matches the rendered
      paragraph. Round-trip unit tests for inline edits.
  - [x] **Production round-trip core** (`src/q2-preview/richtext/`): `schema.ts`
        (tiptap-named PM schema + `chip`), `ast.ts`, `astToProseMirror.ts`
        (`astToDoc`), `serializer.ts` (`docToMarkdown`). Plus shared test oracle
        `src/test-utils/pampaOracle.ts` and `richtext/roundtrip.test.ts` (13
        fixtures, **all pass**). Gate = SEMANTIC equivalence (no dropped/changed
        nodes), with byte-exactness informational per user guidance (imperfect
        round-trip OK, dropped nodes not). Two qmd-specific serializer choices
        landed: italic→`_` (avoids qmd-disallowed `***`), list `tight` from AST
        (Plain vs Para items). Oracle compares inline marks as a *flat set*
        (ProseMirror's model) so `[**x**](u)`≡`**[x](u)**`. tsc clean.
  - [ ] tiptap `Chip` node extension (React NodeView pill).
  - [ ] `RichTextEditor.tsx` (tiptap editor seeded via `astToDoc(...).toJSON()`;
        commit via `docToMarkdown` + `commitTextEdit`; `doc.eq` dirty guard;
        Esc/Cmd-Enter/blur + stale-target guard; Para-only, no structural splits).
  - [ ] Plumb `richText` flag: `PreviewContext` → `Q2PreviewIframe` →
        `UPDATE_AST` payload → `PreviewApp` `?richText=1` (mirror `unlockNestingCursor`).
  - [ ] Dispatcher branch: `Block` renders `RichTextEditor` (vs `EditTextarea`)
        for `Para` when `ctx.richText`.
  - [x] Browser verification (`q2 preview --allow-edit` + `?richText=1`,
        chrome-devtools). **Verified end-to-end 2026-06-23.** Clicking a paragraph
        opens a tiptap editor that is visually identical to the rendered block
        (bold/italic/code/link styled by the theme — same-iframe cascade), just
        with a caret; DOM snapshot confirmed a real ProseMirror contenteditable
        with `<strong>/<em>/<code>/<a>` nodes. A real edit (inserting bolded
        "more") committed through the **unchanged** `commitTextEdit` path and
        wrote clean qmd to disk — `(and **more**!)` — with the rest of the
        paragraph (`**bold text**`, `*italic text*`, `` `inline code` ``,
        `[hyperlink](…)`) round-tripped **byte-clean**. Evidence:
        `claude-notes/richtext-shots/{01-rendered,02-editing-para1}.png`.

**Phase 1a is COMPLETE and verified.** The thesis holds: the editable view looks
like the page, edits are rich-text, and write-back is faithful qmd through the
existing pipeline. Next: 1b (headings + inline polish), 1c (lists/blockquotes +
interactive structural-edit round-trip tests).

### 1a follow-ups (post-completion, all verified in `q2 preview`)

- **Edit-mode tint** — active editor gets a subtle blue tint + ring (the faithful
  WYSIWYG was *too* faithful to tell it was live). Zero text shift.
- **Left-margin affordances** — "Editing…" label + a **rich/plain editor toggle**
  parked in the left margin (absolute, off the text, so it never hijacks
  clicking/selecting). The toggle is the in-place **escape hatch to the textarea**
  for syntax the rich editor can't express (a Phase-2 idea brought forward).
  Shared by both surfaces via `renderMeasuredEdit`; session-sticky `editorMode`
  in `PreviewRoot`. A `editorModeSwitchRef` guard stops the surface swap from
  triggering a blur-commit/close; rich→plain hands content across via
  `editDraftRef` (dirty-aware, so an untouched toggle never reformats). Files:
  `richtext/EditAffordance.tsx`, `richtext/styles.ts`, `dispatchers.tsx`,
  `PreviewRoot.tsx`, `PreviewContext.tsx`. Evidence:
  `richtext-shots/{07-toggle-rich,09-toggle-plain-fixed}.png`.
- **Chip source via `.l`** — chip text now slices from a node's literal `.l`
  location, not the compact pool entry (which is mis-assigned for shortcode
  spans). Shortcode chips render verbatim `{{< meta key >}}`. (Also filed
  bd-u145dg3y: warn when a block-level shortcode is used inline.)
- **Known limitation (Phase 2):** plain→rich re-seeds the rich editor from the
  original AST (it can't parse arbitrary edited markdown in-iframe). rich→plain
  preserves edits. Full bidirectional content handoff needs the parent parse.
- **1b — Headings + inline-formatting polish** (toolbar-free; rely on standard
      marks + theme styling). Tighten visual parity (margins, line-height).
  - [x] `Header` added to `RICHTEXT_SUPPORTED_TYPES`; heading node enabled in the
        tiptap StarterKit config. The bridge already mapped `Header → heading{level}`,
        so the round-trip was ready (added a `heading-with-marks` fixture, 14/14).
  - [x] `enableInputRules: false` / `enablePasteRules: false` — 1b edits existing
        structure only; typing "## " must not convert a paragraph or change a
        heading's level (structural edits are a later phase; Cmd-B/I still work).
  - [x] `trailingNode: false` — a single non-paragraph block (a heading) was
        getting a phantom empty trailing `<p>` (extra editor height + a stray blank
        block on commit). Verified the heading box is now tight in `q2 preview`.
        Evidence: `richtext-shots/12-heading-tight.png`.
  - [x] **Formatting toolbar** (`RichTextToolbar.tsx`) — a small box floating above
        the top-left of the edit box: **B / I / S / x₂ / x² / 🔗**. Mark buttons call
        `toggleMark` (same command as Cmd-B/I), highlight via `isActive`, and use
        mousedown-preventDefault so clicking never collapses the selection. Verified
        end-to-end: selecting a word + Bold writes `**word**` to disk.
        Evidence: `richtext-shots/13-toolbar.png`.
  - [x] **Subscript / superscript as real marks** — added `@tiptap/extension-{sub,super}script`,
        schema marks, serializer (`~x~` / `^x^`), and AST mapping (Pandoc
        `Subscript`/`Superscript` → marks, no longer chips). Round-trip fixture
        `sub-sup-strike` green.
  - [x] **Link create/edit** — toolbar 🔗 opens a URL input; `setLink` /
        `extendMarkRange('link')` / `unsetLink` (edit/remove an existing link by
        placing the cursor in it). Focus scope reworked: the editor commits only on
        `focusout` leaving the whole `.q2-richtext-editor`, so focusing the link
        input keeps the session open. The TS side is correct (commit log shows the
        right markdown), **but** a downstream write-back bug corrupts a new link's
        URL when the paragraph has another link — filed **bd-3zp3z4jx** (shared
        text-channel bug, affects the textarea editor too; single-link edits fine).
- [ ] **1c — Lists / blockquotes, incl. interactive structural edits** (Enter
  split, new item, backspace-merge) with round-trip tests for *interactive*
  edits (new surface beyond the spike's static round-trip).
- [ ] **1d — Chip UX** (pills; read-only in v1) and the first seam toward the
  Phase-2 textarea fallback.

**Risks / open tensions for Phase 1:**

- **Exact visual parity is a long tail.** \~95% free for prose; pixel-exact needs
  iteration, and attributed blocks (`{.lead}`, classed lists) only match if we
  carry their classes onto the editor's root node.
- **Interactive structural edits are new** (1c). ProseMirror creates/splits nodes
  natively; the serializer must produce correct qmd for those — proven only
  statically so far. This also intersects Phase 2's "fall back to textarea for new
  blocks".
- **Chip editing.** A user fixing a typo inside a shortcode/math can't, in v1
  (chips are read-only) — that routes through the Phase-2 textarea fallback.

### Phase 2 — Fallback + new-block authoring (future)

- Escape hatch: when the user needs unsupported syntax, to edit a chip's source,
  or to add a new block, drop from the rich editor to the textarea (mode toggle,
  or auto-fallback on detecting an unsupported edit).
- Inter-block cursoring (arrow-out) under the rich editor.

### Phase 3 — (Stretch) AST channel (future, possibly never)

- Investigate committing via `commitSubtreeEdit` (PM doc → Pandoc AST),
  retiring the markdown serializer entirely. Only if Phase 0/1 expose
  serializer-fidelity pain that an AST bridge would resolve.

---

## Open questions — RESOLVED (2026-06-23, second iteration)

1. **Chip granularity:** ~~block chip for divs?~~ **Resolved:** no block chip for
   callout/divs. We reach into the div and edit inner blocks (refinement 2).
   Chips are primarily *inline*.
2. **Serializer choice:** **Resolved (deferred by design):** start with
   `prosemirror-markdown` + overrides; reassess as a hand-roll only if the spike
   shows we're fighting the library constantly. Decision is evidence-driven,
   post-spike.
3. **Chip detection source of truth:** **Resolved:** walk the **AST in TS**
   (we already have the untransformed AST JSON). No markdown-it (dialect-mismatch
   risk), no extra pampa call for detection (refinement 1).
4. **Fixture corpus sign-off:** **Resolved:** corpus above is sufficient for the
   spike.

## Remaining open questions (post-spike)

- Serializer library vs hand-roll (decide on spike evidence — see #2 above).
- Whether/how to later move to the AST commit channel (Phase 3).

---

## References

- Master spec: `claude-notes/designs/2026-06-06-block-editing-design.md`
- Boundary-splice (pending): `claude-notes/plans/2026-06-18-boundary-splice-edit-design.md`,
  `claude-notes/plans/2026-06-19-boundary-splice-implementation.md`
- Track-me relocation: `claude-notes/plans/2026-06-13-track-me-node-relocation-successor.md`
- Frontend: `ts-packages/preview-renderer/src/q2-preview/{dispatchers.tsx,
  useBlockEditHover.tsx, PreviewContext.tsx, PreviewRoot.tsx, usePreviewEdit.ts,
  sourceIndex.ts}`
- Backend (unchanged): `crates/pampa/src/{apply_node_edit.rs, node_lookup.rs,
  regenerate_nested_buffers.rs}`, `crates/pampa/src/writers/incremental.rs`,
  `crates/quarto-ast-reconcile/`
- tiptap reference (wiring + chip pattern only): `external-sources/assistant/
  packages/ui/src/{TiptapInput.tsx, utils/tiptap-serialization.ts}`
