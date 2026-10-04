# Disable the vendored Lua shortcodes pass (escaped shortcodes expanded twice)

**Branch:** `issue-brand-shortcode` (from `feature/pandoc-wasm` @ `6d03c76ac`), worktree `workspace-7`.
**Strands:** bug `bd-2uva9urq` (this plan); design question `bd-qwgu94f4` ("should pandoc Lua execute shortcodes?"). Related: `bd-xfqx2tuc`, `bd-qnylgu69`.
**Status:** REVIEWED 2026-10-03 (one edit round applied); not started. No code changed yet.
**Blocked by:** `bd-xjg7vl6c` (Rust never expands shortcodes in footnote definitions). T2 must not land
before it, see Prerequisite.

## Problem

`docs/guides/authoring/brand.qmd` fails to render through pandoc.wasm, and natively with
`q2 render --to typst`. The failing text is documentation of the syntax (lines 859-864), written with the
escape: ``### Shortcode - `{{{< brand >}}}` `` and ``Use `{{{< brand color COLOR_NAME VARIANT >}}}` …``.
Errors: `shortcodes-handlers.lua:111` (`brandCommand` is nil, string concat) for the bare form;
`modules/brand/brand.lua:20` (`assert`, mode `"VARIANT"`) for the argument form.
The e2e test `hub-client/e2e/pandoc-warm.harness.spec.ts` ("larger fixtures") hides it by replacing the
shortcodes with `BRAND` via regex.

## Root cause (verified)

Two shortcode passes run over the same text, and expansion is not idempotent:

1. **Rust** (`crates/quarto-core/src/transforms/shortcode_resolve.rs`): `Inline::Code` (≈L2159) and
   `Block::CodeBlock` (≈L1899) call `expand_text_in_place` → `parse_text_shortcodes`
   (`shortcode_text.rs:63-67`), which turns `{{{< x >}}}` into the literal `{{< x >}}` ("applied once").
   Port of Q1's `apply_code_shortcode` (bd-fz6gwfq0); covered by
   `crates/quarto-core/tests/integration/shortcode_text_contexts.rs`.
2. **Pandoc Lua** (vendored Q1 stack): `pre-shortcodes-filter` → `shortcodes_filter()`
   (`filters/customnodes/shortcodes.lua:208`), whose `code_handler`/`attr_handler` run Q1's
   `apply_code_shortcode` over code and attribute text. It now sees the already-unescaped `{{< … >}}`
   and expands it as live.

Confirmed by dumping the retained `pandoc-input.json`: the `Code` node text reaches Lua as
`{{< brand color COLOR_NAME VARIANT >}}`. Reproduced natively again at review (`q2 render x.qmd --to typst`, rc=1, crash at `shortcodes.lua:305`) with three
minimal files: `` `{{{< brand >}}}` `` and `` `{{{< brand color COLOR_NAME VARIANT >}}}` `` fail; a
live `{{< brand color primary >}}` does **not** crash (Rust warns "Shortcode `brand` is not recognized"
and Lua never sees it).

Exposure: text contexts only (code, raw, math, attribute values). Escaped shortcodes in ordinary
inlines become a `Str`, which Lua does not expand. Pandoc paths only: native HTML does not run the Lua
stack, so `shortcode_text_contexts.rs` passes today.

## Decision (Gordon, 2026-10-03)

Disable shortcode execution on the pandoc Lua side for now. Rationale:

- Rust already resolves every shortcode node before Pandoc (including user-extension Lua shortcodes via
  its own `lua_engine`; unresolved ones become `?key` plus a Q-16-5 diagnostic). The Lua pass's only
  reachable input is text contexts that Rust has just processed, so the pass can only double-expand.
- It is the smallest fix and makes native HTML, native typst and wasm consistent: one pass, owned by Rust.
- Cost: Lua-only handlers (`brand`) stop working in code text. They did not work in live position
  anyway (Rust does not know `brand`). `bd-qnylgu69` still owns whether Q2 supports `brand` at all.
- The permanent ownership question is parked in `bd-qwgu94f4`.

Rejected for now: (1) Rust leaves escapes in text contexts for Lua to unescape (couples Rust output to
whether Lua runs afterwards); (2) marker attribute so Lua skips Rust-processed text (extra machinery,
edits vendored Lua handlers); (3) teach Rust `brand` and split ownership by name (largest; belongs to
`bd-qwgu94f4`).

## Prerequisite: Rust skips footnote definitions (found in review)

