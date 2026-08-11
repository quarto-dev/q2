# Lua filters in the returned-table form are silently ignored (bd-lua-filter-table-form-ignored-ph23becz)

**Date:** 2026-08-11
**Braid:** `bd-lua-filter-table-form-ignored-ph23becz` (bug, p1, labels `pampa` / `parity`)
**Branch:** `main` @ `808215fc` (investigated in the main checkout; no worktree created)
**Status:** Design settled (2026-08-11) — see **Settled decisions**. Phases are
written but **implementation has not started and needs the user's go-ahead.**

## Triage verdict

**Ready to design.** The root cause is confirmed exactly as reported, the fix
site is a single function, the machinery it needs already exists in-tree (the
shortcode loader does the same thing, and `apply_full_filter` already takes a
handler table), and Pandoc's reference semantics are now pinned by direct probe
against the same pandoc version the differential oracle is pinned to. What is
*not* settled is the diagnostic story and one deliberate behavior change
(globals shadowed by a returned table) — those are the design questions below.

## Issue context

A Lua filter in the standard Pandoc returned-table form runs none of its
handlers and produces no error, warning, or diagnostic:

```lua
return { Str = function(el) … end }   -- silently does nothing
function Str(el) … end                -- works
```

Both forms are valid Pandoc and Quarto 1 runs both. Real-world hit: the
`posit-dev/mermaid-zoom` extension used by the Posit Connect docs port is
written in the returned-table form, so its pan/zoom overlay is silently absent
from all 14 diagram pages. Diagnosing it required a bisect against a
hand-written control filter, because nothing in the render output signals that
a declared filter did nothing.

Filed 2026-08-11 by Carlos Scheidegger, from a porting session on
`q2-connect-docs`. Observed at 0.16.0, re-verified at 0.17.0.

## Dependency graph

`braid dep list` returns **no edges** — this strand is unlinked in the q2 skein.
That is expected rather than suspicious: the originating strand (`br-u35hvil9`)
lives in the *connect-docs porting* skein, a different document, so no
`discovered-from` edge can span the two. The "why was this filed" context is
therefore carried entirely in the description (and it is unusually complete).

No incoming `blocks` edges, so nothing in q2 is formally waiting on this. The
pressure is external: the Connect docs port needs it.

Topical neighbours, all **closed**, all directly relevant as precedent:

- **bd-a9g50za2** — *"Doc-level Lua filters (Pandoc/Doc/Meta) collected but
  never invoked."* The same class of bug in the same file: handlers collected
  into the filter table and then never dispatched. Worth reading its fix for
  the shape the team settled on.
- **bd-grkrb9nj** — built the two-track Lua conformance harness
  (`tests/lua-conformance/`), including the pandoc-oracle differential suite
  that is the natural home for this fix's regression tests.
- **bd-23yvjfmm** — the filter-return-value cluster; established the
  `filter_return_error` / `Q-11-4` diagnostic contract this fix should reuse
  for bad return values.

## What the code looks like today

Everything the strand says about the code is accurate at `808215fc`.

**The bug, in two lines.** `crates/pampa/src/lua/filter.rs:243-249`:

```rust
lua.load(&filter_source)
    .set_name(filter_path.to_string_lossy())
    .exec_async()          // <-- discards the chunk's return value
    .await?;

let filter_table = get_filter_table(&lua)?;   // <-- globals only
```

`get_filter_table` (`filter.rs:323`) walks a hardcoded list of ~50 element
names and copies same-named globals into a fresh table. Its doc comment
describes a contract the code never implemented — and never has: the function
arrived with the original Lua-filter landing (`0f9fc8da`, 2025-12-02) already
globals-only, and the 2026-04 async conversion (`e537fb80`) turned `exec()`
into `exec_async()` without changing the shape. So this is not a regression;
the returned-table form has never worked in q2.

```rust
// Pandoc filters can either:
// 1. Return a table with filter functions
// 2. Define filter functions as globals
// We'll support both by creating a table that checks globals
```

**Nothing downstream needs to change — verified, not assumed.**

- `apply_full_filter` (`filter.rs:505`) already takes `&Table` and is shared
  with `doc:walk` (`pandoc_doc.rs:143`), which is the strand's `walk-form.lua`
  proof.
- The walker is entirely **name-agnostic**: `inline_fn`/`block_fn`
  (`walk.rs:339,349`) look handlers up by tag with `filter.get::<Function>(tag)`,
  and `pass_is_active` (`walk.rs:421`) iterates the table's own keys rather than
  a whitelist. So a returned table needs **no name list at all** — the ~50-name
  whitelist is an artifact of the globals path only, and would keep applying
  just to that path.
