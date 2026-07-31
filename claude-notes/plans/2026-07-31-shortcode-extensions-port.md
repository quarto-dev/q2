# Shortcode extensions: Quarto 1 → Quarto 2 port plan

**Status:** Reviewed 2026-07-31 — design decisions signed off (see § Design
decisions; Phase 3 deferred). Awaiting go-ahead to implement.
**Braid strand:** bd-540a976a (epic; related: bd-8b0af414, bd-nzdm1wry, bd-u145dg3y, bd-5edooc78, bd-mqk49)
**Date:** 2026-07-31

## Overview

Goal: a Quarto 1 shortcode extension — `_extensions/<org>/<name>/_extension.yml`
with `contributes: shortcodes: [handler.lua]` — works in Quarto 2, with the
documented Q1 handler contract (`handler(args, kwargs, meta, raw_args, context)`)
honored, and with failures surfaced as source-mapped, `Q-*`-coded diagnostics
instead of Q1's silent-passthrough / guess-what-you-meant behavior.

**Headline finding from the study:** this is *not* greenfield. Q2 already has a
working end-to-end path: tree-sitter grammar → `Inline::Shortcode` →
`ShortcodeResolveTransform` → Rust handler (`meta`) or Lua handler loaded from
`_extensions/` (five built-in extensions ship embedded: `kbd`, `lipsum`,
`placeholder`, `version`, `video`). The extension epic
(`claude-notes/plans/2026-03-16-extensions-grand-plan.md`, Phases 1–4 complete)
built the discovery/manifest/resolution machinery. This plan is therefore a
**gap-closure plan**, organized around a compatibility test corpus derived from
Q1's contract.

## Sources studied

- **Q1 docs (user contract):** `external-sources/quarto-web` —
  `docs/extensions/shortcodes.qmd` (authoring contract),
  `docs/authoring/_shortcodes.qmd` (built-in table), `docs/extensions/distributing.qmd`
  (`_extension.yml` schema), `docs/extensions/lua-api.qmd:367-374`
  (`quarto.shortcode.*`), `docs/extensions/_shortcode-escaping.qmd`.
- **Q1 implementation:** `external-sources/quarto-cli` — the LPeg grammar
  `src/resources/pandoc/datadir/lpegshortcode.lua` (authoritative syntax),
  `src/resources/filters/customnodes/shortcodes.lua` (dispatch + return coercion),
  `src/resources/filters/quarto-pre/shortcodes-handlers.lua` (registration +
  built-ins), `src/extension/extension.ts` (discovery),
  `src/command/render/filters.ts:602-705` (activation),
  `src/core/handlers/include.ts`/`embed.ts` (TS-side directives).
- **Q2 current state:** `crates/quarto-core/src/transforms/shortcode_resolve.rs`,
  `crates/quarto-core/src/extension/{types,read,discover,mod}.rs`,
  `crates/pampa/src/lua/shortcode.rs`, `crates/quarto-core/src/stage/stages/include_expansion.rs`,
  `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:623-666`.

## The Quarto 1 contract (condensed)

What a Q1 shortcode extension author was promised:

1. **Layout:** `_extensions/<name>/` or `_extensions/<org>/<name>/` containing
   `_extension.yml` with `contributes: shortcodes: [<file>.lua, …]`. Everything
   above `_extensions/` is not installed.
2. **Activation:** every discovered extension contributing shortcodes is
   **automatically active** — no YAML opt-in (unlike filters/formats). Discovery
   walks built-ins first, then `_extensions/` dirs from project root down to the
   input's directory; later (more local) same-id extensions override earlier ones.
3. **Handler registration:** the Lua file either `return { name = fn, … }` or
   defines global functions harvested from the chunk's sandboxed env. Flat
   namespace keyed by shortcode *name* (org/name namespacing identifies the
   extension, never the invocation). Precedence: document `shortcodes:` YAML <
   extension-contributed < built-ins (Q1 built-ins always win).
4. **Handler signature:** `fn(args, kwargs, meta, raw_args, context)`:
   - `args`: pandoc.List of positional values (inlines/strings);
   - `kwargs`: table whose missing keys yield **empty `pandoc.Inlines`** (not nil);
   - `meta`: lazy dotted-path metadata lookup (`meta["a.b"]` works), not raw Meta;
   - `raw_args` (≥1.3): all arg values in source order, names stripped;
   - `context` (≥1.5): `"block"` | `"inline"` | `"text"`.
