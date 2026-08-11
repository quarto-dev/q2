# Lua filters in the returned-table form are silently ignored (bd-lua-filter-table-form-ignored-ph23becz)

**Date:** 2026-08-11
**Braid:** `bd-lua-filter-table-form-ignored-ph23becz` (bug, p1, labels `pampa` / `parity`)
**Branch:** `main` @ `808215fc` (investigated in the main checkout; no worktree created)
**Status:** In progress. Design settled 2026-08-11 (see **Settled decisions**);
Phases 0–2 complete — the returned-table and filter-list forms work and match
pandoc across all eleven probe shapes. Phases 3–5 (diagnostics, docs,
verification) remain. See the **Phase log**.

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

## Work items

Phases run in order; within a phase, items are ordered by dependency. Each
phase ends at a clean commit (workspace builds, full test suite green).

### Phase 0 — Tests first (TDD)

Every test here must be **observed failing** before any fix lands, and the
observed failure recorded in the phase log.

- [x] 0.1 Unit tests, returned-table forms — `crates/pampa/src/lua/filter_tests.rs`
      (TempDir + `apply_lua_filter`, the existing idiom): single table; list
      applied in order; `traverse` on a returned table; per-entry `traverse` in
      a list; handler name outside the old ~50-name whitelist works in a
      returned table.
- [x] 0.2 Unit tests, globals path unchanged — script returning nothing still
      builds its filter from globals.
- [x] 0.3 Unit tests, the five parity flips — `mixed` runs only the table;
      `empty` and `emptylist` run nothing; `nilret` and `num` error. These
      assert the *new* behavior and so fail in the opposite direction from the
      others (they pass today for the wrong reason — check the failure message,
      not just the red).
- [x] 0.4 Unit test, the stack-count distinction — explicit `return nil` errors
      while falling off the end uses globals. Called out separately because it
      is the single most fumbleable part of Phase 1.
- [x] 0.5 Differential cases + oracles under
      `crates/pampa/tests/lua-conformance/differential/cases/` for the
      AST-shaped probes (`tf`, `lf`, `hybrid`, `trav`, `listtrav`, `mixed`,
      `empty`, `emptylist`). Regenerate with `regen-oracles.sh` (local pandoc
      is 3.9.0.2, matching `ORACLE_VERSION`). The error-shaped probes
      (`nilret`, `num`, `fnret`) **cannot** be oracle cases — the harness
      compares JSON ASTs and pandoc produces none — so they stay unit tests.
- [x] 0.6 Record the observed failures (which tests, which messages) in the
      phase log below, then commit the failing tests.

### Phase 1 — Load-time classification

- [x] 1.1 Replace `exec_async()` with `eval_async::<Value>()` in
      `apply_lua_filter` (`filter.rs:243`).
- [x] 1.2 Classify the result per the pinned rule: **no value returned** →
      globals via `get_filter_table`; **table, `rawlen == 0`** → single filter;
      **table, `rawlen > 0`** → list of filters; **anything else** → `Q-11-6`
      error. The branch hinges on *whether a value was returned at all*, not on
      nil-ness — `return nil` is an error, not a fallback.
- [x] 1.3 Confirm 0.1–0.4 now pass; 0.5 differential cases pass.

### Phase 2 — Multi-pass application

- [x] 2.1 Apply each table in a returned list as a successive
      `apply_full_filter` pass over the whole document, in order, each honoring
      its own `traverse`.
- [x] 2.2 Confirm the list-form and per-entry-traverse tests pass.

### Phase 3 — Diagnostics

- [x] 3.1 Add `Q-11-6` ("Invalid Lua Filter Script Return Value") to
      `crates/quarto-error-catalog/error_catalog.json` **and**
      `docs/errors/lua/Q-11-6.qmd` in the same commit (the
      `error-docs-page-missing` lint enforces this).
- [x] 3.2 Emit `Q-11-6` from the Phase 1 error branch, naming the filter path
      and the offending Lua type, in the `filter_return_error` style from
      bd-23yvjfmm.
