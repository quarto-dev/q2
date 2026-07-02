# Windows: JSON writer emits backslash path separators in output (bd-dff27o04)

**Date:** 2026-07-01
**Braid:** bd-dff27o04
**Worktree:** `.worktrees/bd-dff27o04-windows-json-writer-emits` (branch `braid/bd-dff27o04-windows-json-writer-emits`, based on `main` @ `b8fb38b0`)
**Status:** Design aligned (2026-07-01) — implementing.

## Triage verdict

**Ready to design** — root cause fully confirmed at the source level (single ingress point, single fix), verified against the actual insta snapshot mismatch, no open dependency-graph pressure blocking it.

## Issue context

`ASTContext::with_filename` (`crates/pampa/src/pandoc/ast_context.rs:42`) stores whatever filename string is handed to it verbatim into `self.filenames`. On Windows, callers that derive the filename from a real `Path` (e.g. `path.to_string_lossy()` in the snapshot test harness, or CLI `input_filename`) produce backslash-separated strings. The JSON writer later emits that string verbatim in two places:

- `crates/pampa/src/writers/json.rs:1812` — `FileEntryJson.name` (struct-based, non-streaming `write` path)
- `crates/pampa/src/writers/json.rs:3967` — `w.str_value(filename)?` (streaming writer variant)

Both read from `ast_context.filenames[idx]` with no normalization. The committed insta snapshots (created on macOS/Linux) always have forward slashes, so on Windows the `name` field diverges and the snapshot test fails.