5. **Return values:** string | Inline | Block | Inlines | Blocks | plain array |
   nil, with documented coercions per context (blocks→inlines via
   `blocks_to_inlines` in inline context; inlines wrapped in `Para` in block
   context; `"text"` context stringifies).
6. **Contexts in the document:** shortcodes resolve in prose (inline), alone in a
   paragraph (block), and in *text positions*: `Code`/`CodeBlock` text,
   `RawInline`/`RawBlock`, `Math`, element attributes (single-quoted),
   `Link.target`, `Image.src`. Opt-outs: `cell-code` class (engine output) and
   `{shortcodes=false}` attribute.
7. **Escaping:** `{{{< … >}}}` renders the literal `{{< … >}}`; `{{</* … */>}}`
   comment form ditto.
8. **Helpers:** `quarto.shortcode.read_arg(args, n)` and
   `quarto.shortcode.error_output(name, msg_or_args, context)`; ambient
   script-dir state (`withScriptFile`) drives `quarto.utils.resolve_path`,
   sibling-module `require`, and relative HTML dependencies.
9. **Unknown shortcode:** warn + pass through the original `{{< … >}}` text as a
   raw inline/block (silently, in text context). Documented as a *feature* for
   Hugo interop ("shortcodes not recognized by Quarto are passed through
   unmodified to Hugo").
10. **Built-ins:** `meta`, `var`, `env`, `pagebreak`, `kbd`, `video`, `include`,
    `embed`, `lipsum`, `placeholder`, `contents`, `version`, `brand` (the last
    missing from the docs' own table). `include`/`embed` are **not** Lua handlers
    in Q1 — they're TS text-level directives at pre-/post-engine stages.

Q1 quirks we get to *not* port (see § Design decisions): the undocumented paired
shortcode syntax (`{{< name >}}…{{< /name >}}`, 1.4, zero docs); the shadowed
`local result` bug in `shortcodes.lua:172-173` that drops some nested results;
grid-table non-support caused by Q1's text-level pre-parse (Q2 parses shortcodes
in-grammar, so this restriction may simply not apply — verify in Phase 0).

## Gap analysis: Q1 contract vs Q2 today

| # | Q1 contract item | Q2 status | Gap |
|---|---|---|---|
| 1 | Syntax `{{< … >}}` incl. nesting, escapes | ✅ in-grammar (`grammar.js:623-666`), `Inline::Shortcode` | Verify `{{</* … */>}}` comment-escape form; newlines inside shortcodes (Q1 allows) — Q-2-27/28 currently *reject* line breaks: deliberate strictness, keep, but confirm messaging |
| 2 | Extension discovery + manifest | ✅ `extension/discover.rs`, `read.rs` | **bd-8b0af414**: Q2 hard-requires `title`/`author`/`contributes`; real Q1 extensions omit these and silently fail to load. **bd-nzdm1wry**: load failure → `tracing::warn!` only, then misattributed "unknown shortcode" at use site |
| 3 | Auto-activation of shortcode extensions | ❌ **correctness bug** — on-demand load keyed by shortcode name (`dispatch_shortcode` → `find_extension(&shortcode.name)`, `shortcode_resolve.rs:370`) | Any extension whose shortcode names differ from its extension id is never loaded — the common case (`quarto-tiers` contributes `tier`, `fontawesome` contributes `fa`). Q2's built-ins work only because their ids coincide with their shortcode names. **Confirmed live**: `external-sources/connect-docs/docs-quarto-2` — `{{< tier … >}}` → "Unknown shortcode" despite a valid, discovered `_extensions/quarto-tiers/`. Fix: Q1's eager model — on first shortcode dispatch, load *all* discovered extensions' `contributes.shortcodes` scripts, then dispatch by handler name. Also: conflict/shadowing silent; Q1's `filterBuiltInExtensions` shadow-warning has no analogue |
| 4 | Handler signature 5-tuple | ✅ `shortcode.rs:298` TS-compatible; kwargs empty-Inlines metatable present | `meta` lazy dotted lookup: verify parity; `raw_args` shape: verify |
| 5 | Return-value coercions | ✅ `convert_return_value`/`classify_table_result` | Verify against Q1's table (esp. blocks→inlines in inline ctx, `nil` handling) via corpus |
| 6 | `context = "text"` | ⚠️ `ShortcodeCallContext::Text` exists in pampa, **unreachable** — quarto-core only dispatches Block/Inline | Shortcodes in code blocks, attributes, link targets, image src are **not resolved**. Grammar already parses shortcodes in link destinations + quoted attr strings; resolve wiring missing |
| 7 | Script-dir ambient state | ✅ `push_script_dir` stack, `quarto.utils.resolve_path` | **No `require`/`package.path` at all** (native or WASM) — any extension with a sibling module breaks |
| 8 | Built-in shortcodes | `meta` (Rust), `include` (Rust stage), `kbd`/`lipsum`/`placeholder`/`version`/`video` (Lua) | **Missing: `var` (+ `_variables.yml`), `env`, `pagebreak`, `brand`, `contents`, `embed`** |
| 9 | Unknown-shortcode behavior | warning (uncoded) + visible `?name` inline | No `Q-*` code, no source-mapped location shown, no passthrough option; HTML writer **silently drops** any surviving `Inline::Shortcode` (`html.rs:1059`); `shortcode_to_span` has a `process::exit(1)` on nested kv args (`pampa/src/pandoc/shortcode.rs:95-98`) |
| 10 | Precedence built-ins > user | Inverted-ish: Rust built-ins win, but Lua built-in extensions can be shadowed by user extensions (`find_extension` rfind) | Decide + document + diagnose (see D3) |
| 11 | `shortcodes:` YAML key | ✅ `extract_shortcode_paths` | Also accepts only paths; Q1 also allowed extension *names* in format-contributed `shortcodes:` — defer |
| 12 | `quarto add/remove/list/update` | ❌ 8-line `NotImplemented` stubs | CLI installation story (bd-5edooc78 pins the remove-guard requirement) |
| 13 | `quarto.shortcode.{read_arg,error_output}` | ✅ `shortcode.rs:384` | Verify exact semantics against `init.lua:1002-1032` |
| 14 | Escaped shortcode round-trip | ✅ `is_escaped` → Preserve | Corpus-verify writer output renders literal `{{< … >}}` |

## Design decisions (Q2-native improvements)

These are the places where we deliberately diverge from Q1, following the
project's porting principles: *strictness is acceptable when the diagnostic is
source-mapped and actionable; prefer explicit declaration over inference.*
Each is flagged **[decided]** (follows an existing Q2 policy) or **[needs user
sign-off]**.

**D1. Unknown shortcode → coded, source-mapped diagnostic; no silent drop.
[decided 2026-07-31]**
Q1 warns and passes the raw text through (silently, in text context) — partly a
Hugo-interop feature. Decision: warning-level diagnostic with a new `Q-*` code,
`.with_location()` pointing at the invocation (we have `SourceInfo` on every
`Shortcode` node — Q1 could never do this), plus the visible `?name` marker in
output. For Hugo-style passthrough, require explicitness: a
`shortcodes: passthrough: [ref, figure]` (or similar) config key that names
foreign shortcodes, rather than Q1's pass-everything-unknown.
Unknown-shortcode-as-*error* needs no dedicated flag: `q2 render --strict`
(warnings-as-errors, already shipped) composes with this warning.

**D2. Extension load failure is a real diagnostic, never a downstream
misattribution. [decided — this is bd-nzdm1wry]**
A malformed `_extension.yml` or a Lua file that fails to load must produce a
coded diagnostic naming the extension file and cause, at load/first-use time.
The current behavior (silent `tracing::warn!`, then "unknown shortcode ?greet"
pointing at the *user's document*) is precisely the Q1-style misattribution this
port should eliminate.

**D3. Manifest strictness: relax to Q1-compat intake, validate loudly.
[decided 2026-07-31 — proposal approved as written]**
Q1 requires no named fields in `_extension.yml`. Q2 hard-requires
`title`/`author`/`contributes` (bd-8b0af414), so real Q1 extensions
(julia-engine, marimo) fail to load — and per D2 today they fail *silently*.
Proposal: only `contributes` is structurally required (an extension contributing
nothing is an error, matching Q1's `validateExtension`); missing
`title`/`author` become warnings at most; `version`/`quarto-required` validated
as semver *when present*, with a source-mapped error into the YAML file when
malformed (we have quarto-yaml source locations; Q1 didn't).

**D4. Handler-name conflicts are diagnosed, not silent. [decided 2026-07-31,
conditional on practicality]**
Keep a deterministic precedence (matching Q1: built-ins win; among extensions,
more-local wins; document `shortcodes:` files lowest). The shadowing diagnostic
(naming both files) is approved *if practical* — user flagged a feasibility
concern: at the point where registration overwrites a name, we may not have
good source attribution for both definitions. Assess during Phase 1: if the
engine's handler registry records `(name, script_path)` per registration (it
already tracks the script being loaded), a file-level (not span-level)
diagnostic should be cheap; if it turns out invasive, ship the precedence rule
documented but undiagnosed and file a follow-on strand.

**D5. `include` stays a Rust pre-stage, not a Lua handler. [decided]**
Q2's `include_expansion.rs` already mirrors Q1's TS-side design (and Q1 itself
never had a Lua `include`). Keep circular-include detection and source-mapped
missing-file errors as coded diagnostics (verify they have `Q-*` codes; add if
not).

**D6. `embed` is out of scope for this plan. [confirmed 2026-07-31]**
Q1 `embed` drags in notebook rendering, `notebook-links`/`notebook-view`, and
the jupyter-embed placeholder machinery. User: `{{< embed >}}` needs a more
drastic redesign for Q2 — deferred to its own strand/epic, dependent on Q2's
engine story.

**D7. Paired shortcodes not ported. [confirmed 2026-07-31 — deferred]**
Shipped in Q1 1.4 (`#5902`), never documented, zero occurrences in quarto-web.
User: these likely exist purely for Hugo passthrough — which means if we ever
implement the D1 passthrough config for Hugo interop, paired syntax belongs to
*that* feature (pass the paired form through verbatim), not to the handler
dispatch machinery. File a backlog strand recording this framing.

**D8. In-grammar parsing is the single source of truth. [decided — already Q2
reality; text-position scanning deferred 2026-07-31]**
Q1 has *four* shortcode parsers (LPeg grammar, sentinel encoder, AST-level
metadata re-parser, TS regex parser) because it had to smuggle shortcodes past
Pandoc's reader. Q2 parses them in the tree-sitter grammar with real
`SourceInfo`. Consequences to verify in the corpus: grid tables (Q1-documented
restriction should just vanish); metadata-position shortcodes (Q1 needed
`astshortcode.lua`; where does Q2 stand? — Phase 0 must answer this);
attribute-position quoting rules. User note: the one place Q1's mess may need
partial re-doing is detecting shortcodes inside *opaque text positions* (code
block contents, URL targets, attributes) — that whole area (Phase 3) is
**deferred**; Phase 0 still records the current behavior as known-gap baseline
probes so the deferral is documented, not accidental.

## Work plan

TDD throughout: every phase starts by adding failing tests/corpus entries, per
CLAUDE.md. Each phase is a candidate braid child strand once the plan is
approved.

### Phase 0 — Compatibility corpus + behavioral baseline (the test plan)

The deliverable is a fixture suite that encodes the Q1 contract, so every later
phase has failing tests to turn green, and so we discover *actual* Q2 behavior
where the study only has static reads.

- [x] Port Q1's smoke fixtures — `shorty.lua` + error_output ported as
      `contract-doc-shortcodes` (passing; Q2 renders `[Shortcode Error
      (shorty): error message]` instead of Q1's `?shorty:error message` —
      accepted deviation, clearer text). `?var:` expectations wait for
      Phase 4 var.
- [x] Author contract fixtures (committed 08f29f89, passing):
      `contract-table-return` (table-return registration, dash names,
      block/inline context), `contract-global-fn`, `contract-return-coercions`
      (string/Inline/Inlines/Block/Blocks/array), `contract-escape-braces`,
      `contract-doc-shortcodes`. In-flight (failing = TDD targets, uncommitted):
      `contract-args-kwargs` (raw_args), `contract-meta-dotted` (dotted
      lookup), `contract-nested-arg`. Parked with `tests.run.skip`:
      `contract-escape-comment` (grammar gap, decision pending — recommend
      targeted Q-2-x diagnostic over porting the Hugo `/* */` form).
- [ ] Integration tests driving the real binary path (`render_document_to_file`
      / `q2 render` on fixtures) — not `HtmlRenderConfig::default()` shortcuts.
- [ ] Baseline probes (tests that *document* current behavior, marked
      known-gap): shortcode in code block / attribute / link target / image src;
      shortcode in YAML metadata values (title etc.); shortcode in grid table;
      extension with missing `title`; extension whose Lua `require`s a sibling.
- [ ] Pick 2–3 real published Q1 shortcode extensions (e.g. `quarto-ext/fontawesome`,
      `shafayetShafee/bsicons` class) and add them as fixtures; record what
      breaks. These are the acceptance tests for the whole plan.
- [ ] Real-world acceptance target: `external-sources/connect-docs/docs-quarto-2`
      (Posit Connect docs). Known failure today: `{{< tier … >}}` from
      `_extensions/quarto-tiers/` → "Unknown shortcode" (gap row 3). Copy the
      minimal extension shape into a local fixture (never reference
      `external-sources/` from tests); use the full project as a manual
      end-to-end check.

### Phase 1 — Extension loading: eager activation, Q1-compat intake, loud failures (D2, D3, gap row 3)

- [x] **Fix the name-keyed activation bug (gap row 3)** — done, commit
      08f29f89. `LuaEngineState` wraps engine + one-shot flag; eager load of
      all extensions' scripts on first Lua-stage dispatch; D4 precedence
      (doc `shortcodes:` < extensions in discovery order < Rust built-ins);
      per-script load failures warn naming extension id + script path.
      Verified end-to-end on connect-docs (`tier` renders, 0 warnings).
- [ ] Tests: malformed `_extension.yml` (bad YAML, bad semver, empty
      contributes) → coded, source-mapped diagnostics; minimal Q1 manifest
      (no title/author) loads.
- [ ] Relax `read.rs` required fields per D3; add `Q-*` codes for manifest
      errors (extension subsystem: decide `Q-5-*` vs new subsystem number).
- [ ] `discover_extensions` returns structured failures; `dispatch_shortcode`'s
      unknown-name fallthrough distinguishes "no such extension" from
      "extension found but failed to load" (closes bd-nzdm1wry).
- [ ] Shadowing diagnostic per D4 (closes the silent-`rfind` gap).

### Phase 2 — Handler contract parity + `require` (gap rows 4, 5, 7, 13)

- [ ] Corpus rows from Phase 0 for calling convention + coercions green.
- [ ] Sandboxed, script-dir-relative `require` (native + WASM via
      `SystemRuntime`), scoped to the extension's directory; test with a
      sibling-module fixture. (This is the highest-risk item for real-world
      extensions; likely a `package.preload`-style loader rather than exposing
      the C `package` lib.)
- [ ] `quarto.shortcode.read_arg`/`error_output` parity vs `init.lua:1002-1032`.
- [ ] Fix `shortcode_to_span`'s `process::exit(1)` (nested kv arg) → diagnostic.

### Phase 3 — DEFERRED (2026-07-31): `text` context — shortcodes in code, attributes, targets (gap row 6)

Deferred per user review (see D8): text-position detection is the one place
Q1's parsing mess may partly return, and it is not needed for the current
acceptance targets. Phase 0's baseline probes document the gap. Content kept
below for the eventual follow-on strand:

- Tests: `{{< meta k >}}` in CodeBlock text, Code inline, element attribute
  (single-quoted, per Q1), `Link.target`, `Image.src`; `{shortcodes=false}`
  and `cell-code` opt-outs; unknown shortcode in text context.
- Add `ResolutionContext::Text` in quarto-core; wire traversal over the
  text positions; dispatch with `ShortcodeCallContext::Text`; stringify
  results (Q1 `shortcodes.lua:248`).
- Decide grammar vs post-hoc scan for positions the grammar doesn't reach
  (code block *contents* are opaque to the inline grammar — this likely
  needs a targeted text-level scan; keep it in one module, single parser).
- Q1's `Image.src` default-extension fixup (#14583) — check whether Q2's
  pipeline has the same failure mode before porting it.
- bd-u145dg3y's block-shortcode-used-inline warning (`Q-2-x` request) now
  folds into Phase 5 instead.

### Phase 4 — Missing built-ins: `var`, `env`, `pagebreak` (gap row 8, easy tier)

- [ ] `var`: `_variables.yml` loading (project-scoped, values parsed as qmd
      inlines), dotted lookup, unknown-var diagnostic (coded, source-mapped —
      improvement over Q1's `?var:name`); `quarto.variables.get` Lua API.
- [ ] `env`: positional name + optional fallback arg (Q1 1.5 `#8316`); decide
      unset-and-no-fallback behavior (Q1: `Null`; propose coded warning).
- [ ] `pagebreak`: per-format raw table (html/latex/typst/docx/odt/context/epub,
      `\f` fallback) — implement as Rust handler or built-in Lua extension;
      follow the existing built-in-extension pattern
      (`claude-notes/plans/2026-04-01-builtin-extensions.md`).
- [ ] Each: implement in whichever tier (Rust handler vs embedded Lua ext)
      matches its needs; document the choice in the strand.

### Phase 5 — Unknown-shortcode policy + writer hardening (D1, gap row 9)

- [ ] Implement the D1 policy as signed off: `Q-*` code, `.with_location()`,
      visible marker; passthrough config for foreign shortcodes.
- [ ] HTML writer: surviving `Inline::Shortcode` is never silently dropped —
      emit marker + diagnostic (relates to orphaned `Q-3-30`/`Q-3-42` catalog
      entries; wire or retire them).
- [ ] Backfill `.with_code()` on the existing uncoded warnings in
      `shortcode_resolve.rs` (`:376`, `:398`, `:431`, extract sites).
- [ ] bd-u145dg3y: block-level shortcode used inline → coded warning
      (absorbed here from deferred Phase 3).

### Phase 6 — Deferred / follow-on strands (file, don't implement here)

- [ ] `text`-context resolution (deferred Phase 3 above — D8).
- [ ] `brand` shortcode (depends on brand.yml support status in Q2).
- [ ] `contents` shortcode (needs the collect-and-move filter design).
- [ ] `embed` (own epic; needs a drastic redesign for Q2, engine-dependent — D6).
- [ ] `q2 add/remove/list/update` CLI (own epic; bd-5edooc78 remove-guard;
      network, git, trust prompt).
- [ ] Format-contributed `shortcodes:` naming embedded extensions.
- [ ] Paired shortcodes: backlog strand recording the D7 decision.

## Related strands (link as `related` on the new strand)

- bd-8b0af414 — manifest over-strictness (Phase 1 absorbs)
- bd-nzdm1wry — extension load failure misattribution (Phase 1 absorbs)
- bd-u145dg3y — block shortcode used inline, wants `Q-2-x` (Phase 3/5)
- bd-5edooc78 — `q2 remove` must guard built-ins (Phase 6 CLI epic)
- bd-mqk49 — pipeline stages not extension-registrable (context for Phase 6)
- bd-129m3 / bd-36fr9 — provenance anchors for shortcode values (adjacent)

## Prior art in-repo (read before implementing)

- `claude-notes/plans/2026-03-16-extensions-grand-plan.md` (+ phase plans 1–4)
- `claude-notes/plans/2026-03-20-extensions-phase3-shortcode-resolution.md` —
  documents the TS dispatch internals this plan builds on
- `claude-notes/plans/2026-03-31-shortcode-args-compat.md` — calling convention
- `claude-notes/plans/2026-04-01-builtin-extensions.md` (+ batch2, video) —
  the embedded-extension pattern Phase 4 follows
- `claude-notes/designs/provenance-contract.md` — `stamp_shortcode_anchors`
- `claude-notes/designs/transform-pipeline-phases.md` — where the transform sits

## Appendix: process notes (toward the Q1→Q2 porting-guide skill)

Captured for the guidance document we'll draft at the end of this effort:

1. **Three parallel studies, then reconcile:** (a) Q1 *documented* contract
   (quarto-web) — what users were promised; (b) Q1 *implementation*
   (quarto-cli) — what actually happens, incl. undocumented features and bugs;
   (c) Q2 current state — what already exists (it's rarely zero). The
   interesting deltas are doc-vs-impl (undocumented features: paired
   shortcodes; underdocumented: `raw_args`) and impl-vs-impl (Q1 bug we may
   not want: shadowed `local result`).
2. **Spot-check the studies against the tree** before writing the plan
   (built-ins list, stub files, unreachable enum variants) — static reading
   agents are good but load-bearing claims deserve a grep.
3. **Classify each gap as compat / improve / drop**, with the project's two
   levers named explicitly: (i) added strictness is OK iff the diagnostic is
   source-mapped + actionable (`Q-*` code, `.with_location()`); (ii) prefer
   explicit declaration (e.g. passthrough list) over Q1-style inference
   (pass-through-anything-unknown).
4. **Phase 0 is always a compatibility corpus** ported from Q1's own test
   fixtures plus real published extensions — Q1's tests encode the de-facto
   contract better than its docs; real extensions are the acceptance bar.
5. **Check the braid skein + claude-notes first:** existing strands
   (bd-8b0af414, bd-nzdm1wry…) and completed phase plans reframed this from
   "port a feature" to "close gaps in a mostly-done port".
6. **Q1's architecture workarounds may evaporate in Q2:** Q1's four shortcode
   parsers exist only because Pandoc's reader was in the way; Q2's in-grammar
   parse deletes the whole problem class (and its restrictions, e.g. grid
   tables). Ask "which Q1 mechanisms were workarounds for infrastructure Q2
   replaced?" before porting mechanism-by-mechanism.
