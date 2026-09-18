# `SourceInfo::map_offset` clones the whole file per call (bd-jn7r22g8)

**Strand:** bd-jn7r22g8 (P1, perf). Discovered from bd-5yektmwt; related to
bd-is4q72tt (peak RSS).
**Status:** approved 2026-09-18 — option **A only** (user decision). q2 work on
topic branch `braid/bd-jn7r22g8-map-offset-clone` in the main checkout; crate
work in `~/repos/github/posit-dev/quarto-source-map` (commit + push, then
watch CI). The `external-sources/` clone is used only for the throwaway
timing experiment via an uncommitted `[patch.crates-io]`.
**External crate clone:** `external-sources/quarto-source-map` (at `e328ddd`,
= released 0.1.3).

## Overview

`quarto_source_map::SourceInfo::map_offset` (in `mapping.rs`) clones the
entire file `String` on every call so it can pass `&content` to
`FileInformation::offset_to_location`, which only borrows. pampa's
`process_list` calls `map_offset(length)` up to twice per list item for
loose/tight detection, so parsing a document costs
O(list items × file size). On the 1.1 MB, 15,763-item Connect
`api/index.qmd` that is ~31k transient 1.1 MB clones, and that document is
the wall-clock critical path of the whole 352-document project render.

The fix lives in the external crate. The question this plan settles first is
**whether it can be made without a breaking API change** (the user's concern
going in). Conclusion below: **yes** — the minimal fix touches no public
signature, and the two "nicer" variants are both additive. The genuinely
breaking variant (changing `SourceFile.content`'s type) is not needed for
this strand and is recorded as a follow-up idea only.

## Reproduction (2026-09-18, this checkout, `target/release-perf/q2` built 11:16)

### samply, single document

```
samply record -s -n --unstable-presymbolicate -o warm.json.gz -- \
  target/release-perf/q2 render ~/repos/github/cscheid/q2-connect-docs/docs-quarto-2/api/index.qmd
crates/perf-harness/scripts/analyze_profile.py warm.json.gz --top 25
PYTHONPATH=crates/perf-harness/scripts crates/perf-harness/scripts/bucket_profile.py warm.json.gz _platform_memmove
```

| measure                                                        | value |
|----------------------------------------------------------------|------:|
| total samples (1 kHz, all threads)                             | 1,651 |
| `_platform_memmove` self time                                  | 43.0 % (710) |
| memmove with `SourceInfo::map_offset` as nearest caller        | **26.0 % (430)** |
| next memmove caller (`topdown_traverse_inlines::walk_vec`)     | 2.1 % |
| inclusive under `parse_document` stage                         | 54.7 % |

This matches the 2026-09-17 note (26 % of that document's CPU on the
`quarto-pass2-9` worker). Profile + sidecar saved in the session scratchpad
(`prof/warm.json.gz`), not committed — the 09-17 profiles already in
`claude-notes/research/` are the reference copies.

### Scaling series (synthetic: one bullet list, N items, blank line every 50)

`q2 render N.qmd --to html`, release-perf binary, standalone document:

| items  | bytes   | real   | user   | ratio vs previous |
|-------:|--------:|-------:|-------:|------------------:|
| 2,000  |  89 KB  | 0.20 s | 0.17 s |    — |
| 4,000  | 179 KB  | 0.34 s | 0.31 s | 1.7× |
| 8,000  | 359 KB  | 0.68 s | 0.63 s | 2.0× |
| 16,000 | 725 KB  | 1.54 s | 1.45 s | 2.3× |
| 32,000 | 1.46 MB | 4.06 s | 3.88 s | **2.6×** |

Doubling the input should ~double a linear pipeline; the ratio climbing past
2 is the n² term (items × bytes) overtaking the linear part. Fixture
generator and timings are in the session scratchpad; the generator is
three lines of Python and is reproduced in "Verification" below.

### Fixture facts

`api/index.qmd`: 53,894 lines, 15,763 bullet items, 8 ordered items,
98 fences, 1 shortcode. Registered by `pampa::readers::qmd` via
`SourceContext::add_file(name, Some(content))`, so the file is
**in-memory** (`content: Some`) — the `c.clone()` arm is the one that fires.

## Root cause, precisely

`external-sources/quarto-source-map/src/mapping.rs:31-37`:

```rust
let content = match &file.content {
    Some(c) => c.clone(),                              // full String clone
    None => std::fs::read_to_string(&file.path).ok()?, // full disk read
};
let location = file_info.offset_to_location(absolute_offset, &content)?;
```

`offset_to_location(&self, offset, content: &str)` uses `content` only to
(a) floor `offset` to a char boundary and (b) count chars from line start
to `offset` for the `column`. The row comes from a binary search over
`FileInformation.line_breaks` and needs no content at all.

Callers on the hot path (`crates/pampa/src/pandoc/treesitter.rs:179,209,224`)
use **only `.location.row`**.

Secondary per-node callers of the same function in pampa, all via
`location::source_info_to_qsm_range_or_fallback` (2 calls each): ordered
list markers (`list_marker.rs:31,44`), shortcodes (`shortcode.rs`, 4 sites),
code fence content (`code_fence_content.rs:49`), `treesitter.rs:1377`.
Negligible on this fixture (8 ordered items, 1 shortcode) but the same
O(file) cost per call.

## API-compatibility analysis

### Who touches the relevant surface

`quarto-source-map`'s public types involved: `SourceContext::get_file`,
`SourceFile { pub path, pub content: Option<String>, pub file_info, pub metadata }`,
`FileInformation::offset_to_location(&self, usize, &str)`,
`SourceInfo::map_offset / map_range`.

External published dependents (both consumed by q2 as version deps, and
both would have to be re-released if `SourceFile` changed shape):

- `quarto-error-reporting` 0.2.2 — `diagnostic.rs:828` and `:1024` do
  `match &file.content { Some(c) => c.clone(), None => read_to_string }`
  and then use `content.as_str()`; `coalesce.rs:420` (test) uses
  `content.as_deref()`. Also calls `map_offset` in `json.rs` (3 sites).
- `quarto-yaml` 0.1.3 — only its tests touch `SourceContext` (`add_file`,
  `map_offset`).

In-tree q2 consumers of `SourceFile.content`:
`crates/pampa/src/pandoc/location.rs:310` (`f.content.as_ref()` →
`.as_str()`), `crates/quarto-core/src/crossref/codeblock_shorthand.rs:539`
(`as_deref()`), `crates/quarto-config/src/span_assert.rs:497` (`is_none()`).
Nothing in q2 constructs `SourceFile` literally; `add_file_with_info` is
used at `include_expansion.rs:416`, `engine_execution.rs:685`,
`readers/json.rs:1544`.

### Options

| | change | breaking? | removes the clone for… | notes |
|-|--------|-----------|------------------------|-------|
| **A** | `map_offset`: `Cow<str>` — `Borrowed(c)` / `Owned(read_to_string)` | **No.** No signature changes. | in-memory files (the pampa path) | Disk-backed (`content: None`) files still re-read per call — see follow-ups. |
| **B** | Additive row-only API: `FileInformation::offset_to_row(&self, usize) -> Option<usize>` and `SourceInfo::resolve_offset(&self, usize) -> Option<(FileId, usize)>` (the per-offset sibling of the existing `resolve_byte_range`); pampa's `process_list` composes them and never needs content. | **No.** Purely additive. | all files, and also skips the O(line) column count | Adds public surface to maintain. Perf gain over A alone is negligible (A's residual cost is O(log lines + line length) per call). |
| **C** | `SourceFile.content: Option<Arc<str>>` so `SourceContext` clones are cheap | **Yes** — public field type; `quarto-error-reporting` 0.2.2 would not compile against it; needs a coordinated release of that crate too. | n/a (different problem: context clones) | Not needed for this strand. `SourceContext::clone` does not appear in the profile's top list (10 clone sites in q2, one per pipeline/stage boundary, not per node). Record as a follow-up idea, not work. |
| **D** | Cache the disk read for `content: None` files (e.g. `OnceCell` in `SourceFile`) | **Effectively yes** — a new field on an all-`pub` struct breaks literal construction; interior mutability behind `&self` also changes `Send`/`Sync` story. | disk-backed files | Out of scope. See follow-ups: today `add_file_with_info` files (synthetic names, no disk file) can't map columns at all, which is a latent correctness gap, not a perf one. |

