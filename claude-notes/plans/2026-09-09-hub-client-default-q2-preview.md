# hub-client: `q2-preview` becomes the default renderer; the full-DOM renderer gets its own format name

**Strand:** bd-kltzdhle
**Date:** 2026-09-09
**Status:** Executing (go-ahead 2026-09-09, branch `braid/bd-kltzdhle-hub-client-make-q2`). Phase 1 done; Phase 2 in progress.
**Related:** PR #667 / bd-ew0vak6b (Edit pill; plan
`2026-09-09-q2-preview-edit-toggle.md`), bd-zvh2p (attribution through
`render_page_for_preview` — superseded by D3 below), bd-j3764r9a (React ↔
HTML DOM parity epic — the gaps users will now see by default),
bd-uy4uygha (`2026-07-01-html-format-capture-display.md`, which first
described the hub-client-only "plain html preview" mode this plan retires
as the default).

## Overview

`q2 preview` renders every `format: html` document (explicit or implied by
a missing `format:` key) through the q2-preview React renderer. hub-client
does not: the same document goes to the **full-DOM renderer** (`Preview.tsx`
+ `MorphIframe`), which morphs a complete HTML render into an iframe and has
no comments, attribution, or block-editing chrome. Only documents that opt in
with `format: q2-preview` get the React renderer in hub-client.

This plan brings `q2 preview`'s behaviour to hub-client:

1. **Default = q2-preview.** A document with no `format:` key, `format: html`,
   or `format: html: {…}` renders through `Q2PreviewIframe` in hub-client,
   with comments / Edit / Authors available exactly as for `format: q2-preview`
   today.
2. **The old renderer keeps existing under an explicit name.** A document that
   declares `format: q2-html-render` (see D1) renders through
   the full-DOM `MorphIframe` path, unchanged from today's default.
3. `q2 preview` (the SPA) does not gain a full-DOM renderer. `q2-html-render`
   is a recognised format everywhere (so `q2 render` produces HTML for it,
   like `q2-debug` does), but the SPA reports it as "no live preview for this
   format" rather than silently showing nothing (D5).

## What the source study found

### The renderer decision is a TS whitelist applied *before* the render

`hub-client/src/components/render/PreviewRouter.tsx` is the single fork. On
every content change it parses the document (`parseQmdToAst`,
`PreviewRouter.tsx:124`), reads `meta.format` from the returned AST, and asks
`getQ2Format` (`hub-client/src/components/render/getQ2Format.ts:14`) whether
the React side handles it:

```ts
if (formatStr.startsWith('q2-') || formatStr === 'revealjs') return formatStr;
return null;   // → <Preview> (full-DOM MorphIframe)
```

`reactFormat != null` → `<ReactPreview format=…>`; otherwise `<Preview>`
(`PreviewRouter.tsx:170-180`). The resolved format is also echoed to
`Editor.tsx` (`onFormatChange`, `Editor.tsx:378-383`), which gates chrome on
it: Edit pill `currentFormat !== 'q2-preview'` (`Editor.tsx:1444`), Authors
pill `q2-debug | q2-preview` (`:1448`), slide thumbnails `q2-slides` (`:392`),
and the printable-document affordance `q2-preview | q2-slides | revealjs`
(`FileSidebar.tsx:187-195`).

The `meta.format` string the router reads is already normalised by Rust:
`detect_format_from_content` (`crates/wasm-quarto-hub-client/src/lib.rs:725`)
returns `"html"` for a missing key, a bare `html`, a `format: html: {…}` map
(first key), and a list; `MetadataMergeStage` then overwrites `meta.format`
with `Format::target_format` (`metadata_merge.rs:453-469`). `_quarto.yml`'s
project-level `format:` is not consulted anywhere on the hub-client path.

### Inside `ReactPreview`, the render call re-detects the format from content

`ReactPreview.doRender` (`ReactPreview.tsx:230-`) chooses the WASM entry
point by format string:

