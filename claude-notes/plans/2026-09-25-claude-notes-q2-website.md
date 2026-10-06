# claude-notes as a Quarto 2 website

**Strand:** bd-uk8zgkha (epic)
**Branch:** `braid/bd-uk8zgkha-claude-notes-website` (main checkout, no worktree; not pushed)
**Status:** in progress. Mechanical escaping done for the literal-character classes;
star emphasis needs manual review (queue below); uncoded parse errors not yet triaged.
Merged with `main` on 2026-10-06 (through PR 780) and re-escaped to fixpoint on
nightly .20261006; see "Bringing the branch up to date" below.

## Goal

Render `claude-notes/` with `q2 render` as a website, so that plans are Quarto 2
documents. Fix the notes where they rely on markdown habits q2 rejects, and fix q2
where it is wrong. Then write agent guidance so new notes render cleanly.

## Setup

`claude-notes/_quarto.yml` renders only `**/*.md`. The `.qmd` files here are repro
fixtures, and a positive render pattern replaces the default `**/*.qmd`. It also
excludes `plans/CURRENT.md`, the gitignored per-session symlink. Nested repro
projects (directories with their own `_quarto.yml`) are skipped by q2 itself since
0.33.0-nightly.20260925 (bd-nested-projects-xyb28wnl), with a Q-5-31 warning that
names them; the temporary exclusion list we carried before that is gone.

`README.md` files are already skipped by q2 discovery. Render output
(`claude-notes/_site/`, `claude-notes/.quarto/`) is gitignored.

## Tools

All in `scripts/`:

| Script | Purpose |
|--------|---------|
| `q2-render-tally.py` | Run `q2 render --json-errors`; table of diagnostics by code; `--list CODE`; `--save`/`--compare` baselines. |
| `q2-escape-openers.py` | Escape exactly the delimiter a diagnostic points at; re-render until the class converges. `--only-context` limits it to clearly literal cases (the rest are listed for review); `--block-pattern` covers diagnostics that carry no opener position. |
| `q2-escape-literal.py` | Sweep a pattern through the prose of every `.md` (skips front matter, fences, code spans, comments, URLs, link targets). For delimiters q2 pairs silently. |
| `claude-notes-escape-fixpoint.sh` | Run every reviewed rule jointly until nothing changes. Fixing one class exposes more of the others. |

Every content commit was checked mechanically: each changed line differs from the
original only by inserted backslashes.

### Facts about the JSON diagnostics

- Columns are 1-based and count characters, not bytes.
- "Unclosed X" errors are reported at the *end of the block*. The opener is in
  `details[]` ("This is the opening ..."); Q-2-17 and Q-2-13 opener spans start at
  the whitespace before the delimiter. Some (often pipe tables) have no opener
  detail at all.
- The opener a diagnostic names is where the parser gave up, not necessarily the
  culprit: a stray delimiter earlier in the paragraph can make a correct
  `*italic*` look unclosed.
- Parse-failure records keep the structured list in `diagnostics`; their `error`
  field is ANSI text showing only a subset. Standalone warning records have no
  file path (bd-ckbqmupi).

## Decisions (Carlos)

- Escape in the documents now; do not wait for parser fixes to reach a nightly.
  In q2, a backslash before any syntax character makes it literal, so the escapes
  stay valid after the fixes land.