### Recommendation

**Do A.** It is the whole fix for the quadratic term, it is a one-line
semantic change with zero API impact, and it ships as `quarto-source-map`
0.1.4, which every dependent's `^0.1.x` requirement already accepts (the
q2 lockfile unifies `quarto-error-reporting`, `quarto-yaml`, and the
workspace onto one version).

**B is optional.** I'd hold it unless we want `map_offset`'s content
requirement gone on principle. If we do want it, do it in the same 0.1.4
release (still non-breaking), and have `process_list` use it — but that is
API design, not a perf need, so it should be a deliberate choice.

Drive-by in the same crate PR (also non-breaking): `SourceContext::add_file`
and `add_file_with_id` clone the content once more than necessary
(`(Some(c.clone()), Some(c))` just to build `FileInformation` from it —
build the info from `&c` before moving `c` in). One 1.1 MB clone per parse,
not quadratic; trivial to fix while we're there.

**Where the user's "might break compat" instinct was right:** the obvious
*structural* fixes — dropping the `content: &str` parameter from
`offset_to_location`, or changing `SourceFile.content` — *are* breaking.
Option A sidesteps both by keeping the borrow local to `map_offset`.

## Test design (TDD — tests first, verify they fail)

### In `quarto-source-map` (the crate under change)

1. **Allocation-budget test** (the mechanical "no full-file clone" check).
   New `tests/alloc_budget.rs` with a counting `#[global_allocator]`
   (a thin wrapper over `System` that sums allocated bytes into an
   `AtomicUsize`). Build a 1 MB in-memory file, take a baseline of the
   counter, call `map_offset` 1,000 times, assert the bytes allocated
   during the loop are below a small bound (e.g. 64 KB; the fixed path
   allocates nothing per call). On 0.1.3 code this allocates ≥ 1 GB and
   the test fails; after A it passes. Deterministic, no timing.
   (The crate has no `tests/` dir today; a separate integration binary
   is fine here — the q2 one-binary rule is a q2 convention, and a global
   allocator needs its own binary anyway.)