The Lua pass is not purely redundant. Rust's `shortcode_resolve` treats `Block::NoteDefinitionPara` and
`Block::NoteDefinitionFencedBlock` as leaves (L1136, L1928), so a shortcode in `[^1]: Note {{< meta author >}}.`
is never expanded by Rust. Native HTML shows `?meta` there. Pandoc paths resolve it today only because Lua
catches it. Measured against a scratch share tree: with the Lua pass off, pandoc exits rc=83 on that input (an
unresolved `Shortcode` custom node reaches `render`, which calls `internal_error()`); every other context tried
(tables, captions, callout titles, links, headers, attributes, math, raw, code, quotes, definition lists, fig divs)
is identical with the pass off. Gordon's call: independent bug, filed as `bd-xjg7vl6c` (it also recommends an
audit of every AST context Rust skips). **Order:** `bd-xjg7vl6c` lands before T2, otherwise disabling is a crash
regression for footnote definitions. Not part of this plan's checklist.

**Status (2026-10-04): fixed** on `issue-brand-shortcode` (see
`2026-10-04-shortcode-footnote-defs-lstcap.md`). Rust now expands footnote definitions (single-paragraph and
`::: ^id`) and the `lst-cap` attribute. The audit found no other qmd-reachable gap that Q1 expands; the
remaining unexpanded contexts (link/image titles, Code/CodeBlock/Table attr values, cite prefix/suffix) match
Q1 and are pinned by `shortcode_all_contexts`. The "other walkers in non-shortcode transforms" question was
not checked.

The earlier assumption "Rust already resolves every shortcode node before Pandoc" is therefore true only
modulo that bug (and anything the audit finds).

## Off-switch design

Two registrations of `shortcodes_filter()`:

- `resources/pandoc-filters/filters/main.lua:319-323` (`pre-shortcodes-filter`, flag `has_shortcodes`, `traverser = 'jog'`).
- `resources/pandoc-filters/filters/crossref/crossref.lua:169-171` (same name, no traverser).

**Chosen: keep the `shortcodes_filter()` call, gate the entry off.** Change the entry's flag in both files from
`flags = { "has_shortcodes" }` to a flag nothing ever sets (`flags = { "q2_lua_shortcodes_disabled" }`; the
runner skips a filter whose flags are all unset, `ast/runemulation.lua:54-70`), with a `QUARTO2-PATCH` comment
naming this plan. One-line change per file, trivially reversible, and `bd-qwgu94f4` can re-enable it.

Why not remove the entries (the first draft): `shortcodes_filter()` is evaluated when the filter list table is
built (load time) and assigns the module-local `_shortcodes_filter`. `process_shortcodes()`
(`customnodes/shortcodes.lua:204`) walks with that value, and `quarto-post/foldcode.lua:27` calls it for
`code-summary` in HTML-family pandoc output. Removing the call leaves it `nil`. Keeping the call also keeps the
custom-node handler registration (that is at import time, independent). `normalize/flags.lua` keeps computing
`has_shortcodes`; harmless. Replayed against captured pandoc inputs with each variant: removing the entries and
the flag-gate both fix all three repros and produce byte-identical typst for `shortcode-passthrough` (Rust already
turns passthrough into literal text); only the flag-gate keeps `process_shortcodes` working.

Unresolved `Shortcode` nodes Rust leaves behind: none in the contexts tried except footnote definitions (above).
After that bug is fixed, any remaining leftover would hit `Shortcode.render` -> `internal_error()` rather than
`?key`; the audit in `bd-xjg7vl6c` is the guard. `contents_shortcode_filter` stays but is inert: `contents` is
Lua-only and Rust never resolves it (it already yields `?contents` before Lua runs).

Do **not** edit `customnodes/shortcodes.lua` or `quarto-pre/shortcodes-handlers.lua`.

README: add entries under "Ours vs. pinned" in `resources/pandoc-filters/README.md` for `main.lua` (extend
the existing entry) and `crossref/crossref.lua`, in the exact `` - `<path>` — description `` shape the
`vendored-pandoc-filters` lint requires.

## Tasks

Gates per task: `cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo nextest run -p <crate>`;
one `cargo nextest run --workspace` at the end (capture to a log; compare to the live baseline, not an old
figure). Known red before this work: `pandoc_request_prepare::golden_file_is_structurally_current` and
`::typst_request_matches_the_recorded_native_run` (lane R recording drift).