- `get_walking_order` (`filter.rs:88`) already reads `traverse` off whatever
  table it is handed, so per-filter traversal mode comes for free — including
  per-entry traverse in the list form.

**A second, independent bug found while scoping the warning (bd-18a2r2lp).**
The walker dispatches on `LuaInline::tag_name` / `LuaBlock::tag_name`
(`types.rs:272,1155`), but the globals whitelist is a *separate* hand-written
list — and the two have drifted. Five dispatchable tags are missing from the
whitelist:

```
Attr, BlockMetadata, CaptionBlock, NoteDefinitionFencedBlock, NoteDefinitionPara
```

A filter defining any of these **as a global** is silently dropped. Proven, not
inferred, using `doc:walk` (which hands the walker a table directly and so
bypasses the whitelist):

```lua
function NoteDefinitionPara(el)            -- global: never fires
  quarto.log.output("GLOBAL NoteDefinitionPara FIRED")
end
function Pandoc(doc)
  return doc:walk{ NoteDefinitionPara = function(el)   -- table: fires
    quarto.log.output("WALK-TABLE NoteDefinitionPara FIRED") end }
end
```

```
$ pampa note.md -F whitelist-gap.lua -t plain
WALK-TABLE NoteDefinitionPara FIRED     <- table path fires
(no GLOBAL line)                        <- globals path silently drops it
```

This matters here for two reasons. It is a live instance of exactly the class
the decided warning is meant to surface, and it settles *how* the warning must
be built: **derive the name set from `tag_name` plus the catch-alls**, rather
than adding a third hand-written list that can drift again. Filed as
**bd-18a2r2lp** (`discovered-from` this strand) so it can be fixed
independently if this plan stalls.

**The fix pattern already exists in-tree.** The shortcode loader
(`shortcode.rs:190`) does exactly this: `eval_async()` into a `Value`, and
`if let Value::Table(ref table) = ret` register the returned handlers. The
filter loader is the odd one out.

**Pandoc's reference semantics — pinned by probe, not by memory.** Probes run
against pandoc **3.9.0.2**, which is the version in
`tests/lua-conformance/differential/ORACLE_VERSION`, so every one of these can
become a differential case with a real oracle. Full table and scripts:
`2026-08-11-lua-filter-returned-table-form-investigation/pandoc-probes/`.

The rule:

1. Script returns **no value** → filter is built from the **globals**.
2. Script returns **any value** → that value is the filter; **globals are never
   consulted**:
   - `rawlen(t) == 0` → single filter table;
   - `rawlen(t) > 0` → list of filter tables, applied as successive passes
     (each with its own `traverse`);
   - not a table → **load-time error**.

Three consequences are non-obvious and worth stating explicitly, because they
are the difference between "handles the reported case" and "matches pandoc":

- `return {}` alongside global handlers → pandoc runs **nothing**. An empty
  returned table still wins; there is no fallback-to-globals.
- `return { Str = f, {Str = g} }` → pandoc runs **only `g`**. Disambiguation is
  by array length, and a non-empty array part discards the named keys.
- an explicit `return nil` → pandoc **errors** (`attempt to index a nil value`),
  it does not fall back to globals. Pandoc counts stack values, not nil-ness.

**Reproduced at HEAD.** The strand's four-document repro is copied to
`2026-08-11-lua-filter-returned-table-form-investigation/repro/` (identical
handler bodies, one per form). `q2 render` at `808215fc` renders 4 of 4 files,
exits 0, and prints **no diagnostic of any kind**; `index.html` and
`list-form.html` still contain the untouched `MARKER`, while the control and
walk-form documents show their handlers fired. Full transcript:
`repro/OBSERVED-AT-HEAD.md`.