2. **Behavioural equivalence**: existing `mapping.rs` and `file_info.rs`
   unit tests + doctests stay green (`cargo test --locked`, which runs
   doctests, as CI does). Add one test that `map_offset` on a
   **disk-backed** file (temp file, `content: None`) still resolves —
   covers the `Owned` arm.
3. If B is adopted: unit tests for `offset_to_row` (line boundaries, at a
   `\n`, mid multi-byte char, `offset == total_length`, out of bounds →
   `None`) and for `resolve_offset` on `Original` / `Substring` /
   `Concat` (incl. the exclusive-end branch) / `Generated`, each asserted
   equal to `map_offset(...).location.row` / `.file_id` on the same input.

### In q2

4. `process_list` behaviour is already covered by the list snapshot
   tests in pampa (loose/tight, nested, blank-line-between-blocks); they
   must stay green. No new q2 unit test is needed for A because A does
   not change q2 code. If B is adopted, the `process_list` switch is a
   refactor guarded by those same tests.
5. **End-to-end**: the scaling series above must go linear (ratio ≈ 2.0 at
   every doubling), and the Connect `api/index.qmd` samply re-profile must
   show `map_offset` gone from the memmove callers. Numbers recorded in
   this plan under "Verification".

## Work items

### Phase 0 — crate: tests first

- [x] `tests/alloc_budget.rs` (counting global allocator, 1 MB file,
      1,000 `map_offset` calls, budget assert). Run: **must fail** on the
      current code.
- [x] Disk-backed `map_offset` test (temp file, `content: None`).
- [ ] (only if B) `offset_to_row` + `resolve_offset` unit tests; they fail
      to compile until B lands, which is the expected "red".

### Phase 1 — crate: implement

- [x] `mapping.rs`: `Cow<str>` borrow in the `Original` arm.
- [x] `context.rs`: build `FileInformation` before moving content in
      `add_file` / `add_file_with_id` (drop the extra clone).
- [ ] (only if B) `FileInformation::offset_to_row`,
      `SourceInfo::resolve_offset`; refactor `map_offset` to use
      `resolve_offset` so the two cannot drift.
- [x] `cargo fmt --all --check`, `cargo clippy --all-targets --locked -- -D warnings`,
      `cargo test --locked` (mirrors `.github/workflows/ci.yml`).
- [x] Open PR on `posit-dev/quarto-source-map` — **https://github.com/posit-dev/quarto-source-map/pull/5** (branch `map-offset-borrow`); CI green on all four legs, **merged**. Version bump: **PR #6, merged; 0.1.4 published** to crates.io by the release workflow. Then a
      version-bump PR to 0.1.4 → merge → CI publishes + tags (README
      "Releasing").

### Phase 2 — q2: verify locally before the release exists

- [x] Temporary, **uncommitted** `[patch.crates-io] quarto-source-map = { path = "external-sources/quarto-source-map" }`
      in the workspace `Cargo.toml` (see "External Sources Policy": a
      path into `external-sources/` must never be committed; this is
      local verification only and is reverted before any commit).