- [ ] **T1 Failing test first.** New `crates/quarto-core/tests/integration/shortcode_pandoc_escapes.rs`
  (register in `main.rs`, alphabetical). Use `render_document_to_file` with `native` (AST dump: no writer escaping,
  no typst compile; model on `gfm_shortcode_round_trip` in `pandoc_long_tail_formats.rs`). Cases: the bare
  `` `{{{< brand >}}}` ``, the arguments form, and an escaped shortcode in a fenced code block; assert the render
  succeeds and the output contains the literal `{{< brand ... >}}`. Add a control: a live `{{< brand color primary >}}`
  still only warns. Practical in CI: the existing long-tail tests already run real native pandoc with no skip.
  Confirm it fails (rc 83) before T2. Note `shortcode_text_contexts.rs` is HTML-only, so it never covered this.
- [ ] **T2 Off-switch** (after `bd-xjg7vl6c`): flag-gate the entry in `main.lua` and `crossref.lua` per the design above; README entries.
- [ ] **T3 Dependents (surveyed in review; re-confirm, then no changes expected).** No smoke-all or golden
  fixture uses `brand` or `contents` live; the only users are `docs/guides/authoring/brand.qmd` and the
  `pandoc-warm` e2e. `crates/pampa/tests/wasm_lua.rs` tests pampa's own `LuaShortcodeEngine` (extension
  shortcodes), not the vendored pandoc Lua: unaffected, and no pampa Lua file is edited so the wasm rule does not
  trigger. No Lua unit tests exist in `resources/`. `gfm_shortcode_round_trip` uses an escaped shortcode in plain
  text (a `Str`), which Lua never expanded: unaffected. Also grep `^\[\^.*\]:.*\{\{<` in fixtures (none found).
  The wasm path builds its request from the same stage list as native (`build_pandoc_prefix_stages`, then
  `PandocPrepareStage`), so one Rust-resolved AST feeds both; T5 still verifies it.
- [ ] **T4 Goldens.** Editing `main.lua`/`crossref.lua` changes `share_tree_version`
  (`pandoc_request/share.rs:54`, a hash of the embedded tree). `golden_file_is_structurally_current` asserts it
  equals `schemas/pandoc-request.golden.json`, so it **will go red because of this change**: regenerate with
  `Q2_REGENERATE_GOLDEN=1 cargo nextest run -p quarto-core golden_file` and commit the golden (job_id and
  share_tree_version change), then check `hub-client` `goldenParity.wasm.test.ts` (compares the golden to the
  wasm's live share tree). The recordings under `pandoc-recordings/recordings/share/b540fcb3e42f6fb5/` embed the
  old filter sources but are self-contained (`recordingParity` and `check_against_recordings` use the recorded
  tree, not the live one) and none of the fixtures hit a changed path, so they stay green and are **not**
  re-recorded; note the staleness in the commit message. Correction: at review (HEAD `e38bbe55f`) both
  `golden_file_is_structurally_current` and `typst_request_matches_the_recorded_native_run` **pass** in this
  worktree (13/13 `pandoc_request_prepare::`), so they are not known-red here; re-measure the workspace baseline
  at T7 instead of trusting the "known red" list.
- [ ] **T5 Fixture.** Remove the `BRAND` substitution in `hub-client/e2e/pandoc-warm.harness.spec.ts`
  so `brand.qmd` renders as written. `npm run build:wasm`, then `VITE_E2E=1 npm run build` and
  `npx playwright test --config playwright.harness.config.ts --project=chromium --workers=1 pandoc-warm`.
- [ ] **T6 Native sweep.** `q2 render` to typst and html for `docs/guides/authoring/brand.qmd` and `docs/guides/authoring/shortcodes.qmd` (its table of `{{{< meta key >}}}` code spans was silently double-expanded too, with no crash); confirm the
  escaped examples show literally and note what the live `brand` shortcodes (lines 887, 898) now do.
  Report; do not fix `brand` support here (`bd-qnylgu69`).
- [ ] **T7 Workspace run, strand updates.** Record outcome on `bd-2uva9urq`; leave `bd-qwgu94f4` open.
  Reconcile this checklist with reality before handing off.

## Risks / open items

- `bd-xjg7vl6c` must land first (see Prerequisite); if it slips, T2 waits.
- `foldcode.lua` `process_shortcodes` still runs on `code-summary` text (HTML-family pandoc output only) and
  could double-expand an already-unescaped summary: the same class of bug, out of scope; belongs to `bd-qwgu94f4`.
- Wasm verification is T5 only (the e2e); there is no native proxy for the wasm Lua stdlib.
- Untracked strays in this worktree (`ff.mjs`, `serve-ff.mjs`, `hub-client/h9-demo-check.mjs`): never
  stage them; use explicit paths.
