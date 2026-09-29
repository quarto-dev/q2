# Windows: json_errors ipynb hyperlink test builds expected file:// URL from verbatim path (bd-clq56rem)

**Date:** 2026-09-28
**Braid:** bd-clq56rem (related: bd-1klbq2zd)
**Worktree:** `.worktrees/bd-clq56rem-windows-jsonerrors-ipynb-hyperlink` (branch `braid/bd-clq56rem-windows-jsonerrors-ipynb-hyperlink`, based on `main` @ `e8379cfe`)
**Status:** Piece 1 (test rework) implemented. Decided 2026-09-29: this branch ships Piece 1 only. The product fix (Piece 2) moves to bd-1klbq2zd, audit first. See § Decisions. The S1 sequencing below was decided 2026-09-28 and is superseded for Piece 2.

## Overview

`quarto::integration json_errors::ipynb_diagnostic_hyperlinks_real_notebook` fails on Windows. The assertion that fails compares the OSC8 hyperlink URL. The product's URL is correct; the test builds a wrong expected URL (`file://` + the verbatim `\\?\C:\...` display of a `std::fs::canonicalize`d path).

The first revision of this plan called that a test-only fix. **That was incomplete.** Every Windows failure has to be classified as a test problem, or as Windows support that is missing because the code was written on Linux/macOS, or both. When run from a plain cwd, q2 itself carries `\\?\` paths through its pipeline and into **wire output** (JSON `notebook_path` and `source_file`) and human status lines. The URL is correct only because QER strips the prefix locally before building it. This test exposes that gap and also hides part of it: its `notebook_path` check at `json_errors.rs:~490` is `ends_with("broken.ipynb")`, which a verbatim path passes.

## Classification: test problem or Windows support gap?

**Both.** Evidence below.

| Layer | Finding | Class |
|---|---|---|
| OSC8 URL (the failing assertion) | Product emits `file:///C:/…/broken.ipynb`, which is correct. The test's expected value is built from a verbatim path. | **Test problem** |
| JSON `notebook_path`, `source_file` | `\\?\C:\…\broken.ipynb` emitted even when the user never typed a verbatim path (probe below) | **Windows support gap** (product) |
| Human status line | `Rendering single file: \\?\C:\…\ok.qmd` | **Windows support gap** (product). This is bd-1klbq2zd's reported symptom. |
| `notebook_path` assertion at `json_errors.rs:~490` | `ends_with("broken.ipynb")` passes on a verbatim path, so the test hides the wire leak | **Test hides the gap** |

**Where the prefix enters q2:** `NativeRuntime::canonicalize` (`crates/quarto-system-runtime/src/native.rs:94`) is a bare `path.canonicalize()`. `dispatch` canonicalizes every CLI input through it (`crates/quarto/src/commands/render.rs:278`, and the cwd at `:421`). The verbatim `source.path` then becomes `notebook_path` (`crates/quarto-core/src/stage/stages/parse_document.rs:206`, `.display().to_string()`) and `source_file`, and it reaches the status line at `render.rs:1046`. Beyond that one seam there are ~186 direct `canonicalize(` calls in non-test sources (heaviest in `project_resources.rs`, `project/format_paths.rs`, `engine/capture_files.rs`). The seam is the entry point for CLI inputs; the direct calls are a wider audit.

**Probe** (saved under `2026-09-28-windows-json-errors-file-url-test-investigation/`): render `broken.ipynb` and `ok.qmd` with `target/debug/q2.exe` from a plain `C:\Users\…\probe` cwd using relative arguments. Output:

```
{"notebook_path":"\\\\?\\C:\\Users\\chris\\AppData\\Local\\Temp\\...\\probe\\broken.ipynb"}
{"source_file":"\\\\?\\C:\\Users\\chris\\AppData\\Local\\Temp\\...\\probe\\broken.ipynb"}
Rendering single file: \\?\C:\Users\chris\AppData\Local\Temp\...\probe\ok.qmd
```

## Research: is `\\?\` expected inside a Rust project?

No. The consensus is to use verbatim paths only where they are needed (very long paths, reserved names) and to present or pass plain paths otherwise.

