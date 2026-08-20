# Q-1-20 discards the underlying markdown diagnostic for config values (bd-q120-masks-config-md-diagnostic-a039r80t)

**Date:** 2026-08-19
**Braid:** bd-q120-masks-config-md-diagnostic-a039r80t
**Checkout:** main checkout at `/Users/cscheid/rooms/room-3/q2`, branch `main` @ `6bee9ebe`
**Status:** Design settled 2026-08-20 (user answered all four questions); implementation in progress.

## Triage verdict

**Ready to design.** The strand's root-cause analysis is accurate at HEAD, the repro
reproduces, and the fix surface is small — but the strand's optimistic note that
"the child spans should already be in config coordinates" is **wrong**, and that
changes the shape of the fix (a span-remapping step is required, see below).

## Issue context

Filed 2026-08-20 (UTC) by a Claude session in the external `q2-connect-docs` repo
(origin strand `br-q120-masks-config-md-diagnostic-irbgj5ht` in that repo's skein).
Type: feature, priority 3, label `diagnostics`. Status: open.

When a config metadata value fails to parse as markdown,
`parse_yaml_string_as_markdown_to_config` (crates/pampa/src/pandoc/meta.rs) emits
only the generic Q-1-20 ("Could not parse '…' as markdown", plus the `!str` hint)
and **discards the underlying parser diagnostics** — the `Err(_parse_errors)` arm's
leading underscore says it. The same markdown in a document body produces the
precise Q-2-3 ("Key-value Pair Before Class Specifier in Attribute") with a
two-part span. Porting relevance: Pandoc/Q1 accept either attribute order, so
kv-before-class markdown that worked for years in Q1 configs fails in q2 configs
with no explanation of *what* to fix.

## Dependency graph

**Empty** — no edges in this repo's skein (`braid dep tree` / `dep list` show
nothing). The discovered-from context lives in the *external* q2-connect-docs
skein and is summarized in the description itself, which is unusually thorough.
No incoming `blocks` pressure; priority 3 reflects that.

## What the code looks like today

All paths in the description still exist with the described shape (verified at
`6bee9ebe`):

- **The discard site:** `crates/pampa/src/pandoc/meta.rs:120` —
  `Err(_parse_errors)` receives `Vec<DiagnosticMessage>` from
  `readers::qmd::read` and drops it, then builds the generic Q-1-20 in both the
  `!md` (error) and untagged (warning) branches.
- **The Ok arm one branch up** (meta.rs:107–111) already forwards
  recursive-parse *warnings* into the collector, so the forwarding plumbing
  exists — for the Ok path only.

### Key correction to the strand's suggested fix

The strand asserts "the parse is seeded with the scalar's SourceInfo, so the
child spans should already be in config coordinates." **That holds only for the
Ok path.** Ok-path spans are built via `node_source_info_with_context` /
`range_to_source_info_with_context` (crates/pampa/src/pandoc/location.rs:214,
257), which reroot through `context.parent_source_info` with
`SourceInfo::substring`. The **Err path does not**: `readers::qmd::read`
(crates/pampa/src/readers/qmd.rs:131) calls `produce_diagnostic_messages`
*without* the parent SourceInfo, and the generic generator
(crates/quarto-parse-errors/src/error_generation.rs:139, 217) builds every
location — the main span *and* the note spans — as
`SourceInfo::from_range(FileId(0), …)`, i.e. raw offsets into the embedded
string against the throwaway `<metadata>` file. Forwarding those diagnostics
naively would render spans against the wrong file — exactly the bd-m6wmztln
bug class the `add-file-with-id` lint exists for. **The fix must remap child
spans** (`SourceInfo::substring(parent, start, end)` using each location's
resolved byte range) before forwarding.

### Remapping is possible in-tree

`DiagnosticMessage` (quarto-error-reporting 0.2.1) exposes `location`,
`details: Vec<DetailItem>` (each with `location: Option<SourceInfo>`), and
`kind` as **public fields**, so a remap/demote helper needs no new API in the
externalized crate.

### Who benefits

Non-test callers of `readers::qmd::read` with `Some(parent_source_info)`:
`pandoc/meta.rs:103` (this strand) and `lua/config_value.rs:620` (Lua-filter
config values). Rerooting at the source (option B below) fixes both.

### Repro (reproduced at HEAD)

In-repo fixture: `claude-notes/plans/q120-forward-underlying-diagnostic-investigation/repro/`
(`q2 render` there shows the generic Q-1-20 warning for the footer config and
the precise Q-2-3 error for the identical text in `body-control.qmd`). See the
investigation dir's `observed-output.txt` for the captured render output.

