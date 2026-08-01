# Porting a Quarto 1 feature to Quarto 2

A process guide, distilled from the shortcode-extensions port
(bd-540a976a, `claude-notes/plans/2026-07-31-shortcode-extensions-port.md`).
Follow it when the task is "study whether Q1 feature X exists in Q2 and
port what's missing."

## The two levers (read this first)

Q2 ports aim for direct compatibility, *except* where Q2's structural
advantages let us do better. Every deliberate divergence must be justified
by one of exactly two levers:

1. **Strictness with actionable diagnostics.** Q1 often guessed what the
   user meant because it couldn't point at source locations. Q2 can:
   every AST node carries `SourceInfo`. Added strictness is acceptable
   iff the failure produces a source-mapped, `Q-*`-coded, actionable
   message. (Example: unknown shortcodes warn with location instead of
   silently passing through.)
2. **Explicit declaration over inference.** Where Q1 inferred (engine
   detection, pass-through-anything-unknown), Q2 asks the user to declare
   (`engine:` metadata, `shortcode-passthrough: [names]`). An inference
   the user can't see can't be diagnosed; a declaration can be validated.

Everything else should behave like Q1, *including its documented quirks*.
Undocumented Q1 bugs (e.g. the shadowed `local result` in
`shortcodes.lua`) are candidates to drop — but record the decision.

## Phase A — Study (before writing any plan)

Run **three parallel studies** and only then reconcile:

- **Q1 documented contract** — `external-sources/quarto-web`. What users
  were promised. Collect: syntax, config keys, authoring workflow, escape
  hatches, documented restrictions, and the changelog entries (they
  encode contract details the prose never mentions, e.g. "must not crash
  on bad input" fixes).
- **Q1 implementation** — `external-sources/quarto-cli`. What actually
  happens: real data shapes, dispatch order, precedence rules, error
  behavior, and *where* in Q1's architecture the feature runs (TS
  pre-engine? Lua filter? postprocessor?). Get `file:line` references.
- **Q2 current state** — this repo. **It is rarely zero.** Check, in
  order: `claude-notes/plans/` + `claude-notes/designs/` (prior epics
  often did phases of this already), the braid skein (`braid list`, grep
  `.braid/snapshot.jsonl`), then the crates. Existing open strands often
  reframe the work from "port a feature" to "close gaps".

The interesting deltas:
- **doc vs Q1 impl** → undocumented features (decide: port or drop) and
  under-documented contract points (the impl is the contract);
- **Q1 impl vs Q2** → the gap list;
- **Q1 tests** → `external-sources/quarto-cli/tests/` encodes the
  de-facto contract better than the docs. Port fixtures, don't reread
  prose.

**Spot-check the studies against the tree before trusting them.** If
subagents did the reading, grep the load-bearing claims yourself (does
that handler exist? is that CLI a stub? is that enum variant reachable?).

**Ask which Q1 mechanisms were workarounds for infrastructure Q2
replaced.** Q1 had four shortcode parsers only because Pandoc's reader
was in the way; Q2 parses in-grammar and the whole problem class (and
its documented restrictions, e.g. grid tables) evaporates. Don't port
mechanism-by-mechanism; port the contract.

## Phase B — Plan document

Write `claude-notes/plans/YYYY-MM-DD-<feature>-port.md` with:

1. **The Q1 contract, condensed** — what a user relying on the feature
   gets to assume. Cite `file:line` for each claim.
2. **Gap table** — one row per contract item: Q2 status (✅/⚠️/❌),
   precise gap, fix sketch.
3. **Design decisions** — every divergence, tagged `[decided]` or
   `[needs user sign-off]`, each justified by lever 1 or 2. Flag scope
   exclusions (defer big adjacent features to their own strands).
4. **Phased checklist**, Phase 0 always being the compatibility corpus.
5. **Related strands** (link with `braid dep add ... --type related`) and
   prior art in claude-notes.

Create a braid epic referencing the plan. Iterate with the user; get
explicit sign-off on the decision points *before* implementing. Record
their answers in the plan (`[decided YYYY-MM-DD]`) and in a braid
comment — sessions end; the plan file is the memory.

## Phase C — Compatibility corpus (Phase 0 of every port)

Encode the contract as **many small fixtures** before fixing anything:

- Port Q1's own test fixtures (`tests/docs/<feature>/`, smoke tests) —
  copy into the repo, never reference `external-sources/` from tests.
- Author contract fixtures: one per contract item, uniquely-grepable
  output tokens (`DASH-OK`, `META[deep-val]`).
