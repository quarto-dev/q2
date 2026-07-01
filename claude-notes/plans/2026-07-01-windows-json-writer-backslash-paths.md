# Windows: JSON writer emits backslash path separators in output (bd-dff27o04)

**Date:** 2026-07-01
**Braid:** bd-dff27o04
**Worktree:** `.worktrees/bd-dff27o04-windows-json-writer-emits` (branch `braid/bd-dff27o04-windows-json-writer-emits`, based on `main` @ `b8fb38b0`)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

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

## Proposed phases (draft)

- **Phase 0 — Test** (TDD, per project policy): add a unit test that constructs an `ASTContext` (or calls `qmd::read`) with a backslash-containing filename string (no `#[cfg(windows)]` needed — the input is a literal string, not a real OS path, so the test should run identically on all platforms) and asserts the JSON writer's `files[].name` (and the streaming variant) come out forward-slash-only. Run it first, confirm it fails against current `with_filename`.
- **Phase 1 — Fix**: normalize in `ASTContext::with_filename` (`ast_context.rs:42`) via `quarto_util::to_forward_slashes(Path::new(&filename_str))`. Also normalize in `add_filename` (`ast_context.rs:70`) for consistency — same vector, same one-line fix, and leaving it inconsistent would silently reintroduce the bug the day a real second-file caller shows up (see design question 2, resolved below). Both writer sites and any diagnostics built from `ast_context.filenames`/`source_context` inherit the fix for free — no changes needed at the two writer call sites themselves.
- **Phase 2 — Verify**: re-run `unit_test_snapshots_json` (and the sibling `unit_test_snapshots_native`/etc., since `with_filename` is shared) to confirm no snapshot regressions; full `cargo nextest run --workspace` per project policy. Then **end-to-end CLI check** (mandatory per this repo's "End-to-end verification before declaring success" policy — unit tests alone don't count as done for a CLI-visible feature): run `cargo run -p pampa -- -t json -i <path-with-backslash>` on Windows against a small fixture, and inspect the actual `astContext.files[0].name` in the printed JSON to confirm forward slashes. Also check whether CLI diagnostic/error output (which reads the same `SourceContext`) now shows the normalized path, and record what it shows.
- **Phase 3 — Docs/changelog**: none expected beyond the fix itself — this is an internal writer-determinism bug, not a user-facing feature. The diagnostic-path side effect (see design question 4) may warrant a one-line changelog note if it changes what users see in error output.

## Open design questions for the user

1. **Normalization point.** Description proposes normalizing at `ASTContext::with_filename` ingress (single point, covers both writer sites and diagnostics). Confirm that's preferred over normalizing at each of the two writer call sites individually (which would NOT fix diagnostics that also read `ast_context.filenames`).
2. **`add_filename` (ast_context.rs:70).** Recommendation above is to normalize it too, for consistency with `with_filename` — same vector, same writer, no reason for the two ingress points to diverge even though `add_filename` has no production caller yet. Confirm, or push back if there's a reason to keep it scoped to `with_filename` only.
3. **Test cfg.** Since the bug is reproducible with a literal `"tests\\snapshots\\json\\001.qmd"` string (no actual Windows path APIs involved), the regression test can run on every platform, not just Windows. Confirm that's the intent — a platform-gated test would under-cover this (CI on Linux/macOS would never catch a regression).
4. **Diagnostic/error-message paths.** `with_filename` also seeds `SourceContext`, which CLI diagnostics read. Normalizing at ingress means Windows users will start seeing forward-slash paths in `pampa`/`q2` error messages too, not just JSON output — that's a small but real, user-visible behavior change beyond "fix the JSON writer." Confirm this is desired (it's consistent with the rest of the codebase's forward-slash convention), rather than leaving it as an unplanned side effect.

## Risks / tradeoffs (draft)

- Low risk: `to_forward_slashes` is already battle-tested elsewhere in the codebase for identical purposes (per the strand description). The change is additive (one call in one function) and doesn't touch byte offsets or the line-ending preserve policy.
- Snapshot fallout: normalizing at ingress could change filenames embedded in *other* existing snapshots beyond `json/001` if any of them currently encode backslashes on non-Windows CI (unlikely, since CI presumably runs Linux/macOS) — worth a full snapshot diff check after the fix, not just the one known-failing file.