- [x] 3.3 Build the canonical handler-name set from `LuaInline::tag_name` /
      `LuaBlock::tag_name` plus the catch-alls (`Pandoc`, `Doc`, `Meta`,
      `Inline`, `Inlines`, `Block`, `Blocks`). **One source of truth** — do not
      hand-write a third list.
- [x] 3.4 Point `get_filter_table`'s globals scan at that set, which fixes
      **bd-18a2r2lp** (the five dropped tags) as a side effect rather than as
      separate work.
- [x] 3.5 Add the unrecognized-handler-name warning (new code, likely
      `Q-11-7`, with its own docs page) — for both the returned-table and
      globals paths.
- [x] 3.6 Tests for 3.2 and 3.5, plus a regression test for bd-18a2r2lp
      (a global `NoteDefinitionPara` handler fires).

### Phase 4 — Docs

- [x] 4.1 `docs/guides/authoring/lua-filters.qmd`: document the returned-table
      form, the ordered list of passes, and `traverse` — today the page shows
      only the top-level-function form and never mentions a returned table.
- [x] 4.2 Document the strictness rule explicitly: returning a value means
      globals are ignored entirely.

### Phase 5 — Verification and close-out

- [x] 5.1 `run-parity-matrix.sh` with `PAMPA=` pointed at the patched build;
      paste the resulting table into the phase log. Every row must match the
      pandoc column except the three error rows, where q2 should error with a
      `Q-11-6` message rather than pandoc's `attempt to index a …`.
- [ ] 5.2 End-to-end through the real binary: `q2 render` in
      `…-investigation/repro/`, confirming `index.html` and `list-form.html`
      now show `TABLE-FORM-RAN` and `LIST-FORM-RAN`. Inspect the output, do not
      infer from exit code.
- [ ] 5.3 `cargo xtask verify` (full, not `--skip-hub-build` — pampa is in the
      WASM closure). Remember `test:wasm` needs a fresh WASM artifact.
- [ ] 5.4 Update the strand; report to the user before pushing.

## Phase log

Appended as work lands, so a later session can pick up mid-plan.

### 2026-08-11 — Phases 0–2 (tests, load-time classification, multi-pass)

**Commit convention note.** Phase 0 is TDD, so its artifact is a *failing*
suite — which conflicts with the repo rule that a phase-boundary commit is
green. Resolved by observing and recording the failures here, then committing
Phases 0–2 together once green. The failure evidence below is the substitute
for a red commit.

**0.1–0.4 observed failing** (13 unit tests, before any src change; each ran in
both the `pampa` lib and `bin/pampa` test binaries):

```
test_returned_table_form_runs_handlers                     left: "MARKER"      right: "TABLE-RAN"
test_returned_list_applies_passes_in_order                 left: "MARKER"      right: "PASS-TWO"
test_returned_table_honors_traverse_topdown                left: "x"           right: "S-P"
test_returned_list_traverse_is_per_entry                   left: "x"           right: "Q"
test_returned_table_dispatches_name_outside_globals_…      (handler never ran)
test_hybrid_table_prefers_array_part                       left: "MARKER"      right: "ARRAY-RAN"
test_empty_returned_table_disables_globals                 left: "GLOBAL-RAN"  right: "MARKER"
test_empty_returned_list_disables_globals                  left: "GLOBAL-RAN"  right: "MARKER"
test_returned_table_shadows_globals                        left: "GLOBAL-RAN"  right: "TABLE-RAN"
test_explicit_return_nil_is_an_error                       got Ok("GLOBAL-RAN")
test_non_table_return_is_an_error                          got Ok("GLOBAL-RAN")
test_returned_function_is_an_error                         got Ok("MARKER")
test_globals_used_when_script_returns_nothing              PASS (control, correctly green throughout)
```