- [x] Baseline **before** numbers with a plain `cargo build --release --bin q2`: full Connect docs project render (warm, 16 workers), plus the scaling series and single-doc samply on release-perf.
- [x] Apply the A fix in the `external-sources/` clone, rebuild both profiles; re-run the same three measurements; record numbers below.
- [ ] (only if B) `process_list`: replace the three `map_offset(...).row`
      sites with the row-only API; run pampa list tests.
- [x] Revert the patch.

### Phase 3 — q2: consume the release

- [x] After 0.1.4 is on crates.io: workspace `Cargo.toml`
      `quarto-source-map` → `version = "0.1.4"` (an explicit floor, so
      `--locked` checkouts can't resolve 0.1.3), `cargo update -p quarto-source-map`.
- [x] `cargo xtask verify` (full — pampa/quarto-core are in the WASM
      closure).
- [x] Final end-to-end: Connect docs full-project warm render timing +
      samply, compared with the 09-17 numbers (2.0 s wall, 1.5 s for
      `api/index.qmd` alone).
- [x] Commit on `braid/bd-jn7r22g8-map-offset-clone`; ask before pushing.

### Phase 4 — bookkeeping

- [ ] `braid comment bd-jn7r22g8` with the before/after numbers; close.
- [ ] Addendum to `claude-notes/research/2026-09-17-connect-docs-render-profile.md`
      finding 1.
- [x] Follow-up strands (discovered-from bd-jn7r22g8): **bd-531vern5** (disk re-read per call), **bd-7z3axloo** (`add_file_with_info` can't map a column).
      - disk-backed (`content: None`) files re-read the file on every
        `map_offset`; and `add_file_with_info` files (synthetic names) can
        never map a column at all — `offset_to_location` returns `None`
        when the read fails. Perf + correctness gap, needs the D-style
        design (breaking) — worth a design note before any code.
      - (idea) `SourceFile.content` as `Arc<str>` to make `SourceContext`
        clones cheap (option C). Only if a profile ever shows
        `SourceContext::clone`.

## Decisions (2026-09-18)

1. A only. B is not pursued for now.
2. Local verification via uncommitted `[patch.crates-io]` → `external-sources/quarto-source-map`, reverted before any commit. Also compare a **plain `--release`** q2 rendering the whole Connect docs project before/after.
3. Real fix in `~/repos/github/posit-dev/quarto-source-map`: I commit and push; we watch CI together and take it from there.
4. q2 side on a topic branch in this checkout.
5. The two follow-ups become separate strands, worked in separate sessions.

## Open questions for the user (resolved above; kept for the record)

1. **A only, or A + B?** My recommendation is A only (smaller release, no
   new API to maintain); B only if we want the "rows never need content"
   property as a matter of API design.
2. **Local verification route.** OK to use an uncommitted
   `[patch.crates-io]` pointing at `external-sources/quarto-source-map`
   for Phase 2, reverted before commit? The alternative is to skip Phase 2
   and verify only after 0.1.4 is published.
3. **Release mechanics.** I prepare the crate PR branch in the
   `external-sources/quarto-source-map` clone; you push, review, merge,
   and merge the version-bump PR (Trusted Publishing does the rest).
   Confirm that's the division of labour you want.
4. **Worktree.** This investigation ran in the main checkout (that is
   where the release-perf binary lives). The q2-side edits are small
   (Cargo.toml bump, possibly `process_list`); fine to do here on a
   `braid/bd-jn7r22g8-…` branch, or I can `create-worktree` and rebuild
   there — your call.

## Verification (filled in during execution)

### Allocation-budget test, prototyped against published 0.1.3

Counting `#[global_allocator]`, 1 MB in-memory file, 1,000 `map_offset`
calls: **1,048,610,000 bytes allocated** (≈ 1 MB per call). The test fails
on 0.1.3 as designed. (Prototype in the session scratchpad; the real test
goes into the crate repo in Phase 0.)

### Before (2026-09-18, plain `cargo build --release --bin q2`, branch at `d1ba3440d`)

`/usr/bin/time -l`, Connect docs project, warm (one warm-up render first),
16 workers:

| run | real | user | sys | max RSS |
|----:|-----:|-----:|----:|--------:|
| full project 1 | 2.36 s | 9.84 s | 1.77 s | 1.75 GB |
| full project 2 | 2.09 s | 9.90 s | 1.75 s | 1.68 GB |
| full project 3 | 2.14 s | 9.71 s | 1.77 s | 1.76 GB |
| `api/index.qmd` alone 1 | 1.49 s | 1.30 s | 0.32 s | 1.06 GB |
| `api/index.qmd` alone 2 | 1.49 s | 1.31 s | 0.31 s | 1.08 GB |

Scaling series (release-perf, before): see the table under
"Reproduction" — 0.20 / 0.34 / 0.68 / 1.54 / 4.06 s for 2k–32k items.

### After (patched to the Cow fix via temporary `[patch.crates-io]`, same commands)

| run | real | user | sys | max RSS | before (real) |
|----:|-----:|-----:|----:|--------:|--------------:|
| full project 1 | 1.74 s | 9.37 s | 1.78 s | 1.77 GB | 2.36 s |
| full project 2 | 1.87 s | 9.31 s | 1.79 s | 1.77 GB | 2.09 s |
| full project 3 | 1.75 s | 9.34 s | 1.83 s | 1.80 GB | 2.14 s |
| `api/index.qmd` alone 1 | 1.12 s | 0.92 s | 0.32 s | 1.07 GB | 1.49 s |
| `api/index.qmd` alone 2 | 1.08 s | 0.89 s | 0.31 s | 1.09 GB | 1.49 s |

Full-project wall clock −18 % (2.1–2.4 s → 1.75–1.87 s); `api/index.qmd`
alone −27 % wall / −30 % user. **Peak RSS is unchanged** — the 31k
transient clones were freed immediately and the allocator reused the
block, so bd-is4q72tt's 1.1 GB is something else (the profile's guess
that this churn inflated the footprint was wrong).

Scaling series (release-perf, after): 0.23 / 0.32 / 0.57 / 1.11 / 2.18 s
for 2k / 4k / 8k / 16k / 32k items — ratios 1.4 / 1.8 / 1.9 / 2.0, i.e.
linear. Before: 0.20 / 0.34 / 0.68 / 1.54 / 4.06 (ratio 2.6 at the top).

samply, single document, after: 1,217 samples (was 1,651, −26 %);
memmove self time 23.7 % (was 43.0 %); `SourceInfo::map_offset` **absent**
from the nearest-caller list for memmove (was 26.0 % of all samples). Top
remaining memmove callers are the diffuse `Vec<Inline>` / `Inline::clone`
traversal tail (2–3 % each), as bd-5yektmwt describes.

### Final (q2 on published quarto-source-map 0.1.4, plain `--release`, same script)

| run | real | user | sys | max RSS |
|----:|-----:|-----:|----:|--------:|
| full project 1 | 1.78 s | 9.26 s | 1.83 s | 1.63 GB |
| full project 2 | 1.99 s | 9.44 s | 1.89 s | 1.61 GB |
| full project 3 | 2.05 s | 9.54 s | 2.03 s | 1.78 GB |
| `api/index.qmd` alone 1 | 1.21 s | 0.95 s | 0.38 s | 1.00 GB |
| `api/index.qmd` alone 2 | 1.16 s | 0.93 s | 0.38 s | 0.99 GB |

Consistent with the patched experiment (`api/index.qmd` user time 1.30 s →
0.93 s). Output inspected: `_site/api/index.html` renders (2.0 MB); list
semantics are covered by pampa's loose/tight snapshot tests, which passed
under `cargo xtask verify` — the change is a borrow-for-clone with no
semantic difference, so no before/after HTML diff was taken.

### `cargo xtask verify` record (2026-09-18, Node 24.20.0 via fnm)

- First run failed at step 6 (ts-packages build) with a TypeScript error in
  `quarto-sync-client` — a **stale `node_modules`** (automerge-repo 2.5.6
  installed vs 2.6.0-alpha.5 in the lockfile). `npm install` from the repo
  root fixed it; the only lockfile churn it produced (`"peer": true` flags
  in `hub-client/quarto-hub-sandboxed-preview/package-lock.json`) was
  reverted, not committed.
- Second run: steps 1–10 green; step 11 (shared preview package tests)
  failed **only** on the two tests that the pre-existing *uncommitted*
  working-tree diff adds to
  `ts-packages/preview-renderer/src/utils/iframePostProcessor.integration.test.ts`
  ("does not stack a second click listener …" ×2). Those are unrelated
  in-progress work in this checkout, not part of this change.
- Third run with steps 3–11 skipped (already green): steps 1, 2, 12, 13 green;
  14 is the opt-in Playwright leg (`--e2e`), skipped as in CI. **Exit 0.**