- **std docs** (`std::fs::canonicalize`): on Windows it returns extended-length syntax, which "may be incompatible with other applications". `std::path::absolute` (stable since 1.79) does no I/O and resolves no symlinks. Source: context7 `/rust-lang/rust`.
- **`dunce` 1.0.5** (read from source, `~/.cargo/registry/.../dunce-1.0.5/src/lib.rs`): the de facto fix. `dunce::canonicalize` calls std, then `simplified()` strips `\\?\` *only when that is safe*. It keeps the verbatim form for reserved names (`\\?\C:\COM`), invalid names, and paths over 260 chars. It is a no-op off Windows and does no I/O in `simplified`. q2 has it in `Cargo.lock` only as a build-time dep of `aws-lc-sys`; no q2 crate uses it.
- **uv** (verified in source, `crates/uv-fs/src/path.rs:52-64,397`): wraps `dunce::simplified` / `dunce::canonicalize` in a `Simplified` trait and uses it for display, comparisons and `relative_to`.
- **QER itself** (`diagnostic.rs:935-955`): strips the prefix by hand before building URLs. Upstream already treats verbatim paths as unfit for user-facing output.
- DeepWiki-only, not verified in source: cargo avoids `std::fs::canonicalize` (`cargo-util` `normalize_path`, `try_canonicalize`), and rustc has `fix_windows_verbatim_for_gcc`. Deno strips via `strip_unc_prefix`, and Tauri uses `dunce`. A DeepWiki claim about ripgrep's hyperlink code did **not** check out (the "Verbatim" hits are a template parser) and is dropped.
- Prior knowledge (basic-memory `patterns/windows-rust-canonicalize-path-prefix-issue`): verbatim and plain forms of the same path are unequal under `PathBuf ==` and `starts_with`. So carrying `\\?\` internally is a correctness hazard (prefix checks, relative-path computation) as well as a UX one.

## Triage verdict

**Ready to implement (S1), pending go-ahead.** The failing assertion is a test bug, but fixing only the test would leave a confirmed product gap (verbatim paths in JSON wire output) hidden behind a test that goes green. Decided 2026-09-28:
- One branch. First rework the test: a URL round-trip, plus a strengthened `notebook_path` assertion as the Windows RED.
- Then fix the product at `NativeRuntime::canonicalize` under bd-1klbq2zd.
- bd-1klbq2zd's scope is widened from display to wire output and status lines, under the contract in § Wire-path contract: plain form whenever an equivalent plain form exists.

## RED (captured 2026-09-28, Windows, HEAD `e8379cfe`)

```
cargo nextest run -p quarto -E 'test(ipynb_diagnostic_hyperlinks_real_notebook)'