**The parity matrix — including four rows that change behavior.** Running each
probe through both engines (`pampa … -F` vs `pandoc … -L`, the differential
suite's own comparison) gives:

| probe | script shape | pandoc 3.9.0.2 | q2 0.17.0 | |
| --- | --- | --- | --- | --- |
| `tf` | `return {Str=f}` | `TABLE-FORM-RAN` | `MARKER` | ✗ reported bug |
| `lf` | `return {{Str=f},{Str=g}}` | `LIST-FORM-RAN` | `MARKER` | ✗ reported bug |
| `hybrid` | `return {Str=f, {Str=g}}` | `ARRAY-RAN` | `MARKER` | ✗ |
| `trav` | `return {traverse='topdown', …}` | `STR-TOPDOWN-PARA` | `MARKER` | ✗ |
| `listtrav` | list, per-entry `traverse` | `S-P1` | `MARKER` | ✗ |
| `fnret` | `return function(x) … end` | **error** | `MARKER` | ✗ (silent) |
| `mixed` | global `Str` + `return {Str=g}` | `TABLE-RAN` | `GLOBAL-RAN` | ⚠ **flips** |
| `empty` | global `Str` + `return {}` | `MARKER` (nothing) | `GLOBAL-RAN` | ⚠ **flips** |
| `emptylist` | global `Str` + `return { {} }` | `MARKER` (nothing) | `GLOBAL-RAN` | ⚠ **flips** |
| `nilret` | global `Str` + `return nil` | **error** | `GLOBAL-RAN` | ⚠ **flips** |
| `num` | global `Str` + `return 5` | **error** | `GLOBAL-RAN` | ⚠ **flips** |

The ✗ rows are the bug as filed. The ⚠ rows are the part that is *not* a pure
addition: those five filters work today and would change behavior under strict
parity, the last two by starting to error. That is what design question 1 is
about. `run-parity-matrix.sh` in the probes directory regenerates the whole
table in one command and takes `PAMPA=…` to point at a patched build, so it is
also the fix's smoke check.

## Phases

Now that the four decisions are settled, these are real phases rather than
headings. Still to be confirmed with the user before implementation starts.

- **Phase 0 — Tests first (TDD).** Every test below must be **observed failing**
  before any fix lands.
  - Unit tests in `crates/pampa/src/lua/filter_tests.rs` (TempDir +
    `apply_lua_filter`, the existing idiom, 124 tests already): single table,
    filter list applied in order, `traverse` on a returned table, per-entry
    `traverse` in a list, globals-still-work when nothing is returned, and one
    test per ⚠ row asserting the *new* parity behavior (`mixed` runs only the
    table; `empty`/`emptylist` run nothing; `nilret`/`num` error).
  - Differential cases under `tests/lua-conformance/differential/cases/` with
    real pandoc oracles. `regen-oracles.sh` refuses to run unless the local
    pandoc matches `ORACLE_VERSION`; it currently does (3.9.0.2), so oracles
    can be generated as-is. The eleven probe scripts in
    `…-investigation/pandoc-probes/` map onto cases nearly one-to-one — note
    the error-shaped ones (`nilret`, `num`, `fnret`) cannot be oracle cases,
    since the harness compares JSON ASTs; those stay unit tests.
  - `run-parity-matrix.sh` with `PAMPA=` pointed at the patched build, as the
    end-to-end smoke check.
- **Phase 1 — Load-time classification.** `eval_async::<Value>()` in
  `apply_lua_filter`, then classify per the pinned rule: **no value** →
  globals via `get_filter_table`; **table with `rawlen == 0`** → single filter;
  **table with `rawlen > 0`** → list of filters; **anything else** → `Q-11-6`
  error. Note the classification hinges on *whether a value was returned at
  all*, which is a stack-count question, not a nil-ness question — an explicit
  `return nil` must land in the error branch, not the globals branch. That
  distinction is the single most easily-fumbled part of the change; it deserves
  its own test.
- **Phase 2 — Multi-pass application.** Apply each filter table in the list as
  a successive `apply_full_filter` pass over the whole document, in order, each
  honoring its own `traverse`.
- **Phase 3 — Diagnostics.**
  - `Q-11-6` for an invalid script return, naming the filter path and the Lua
    type, in the `filter_return_error` style from bd-23yvjfmm.
  - The unrecognized-handler-name warning (decision 3). **Build its name set
    from `tag_name` plus the catch-alls** — do not hand-write a third list.
    bd-18a2r2lp is the cautionary example of what a hand-written list does over
    time, and fixing it here (deriving the globals whitelist from the same
    source) makes the two changes one coherent piece of work rather than two.
  - Both new codes need `docs/errors/lua/<code>.qmd` in the same commit.
- **Phase 4 — Docs.** `docs/guides/authoring/lua-filters.qmd` shows only the
  top-level-function form and never mentions a returned table; document both
  forms, the ordered list of passes, `traverse`, and — since we are now strict
  about it — the rule that returning a value means globals are ignored.

## Settled decisions (user, 2026-08-11)

All four design questions are answered. Recorded with their reasoning, because
the reasoning is what a future reader needs when an edge case shows up.

1. **Strict pandoc parity, wholesale.** No softening for the shapes pandoc
   treats as errors. The rationale: every divergent shape is a filter doing two
   conflicting things at once (returning a value *and* defining globals), so
   there is no defensible "right" answer to pick between them — and where no
   choice is clearly correct, matching pandoc is the least harm. All five ⚠
   rows in the matrix flip, deliberately.

2. **Invalid script return → hard error with an actionable diagnostic.**
   Behavior matches pandoc (it is an error); the message does not — ours names
   the filter path and the offending Lua type. Explicit `return nil` is an
   error too, on the same footing as `return 5`. The framing that makes this
   feel right rather than pedantic: **`return { … }` is an affirmative choice**,
   whereas a global `Str = function` can plausibly be an accidental collision.
   A script that returns something uninterpretable has stated an intent we
   cannot honor, so saying so beats guessing.

3. **Yes — warn on unrecognized handler names.** Adopted into Phase 3 rather
   than deferred. The decisive argument is not typo-catching but **regression
   surfacing**: if q2 ever fails to recognize a handler name it should support,
   that gap reaches the user as a visible (if wrongly-emitted) diagnostic
   instead of silence. Silence mid-render is the thing that is hard to notice —
   which is the whole complaint behind this strand. bd-18a2r2lp is a live
   example of precisely that gap, found while scoping this.

4. **New error code `Q-11-6`,** "Invalid Lua Filter Script Return Value" —
   not a widened `Q-11-4`, which is about a *filter function's* return and
   gives different advice. `docs/errors/lua/Q-11-6.qmd` ships **in the same
   commit** (the `error-docs-page-missing` lint enforces this). The warning
   from decision 3 needs its own code as well — likely `Q-11-7`, with its own
   page, same rule.

## Adjacent findings (carried across; each needs its own decision)

Recorded here so they are not lost, but deliberately **not** folded into this
strand's scope.

- **Filter ordering vs the mermaid transform.** Under q2 a `{mermaid}` cell is
  still a `CodeBlock.mermaid` when user filters run;
  `MermaidRenderTransform` (`quarto-core/src/transforms/mermaid.rs:166`) is a
  `TransformPhase::Finalization` transform inside `AstTransformsStage`. Quarto 1
  is the other way round, which is why `mermaid-zoom.lua` matches `RawBlock`.
  **New information from this investigation, verified by running it.** q2 runs
  user filters at *two* positions — `UserFiltersStage::pre()` and `::post()`,
  either side of `AstTransformsStage` (`quarto-core/src/pipeline.rs:346-348`)
  — and `filter_resolve.rs` already supports a per-filter `at:` field over
  eight entry points plus a `quarto` sentinel. Probing with a filter that
  reports what it sees (` ```mermaid ` fence, `q2 render`):

  | filter declaration | what the filter sees |
  | --- | --- |
  | `filters: [probe.lua]` (default → `pre-quarto`) | `CodeBlock` classes `[mermaid]` |
  | `filters: [quarto, probe.lua]` | `RawBlock` html `<pre class="mermaid">` |
  | `filters: [{path: probe.lua, at: post-quarto}]` | `RawBlock` html `<pre class="mermaid">` |

  So **the Q1-equivalent `RawBlock` view is reachable today with a one-line
  metadata change and no code change.** That reduces the finding from "port
  blocker" to "defaulting question": is `pre-quarto` the right default for
  ported extensions, and is the choice discoverable? Worth its own strand.
  **Filing recommended — say the word and I will.**

  One correction to the strand's write-up while I was in here: at the default
  pre position the class is `mermaid` only for the plain ` ```mermaid ` fence.
  Q1's ` ```{mermaid} ` executable-cell spelling stays a `CodeBlock` with the
  literal class `{mermaid}` all the way through and renders no diagram — but
  that is **not** a bug to file: it is a deliberate, documented syntax decision
  (`claude-notes/plans/2026-07-20-mermaid-regular-rendering.md` § Syntax
  decision, guarded by a `brace_form_mermaid_cell_untouched` test). A ported
  extension has to use the plain fence regardless of filter position.
- **`quarto.doc.add_html_dependency` warns `Q-11-1` on `version`.** Assets do
  land under `_files/<name>/`; the `version` field is accepted-then-ignored
  with a warning. Cosmetic but noisy for ported extensions. Also worth its own
  strand.

## Risks / tradeoffs (draft)

- **Behavior change, not pure addition.** Design question 1 is the only real
  risk in the change: filters that today return a table *and* define globals
  would flip from running-the-globals to running-the-table. Such filters are
  presumably rare (the globals-plus-table combination is unusual), and any that
  exist are arguably already broken relative to pandoc — but the change is
  silent for them, which is the same failure mode this strand is about. A
  one-line `--verbose` note when a returned table shadows recognized globals
  would make it observable at essentially no cost.
- **Multi-pass application cost.** The list form runs `apply_full_filter` once
  per entry, i.e. a full document walk per pass. That is exactly what pandoc
  does, so it is parity rather than regression — but a filter list of N entries
  is N walks, worth knowing before someone reports it as slow.
- **Test surface is well-prepared, so cost is low.** `filter_tests.rs` has the
  TempDir + `apply_lua_filter` idiom (124 tests), and the differential suite
  has a working oracle regen path with the pinned pandoc already installed
  locally.