- A bare `@` should parse as a literal `@` (bd-bare-at-literal-w3ytmu8e).
- Plural possessives (`engines' state`) stay a parser error; escape them.
- An apostrophe right after a span closer (`` `a.rs`'s ``) should become an
  apostrophe in the parser (bd-apostrophe-after-span-6l9c8c26).
- Star emphasis: escape only clearly literal stars; review the rest by hand.
- Nested projects: stop the implicit glob at `_quarto.yml` boundaries and warn
  (bd-nested-projects-xyb28wnl, shipped in 0.33.0-nightly.20260925).
- An unmatched backtick run is an intentional rejection (mismatched backticks render
  poorly), unlike CommonMark; it needs a Q-code and a good message
  (bd-unmatched-backtick-run-literal-tjfo21xq).

## Findings filed as strands

| Strand | Finding |
|--------|---------|
| bd-ckbqmupi | `--json-errors` warning records carry no file path |
| bd-bare-at-literal-w3ytmu8e | bare `@` is an uncoded parse error; it can break fenced code blocks later in the file |
| bd-apostrophe-after-span-6l9c8c26 | apostrophe after a span closer is read as an opening quote |
| bd-code-span-longer-backtick-run-nycn85a8 | code span containing a longer backtick run than its delimiter fails |
| bd-whitespace-flanked-delimiters-0ncy8bgq | whitespace-flanked `*`, `_`, `^` and `~` pair up *silently* into emphasis, sub- or superscript |
| bd-nested-projects-xyb28wnl | nested `_quarto.yml` silently absorbed by the outer render |
| bd-angle-bracket-u27e8-parse-error-r6l55zmh | a Unicode angle bracket (U+27E8) anywhere in prose is a parse error |
| bd-dollar-math-flanking-wzjx4hn8 | `$` without Pandoc's flanking rules: prices and shell prompts fail |
| bd-unmatched-backtick-run-literal-tjfo21xq | unmatched backtick run: intentional rejection, needs a Q-code |
| bd-uncoded-braces-indent-9eq8004k | braces in prose and indented lines fail without a Q-code |

Shipped in 0.33.0-nightly.20260925: the code-span fix, the nested-project boundary,
and the flanking fix for `*`, `~` and `^` (now literal, no error). `_` is half done:
no more silent pairing, but an unpaired `_` is still an error (Q-2-5).

Shipped by 0.33.0-nightly.20261006 (probed with one-line documents): a bare `@` is
literal text, and `⟨` parses. So `~`, bare `@`, whitespace-flanked `*` and `^` no
longer need escaping in new notes; the existing escapes stay valid. Still errors on
this nightly: an apostrophe after a span closer, plural possessives, underscore
filenames in prose (one or two per paragraph), and `$` prices.

The silent pairing is the nastiest: an odd number of literal delimiters in a
paragraph is an error, an even number mis-renders without a word. Before the tilde
sweep, 66 rendered pages had accidental subscripts; now there are none.

## Escaping applied so far

| Class | What | How |
|-------|------|-----|
| `\~` | every single tilde in prose ("approximately", `~/path`, Lua `~=`) | sweep; `~~strikethrough~~` kept |
| `\'` | apostrophes q2 reads as quotes (Q-2-7, Q-2-10) | per diagnostic |
| `\@` | bare `@` (the `` `main` @ `sha` `` header) | per diagnostic |
| `\*` | whitespace-flanked, glob and wildcard stars | sweep + per diagnostic |
| `\_` | underscore filenames in prose (`_quarto.yml`, `_site/`) | sweep |

Hand fixes: quoted literals moved into code spans, and one quoted Rust comment
whose original markup had an unbalanced backtick.

## Progress

| | Rendered | Files with errors | Uncoded parse errors |
|--|--:|--:|--:|
| Baseline (2026-09-23) | 448 / 1365 | 917 | 1458 |
| 2026-09-25, nightly .20260922 | 1006 / 1352 | 348 | 491 |
| 2026-09-25, nightly .20260925 | 1075 / 1353 | 280 | 316 |
| 2026-10-06, nightly .20261006, before the merge | 1087 / 1353 | 268 | 332 |
| 2026-10-06, after merging `main` | 1113 / 1436 | 325 | 469 |
| 2026-10-06, after re-escaping to fixpoint | 1146 / 1436 | 292 | 329 |

Remaining error classes (nightly .20261006, after the merge): uncoded parse errors
329 (145 files), Q-2-12 48, Q-2-11 30, Q-2-41 22, Q-2-13 21, Q-2-5 10, Q-2-35 9,
Q-2-7 8, plus a tail. The uncoded errors have not been broken down by cause on this
nightly; on .20260925 it was end-of-line cascades 76, backtick runs 44, indented
lines 58, braces 20, `⟨` 6, `$` 5. Warnings are not yet addressed: Q-2-49 203,
Q-2-9 99, Q-16-3 51, Q-16-5 48.

### Bringing the branch up to date (2026-10-06)

`main` had moved 281 commits. It added 83 notes and revised 16 that this branch had
escaped. The merge had two conflicts, both a line that `main` reworded and this
branch had escaped; each took the text from `main` with the escape reapplied. Checked
after the merge: every note differs from `main` only by inserted backslashes, except
the four hand-fixed files, which `main` did not touch.

The new notes arrive unescaped, so every merge from `main` needs a rerun of
`scripts/claude-notes-escape-fixpoint.sh` (6 minutes; this time 330 `\'` in 55
files, nothing from the other rules). 8 Q-2-7 remain that the script rule does not
match (one is the year abbreviation in "July \'26"); they need a look by hand.

The star review queue below was collected on nightly .20260922, before the merge;
the flanking fix resolved most of the Q-2-13 items, and the merged notes added new
ones. Regenerate it before working through it:
`scripts/q2-escape-openers.py claude-notes Q-2-12 --dry-run --only-context '...'`
with the star pattern from `claude-notes-escape-fixpoint.sh`.

## Manual review queue: star emphasis

Each line is `file:line:col` and the reported opener, in brackets. For each:
find the real culprit in that paragraph, which may be a different star. Close
genuinely unclosed markup, escape literal stars, or move code-like text into a
code span.

Recurring causes seen so far:

- A filename starting with `_` inside bold, e.g. `**renders without _quarto.yml**`
  (mostly fixed by the underscore sweep).
- An apostrophe after a code span inside bold, e.g. `` **`findDoc`'s bail** ``.
- Bold that is closed with a single star, or never closed.
- Intended `*italic*` reported as unclosed because of another stray star earlier
  in the paragraph.

### Q-2-12 (unclosed `*`)

```text
citeproc-rust-port-notes.md:381:61: cascades must be handled carefully (et al., et al.[*], et al.+)
designs/path-resolution-model.md:121:28:    keys\' strings are only [*]sometimes* paths (`theme` shares its namespace
investigations/2025-11-20-emoji-sequences-reference.md:65:59: es**: 0️⃣ 1️⃣ 2️⃣ 3️⃣ 4️⃣ 5️⃣ 6️⃣ 7️⃣ 8️⃣ 9️⃣ #️⃣ [*]️⃣
investigations/2025-11-20-keycap-emoji-fix-plan.md:24:40: **Keycap emojis** like 1️⃣ 2️⃣ 3️⃣ #️⃣ [*]️⃣ fail because:
plans/2026-01-24-html-rendering-parity.md:38:32: 1. Structure the AST correctly [*]before* HTML generation
plans/2026-04-01-lua-api-quarto-doc.md:371:69: MAT is in a specific set (html, html4, html5, epub[*],
plans/2026-04-16-plan1a-protocol.md:460:35: `id` envelope; 1.6 swaps only the [*]transport* underneath it. The stdout footgun
plans/2026-04-20-syntax-highlighting-phase-3.5.md:15:79: l-spans`**. Decision 1 in the original plan says: [*]"a user filter producing the same encodi
plans/2026-05-04-q2-preview-plan-2a-iframe-foundation.md:16:254:  muted-gray "T (not yet implemented)" placeholder [*]and recurses into children via `
plans/2026-05-04-q2-preview-plan-6-provenance-audit.md:301:31:   fix relies on the invariant [*]"`AttrSourceInfo.attributes[i]` is the
plans/2026-05-09-q2-preview-plan-2c-customnode-rendering.md:509:318: is unchanged. 2C's tests below mount the per-type [*]components* against the regi
plans/2026-05-20-bd-8d6rk-navigation-diagnostics.md:85:42: ## New error subsystem: navigation (Q-13-[*])
plans/2026-05-20-render-no-project-skip-walk.md:235:59: st tests, hub-client tests, trace-viewer, preview-[*],
plans/2026-05-25-reactji-authorship-q2-preview.md:95:229: eNodeAttribution` soft assertion (passes post-2a).[*]
plans/2026-06-01-q2-preview-plan-7g-source-range-tiling.md:222:60: are bugs vs. genuinely scattered). This item only [*]defines what one
plans/2026-06-18-boundary-splice-edit-design.md:39:5: are [*]boundaries* (gaps between blocks), and the verb t
plans/2026-06-18-qmd-per-line-provenance.md:184:56: iling changes.** Today each top-level block piece [*]absorbs* its preceding
plans/2026-06-19-smoke-all-e2e-deflake.md:15:62: ing synced into the VFS. `smoke-all.spec.ts` only [*]sorts* the target
plans/2026-06-25-plan1a-return-to-q1.md:311:70: stance`, `ts_engine.rs:225-230`) at first launch. [*](Why the instance must hold it
plans/2026-06-26-extract-quarto-yaml-validation-design.md:474:77: e` (catalog-agnostic) + `quarto-error-catalog` (Q-[*]
plans/2026-06-26-plan5-engine-host-pooling.md:109:8:    the [*]seconds*-scale cost) **already survives a subproc
plans/2026-07-01-plan1c2-engine-extensions-loose-ends.md:237:53: > `content-pattern`: admission then evaluates the [*]same* predicate the claim stage 
plans/2026-07-02-preview-capture-delivery.md:268:82: ver boot) — documented in the spec's file header.)[*]
plans/2026-07-08-plan1c3-build-ts-extension-command-name.md:155:182: ` arms become unreachable — harmless; leave them.)[*]
plans/2026-08-10-project-profiles-port.md:185:55: New error-catalog codes (subsystem `project`, Q-5-[*])
plans/2026-08-11-error-docs-page-coverage-lint.md:83:59: quarto-error-catalog/error_catalog.json` — a JSON [*]object* keyed by
plans/2026-08-12-adjacent-footnote-definitions.md:255:13:   and it is [*]inside a fenced ` ```markdown ` block* — illustra
plans/2026-08-20-listing-numeric-config-keys.md:49:60:  one of the affected keys unquoted — which is the [*]natural* way to
plans/2026-09-17-automerge-npm-upgrade.md:154:29:       trace-viewer; preview-[*]; hub MCP; q2-preview-spa build)
plans/2026-09-18-pandoc-hybrid-P7-implementation.md:64:77: y genuinely-skipped surface, and it is skipped by [*]not being a test* —
research/2026-06-18-line-number-provenance-failures.md:9:74: es/provenance_probe.rs`, since removed) drove the [*]real*
research/2026-06-26-engine-api-usage-model.md:418:53: **All PROVIDED interface members** are present as [*]types* in
research/2026-06-29-plan3-vs-usage-model-reconciled.md:195:59: real hazard is the plan's prose:** it describes a [*]live* "protect HTML / restore for
research/2026-08-19-path-resolution-class-assessment.md:116:22:    root. This is the [*]newest* correct implementation — built two weeks 
research/vendored-dependencies-inventory.md:109:6: (see [*]Note on `tree-sitter-qmd`'s stale "fork" framing*
rust-task-dag-libraries-comparison.md:477:68: oization** - Like regular memoization, but tracks [*]which specific data accesses* occur during comput
2025-10-25-type-cleanup-analysis.md:212:76: - **pandoc-types.ts**: PandocDocument, QmdPandocDocument, Annotated_* types
investigations/2025-11-23-error-code-audit-results.md:61:106: | Q-4-*, Q-5-*, Q-6-*, Q-7-*, Q-8-* | Design documents (future subsystems?) | Document i
lua-reference/manual-index.md:307:41: ## Section 5: Auxiliary Library (luaL_*)
plans/2025-10-19-sourceinfo-pool-serialization.md:81:40: #### Task 1.3: Update write_* Functions
plans/2025-11-27-csl-conformance-progress.md:56:44: **Tests to unlock**: name_* category (many)
plans/2025-11-27-csl-failing-test-analysis.md:16:60: **Example**: `collapse_CitationNumberRangesWithAffixes.txt`
plans/2025-12-03-reader-writer-options-design.md:674:62: (Plus many format-specific fields: epub_*, cite_method, etc.)
plans/2026-05-14-list-table-multiblock-cell-fix.md:42:40: - [x] **2.4** Surveyed all `list-table-*.qmd` snapshots — every existing one uses single-Pla
plans/2026-05-28-integration-test-consolidation.md:73:46: | quarto-doctemplate     |                2 |
plans/2026-07-04-plan10-check-installation.md:124:243: | J3 | 6 | e2e-rs (python+kernel-gated) | jupyter test render | with jupyter_core + python kern
plans/2026-08-06-listing-glob-provenance.md:56:62: 6. **Silent glob corruption when the markdown parse *succeeds*** (fixture
plans/2026-08-07-md-render-support.md:50:53: - **Q1 includes `.md` in the render list *by default*** — this is the big
plans/2026-08-25-custom-template-not-templated.md:34:13: - All `Q-12-*` diagnostics in `listing_render.rs` go through the existing `push_diag(diags, c
plans/lua-filter-pipeline/00-index.md:89:68: | **Total** | **\~57** | **6** | **9** | **0** | **3** | **\~14** |
plans/lua-filter-pipeline/02-normalize-filters.md:188:57: | normalize-combine-2 | None | `pandoc.read` | Partial |
plans/lua-filter-pipeline/03-pre-filters.md:278:45: | pre-write-results | `FW` | None | **No** |
plans/lua-filter-pipeline/07-finalize-filters.md:19:65: | finalize-wrapped-writer | Wrapped writer setup | None | None |
research/2026-06-26-1b-vs-usage-model-OPUS-B.md:20:44: the actual return shape *structurally wrong*** for the two engines that use the
```

### Q-2-13 (unclosed `**`)

```text
issue-reports/206/triage.md:77:4: 1. [**]Tighten `caption`'s first token via the external
plans/2025-12-06-project-naming.md:170:4:    [**]Answer**: Cultural authenticity matters (hence r
plans/2026-04-16-plan1c-extension-integration.md:578:61: ning (extension) registry engines sorted by name. [**]NB the
plans/2026-04-16-quarto-jupyter.md:202:69: /jupyter` subpath to `@quarto/api`'s `exports` map[**]
plans/2026-04-22-serde-json-value-intermediate.md:122:4: 4. [**]Tree-sitter parsing is <2% at every scale.** The
plans/2026-05-04-q2-preview-plan-1-pipeline.md:113:3: - [**]`ReactPreview.tsx`'s `doRender` gains a temporar
plans/2026-05-07-listings-L8-custom-templates.md:882:41:   broader future direction: the planned [**]`!path` YAML tag
plans/2026-05-08-listings-L9-rss-feeds.md:1703:3: - [**]D10 (full-reader transforms in v1: urls-to-absol
plans/2026-05-30-hub-path-traversal-containment.md:85:1: [**]Do not reuse `quarto-core`'s `lexical_clean` (`o
plans/2026-06-02-get-config-command.md:73:3: - [**]`ConfigValue`'s default serde output is NOT suit
plans/2026-06-12-graceful-dangling-entries.md:289:3: - [**]`findDoc`'s `connectedPeers.size === 0` bail** (
plans/2026-06-14-nesting-cursor-ui-enhancements.md:514:93: lready use. **Split `hoveredRef`'s overloaded role[**]
plans/2026-06-15-breadcrumb-visual-design.md:531:90:  left, not the `#q2-active-edit-region` wrapper's.[**]
plans/2026-06-15-nesting-cursor-navigation-and-list-items.md:56:78: [Str]]` (inlines, no block node) → correctly out. [**]The A1 predicate and
plans/2026-06-18-block-editing-glitches-2.md:191:36:   `cancelPendingLand` (abort), and [**]`executeLanding`'s `'focus'` branch** (the
plans/2026-06-29-yaml-stack-extraction-handoff.md:247:3: - [**]`| tail` masks `cargo xtask verify`'s real exit 
plans/2026-07-02-plan4c-marimo-validation.md:88:4: 8. [**]`breakQuartoMd`'s custom-regex arg is supported*
plans/2026-07-02-strict-mode-warnings-as-errors.md:150:3: - [**]`quarto-doctemplate`'s internal `strict_mode`: u
plans/2026-07-08-plan1a6-off-stdout-loopback-tcp.md:390:5:   - [**]`ensure_started_inner`'s `init` closure (`:566-5
plans/2026-07-28-q2-use-brand-command.md:621:4: 6. [**]`ReplaceRange`'s `expected` is sliced from the c
plans/2026-07-30-commentblock-defensive-resolvesource.md:30:4: 2. [**]The s0 test harness's `resolveSource` stub preda
plans/2026-08-09-q58-extension-script-diagnostic-span.md:104:23: unconditionally binds [**]`_quarto.yml`'s path and content** to it.
plans/2026-08-12-footnotes-appendix-heading.md:380:92: `cargo xtask verify | tail -30` reports **`tail`'s[**]
plans/2026-08-12-warning-suppression-and-lone-bracket-diagnostic.md:297:1: [**]Also deferred: re-keying `qmd-syntax-helper`'s `
plans/2026-08-18-tabset-headings-in-toc.md:217:3: - [**](B)'s blast radius measured lower than feared.**
plans/2026-08-20-pandoc-hybrid-P1-neutral-core.md:48:4: 2. [**]`Pandoc(fmt)`'s payload is `String`** (the raw `
plans/2026-08-20-pandoc-hybrid-P4-run-machinery.md:474:93: per Findings item 9 — a final decision, not a gap.[**]
plans/2026-08-20-pandoc-hybrid-P5-lua-shim.md:148:35:   node for a resolved ref at all. [**]Recommendation: the shim resolves `CrossrefResol
plans/2026-08-20-pandoc-hybrid-P7-format-tail.md:388:99: plementation companion Task 5, commit `e8d6cea4e`.[**]
plans/2026-08-20-pandoc-hybrid-epic.md:41:3: - [**]`enable-crossref`'s gate surface is two structur
plans/2026-08-22-click-align-editor-y.md:192:3: - [**]`lineForClickTarget` gates only the `hostY` comp
plans/2026-08-24-repo-actions.md:107:3:   [**]`none` clears the list outright — it also suppre
plans/2026-09-02-failed-include-fails-document.md:113:64: ed render must not empty `q2 preview`'s watch set.[**]
plans/2026-09-03-julia-engine-static-declarations-epic.md:190:32: `git subtree`'s merge tracking?[**]
plans/2026-09-14-scss-cache-import-closure.md:267:3: - [**]Native recording point: `RuntimeFs::read`'s runt
plans/2026-09-18-pandoc-hybrid-P4-implementation.md:1165:27: - *Windows specifically:* [**]`accepted-untested` on Windows, bound as pure lo
plans/2026-09-18-pandoc-hybrid-P5-implementation.md:633:88: ified present and reachable in `main.lua`'s state:[**]
research/2026-06-24-plan1b-review-findings.md:196:5:     [**]Action: state whether writes are a single `write
research/2026-06-26-1a-2a-calibration-reconciled.md:31:3: - [**]One genuinely uncovered gap, epic-wide: `system.
research/2026-07-02-julia-engine-q2-compat.md:288:4: 1. [**]`build-ts-extension`'s directory-resolution conv
research/2026-07-02-marimo-engine-q2-compat.md:273:4: 2. [**]`find_entry_ts`'s naming convention doesn't matc
research/2026-07-03-marimo-migration-guide.md:97:1: [**]b. `e8ec4fb` — `extract.py`'s `BARE_SQL_FENCE_RE
research/2026-07-03-plan10-check-installation-research.md:189:3: - [**]So extension engines' `checkInstallation` IS inv
research/2026-08-21-provenance-audit-findings.md:520:75: ine.rs:94`'s `NodeValue::Escaped` arm is dead code[**]
research/2026-09-18-typst-epub-pandoc-q1-inventory.md:183:71: llback. This branch existing is a head start, but [**]confirming it
scratch/2026-08-06-memo-quarto-source-map-default-sourceinfo.md:120:74: ern-match `Default`'s output expecting `Original`?[**]
```

## Next

1. Work through the star queue by hand (with Carlos).
2. Triage the remaining uncoded parse errors by the character at the error position.
   Known sources: code spans with long backtick runs (use a longer delimiter),
   `$ ` shell prompts in prose, braces in prose (Q-2-41 too), indented lines.
3. Q-2-11 (unclosed `"`), Q-2-35 (indented code), Q-2-16 (`^`), then the warnings.
4. Write the agent guidance doc (a "writing markdown for Q2" page) from the escaping
   table above, and point `AGENTS.md` at it. Update plan templates, e.g. the
   `` `main` @ `sha` `` header.
5. Site polish: `index` page (00-INDEX.md is stale), navigation, and whether CI
   should require a clean render.
