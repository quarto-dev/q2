# The q2-preview idempotence contract

A note for transform / filter authors. Read this before adding a new
Rust transform to `build_q2_preview_transform_pipeline`, a new stage
to `build_q2_preview_pipeline_stages`, or a new built-in Lua filter
under `resources/extensions/`.

The contract is enforced by the CI gate at
`crates/quarto-core/tests/idempotence.rs`, which is the Phase-3
deliverable of the provenance epic. The full design lives in
`claude-notes/plans/2026-05-04-q2-preview-plan-3-builtin-filter-idempotence.md`.

## What the contract says

Running the q2-preview pipeline twice on the same input must produce
the same structural AST: identical `blocks` hash and identical `meta`
hash with `meta.rendered.*` excluded.

"Same input" means the same byte sequences for the same file layout —
but **not** necessarily the same absolute paths. Each idempotence
fixture runs both pipeline invocations inside a fresh `TempDir`, so
the project root differs across runs while the content is identical.
A transform that captures the absolute project root into the AST will
fail the gate.

## What the hash includes and excludes

Defined by `compute_blocks_hash_fresh` /
`compute_meta_hash_fresh_excluding_rendered` in
`crates/quarto-ast-reconcile/src/hash.rs`.

Included:

- All block / inline structure (type, text, attributes, children).
- All meta tree structure: scalars by `Yaml` payload; `Map` entries
  in **insertion order** (no sort); `Array` entries in order;
  `merge_op` on every `ConfigValue`.
- `PandocInlines` / `PandocBlocks` payloads inside meta values,
  recursed via the existing block/inline hashers.

Excluded:

- `SourceInfo` on every block, inline, and `ConfigValue`.
- `key_source` on every `ConfigMapEntry`.
- Top-level `meta.rendered.*` — chrome transforms, `IncludeResolveStage`,
  the favicon transform, and Bootstrap/clipboard injection populate
  HTML/text strings under `rendered.*` that may legitimately vary in
  trivial whitespace or attribute ordering; HTML-shape canonicalization
  is a different concern.

Source-info is excluded by design so Plan 4's source-info churn
doesn't break the contract.

## What this means in practice

A new transform / stage / filter must:

### 1. Not depend on undefined-iteration-order state

If you populate a `Map` value in `meta` from a `HashMap`, the
iteration order is undefined and two runs will produce different
hashes. The gate uses insertion-order map hashing precisely to catch
this — sorting would silently mask it.

Use `Vec<(key, value)>`, `BTreeMap`, or `LinkedHashMap` and append
in a deterministic order.

### 2. Not capture process-local state into the AST

No timestamps, no PIDs, no random IDs, no absolute paths derived
from the project root, no `temp_dir()` output. If you need to refer
to a file, emit a path relative to the project root.

Source-info is the only legitimate place absolute paths live, and
the hash excludes source-info by design.

### 3. Use fresh Lua state per pipeline run (Lua filters / shortcodes)

The shortcode resolver and per-filter Lua engine are constructed
fresh inside their respective transforms; do not stash global state
on `_G` and expect it to survive between runs. If you need a cache,
key it by the *filter* identity, not the *pipeline run* — and clear
it on `Lua` construction.

### 4. Not execute engine cells

CI doesn't run Jupyter / Knitr. Fixtures use only fenced code blocks
(`` ```python `` etc.) — AST nodes, not executed. If your transform's
behavior is conditional on engine-execution side effects, the gate
cannot exercise it.

## Adding a fixture when you add a new transform

Every new transform / filter must come with at least one fixture
that exercises its happy path. Add it to
`crates/quarto-core/tests/idempotence.rs`:

- Trivial single-page fixture: use the `doc_fixture(name, content)`
  helper. Writes `index.qmd` to a fresh `TempDir` and runs both
  `DriveMode::SingleFile` and `DriveMode::ProjectOrchestrator`.
- Multi-file fixture (sibling files, includes, image resources):
  write an inline `setup` closure that writes everything into the
  fresh `TempDir`. Same dual-mode run.
- Website-chrome / link / listing fixture: use
  `modes: ORCHESTRATOR_ONLY`. Chrome transforms need a populated
  `ProjectIndex`, which only the orchestrator pass-1 builds.
- Attribution exercise: set `attribution_json: Some(...)` with a
  deterministic transport-shape JSON; `PreBuiltAttributionProvider`
  is installed on the `RenderContext` automatically. Do not use
  `GitBlameProvider` here — it depends on actual git history.

See `crates/quarto-core/tests/fixtures/idempotence/README.md` for
the per-fixture rules (no engine cells, no absolute paths, mode
mapping).

## If your new fixture fails on first run

Two possibilities:

1. **Your transform really is non-deterministic.** Trace the
   `DivergencePoint` the panic message hands you (block index, or
   meta key path) and fix the underlying state — usually a
   `HashMap` iteration, a `SystemTime::now()`, or an absolute path
   stuffed somewhere it shouldn't be.

2. **The hasher is wrong.** Vanishingly unlikely with FxHasher,
   but if you've ruled out (1), file a bug against
   `quarto-ast-reconcile`.

Per the plan's long-lived-integration-branch policy, **do not
`#[ignore]` the failing test** without explicit user approval.
Failing fixtures are the triage backlog; the integration branch
(`feature/provenance`) is allowed to be red while the queue is
drained.

## Related

- `claude-notes/plans/2026-05-04-q2-preview-plan-3-builtin-filter-idempotence.md` —
  the plan that introduced this gate, with the design rationale.
- `claude-notes/plans/2026-05-04-q2-preview-plan-7a-user-filter-idempotence.md` —
  the runtime counterpart: per-user-filter idempotence detection at
  render time, with `idempotent: false` opt-out. The contract this
  file describes is the CI-time half for built-ins; Plan 7a is the
  runtime half for user filters.
- `crates/quarto-ast-reconcile/src/hash.rs` — the hash implementations
  and unit tests.
- `crates/quarto-core/tests/idempotence.rs` — the gate.
- `crates/quarto-core/tests/fixtures/idempotence/README.md` — the
  fixture-format rules.