| hub-client format | entry point | `prefer_preview_format` | `RenderHost` |
|---|---|---|---|
| `q2-preview` | `render_page_in_project_with_attribution` (`lib.rs:1211`) | `false` | `HubClient` |
| `revealjs` | `render_page_for_preview` (`lib.rs:1318`) | `true` | `NativePreview` |
| `q2-debug`, `q2-slides`, `q2-sandboxed-preview` | `parse_qmd_to_ast_with_attribution` (parse-only) | — | — |

Every WASM render entry point **re-reads the front matter itself**
(`detect_format_from_content` at `lib.rs:1474` / `:1664`) and dispatches on
`Format::pipeline_kind` (`lib.rs:1513`, `:1702`). So routing an `html`
document to `ReactPreview` with `format='q2-preview'` is not enough: with
`prefer_preview_format=false` the WASM would detect `html`, take the HTML
branch, and return `html` instead of `ast_json`. The `q2 preview` SPA avoids
this because `render_page_for_preview` passes `prefer_preview_format=true`,
which applies:

```rust
fn map_format_for_preview(format_str: &str) -> &str {        // lib.rs:693
    match format_str { "html" => "q2-preview", "revealjs" => "q2-slides", other => other }
}
```

hub-client cannot simply call `render_page_for_preview`: it hard-codes
`RenderHost::NativePreview` (`lib.rs:1372`), which suppresses the Q-5-12
"pre-render scripts will not run" warning that is *correct* for hub-client
(`render_scripts.rs:162-175`). It also has no `attribution_json` argument
(bd-zvh2p).

### Where pseudo-formats are registered

One table, `builtin_pseudo_format` (`crates/quarto-core/src/format.rs:112-131`):

```rust
"q2-slides"            => Some(("html", Some("preview"))),
"q2-debug"             => Some(("html", None)),
"q2-preview"           => Some(("html", Some("preview"))),
"q2-sandboxed-preview" => Some(("html", None)),
```

plus companions `lua_format_for` (`format.rs:166`) and, in the WASM crate,
`coerce_format_for_print` (`lib.rs:714`). The JS mirror is
`pipelineKindForFormat` (`ts-packages/preview-runtime/src/pipelineKind.ts:27`).
Verified end-to-end today: `q2 render` on a `format: q2-debug` document writes
a normal HTML file; on `format: q2-html-render` it fails with
`Unknown format: q2-html-render`.

### What the e2e suite assumes

`hub-client/e2e/helpers/previewExtraction.ts:24-30` names three iframe kinds
(`'html' | 'q2-debug' | 'q2-preview'`); `'html'` selects the MorphIframe
(`iframe.preview-active`) and is the default. `smoke-all.spec.ts:126-132`
maps a fixture's `_quarto: tests:` format to a kind with `else → 'html'`.
Of the 173 smoke-all fixtures, 138 carry an `html:` test spec and 40 have no
`format:` key at all; all of those will render in `Q2PreviewIframe` after
this change. Assertions are mostly renderer-independent —
`ensureFileRegexMatches` runs a separate in-page `render_page_in_project`
call and matches the HTML *string* (`smokeAllAssertions.ts:111`) — but
`ensureHtmlElements` queries the **iframe DOM** (`smokeAllAssertions.ts:136`);
48 html-spec fixtures use it and will now be checked against the q2-preview
DOM. Three other specs assert on `iframe.preview-active` for plain fixtures
(`search.spec.ts:53`, `import-zip.spec.ts:113`, `project-loading.spec.ts:51`),
and `q2-preview-click-to-editor-scroll.spec.ts:328-375` deliberately compares
the two renderers.

### What users will notice (known q2-preview gaps become the default)

These are open strands on the React renderer that `format: html` documents
did not hit before. None is made worse by this plan, but each becomes
visible to everyone instead of only to `format: q2-preview` opt-ins.
`format: q2-html-render` is the per-document escape hatch until they close.