Each failure is for the *right* reason: the returned-table tests show the input
unchanged (the filter never ran), the parity flips show `GLOBAL-RAN` where the
table should have won, and the error tests show `Ok(...)` where an error was
required.

**0.5 observed failing** — all 8 new differential cases reported by the ratchet
as `unexpected FAILURE (not in differential/xfail.txt)`. No `xfail.txt` entries
were added: the cases went from red to green within this same work session, so
the ratchet never needed to record a gap. Oracles were generated with
`regen-oracles.sh` against pandoc 3.9.0.2 and **no existing oracle changed**
(checked with `git status` after the regen, since the script rewrites all
cases).

**1.1–1.2 — two implementation decisions worth recording**, because the obvious
reading of the plan would have got both wrong:

- **`call_async`, not `eval_async`.** mlua's `eval` first tries to compile the
  source as an *expression* and only falls back to a block. That would make q2
  accept a filter file containing a bare `{ Str = f }` with no `return` — which
  pandoc rejects as a syntax error. `call_async` compiles strictly as a block,
  matching pandoc's `dofile`. (The shortcode loader uses `eval_async` and has
  the same latent widening; out of scope here, not filed — it is a different
  contract with no pandoc equivalent.)
- **`MultiValue`, not `Value`.** `Value` collapses "returned nothing" and
  "returned nil" into the same `Value::Nil`, but those must diverge: the first
  uses the globals, the second is an error. The classifier therefore switches
  on `returned.is_empty()` — a *count* of returned values. This is the
  distinction test 0.4 exists to pin.

**Results.** 13/13 unit tests pass; all 8 differential cases match pandoc's
oracle after normalization; full `pampa` suite 4360/4360 green.

**Parity matrix against the patched build** (`run-parity-matrix.sh`, work item
5.1 done early since the script was already at hand):

```
probe      | pandoc                   | q2
-----------+--------------------------+-------------------------
tf         | Marker: TABLE-FORM-RAN   | Marker: TABLE-FORM-RAN
lf         | Marker: LIST-FORM-RAN    | Marker: LIST-FORM-RAN
hybrid     | Marker: ARRAY-RAN        | Marker: ARRAY-RAN
trav       | STR-TOPDOWN-PARA         | STR-TOPDOWN-PARA
mixed      | Marker: TABLE-RAN        | Marker: TABLE-RAN
empty      | Marker: MARKER           | Marker: MARKER
nilret     | Error running filter nil | Filter error: … Q-11-6 …
num        | Error running filter num | Filter error: … Q-11-6 …
fnret      | Error running filter fnr | Filter error: … Q-11-6 …
emptylist  | Marker: MARKER           | Marker: MARKER
listtrav   | S-P1                     | S-P1
```

Every AST-shaped row matches; the three error rows error in both engines, with
q2 giving the better message.

**Carried into Phase 3:** the error currently surfaces as
`Filter error: Lua filter error: runtime error: Q-11-6: …` — three nested
prefixes before the code. Phase 3.2 should render it as a proper diagnostic
rather than a stringified runtime error.

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

### 2026-08-11 — Phases 3–4 (diagnostics, docs)

**3.3/3.4 — the drift-proof name set, done properly.** The plan said "derive
the set from `tag_name`", but `tag_name` is a method on a *value*, so there was
nothing to enumerate. Rather than hand-writing the list a second time and
adding a test to keep the two in step — which is the same arrangement that
produced bd-18a2r2lp, just with an alarm on it — `tag_name` is now generated by
an `element_tag_names!` macro that emits **both** the match arms and a
`&[&str]` of every name it can return (`INLINE_TAG_NAMES`, `BLOCK_TAG_NAMES` in
`types.rs`). The match stays exhaustive, so a new AST variant fails to compile
until it is listed, and the constant cannot disagree with the dispatch because
both come from the same lines. Drift is now unrepresentable rather than merely
detected.

`get_filter_table`'s ~50-name whitelist is deleted in favor of
`recognized_handler_names()` over those constants plus the seven catch-alls.
That fixes **bd-18a2r2lp** as a side effect: `pampa note.md -F whitelist-gap.lua`
now prints `GLOBAL NoteDefinitionPara FIRED` where it printed nothing before.