Originally filed under the line-ending epic (bd-eehxwr29) because it shared the `json/001` failure symptom, then detached 2026-06-26 as a distinct Windows/Linux parity issue — explicitly **not** a line-ending/byte-offset problem: normalizing `\` → `/` in a filename string doesn't touch source byte offsets, so it's orthogonal to the preserve-line-endings invariant that epic protects.

## Dependency graph

- **supersedes bd-238o** (closed 2026-06-26): bd-238o originally bundled 3 unported Windows fixes from quarto-markdown (native `\r` escape, CRLF test-read normalization, JSON path separator). It was split apart during the line-ending epic's decomposition: fix 1 → bd-ske10iyd, fix 2 → superseded by the `.gitattributes` LF pin (bd-mv2ggmr5, rejected the CRLF-normalize-on-read approach per PR #329), fix 3 (this one) → bd-dff27o04.
- **related bd-eehxwr29** (open epic, "Enforce preserve line-ending policy across q2"): shares a label but is explicitly a different mechanism — no blocking dependency, just shared history.
- No `blocks` edges. No incoming pressure beyond "this is the last of the three original quarto-markdown ports still open."

## What the code looks like today

Confirmed by direct inspection (not just re-reading the description):

1. `crates/pampa/tests/integration/test.rs:345` — `test_snapshots_for_format` calls `readers::qmd::read(input.as_bytes(), false, &path.to_string_lossy(), ...)`. `path` comes from `glob("tests/snapshots/{format}/*.qmd")`; on Windows `glob` returns native-separator paths, so `path.to_string_lossy()` is backslash-separated (e.g. `tests\snapshots\json\001.qmd`).
2. `crates/pampa/src/readers/qmd.rs:98` — `ASTContext::with_filename(filename.to_string())` stores that raw string with no normalization.
3. `crates/pampa/src/pandoc/ast_context.rs:42-51` — `with_filename` just does `filenames: vec![filename_str]`. No other ingress point currently exists in production code — `add_filename` (`ast_context.rs:70`) is only exercised by unit tests today, no real caller adds a second file.
4. `crates/pampa/src/writers/json.rs:1801-1823` builds `AstContextJson.files: Vec<FileEntryJson>` from `ast_context.filenames[idx]` directly (`name: filename.clone()`), and the streaming variant at `json.rs:3951-3967` does the same via `w.str_value(filename)?`.
5. Verified against the *actual* insta snapshot (not the stale-looking `tests/snapshots/json/001.json` fixture file, which is unrelated/unused by this test): `crates/pampa/snapshots/json/001.snap` line 6 contains `"astContext":{"files":[{"...,"name":"tests/snapshots/json/001.qmd",...}]}` — forward slashes, committed from a Unix machine. This is exactly the field built at json.rs:1812.
6. `quarto_util::to_forward_slashes(path: &Path) -> String` (`crates/quarto-util/src/path.rs:23`) already exists and is used elsewhere for this exact purpose (PR #340 Lua side, HTML resource paths, DocumentProfile, listings) — no new helper needed, just a new call site.
7. **Invariant check (all real call sites of `with_filename`/`add_filename`, checked by grep):** every production caller passes either a genuine file path (CLI `input_filename`, test-harness `path.to_string_lossy()`, Lua `readwrite.rs` file reads) or a bracketed placeholder literal with no path separators (`"<input>"`, `"<unknown>"`, `"<anonymous>"`). There is no current call site that passes an opaque, non-path identifier that could contain a legitimate literal backslash. So `ASTContext.filenames` is, in practice, always either "a portable/display path" or "a placeholder" — never an arbitrary opaque string — which is the invariant the forward-slash fix relies on.

**Reproducibility:** not yet re-run end-to-end on this machine — see "Environment note" below. Source-level analysis is unambiguous: `path.to_string_lossy()` on a glob-returned Windows path will contain `\`, and nothing between there and the JSON writer normalizes it. The only reason this isn't currently failing loudly in this session is an unrelated, transient native-build issue (see below) that blocked running the test at all; it is not evidence the bug is absent.

**Environment note (tangential, not part of this fix):** `cargo xtask verify --skip-hub-build` failed on first attempt in this fresh worktree with `aws-lc-sys` / `crypto-common` C-compile errors under `cl.exe` (unrelated to pampa/JSON — pulled in transitively via `quarto-hub → reqwest → rustls → aws-lc-rs → aws-lc-sys`). Rebuilding `aws-lc-sys` in isolation (`cargo build -p aws-lc-sys`) succeeded immediately after, confirming this was a parallel-build resource-contention flake in the fresh worktree's `target/`, not a real regression — historical measurement logs (`claude-notes/research/measurements/baseline-debug.log`) show this exact crate building fine on this machine before. A clean re-run of `cargo xtask verify --skip-hub-build` is in flight; if it comes back green, treat this as noise. If it recurs deterministically, it's a separate strand, not part of this fix.

## Phases — completed 2026-07-01

- **Phase 0 — Test** ✅: added `test_with_filename_normalizes_backslashes` and `test_add_filename_normalizes_backslashes` in `ast_context.rs`. Confirmed RED against unmodified `with_filename`/`add_filename` before implementing.
- **Phase 1 — Fix** ✅: normalized in both `ASTContext::with_filename` and `add_filename` via `quarto_util::to_forward_slashes`. Confirmed GREEN.
- **Phase 1b — scope extension (found during E2E verification)**: end-to-end CLI testing (`cargo run -p pampa -- -t json -i <backslash-path>`) surfaced a second, un-normalized ingress point: `main.rs:279`'s ad hoc fallback `SourceContext` (built directly from the raw CLI arg on the hard-parse-error path) feeds the same ariadne `-->` file:line label as the happy path, but bypasses `ASTContext` entirely. Confirmed with user this was in-scope (same bug class, same fix pattern) rather than deferred. Added `hard_parse_error_diagnostic_uses_forward_slashes` integration test (spawns the real `pampa` binary via `CARGO_BIN_EXE_pampa`, drives an unclosed-span hard error through a native-separator temp path) — RED confirmed via `git apply -R` on just the `main.rs` fix, then GREEN after reapplying. One iteration needed: the first version of this test asserted "no backslash anywhere in stderr," which false-positived on ariadne's OSC-8 hyperlink terminator (`ESC \`, an unrelated ANSI control byte) — tightened to check the specific path fragment instead.
  - **Explicitly NOT fixed, by design**: `main.rs:217-225`'s "Missing Newline at End of File" warning, which interpolates the raw CLI arg into a plain sentence (`` File `{input_filename}` does not end with a newline ``). This isn't a portable identifier — it's echoing the user's own typed path back to them, which is standard CLI convention (matches `rustc`, `cat`, etc.). Normalizing it would be surprising, not helpful.
- **Phase 2 — Verify** ✅: full `pampa` crate suite (`cargo nextest run -p pampa --no-fail-fast`) — 8 pre-existing failures confirmed via `git stash`/baseline re-run to be unrelated CRLF byte-offset issues (catalogued Windows issue, orthogonal to this fix), zero new regressions. Full `cargo nextest run --workspace` was intentionally skipped per user direction — CI covers workspace-wide verification, and known Windows-only CRLF failures aren't fully resolved yet, so a full run adds cost without new signal; instead did a targeted grep-based cross-crate check confirming no crate outside `pampa` reads `ASTContext.filenames` as a literal disk path (Windows file APIs accept forward slashes natively regardless). E2E CLI check done for both the JSON writer (`astContext.files[0].name`) and the diagnostics path (ariadne file:line label).
- **Phase 3 — Docs/changelog**: none needed — internal writer-determinism bug, not a user-facing feature change beyond what's captured in the design questions above.

## Design questions — resolved 2026-07-01

All 4 confirmed by user, matching this plan's recommendations:

1. **Normalization point:** `ASTContext::with_filename` ingress (single point, covers both writer sites and diagnostics). ✅ Confirmed.
2. **`add_filename` (ast_context.rs:70):** normalize it too, for consistency with `with_filename`. ✅ Confirmed.
3. **Test cfg:** regression test runs on all platforms (no `#[cfg(windows)]` gate) — reproducible via a literal backslash string, no real Windows path APIs needed. ✅ Confirmed.
4. **Diagnostic/error-message paths:** Windows users will see forward-slash paths in CLI error output too, not just JSON — accepted as an intended, desired side effect (consistent with codebase + quarto-cli convention). ✅ Confirmed.

**Status: design aligned. Proceeding to Phase 0 (TDD).**

## Ecosystem precedent

Checked TypeScript Quarto (quarto-dev/quarto-cli) for how it handles this exact
problem, since it's the sibling codebase with a decade of Windows mileage.
Confirmed against source (not just DeepWiki's summary): `pathWithForwardSlashes`
in `src/core/path.ts:199` does the identical `path.replace(/\\/g, "/")`, applied
at the same class of boundary q2 needs — resource paths, project-cache keys
(`FileInformationCacheMap`), and output emission (Pandoc metadata, TOML,
generated JS for OJS). There's also a companion `normalizePath` that uppercases
Windows drive letters for consistent cache-key comparison — out of scope here
(no drive-letter-comparison need in `ASTContext`), but worth knowing exists if
a future path-identity bug surfaces.

This doesn't change any of the open design questions below — it confirms the
proposed approach (normalize at ingress, forward slashes in output) matches the
established convention in both codebases, not just q2's own prior art
(`quarto_util::to_forward_slashes`).

## Risks / tradeoffs (draft)

- Low risk: `to_forward_slashes` is already battle-tested elsewhere in the codebase for identical purposes (per the strand description), and the same pattern is independently proven in TypeScript Quarto (see Ecosystem precedent above). The change is additive (one call in one function) and doesn't touch byte offsets or the line-ending preserve policy.
- Snapshot fallout: normalizing at ingress could change filenames embedded in *other* existing snapshots beyond `json/001` if any of them currently encode backslashes on non-Windows CI (unlikely, since CI presumably runs Linux/macOS) — worth a full snapshot diff check after the fix, not just the one known-failing file.