## Settled design (user answered 2026-08-20)

1. **Reroot at birth** (option a). Implementation refinement: the reroot
   happens inside `readers::qmd::read` immediately before the `Err` return —
   *after* `produce_diagnostic_messages` and the pruning/widening passes, which
   do their offset arithmetic in the raw-input-bytes domain and must keep doing
   so. From every caller's perspective the diagnostics are born rerooted; the
   Lua `config_value.rs:620` caller is fixed for free; no signature change in
   `quarto-parse-errors` needed.
2. **Fold children into the single Q-1-20 diagnostic as located details**
   (option b). Untagged branch stays a warning; `!md` branch stays an error;
   in both, each child diagnostic's title (+code) and its located notes become
   `add_info_at` details on the Q-1-20 message. No severity change.
3. **Forward all children.** Revisit only on a compelling real-world "too
   many diagnostics" case.
4. **Q-1-20's own text unchanged.**

## Phases

### Phase 0 — Tests (TDD, written first, verified failing)

- [x] `pampa::pandoc::meta` unit test (untagged): parse
  `![logo](images/logo.svg){width="65px" .light-content}` with parent
  `SourceInfo::original(FileId(7), 100, …)`; assert single Q-1-20 warning
  whose details carry the Q-2-3 content, with every detail location
  resolving into FileId(7) at offsets shifted by 100.
  (`config_string_parse_failure_forwards_underlying_diagnostic`)
- [x] `pampa::pandoc::meta` unit test (`!md` branch): same input via
  `parse_yaml_string_as_markdown_to_config(…, is_explicit_md = true, …)`;
  assert error kind + forwarded located details.
  (`explicit_md_parse_failure_forwards_underlying_diagnostic`)
- [x] Reroot test at the `read` level (covers the Lua caller path):
  `readers::qmd::tests::err_path_diagnostics_reroot_through_parent_source_info`.
- [x] All three verified failing before the fix (FileId(0), raw offsets
  38..52 observed in the failure output).

### Phase 1 — Reroot Err-path diagnostics in `readers::qmd::read`

- [x] `reroot_diagnostics_into_parent` walks each diagnostic's `location` +
  `details[].location` and rebuilds them as
  `SourceInfo::substring(parent, start, end)` from the resolved byte range.
- [x] Applied when `context.parent_source_info` is `Some`, after pruning,
  before `return Err(diagnostics)`.

### Phase 2 — Forward children from `parse_yaml_string_as_markdown_to_config`

- [x] `fold_child_diagnostics` in `meta.rs`, applied in both the `!md` and
  untagged branches. Folding shape (refined after e2e inspection): the
  child's `problem` becomes the located label on the child's main span —
  so the config rendering mirrors the body rendering exactly — and
  `[code] title` becomes a location-less Info footer line; child details
  keep their own `DetailKind`; child hints forwarded with exact-dup dedup.

### Phase 3 — Verification + docs

- [x] Full `cargo xtask verify` green (includes `cargo nextest run
  --workspace`; pampa is in the WASM closure).
- [x] End-to-end: `cargo run --bin q2 -- render` on the repro fixture;
  captured output (`repro/observed-output.txt`, inspected) shows the
  Q-1-20 warning on `_quarto.yml:8` carrying the Q-2-3 two-part span
  ("This key-value pair cannot appear before the class specifier." /
  "This class specifier appears after the key-value pair.") plus an
  `ℹ [Q-2-3] Key-value Pair Before Class Specifier in Attribute` footer.
- [x] Check `docs/errors/yaml/Q-1-20.qmd` — message text unchanged (decision
  4), so likely no edit; update only if it shows example output that now
  differs materially.
- [x] Close the strand (commit 4e116ba5).

## Risks / tradeoffs (draft)

- **Snapshot churn:** Q-1-20 renders in existing snapshot tests; changing its
  detail structure will touch snapshots (must be called out per the snapshot
  policy).
- **quarto-error-reporting is external:** the plan above deliberately uses only
  public fields; if the design lands on needing a real "nested diagnostic"
  concept, that becomes an upstream (posit-dev/quarto-error-reporting) change
  and a version bump — scope grows.
- **The Err-path coordinate-domain trap** (spans in `FileId(0)`-of-a-throwaway
  context) is a latent bug class beyond this strand; option 1(a) retires it,
  option 1(b) leaves it documented-but-armed.
