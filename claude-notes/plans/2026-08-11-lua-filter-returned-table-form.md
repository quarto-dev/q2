# Lua filters in the returned-table form are silently ignored (bd-lua-filter-table-form-ignored-ph23becz)

**Date:** 2026-08-11
**Braid:** `bd-lua-filter-table-form-ignored-ph23becz` (bug, p1, labels `pampa` / `parity`)
**Branch:** `main` @ `808215fc` (investigated in the main checkout; no worktree created)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

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

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion below.

- **Phase 0 — Tests first.**
  - Unit tests in `crates/pampa/src/lua/filter_tests.rs` (TempDir + `apply_lua_filter`,
    the existing pattern): table form, list form, per-entry `traverse`,
    globals-still-work, and whichever edge-case semantics Q1 of the design
    questions settles on.
  - Differential cases under `tests/lua-conformance/differential/cases/`
    with real pandoc oracles (`regen-oracles.sh`; local pandoc already matches
    the pinned 3.9.0.2). Candidates: `filter-returns-table`,
    `filter-returns-filter-list`, `filter-returns-table-traverse`,
    `filter-returns-list-per-entry-traverse`. Each must be *observed failing*
    before the fix.
- **Phase 1 — Load-time change.** `eval_async::<Value>()` in `apply_lua_filter`;
  classify the result (list / table / absent / invalid); keep `get_filter_table`
  as the globals fallback.
- **Phase 2 — Multi-pass application.** Apply a list of filter tables as
  successive `apply_full_filter` passes over the document, in order.
- **Phase 3 — Diagnostics.** Whatever Q2 below settles on for invalid returns
  (and, if we choose it, the "declared filter did nothing" signal).
- **Phase 4 — Docs.** `docs/guides/authoring/lua-filters.qmd` currently shows
  only the top-level-function form and never mentions the returned table;
  document both, plus the list form and `traverse`.

## Open design questions for the user

1. **Full pandoc parity on the shadowing rules, or a friendlier subset?**
   Pandoc's rule is "any returned value wins outright" — so `return {}` next to
   working global handlers silently disables them, and a mixed
   globals-plus-returned-table filter runs only the table. Strict parity is my
   recommendation (predictability for ported filters, and it is what
   `lua_differential` would assert), but the matrix above shows it flips four
   filter shapes that work today. Do we take parity wholesale, or fall back to
   globals in the shapes pandoc treats as errors (`nilret`, `num`) and
   knowingly diverge there?

2. **What should an invalid return value do?** Pandoc errors, with a bad
   message (`attempt to index a number value`, naming neither the filter nor
   the offending value). Options: (a) error with a proper `Q-11-x` diagnostic
   naming the filter path and the returned Lua type — strict on behavior,
   better on message; (b) warn and fall back to globals. I lean (a), reusing
   the `filter_return_error` contract from bd-23yvjfmm. Does an explicit
   `return nil` get the same treatment as pandoc (error), or is that a
   parity detail worth softening?

3. **Do we also want a "declared filter did nothing" signal?** The strand's
   sharpest complaint is not the missing feature but the silence. Even after
   this fix, a filter whose returned table has no keys the walker ever looks
   up (e.g. a typo'd `Strs`, or a Q1-only handler name) stays silent. A
   load-time warning for "filter defines no recognized handlers" would catch
   that whole class. It is out of the literal scope of this strand — should it
   be a follow-up strand, or folded into Phase 3 here?

4. **New error code, or stretch `Q-11-4`?** The existing `Q-11-4` is
   *"Invalid Lua Filter Return Value"* and its message is specifically about a
   **filter function's** return ("Return nil (keep the element), an element, a
   list of elements…"). A bad **script-level** return is a different thing with
   different advice, so my read is that it wants its own code (`Q-11-6`,
   "Invalid Lua Filter Script Return Value") rather than a widened `Q-11-4`.
   If we add one, the `error-docs-page-missing` lint requires
   `docs/errors/lua/Q-11-6.qmd` **in the same commit**.

## Adjacent findings (carried across; each needs its own decision)

Recorded here so they are not lost, but deliberately **not** folded into this
strand's scope.

- **Filter ordering vs the mermaid transform.** Under q2 a `{mermaid}` cell is
  still a `CodeBlock.mermaid` when user filters run;
  `MermaidRenderTransform` (`quarto-core/src/transforms/mermaid.rs:166`) is a
  `TransformPhase::Finalization` transform inside `AstTransformsStage`. Quarto 1
  is the other way round, which is why `mermaid-zoom.lua` matches `RawBlock`.
  **New information from this investigation:** q2 runs user filters at *two*
  positions — `UserFiltersStage::pre()` and `::post()`, either side of
  `AstTransformsStage` (`quarto-core/src/pipeline.rs:346-348`) — and
  `filter_resolve.rs` already supports a per-filter `at:` field over eight
  entry points plus a `quarto` sentinel. A bare `filters: [x]` defaults to
  `pre-quarto` (→ `Position::Pre`, sees the `CodeBlock`); `at: post-quarto`
  (or listing the filter *after* the `quarto` sentinel) puts it in
  `Position::Post`, downstream of `AstTransformsStage`, where the `RawBlock`
  exists. So **the Q1-equivalent behavior looks reachable today with a
  one-line metadata change and no code change** — that wants confirming by
  actually running it, and then the real question is whether bare `filters:`
  defaulting to pre is the right default for ported extensions. Worth its own
  strand. **Filing recommended — say the word and I will.**
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
