# Q-1-20 discards the underlying markdown diagnostic for config values (bd-q120-masks-config-md-diagnostic-a039r80t)

**Date:** 2026-08-19
**Braid:** bd-q120-masks-config-md-diagnostic-a039r80t
**Checkout:** main checkout at `/Users/cscheid/rooms/room-3/q2`, branch `main` @ `6bee9ebe`
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

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

## Proposed phases (draft)

Skeleton only — actual phase contents wait on the design discussion.

- **Phase 0 — Test plan (TDD).** Unit tests in `pampa::pandoc::meta` following
  the existing `config_string_image_target_source_reroots_through_parent`
  pattern: parse `![logo](x.svg){width="65px" .light-content}` with a parent
  `SourceInfo::original(FileId(7), 100, …)`; assert the emitted diagnostics
  carry the underlying Q-2-3 content AND that every forwarded span
  (`location` + detail locations) `resolve_byte_range()`s into FileId(7) at
  shifted offsets. Plus an end-to-end render test against the repro fixture
  shape (per the end-to-end verification policy).
- **Phase 1 — Span rerooting for Err-path diagnostics.** Either in
  `readers::qmd::read` / `produce_diagnostic_messages` (at birth) or via a
  remap helper applied in `meta.rs` (post-hoc) — see design question 1.
- **Phase 2 — Forward the diagnostics from `parse_yaml_string_as_markdown_to_config`.**
  Attach/emit remapped child diagnostics in both the `!md` and untagged
  branches; resolve the severity question (design question 2).
- **Phase 3 — End-to-end verification + docs.** Render the repro fixture via
  `cargo run --bin q2 -- render`, inspect stderr; update
  `docs/errors/*/Q-1-20.qmd` if the message shape changed.

## Open design questions for the user

1. **Where to reroot the spans.** (a) At birth: thread `parent_source_info`
   into `readers::qmd::read`'s error path so `produce_diagnostic_messages`
   builds rerooted spans — matches how the Ok path behaves, and fixes the Lua
   `config_value.rs` caller for free; requires a signature change in
   `quarto-parse-errors::produce_diagnostic_messages` (in-tree, cheap). (b)
   Post-hoc: a `remap_diagnostic_into_parent(diag, parent)` helper applied only
   in `meta.rs` — smaller blast radius, but leaves the Err path's
   coordinate-domain trap armed for the next caller. I lean (a).
2. **Severity in the untagged branch.** Today an untagged config value that
   fails to parse is a *warning* + literal-text fallback; the underlying parser
   diagnostics are *errors*. Forwarding them as errors would turn a
   warn-and-continue situation into a render failure. Options: (a) demote
   forwarded children to warnings (`kind` is a pub field); (b) fold the
   children into the single Q-1-20 warning as located details
   (`add_info_at`-style), keeping one diagnostic; (c) keep them errors
   (behavior change). For the `!md` branch the parse failure is already an
   error, so children can stay errors there. I lean (b) for untagged — one
   diagnostic, Q-1-20 stays the code, the child's message + two-part span
   become its details — and (b)-or-sibling-errors for `!md`.
3. **All children or first?** A config string that fails to parse can produce
   several diagnostics (the generator dedups by position but can still emit
   more than one). Forward all, or first-N with a "and N more" note? (Config
   strings are short; I lean "all".)
4. **Does Q-1-20's own text change?** If children are folded in as details,
   the "Could not parse '…' as markdown" problem line could drop the raw value
   echo (the span already shows it) — or stay unchanged for snapshot stability.
   Any change ripples into snapshots and possibly `docs/errors` examples.

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