panicked at crates\quarto\tests\integration\json_errors.rs:597:5:
rendered must hyperlink the real notebook (file://\\?\C:\Users\chris\AppData\Local\Temp\.tmpcKSHSn\broken.ipynb); got:
"...\u{1b}]8;;file:///C:/Users/chris/AppData/Local/Temp/.tmpcKSHSn/broken.ipynb\u{1b}\\broken.ipynb[cell 2, markdown]\u{1b}]8;;\u{1b}\\:3:1 ..."
```

- Actual (product): `file:///C:/Users/chris/AppData/Local/Temp/.tmpcKSHSn/broken.ipynb`. This is a well-formed URL with no `#fragment`, and the label is correct.
- Expected (test): `file://\\?\C:\...\broken.ipynb`. This is not a valid file URL.

This confirms both of the brief's hypotheses: the product side is correct, and the test builds its expectation by string concatenation.

## Findings

### Q1 — Where the file:// URL is built

In `quarto-error-reporting` 0.3.2 (a crates.io version dep, `Cargo.lock:6154`), source under `~/.cargo/registry/src/*/quarto-error-reporting-0.3.2/`:

- `src/diagnostic.rs:889` `DiagnosticMessage::wrap_path_with_hyperlink(path, link_target, line, column, enable_hyperlinks)`
  - `:906` `std::fs::canonicalize(target)`. On any error it skips the link.
  - `:912` `url::Url::from_file_path(Self::plain_absolute_path(abs_path))`. The `url` crate turns `\` into `/`, adds the third slash (`file:///C:/…`), and percent-encodes.
  - `:919-927` adds the `#line:col` fragment only when `target == path`. For an origin link (notebook cell → `.ipynb`) there is no fragment.
- `src/diagnostic.rs:944` `fn plain_absolute_path(p: PathBuf) -> PathBuf`. On Windows it strips `\\?\UNC\` → `\\` and `\\?\` → `` from the lossy string, and does nothing on other platforms.
- **Visibility: private.** It is a bare `fn` on `DiagnosticMessage` behind `#[cfg(all(feature = "ariadne", not(target_family = "wasm")))]`, so q2 cannot call it. QER's own unit test (`:2705-2711`) calls it to compute its expected URL. That is fine inside QER, but it would be circular for us to copy it.

### Q2 — Existing helpers / deps usable by the test

- **`url` 2.5 is already a normal `[dependencies]` entry of `crates/quarto`** (`crates/quarto/Cargo.toml:55`, inside `[dependencies]` at `:14`), so integration tests can use it at no cost.
- **No verbatim-prefix helper in the q2 tree.** `rg 'dunce|\\\\?\\|verbatim|from_file_path'` over non-test sources finds nothing relevant. `quarto_util::to_forward_slashes` (used at `crates/quarto-brand/src/types.rs:879`) only swaps separators. It does not strip `\\?\` and does not build URLs, so it would not help here.
- **`dunce` 1.0.5 is in `Cargo.lock:1797`, but only as a transitive dependency.** No workspace crate declares it. Using it would mean adding a dev-dep (no new download).

### Q3 — Sibling tests with the same fragile pattern

`rg 'file://' crates/*/tests`:

| file:line | Pattern | Fragile on Windows? |
|---|---|---|
| `crates/quarto/tests/integration/json_errors.rs:595` | `format!("file://{}", canonical(..).display())` compared against product OSC8 | **Yes: this bug** |
| `crates/quarto/tests/integration/bootstrap_sh.rs:192, 624, 1084, 1102, 1239` | `format!("file://{}", path.display())` fed to `install.sh` | Would be, but it never runs on Windows. The whole module is `#![cfg(unix)]` (`:31`) because it drives bash `install.sh`, a genuine platform limit. Out of scope. |
| `crates/quarto-lsp/tests/integration_test.rs:311, 347, 383, 407, 446, 504` | Literal `"file:///test/…"` URIs used as LSP document ids | No. These are constant strings and never touch disk. |

`canonical(` / `canonicalize(` under `crates/*/tests`: 472 hits in 87 files. A sonnet Explore agent classified each file by whether a canonicalized value flows into a *string* comparison against product output. It found **no other fragile cases**. Every other use is a filesystem sink (cwd, write, exists, CLI arg), a Path-to-Path comparison, a `file_name()` comparison, or appears only in panic-message text. Also skipped as `#![cfg(unix)]`: `crates/quarto/tests/integration/nightly_gate.rs:21` (verified).

The partial Windows run of the `quarto` crate suite agrees: no other failure there is a test-built `file://` URL (see § Crate suite snapshot).

Note: the test-local `fn canonical` helper is copied into about 55 test files. It is a plain passthrough to `std::fs::canonicalize`, so any *future* test that string-compares its output against product text will hit this same bug. Hardening every copy is out of scope here (YAGNI), see § Decisions (shared `canonical()`).

### Q4 — Relationship to bd-1klbq2zd

These two strands are **not** independent. The first revision said they were, and the probe disproved it.

- The OSC8 URL is built inside QER, which re-canonicalizes and strips the prefix itself (`diagnostic.rs:906, 912`). The *URL assertion* alone therefore does not depend on bd-1klbq2zd.
- The rest of the test's contract does depend on it. `origin.notebook_path` and `source_file` in the JSON output carry `\\?\` because of the same root cause bd-1klbq2zd describes (`NativeRuntime::canonicalize`). bd-1klbq2zd frames this as noisy *display*. It is actually a wire-format leak: JSON consumers (editors, the LSP, CI tooling) receive paths that many Windows programs cannot open.
- A strengthened version of this test (assert that `notebook_path` is a plain path to the real notebook) is a real Windows RED for bd-1klbq2zd's product fix. This is the shared piece that justifies coordinating the two strands.

## Fix options

The fix is two pieces with an ordering question between them.

### Piece 1 — Test: stop building a string oracle, and stop hiding the leak

**Option C, semantic round-trip (recommended for the URL assertion).** Pull out the OSC8 target that wraps the `broken.ipynb[cell 2, markdown]` label, then assert:
1. `url::Url::parse(target)` succeeds with `scheme() == "file"`.
2. `url.fragment().is_none()`. This keeps the "no `#line:col` on origin links" contract.
3. `std::fs::canonicalize(url.to_file_path()?) == canonical(&dir.join("broken.ipynb"))`, compared Path-to-Path with std on both sides.

The test never builds a URL itself, so there is no tautology with QER's logic. It needs no new dependency (`url` is already a `[dependencies]` entry of `crates/quarto`). It fails on the real regressions: a pseudo-path target, a `file://?/C:/…` verbatim leak (`?` parses as a query), or an added fragment. The cost is a small OSC8 extractor (about 10-15 lines) and a rewritten doc comment at `:510-521`. It no longer pins the exact URL spelling, which is QER's contract, covered by QER's own unit test.

**Plus: strengthen the `notebook_path` assertion (`:~490`)** from `ends_with("broken.ipynb")` to:
- `!nb_path.starts_with(r"\\?\")`. This is the wire contract applied to this fixture, and it is a no-op on Unix. It holds only if the fixture has a plain form (see § Wire-path contract). The names tempfile creates start with `.tmp`, but the TEMP root above them is arbitrary, so the whole path has to be checked. So first assert one precondition on the canonical fixture path, with a message that blames the environment's TEMP root: `dunce::simplified(&canon)` has no `\\?\` prefix. `dunce` (dev-dep, already in `Cargo.lock`) keeps the verbatim form exactly when no plain form is safe: a reserved or invalid name in *any* component, more than 260 UTF-16 units, or a non-disk prefix (a TEMP on a network share, `\\?\UNC\…`, also fails here, which is intended). An unusual TEMP then fails as a setup error instead of rejecting behavior the contract allows. Checking only length and the drive prefix is not enough: on Windows 11, plain Win32 APIs create directories named `CON`, `AUX` or `COM4.txt` (verified 2026-09-28 with `[IO.Directory]::CreateDirectory` and `cmd /c mkdir`; a trailing `.` is stripped instead), so a TEMP root can contain such a component without any `\\?\`-aware tool. The precondition only gates the assertion. The assertion itself is still on product output, so using `dunce` here does not make the test agree with the product by construction;
- `canonicalize(nb_path) == canonical(dir.join("broken.ipynb"))`, i.e. it names the real file.

This assertion **fails on Windows today**, which makes it the genuine RED for the product fix. The same check could apply to `source_file` if the user wants the wire contract pinned there too.

Alternatives considered for the URL assertion:
- **A. Mirror QER's strip in the test.** Rejected: it copies product logic, so the test would prove only that the two copies agree.
- **B. `dunce` + exact URL string.** This is less objectionable now that `dunce` is a likely candidate for the product fix too. But if the product adopts `dunce`, a `dunce`-based oracle shares the product's library, and it still fails on harmless spelling differences. C is stronger.

### Piece 2 — Product: q2 carries plain paths wherever a plain form exists (bd-1klbq2zd)

This is out of scope for this plan to *design*, but it bounds the choice.

**Wire-path contract.** On Windows, a path q2 emits (JSON fields, status lines) or compares uses the plain form whenever an equivalent plain form exists. The verbatim `\\?\` form is allowed only for paths that have no plain equivalent. "Never emit `\\?\`" would be wrong: some real paths can only be named in verbatim form. Per `dunce` 1.0.5 `is_safe_to_strip_unc` (`src/lib.rs:152-181`), the cases where stripping is unsafe are:
- reserved DOS names (`CON`, `COM4.txt`, …): `C:\CON` names the device, not the file;
- invalid filenames (trailing `.`/space, forbidden characters);
- paths longer than 260 UTF-16 units;
- any verbatim prefix other than a disk prefix. That includes `\\?\UNC\server\share\…`, which *often* has a plain equivalent (`\\server\share\…`). It has one only when the same three conditions hold for the UNC path: no reserved names, no invalid names, within the length limit. QER's `plain_absolute_path` converts every UNC path unconditionally, so it is not evidence for or against a particular case. `dunce` never converts UNC paths, so on its own it under-delivers for shares that do have a plain form.

**Exception (decided 2026-09-29): UNC shares may stay verbatim.** A `\\?\UNC\…` path may be emitted or compared in verbatim form even when a plain `\\server\share\…` equivalent exists, which matches what `dunce` does and what q2 does today. A follow-up strand may narrow this exception by converting shares only when the three conditions above hold. That work must add behavioral tests for a convertible share path and for one that has to stay verbatim.

Candidate directions, to be decided in bd-1klbq2zd:
- Fix at the seam: `NativeRuntime::canonicalize` (`native.rs:94`) returns the plain form via `dunce::canonicalize`, which leaves UNC shares verbatim under the contract's exception. This covers CLI inputs and everything derived from them in one place, and keeps the verbatim form only where the contract allows it.
- Audit the ~186 direct `std::fs::canonicalize` calls in non-test sources, which bypass the runtime seam. Decide whether they route through the seam, use a shared `quarto_util` helper, or stay put because they never reach output or comparisons.
- `std::path::absolute` is **not** a drop-in replacement: it resolves no symlinks, and the tests canonicalize precisely because of macOS `/var` → `/private/var`.

### Sequencing options

- **S1 (recommended). One branch, both strands, TDD order.** Write Piece 1 (Option C + strengthened `notebook_path`). The URL assertion goes green and the `notebook_path` assertion stays RED on Windows. Then implement Piece 2 at the seam (bd-1klbq2zd) → GREEN. Nothing lands that hides the leak, and the product fix gets a real Windows RED.
- **S2. Test-only now (Option C), `notebook_path` strengthening deferred to bd-1klbq2zd.** Smallest step, but the test goes green on Windows while the wire leak persists. This is the "green board without parity" outcome CLAUDE.local.md warns against.
- **S3. Park bd-clq56rem behind bd-1klbq2zd** (`blocks` edge) and do everything in bd-1klbq2zd. This is equivalent to S1, with the test work owned by the product strand.

## Decisions

Decided 2026-09-28:
- **Sequencing: S1.** One branch: test rework with the strengthened `notebook_path` RED first, then the product fix at the runtime seam.
- **bd-1klbq2zd scope: widened** from user-facing display to wire output (JSON) and status lines, under § Wire-path contract. The `NativeRuntime::canonicalize` seam is the first fix site, and the ~186 direct calls are an audit item.

Decided 2026-09-29:
- **UNC shares may stay verbatim.** The exception is written into § Wire-path contract. Conditional UNC conversion is a follow-up strand.
- **This branch fixes the seam only.** `NativeRuntime::canonicalize` calls `dunce::canonicalize` directly (native-only dep, already in `Cargo.lock`). No `quarto_util` helper yet. The direct-call audit is its own strand.
- **Wire contract breadth in this test: `notebook_path` only.** `source_file` is not pinned here. The bd-1klbq2zd acceptance covers it through the probe.
- **Shared `canonical()` in tests: leave the ~55 copies alone.** They are correct as a filesystem oracle as long as comparisons stay Path-to-Path.

Decided 2026-09-29, after the seam prototype (supersedes "this branch fixes the seam only"):
- **This branch ships Piece 1 only.** The `notebook_path` RED stays failing on Windows. It is tracked by bd-1klbq2zd, not skipped. The seam fix moves to bd-1klbq2zd on its own branch, with the direct-call audit done **before** the seam switch.

## Seam prototype results (2026-09-29, Windows)

The prototype replaced the body of `NativeRuntime::canonicalize` with `dunce::canonicalize(path)`, adding `dunce = "1"` as a native-only dep of `quarto-system-runtime`.

- The RED went green, and the saved probe showed `notebook_path`, `source_file` and `Rendering single file:` all plain.
- `cargo nextest run -p quarto -p quarto-system-runtime`: the baseline has 7 failures, the seam has 26. The 20 new failures are:
  - 17 test expectations that build the expected path with `std` canonicalize (verbatim) and compare it with product output (now plain): the `commands::render` `classify_*` and `render_once_*` unit tests, and `render_scripts_cli::{env_contract_full_render, post_render_script_receives_output_files}` (`QUARTO_PROJECT_DIR`).
  - `render_cli_e2e::output_equal_to_input_refuses_and_preserves_source`. The overwrite guard (`render_to_file.rs:647`) compares paths lexically. The test passes a verbatim `-o`, and the input is now plain. With plain user input the guard already missed before the fix, so the gap moves rather than appears.
  - `preview_static_e2e::a_page_inside_the_project_opens_on_that_page`, not classified.
  - `quarto-system-runtime cache_lru::tests::concurrent_get_and_set_lru_do_not_lose_the_set` (os error 5 on persist), probably flaky, not classified.
- Conclusion: the seam change is not local. Seam output (plain) meets verbatim paths from the ~186 direct calls, so the audit has to come first.


## Checklist (this branch: Piece 1 only; Piece 2 items belong to bd-1klbq2zd)

- [x] RED: capture the real failure on Windows (above)
- [x] Confirm the product URL builder and its visibility (QER `diagnostic.rs:889/944`, private)
- [x] Sweep sibling tests (Q3). Only json_errors.rs:595 is a string-built URL.
- [x] Classify test problem vs support gap. Result: both (§ Classification, probe saved)
- [x] Research ecosystem precedent (§ Research)
- [x] Piece 1: Option C for the URL assertion + strengthened `notebook_path` assertion in `json_errors.rs`. Update the doc comment `:510-521` and the stale comment `:594`. (The `notebook_path` assertion lives in `ipynb_parse_error_json_carries_cell_origin`, the test that owns `:490`.)
- [x] Confirm the new state: URL assertion passes, and the `notebook_path` assertion fails on Windows with `\\?\C:\…` (the product RED)
- [x] Prototype the seam fix and measure the fallout (§ Seam prototype results). Piece 2 moved to bd-1klbq2zd.
- [x] Sanity check that the URL assertions catch regressions: a verbatim-leak target, a fragment, the wrong file, and the pseudo-path each fail; a correct target passes. The `notebook_path` equality check gets its sanity pass once bd-1klbq2zd turns it green.
- [ ] Close bd-clq56rem once this branch merges. bd-1klbq2zd stays open and owns the RED.

Handed off to bd-1klbq2zd (not tracked by this checklist): design and apply the seam fix, then GREEN. Its task breakdown (seam, UNC behavior, direct-call audit dispositions) belongs in bd-1klbq2zd's own plan. Its acceptance must include CLI checks, via the saved probe, that `notebook_path`, `source_file` **and** the `Rendering …` status line are plain on Windows. This json_errors test pins only `notebook_path` (§ Decisions), so the other outputs must not rely on it.

## Verification

```bash
# target test (crate-scoped; no workspace build)
cargo nextest run -p quarto -E 'test(ipynb_diagnostic_hyperlinks_real_notebook)'

# rest of json_errors
cargo nextest run -p quarto -E 'test(json_errors::)'
```

No full-crate or workspace *test* run locally: CLAUDE.local.md overrides AGENTS.md's pre-push steps on this machine, and CI (Linux/macOS) runs the full suite. Before opening the PR, ask the user about one `cargo build --workspace`. CI has no Windows leg, so that build is the only place Windows-only compile errors (e.g. cfg-gated dead code under `-D warnings`) get caught.

## Crate suite snapshot (Windows, HEAD `e8379cfe`, informational only)

Verification policy until Windows CI exists: check only this strand's own failure locally, and trust CI (Linux/macOS) for the rest. The full `quarto` crate suite has other pre-existing Windows failures, so a full-suite run is not a pass/fail gate for this fix.

A partial run (542/545 tests, stopped early) showed 6 failures. Only the first one belongs to this strand:

| Test | Symptom | Class |
|---|---|---|
| `json_errors::ipynb_diagnostic_hyperlinks_real_notebook` | expected `file://\\?\C:\…` | **this strand** |
| `project_profile_cli::render_verbose_echoes_active_profiles` (`:163`) | stderr shows `Rendering project: \\?\C:\…`. The assertion is about echoing the profile, and it is not yet clear whether the path causes the failure. | product-side verbatim display. Likely bd-1klbq2zd territory, not a test-built URL. |
| `render_scripts_cli::list_form_runs_scripts_in_order` (`:525`), `…explicit_interpreter_command_line_with_args` (`:557`) | `"one\r\ntwo\r\n"` | CRLF, epic bd-eehxwr29 class |
| `preview_static_e2e::cli_flags_override_project_preview_keys` (`:847`), `…project_preview_keys_set_the_defaults_and_unsupported_keys_warn` (`:800`) | watcher / port assertions | unrelated (preview config) |

These are not investigated here. Tracking status for the last five was not checked.

## Risks / tradeoffs

- Option C's OSC8 extractor must find the link wrapping the *cell label*, not just the first `ESC]8;;` in the output, in case QER ever links more than one file per diagnostic.
- `Url::to_file_path` on Windows requires a drive-letter or UNC form. It returns `Err` on the verbatim-leak shape, which is the behavior we want (the test fails loudly). This is unverified until implementation: confirm by hand-feeding a malformed URL during the sanity step.