**3.2 — error rendering.** `Q-11-6` gets its own `LuaFilterError::InvalidScriptReturn`
variant that renders its message verbatim, instead of riding on
`mlua::Error::runtime` and arriving as
`Filter error: Lua filter error: runtime error: Q-11-6: …`. It now reads:

```
Filter error: Q-11-6: Lua filter 'num.lua' returned number, which is not a
filter. A filter script may return a table of handler functions, a list of such
tables (applied as successive passes), or nothing at all (in which case the
handlers are taken from the script's global functions).
```

**3.5 — the warning, and one thing that fell out nicely.** `Q-11-7` warns about
function-valued keys that are not names q2 dispatches, with a
smallest-edit-distance suggestion (`'Strs'` → *Did you mean 'Str'?*), naming the
pass when the filter is a list. Two scoping decisions worth recording:

- **Only function-valued keys warn.** `traverse` is a string and data parked on
  the table is not a function, so both are ignored without needing a special
  case — matching pandoc's tolerance for extra keys.
- **The globals path cannot produce a false positive.** The globals-derived
  table is *built from* the recognized set, so scanning uniformly across every
  filter table is safe: a user's unrelated global helper can never reach the
  warning. This is why there is no "is this from globals?" flag threaded
  through. A test pins it (`test_globals_form_never_warns`).

  Worth being explicit about the limitation this implies: a **typo in a global
  handler name** (`function Strs(el)` at top level) still cannot be warned
  about, because there is no way to tell it from an ordinary helper function.
  The warning covers the returned-table form only. That is a real gap, and the
  honest mitigation is the docs note steering authors toward `local` helpers.

**Live output:**

```
$ pampa input.md -F typo.lua -t plain
Warning [Q-11-7]: Lua filter 'typo.lua' defines a handler 'Strs', which is not
an element type Quarto knows about, so it will never run. Did you mean 'Str'?
```

**3.1 — catalog + pages.** `Q-11-6` and `Q-11-7` added to
`error_catalog.json` with `docs/errors/lua/Q-11-6.qmd` and `Q-11-7.qmd` in the
same change; `cargo xtask lint` (which enforces exactly this) passes.

**4.1/4.2 — docs.** `docs/guides/authoring/lua-filters.qmd` gains three
sections: the two ways to organize a filter (with a callout on the returned
table replacing the globals), several passes in a fixed order, and when a
handler never runs. Rendered with `q2` (never Q1, per CLAUDE.md) and the output
inspected — the first attempt used `/docs/errors/…` absolute links, which the
project rejected because the project root *is* `docs/`; fixed to `../../errors/…`,
matching `extensions.qmd`. While there, the sidebar in `docs/_quarto.yml` listed
only `Q-11-1` of the six existing lua pages; all seven are now listed.

**Test counts.** pampa 4378/4378 green (up from 4360: 13 return-value tests, 2
bd-18a2r2lp regressions, 7 warning/edit-distance tests). `cargo xtask lint`
clean.

### 2026-08-11 — Phase 5 (verification)

**5.1/5.2 done** and recorded above / in `repro/OBSERVED-AT-HEAD.md`.

**5.3 — the gate caught something the tests did not.** `cargo nextest run
--workspace` was 11708/11708 green and `cargo xtask lint` was clean, but
`cargo xtask verify` failed at *step 1 of 14* on clippy's
`needless_borrows_for_generic_args`: `DiagnosticMessageBuilder::warning(&format!(…))`
where the builder takes `impl Into<String>`, so the `&` is redundant. One
character. This is exactly the gap CLAUDE.md's git-push policy warns about —
`cargo build` and `cargo nextest` do not run with `-D warnings`, so a clean
test run says nothing about whether CI will accept the change. Worth
remembering: run `verify` *before* believing a phase is done, not after.