- Include at least one **real published extension/document** as an
  acceptance fixture, plus a real-world project if one is at hand
  (connect-docs caught the extension-id≠shortcode-name bug that all
  synthetic fixtures initially missed — realistic naming matters;
  synthetic fixtures tend to accidentally mirror implementation
  assumptions).
- Run the corpus. Sort failures into: (a) TDD targets for a phase —
  keep the fixture uncommitted (or `tests.run.skip` with a reason
  naming the strand) until its fix lands; (b) accepted deviations —
  assert Q2's actual behavior and record the deviation in the plan;
  (c) deferred gaps — park with `tests.run.skip` + reason.

Never commit a red fixture without a skip reason. Never enshrine
known-bad behavior as a passing assertion.

## Phase D — Implement, one gap at a time

Strict TDD loop per gap (non-negotiable, per CLAUDE.md): failing test →
verify the failure is the expected one → fix → test green → full
`cargo nextest run --workspace` (monorepo: your pampa change breaks
quarto-core consumers; the meta-structuring change here broke video's
auto-stretch gate, caught only by the workspace suite).

Commit in checkpoint-sized units (one theme per commit), message
explaining Q1 parity rationale and naming the strand. Update the plan
checkboxes and braid comments as you go.

### Diagnostics idiom

- New failure class → new `Q-*` code in
  `crates/quarto-error-catalog/error_catalog.json` (`since_version:
  "99.9.9"` placeholder). Claim a subsystem number deliberately
  (extensions = 16).
- Emit via `DiagnosticMessageBuilder::warning(...).with_code("Q-16-N")
  .problem(...).add_hint(...).with_location(source_info)`.
- **Misattribution is the enemy**: a failure at load/discovery time must
  be reported *there*, naming the failing artifact — never left to
  surface later as a confusing failure attributed to the user's document
  (bd-nzdm1wry pattern).
- Advisory notices that must not break `--strict` or the smoke suite's
  default assertion → `DiagnosticMessageBuilder::info` (info does not
  trip `noErrorsOrWarnings`).

### End-to-end verification (per CLAUDE.md, non-negotiable)

Tests passing ≠ done. Render a real fixture through `cargo run --bin q2
-- render`, grep the actual output, and record invocation + observed
output in the plan/transcript. If a real-world project exhibits the bug,
verify against it after the fix.

## Repo gotchas (learned the hard way; will bite again)

- **`include_dir!` staleness**: adding files under `resources/` does not
  trigger a rebuild; `touch` the Rust file containing the macro (e.g.
  `crates/quarto-core/src/extension/mod.rs`) or the new resource is
  silently absent from the test binary.
- **`metadata-as-str` failure class**: bare YAML strings in
  document-metadata context are `ConfigValueKind::PandocInlines`;
  `as_str()` silently returns `None`. Use `as_plain_text()`. The lint
  only catches some shapes — check every metadata read by hand.
- **Smoke fixture assertions are parsed as markdown**: an
  `ensureFileRegexMatches` pattern containing `<strong>` triggers
  "HTML element converted to raw HTML" warnings that fail the fixture's
  own default assertion. Write patterns without `<` (e.g.
  `strong>TOKEN`).
- **`noErrorsOrWarnings` is on by default** in smoke fixtures;
  `printsMessage` alone does not suppress it — pair with `noErrors:
  true`. The captured message text is the diagnostic *title*, not the
  problem body.
- **`nextest` runs each test in its own process** — a `process::exit`
  in library code shows up as an opaque test failure; also means
  `std::env::set_var` in tests is race-free.
- **Interior state must live with the resource it describes**: a
  loaded/initialized flag on a transform outlives the per-document
  engine; wrap them together (`LuaEngineState`) so a fresh engine can't
  see a stale flag.
- **Precedence is part of the contract**: registration/override order
  (doc-level < extension < built-in; later file wins) must be chosen
  deliberately, tested, and diagnosed when shadowing occurs.

## Phase E — Wrap-up

- Full `cargo xtask verify` (not just `--skip-hub-build`) when pampa /
  quarto-core / quarto-pandoc-types changed — the WASM leg compiles a
  different target and the hub-client vitest sweep runs the same
  smoke-all fixtures.
- `cargo xtask lint`.
- Plan checkboxes current; braid comment summarizing commits, remaining
  gaps, and decisions still pending; file follow-on strands
  (`--deps discovered-from:<epic>`) for anything deferred.
- Do not push without explicit user approval.