- bd-b3oq2fsy — user-declared `css:` not applied in VFS mode
- bd-47afd5ro — tabsets not rendered as a component
- bd-3cpv7dah — draft-alert banner missing
- bd-tqijrhsu — `toc-location` left/body variants
- bd-sm314r1x (deferred) — KaTeX in preview vs MathJax in render
- bd-c3dtpe36 / bd-e3m3rkik — mermaid component / copy-button chrome
- bd-ddfyqmfm — relative links to non-renderable files blank the iframe
- bd-2yd37vuk — no `#quarto-header` wrapper
- the "preview parity:" family under bd-j3764r9a (attribute drops, `<s>`
  vs `<del>`, callout titles, float markup, localisation)

Also a UX change, not a gap: with `previewEditing` defaulting to `true`
(PR #667, D2 there), every plain document becomes click-to-edit in the
preview by default. See D6.

## Design decisions (to iterate on)

- **D1 — Name: `q2-html-render` (DECIDED 2026-09-09).** Registered as a builtin
  pseudo-format with base `html` and `pipeline_kind: None`, i.e. identical to
  `q2-debug` in the pipeline; only the hub-client router treats it specially.
  *Rationale (user):* `q2-full-render` does not say what "full" means;
  `q2-html-render` is a pleonasm, but it names what you get, and if the
  awkward name discourages people from reaching for it, good. It is a
  format of last resort for the odd case that truly needs the full HTML DOM
  in the preview; going forward, live previews should be based on the React
  renderers as much as possible. *Rejected:* `q2-full-render`,
  `q2-dom-preview`.

- **D2 (DECIDED 2026-09-09) — The `html → q2-preview` substitution for hub-client lives in the TS
  router, mirroring Rust.** `getQ2Format` becomes:

  ```ts
  if (formatStr === 'q2-html-render') return null;   // explicit full-DOM renderer
  if (formatStr === 'html') return 'q2-preview';     // hub-client default == q2 preview default
  if (formatStr.startsWith('q2-') || formatStr === 'revealjs') return formatStr;
  return null;                                        // pdf, docx, acm-html, …: full-DOM fallback
  ```

  with a comment naming `map_format_for_preview` (`lib.rs:693`) as the
  Rust counterpart, the same way `pipelineKind.ts` names
  `builtin_pseudo_format`. *Rationale:* the router needs the answer before
  any render call, and today's probe already returns the normalised
  `meta.format`; keeping the probe unchanged means
  `formatDetection.wasm.test.ts` (the `parse_qmd_to_ast` contract) is
  untouched. *Alternative considered:* a new cheap WASM export
  `resolve_preview_format(content)` returning the mapped format, replacing
  the full parse in the router. It would make Rust the single source of
  truth and is cheaper per keystroke, but the mapped string is ambiguous for
  reveal decks (`revealjs → q2-slides`, while hub-client routes explicit
  `q2-slides` to the legacy `SlideAst` carousel), so it would either need a
  second Rust mapping table or the q2-slides migration (Plan 2E, still
  DRAFT). Not worth coupling to this change. Follow-up strand if wanted.

- **D3 (DECIDED 2026-09-09) — hub-client's preview render call gets the `prefer_preview_format`
  knob.** `render_page_in_project_with_attribution` gains a trailing
  `prefer_preview_format: bool` parameter (JS `boolean`; omitted →
  `false`, so every existing caller is byte-identical). It keeps
  `RenderHost::HubClient`. `ReactPreview.doRender` passes `true` for every
  `usesPreviewPipeline` format, and the `revealjs` branch switches from
  `render_page_for_preview` to this same call — which incidentally gives
  reveal decks in hub-client attribution (closing bd-zvh2p) **and** the
  correct `RenderHost` (they currently miss the Q-5-12 warning). After this,
  `render_page_for_preview` is SPA-only, as its doc comment already says.
  *Alternative:* add `attribution_json` + a host argument to
  `render_page_for_preview` and switch hub-client to it. Two arguments
  instead of one, and two entry points with overlapping meaning; rejected.

- **D4 (DECIDED 2026-09-09) — Routing rule for non-html formats is unchanged.** `pdf`, `docx`,
  `typst`, extension formats (`acm-html`) keep going to the full-DOM
  renderer, which renders them through the HTML pipeline as it does today.
  This is the one place hub-client stays *more* capable than `q2 preview`
  (whose SPA logs `renderPageInProject failed` for these). Widening the
  substitution to html-based extension formats is a separate question for
  both hosts and is out of scope. *User note:* how extension formats
  should preview is a real open question, but Q2 does not yet know enough
  about how non-html formats will work generally to decide it here. No
  epic for non-html / extension formats exists in the skein as of
  2026-09-09 (checked), so the question is parked as bd-vhd3lugq (type
  `question`, discovered-from this strand) rather than on an epic.

- **D5 (DECIDED 2026-09-09; message in scope) — `q2 preview` does not get a full-DOM renderer.** Per your note,
  `q2 render` covers that need. The SPA's render else-branch
  (`q2-preview-spa/src/PreviewApp.tsx:1195`) gets an explicit case: a
  successful response carrying `html` but no `ast_json` renders a short
  message ("`format: <name>` has no live preview in `q2 preview`; run
  `q2 render`"), instead of `console.error` + a frozen last-good view. This
  also improves the existing behaviour for `format: pdf`. *Alternative:*
  leave the SPA alone (pure scope discipline). Recommend the message: it is
  ~20 lines and it is the first thing a user trying the new format name in
  the CLI will hit.

- **D6 (DECIDED 2026-09-09) — Keep `previewEditing` defaulting to ON.** That was the decision in
  PR #667 (D2 there) and the pill makes it a one-click, persisted opt-out.
  Flagging because the audience widens from opt-in `q2-preview` documents to
  every document. *Alternative:* flip the schema default to `false` in the
  same change, so the wider audience gets plain links by default and opts
  into editing. Your call; it is a one-line schema change either way.

- **D7 (DECIDED 2026-09-09) — Rename the e2e iframe kind `'html'` to `'q2-html-render'`.** The
  kind names the *renderer*, not the document format; after this change
  `'html'` would be actively misleading (an html document renders in the
  q2-preview iframe). ~10 sites, mechanical.

- **D8 (DECIDED 2026-09-09, as an intermediate step only) — e2e smoke fixtures whose `ensureHtmlElements` selectors fail on
  the q2-preview DOM are skip-listed with a strand, not rewritten.** The
  e2e suite should exercise what users get by default. For each failing
  fixture: if the selector failure is a genuine parity gap, file (or link)
  a strand under bd-j3764r9a and add the fixture to an explicit
  `HTML_RENDER_ONLY` set in `smokeAllDiscovery.ts` (precedent:
  `SKIP_PRINTS_MESSAGE`, same file) carrying the strand id; if the selector
  is simply HTML-writer-specific and the q2-preview equivalent is obvious,
  add the equivalent selector to the fixture. *Alternative:* have the runner
  inject `format: q2-html-render` into every html-spec fixture at upload
  time, preserving all current assertions verbatim — but then the suite
  never exercises the default path. Rejected. The count is unknown until
  Phase 3 lands (upper bound 48). *User amendment:* the skip-list is
  acceptable only as a staging device. We rely heavily on tests and do not
  want a habit of reducing coverage, so the skip-listed fixtures are worked
  off in follow-up commits on the same PR (Phase 4b) — either by fixing the
  parity gap or by adding the q2-preview-equivalent selector — with the
  goal that the PR merges with the skip-list empty or nearly so.

## Behaviour matrix after the change

| front matter | hub-client renderer | chrome (comments / Edit / Authors) | `q2 preview` | `q2 render` |
|---|---|---|---|---|
| *(none)* / `format: html` / `format: html: {…}` | `Q2PreviewIframe` | yes | `Q2PreviewIframe` (unchanged) | HTML (unchanged) |
| `format: q2-preview` | `Q2PreviewIframe` (unchanged) | yes | unchanged | HTML (unchanged) |
| `format: q2-html-render` | full-DOM `MorphIframe` | no | "no live preview" message (D5) | HTML (new; like `q2-debug`) |
| `format: revealjs` | `Q2PreviewIframe` + reveal shell (unchanged); now with attribution | yes | unchanged | unchanged |
| `format: q2-debug` / `q2-slides` / `q2-sandboxed-preview` | unchanged | unchanged | unchanged | unchanged |
| `format: pdf` / `docx` / `acm-html` / … | full-DOM `MorphIframe` (unchanged) | no | unchanged (fails; now with the D5 message) | unchanged |

## Work items

TDD throughout: each phase lists its tests first; run them, watch them fail
for the right reason, then implement. Full-workspace tests + `cargo xtask
verify` (full, not `--skip-hub-build`: `quarto-core` and the WASM crate both
change) at each phase boundary; commit at each clean boundary.

### Phase 1 — Register `q2-html-render` (Rust)

- [x] Tests, `crates/quarto-core/src/format.rs`: `test_from_format_string_q2_html_render`
      (identifier `Html`, `target_format` preserved, `pipeline_kind: None`,
      `output_extension: "html"`); extend `test_lua_format_for_maps_preview_pseudo_formats`,
      `test_format_lua_format_canonicalizes`, and
      `test_lua_format_helpers_agree_on_shared_cases` with the new name.
- [x] Add `"q2-html-render" => Some(("html", None))` to `builtin_pseudo_format`
      and the name to `lua_format_for`; update both doc comments and the
      `from_format_string` doc list.
- [x] `crates/wasm-quarto-hub-client/src/lib.rs`: add `q2-html-render` to
      `coerce_format_for_print` (printable version is plain HTML). Check
      `map_format_for_preview` needs nothing (falls into `other`).
- [x] Grep for other exhaustive pseudo-format lists (`q2-sandboxed-preview`
      is the tracer: `grep -rn 'q2-sandboxed-preview' crates ts-packages hub-client q2-preview-spa`)
      and add the new name wherever the list is meant to be exhaustive.
- [x] End-to-end (2026-09-09): `cargo run --bin q2 -- render <scratch>/fmt/htmlrender.qmd`
      on a document whose front matter is `format: q2-html-render` wrote
      `htmlrender.html` + `htmlrender_files/`; inspected output contains
      `<title>Full DOM opt-out</title>`, `<main class="content" id="quarto-document-content">`
      and `<p>Hello <em>world</em>.</p>`. Before Phase 1 the same command
      failed with `Error: Unknown format: q2-html-render`.

### Phase 2 — `prefer_preview_format` on the hub-client WASM entry point

- [x] Tests, hub-client WASM tier (`hub-client/src/services/*.wasm.test.ts`,
      new file `previewFormatSubstitution.wasm.test.ts`; 7 cases, ran red
      against the pre-knob WASM: a/e/f failed with no `ast_json`, b/b'/c/d passed): through
      `renderPageInProjectWithAttribution` on a VFS project,
      (a) no-`format:` doc with `preferPreviewFormat=true` → `ast_json` set,
      `html` absent; (b) same doc with the flag omitted → `html` set,
      `ast_json` absent (today's contract, pinned); (c) `format: q2-html-render`
      with `true` → `html`; (d) `format: q2-preview` with either → `ast_json`;
      (e) `format: revealjs` with `true` → `ast_json` and `is_slides: true`;
      (f) the Q-5-12 warning still appears for a project with
      `pre-render:` when the flag is `true` (host stays `HubClient` — this is
      the reason D3 rejects `render_page_for_preview`).
- [x] `lib.rs`: add `prefer_preview_format: Option<bool>` (explicit `None` for an omitted JS arg) as the last parameter of
      `render_page_in_project_with_attribution`, threaded into its two
      `render_single_doc_to_response` / `render_project_active_page_to_response`
      calls (`lib.rs:1260`, `:1285`) in place of the literal `false`; update its doc
      comment (the "hub-client keeps using `render_page_in_project` so its
      existing format dispatch is unchanged" sentence at `lib.rs:1303` and
      the matching one in `wasmRenderer.ts:552` are now false — rewrite them).
- [x] `ts-packages/preview-runtime/src/wasmRenderer.ts` (the `.d.ts` does not
      declare this entry point, nothing to add there): add the optional `preferPreviewFormat?: boolean`
      parameter to `renderPageInProjectWithAttribution`; document that
      omitting it preserves the old behaviour.
- [x] `cd hub-client && npm run build:wasm`; the new file + `renderScriptsWarning` +
      `formatDetection` WASM tiers: 20/20 green.

### Phase 3 — hub-client routing and chrome

- [ ] Tests first:
  - new `hub-client/src/components/render/getQ2Format.test.ts` covering
    the full rule in D2 (`html → q2-preview`, `q2-html-render → null`,
    `q2-preview`/`q2-debug`/`q2-slides`/`revealjs` pass through,
    `pdf`/`acm-html`/missing → `null`);
  - `ts-packages/preview-runtime/src/pipelineKind.test.ts`: `q2-html-render → undefined`;
  - `ReactPreview.capture.integration.test.tsx`,
    `ReactPreview.rerender.integration.test.tsx`,
    `ReactPreview.editToggle.integration.test.tsx`: assert the render mock
    is called with `preferPreviewFormat === true` for `q2-preview` **and**
    `revealjs`, and that `renderPageForPreview` is no longer called from
    hub-client;
  - a `PreviewRouter` integration test (there is none today; model it on
    `ReactRenderer.integration.test.tsx:189-292`): no-`format:` content
    mounts `ReactPreview` with `format='q2-preview'` and reports
    `onFormatChange('q2-preview')`; `format: q2-html-render` mounts
    `Preview` and reports `null`.
- [ ] `getQ2Format.ts`: implement D2; update the file comment.
- [ ] `ReactPreview.tsx`: `doRender` uses `renderPageInProjectWithAttribution(…, true)`
      for every `usesPreviewPipeline` format; delete the `revealjs`-only
      `renderPageForPreview` branch (`ReactPreview.tsx:263-`); keep
      `isSlidesPreview` for the slide-shell props. Check `handleSetAst`
      (`:820`) needs no change (it dispatches on the format string, which is
      `q2-preview` for html docs).
- [ ] `Editor.tsx` gates (`:392`, `:1444`, `:1448`) and
      `FileSidebar.tsx:187-195` need no code change — verify with the router
      test that `currentFormat` is `q2-preview` for a plain document, so the
      Edit and Authors pills enable and the printable affordance appears.
- [ ] `ReactRenderer.tsx:218` render-components gate: unchanged
      (`q2-preview` is already in it).
- [ ] `Preview.tsx` header comment and `PreviewRouter.tsx:57-65,170-180`
      comments: rewrite "default html preview" wording to "`q2-html-render`
      / non-html fallback".
- [ ] Vitest tier green: `cd hub-client && npm run test:ci`; production
      build: `npm run build:all`.

### Phase 4 — e2e suite

- [ ] `previewExtraction.ts`: rename kind `'html'` → `'q2-html-render'` (D7),
      selector unchanged; **remove the `?? 'html'` defaults** in
      `waitForPreviewRender` and `runAssertions` (a default kind hides
      wrong assumptions — make every caller explicit).
- [ ] `smoke-all.spec.ts:126-132`: kind = `q2-debug` for q2-debug specs,
      `q2-html-render` when the fixture's *own front matter* declares
      `format: q2-html-render`, else `q2-preview`. `smokeAllDiscovery.ts`
      already exposes the front matter; add `documentFormat` to
      `DiscoveredTest` if it is not there.
- [ ] `search.spec.ts:53`, `import-zip.spec.ts:113`, `project-loading.spec.ts:51`:
      switch to the q2-preview iframe selector (they test default behaviour).
- [ ] `q2-preview-click-to-editor-scroll.spec.ts`: the html-kind fixture
      (`:375`) declares `format: q2-html-render` explicitly; kind
      `'q2-html-render'`.
- [ ] Run `cargo xtask verify --e2e`. Triage every `ensureHtmlElements`
      failure per D8; record the resulting `HTML_RENDER_ONLY` list and its
      strands in this plan.

### Phase 4b — work the skip-list back down (same PR, follow-up commits)

Per D8 the skip-list is staging, not an end state. One commit per fixture
or per shared root cause:

- [ ] For each `HTML_RENDER_ONLY` entry, classify: (a) the selector is
      HTML-writer-specific and q2-preview has an obvious equivalent → add
      the equivalent selector to the fixture's `ensureHtmlElements` and
      remove the entry; (b) a genuine parity gap with a small fix → fix it
      in `ts-packages/preview-renderer` (with its own unit test) and remove
      the entry; (c) a genuine gap that is real work → leave the entry, but
      the strand it cites must be P2 or higher and linked to bd-j3764r9a.
- [ ] Target: the PR merges with the list empty; any remainder is listed
      here by fixture and strand so the reviewer sees exactly what coverage
      is still deferred.

### Phase 5 — `q2 preview` SPA message (D5)

- [ ] Test, `q2-preview-spa/src/PreviewApp.integration.test.tsx`: a render
      result `{success: true, html: '…'}` with no `ast_json` renders the
      "no live preview" message naming the format; no `console.error`.
- [ ] Implement in the else-branch at `PreviewApp.tsx:1195`; format name
      from `result` if present, else from the front matter.

### Phase 6 — docs, changelog, strands

- [ ] `hub-client/changelog.md` entry (two-commit workflow) describing the
      default switch and the `format: q2-html-render` opt-out.
- [ ] `claude-notes/plans/2026-07-01-html-format-capture-display.md`
      and `2026-09-09-q2-preview-edit-toggle.md`: add a one-line note that
      the "plain html preview" mode is now `q2-html-render` (they describe
      it as the default).
- [ ] Close bd-zvh2p (attribution now reaches reveal decks via D3), link it
      from bd-kltzdhle. File a follow-up strand for D2's alternative (Rust
      single-source router probe) only if you want it.

### Phase 7 — end-to-end verification (before declaring done)

- [ ] Rebuild the whole chain: `cd hub-client && npm run build:wasm`,
      `cargo xtask build-q2-preview-spa`, `cargo build --bin q2`.
- [ ] hub-client in a real browser (`npm run local-prod` or `npm run dev`
      against a local hub): open a project with (a) a no-front-matter
      document, (b) `format: html: toc: true`, (c) `format: q2-html-render`,
      (d) `format: revealjs`. For (a)/(b): the iframe is `q2-preview.html`,
      the Edit / Authors / Comments pills are enabled, a comment can be
      added from the preview. For (c): the iframe is `.preview-active`,
      pills disabled. For (d): Authors pill works. Record screenshots or
      DOM snippets here.
- [ ] `q2 preview --ui editor <project>` shows the same routing as hub-client.
- [ ] `q2 preview <doc-with-q2-html-render>` shows the D5 message;
      `q2 render` on the same doc writes HTML.
- [ ] Full `cargo xtask verify` green; then ask before pushing.

## Open questions

- **Q1 (D1)** — resolved: `q2-html-render`.
- **Q2** — resolved: no `docs/` changes. hub-client has effectively no user
  documentation today and remains in heavy flux; changelog entry only.
- **Q3 (D6)** — resolved: keep editing on by default.
- **Q4 (D5)** — resolved: the SPA message is in scope.

## Out of scope (deliberately)

- Adding `MorphIframe` to `q2-preview-spa` (a full-DOM renderer in `q2 preview`).
- Closing any of the parity gaps listed above; this plan only changes who
  sees them and documents the escape hatch.
- Consulting `_quarto.yml` project-level `format:` in hub-client (it is
  ignored today on every path; unchanged).
- Substituting html-based extension formats (`acm-html`) into q2-preview.
- The q2-slides migration (Plan 2E) and the router-probe consolidation
  discussed under D2.
