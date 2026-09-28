# Windows: json_errors ipynb hyperlink test builds expected file:// URL from verbatim path (bd-clq56rem)

**Date:** 2026-09-28
**Braid:** bd-clq56rem (related: bd-1klbq2zd)
**Worktree:** `.worktrees/bd-clq56rem-windows-jsonerrors-ipynb-hyperlink` (branch `braid/bd-clq56rem-windows-jsonerrors-ipynb-hyperlink`, based on `main` @ `e8379cfe`)
**Status:** Investigation done. Design is pending alignment with the user. **Do not start implementation until the user gives the go-ahead.**

## Overview

`quarto::integration json_errors::ipynb_diagnostic_hyperlinks_real_notebook` fails on Windows. The product is correct. The test's *expected* URL is wrong: it builds `file://` + the display form of a `std::fs::canonicalize`d path, which on Windows is the verbatim `\\?\C:\...` form. On Unix the same concatenation yields a valid `file:///tmp/...` URL by coincidence, which is why nobody saw the bug.

This is a test-only fix. No product change is needed, and the fix does not depend on bd-1klbq2zd.

## Triage verdict

**Ready to design.** The root cause is confirmed at HEAD, only one test is affected, and there are three viable fixes. One design question remains (which oracle the test should use, below).

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

Note: the test-local `fn canonical` helper is copied into about 55 test files. It is a plain passthrough to `std::fs::canonicalize`, so any *future* test that string-compares its output against product text will hit this same bug. Hardening every copy is out of scope here (YAGNI), but see design question 2.

### Q4 — Relationship to bd-1klbq2zd

The two strands are independent in both directions.

- The URL is built entirely inside QER, which re-canonicalizes and strips the prefix itself (`diagnostic.rs:906, 912`). Whatever bd-1klbq2zd does to q2's own path display, this OSC8 URL does not change. So this fix does not depend on it.
- The recommended fix (Option C) adds no helper and no dependency, so it does not pre-empt bd-1klbq2zd's choice between `dunce`, a `quarto_util` helper, and strip-at-display. Option B *would* add `dunce` to the tree as a test dep, which is a small nudge toward one answer to bd-1klbq2zd. That is one more reason to prefer C.
- One adjacent observation, not verified: `origin.notebook_path` in the JSON diagnostic (asserted only by `ends_with` at `json_errors.rs:~490`) may carry the verbatim prefix on Windows. If so, it belongs to bd-1klbq2zd's "where do these paths flow" audit, not here.

## Fix options

### Option A — Mirror QER in the test: strip `\\?\` and use `Url::from_file_path`

Add a small test helper that strips the verbatim prefix, then call `url::Url::from_file_path(stripped).as_str()`, and keep the exact `contains(osc8)` assertion.

- Pros: smallest diff, and it keeps the exact-sequence assertion (which also proves there is no fragment).
- Cons: it reimplements QER's `plain_absolute_path` in the test. Both sides would compute the URL "the same way", so the test proves they agree, not that the link is correct. If QER's strip and the copy drifted in the same wrong direction (say, both mishandling UNC), the test would still pass. This is the tautology the brief warns against.

### Option B — Canonicalize without the verbatim form (`dunce`), then `Url::from_file_path`

Change the test so `dir` comes from `dunce::canonicalize` (or apply `dunce::simplified` to the URL input only), then `Url::from_file_path(dir.join("broken.ipynb"))`, and keep the exact match.

- Pros: uses an independent, widely used ecosystem oracle instead of a hand-written copy of QER's strip. The diff is small and keeps the exact assertion. If applied to the shared `canonical()` in json_errors.rs, the cwd we hand to q2 is also the non-verbatim form, which is closer to what a user would have.
- Cons: adds a dev-dep to `crates/quarto`. It changes `canonical()` for 14 other tests in the file (they pass today, and dunce is identical to std on Unix). It nudges bd-1klbq2zd toward `dunce` before that strand has done its ecosystem research. It still compares URL *spelling*, so a legitimate encoding difference (e.g. a lowercase drive letter) would fail the test even though the link works.

### Option C — Semantic round-trip: parse the product's link, resolve it, compare paths (recommended)

Pull out the OSC8 target that wraps the `broken.ipynb[cell 2, markdown]` label, then assert:

1. `url::Url::parse(target)` succeeds with `scheme() == "file"`.
2. `url.fragment().is_none()`. This keeps the "no `#line:col` on origin links" contract that the exact-sequence match used to prove implicitly.
3. `std::fs::canonicalize(url.to_file_path().unwrap()) == canonical(&dir.join("broken.ipynb"))`, a Path-to-Path comparison where both sides are canonicalized by std, so verbatim-vs-plain cannot differ.

- Pros:
  - The test never builds a URL itself, so there is no copy of product logic and no tautology.
  - It proves exactly what the doc comment promises: the link opens the real notebook on disk.
  - It is platform-neutral with no cfg branches and no new deps (`url` is already a dependency).
  - It still fails on the real regressions:
    - A pseudo-path target (the pre-`FileOrigin` bug) → `to_file_path`/canonicalize fails or points elsewhere.
    - A verbatim leak (`file://?/C:/…`) → `?` parses as a query, so the path is wrong or empty and the comparison fails.
    - An added fragment → assertion 2 fails.
- Cons:
  - More test code: a small OSC8-target extractor, about 10-15 lines, local to the file.
  - It no longer pins the exact URL string, e.g. percent-encoding choices. That is QER's contract, covered by QER's own unit test (`diagnostic.rs:~2690`), not q2's. q2's contract here is that the right file gets linked.
  - The doc comment at `json_errors.rs:510-521` ("Asserting the exact `ESC]8;;URL ESC\` sequence covers both") must be rewritten to describe the new assertions.

**Recommendation: Option C.** It answers the brief's constraints directly: it fixes the root cause (a string-built oracle) instead of swapping in another string-built oracle, it does not weaken the check to "ends with broken.ipynb" (a full path identity check is stronger than today's check), it is not a tautology, and it is independent of bd-1klbq2zd. Option B is the fallback if the user prefers keeping an exact-string assertion.

## Open design questions for the user

1. **Oracle choice.** Option C (semantic round-trip, no exact URL string) or Option B (exact string via `dunce`, adds a dev-dep)? Recommended: C.
2. **Shared `canonical()` hardening.** Leave the ~55 copies of the test-local `canonical()` alone (the recommendation, since nothing else is fragile today), or file a low-priority strand to note the trap near those copies?
3. **`origin.notebook_path` verbatim check.** Should I spend one extra probe confirming whether the JSON `notebook_path` carries `\\?\` on Windows, and add the result as a comment on bd-1klbq2zd? This is not part of this fix.

## Checklist (after design sign-off)

- [x] RED: capture the real failure on Windows (above)
- [x] Confirm the product URL builder and its visibility (QER `diagnostic.rs:889/944`, private)
- [x] Sweep sibling tests (Q3). Only json_errors.rs:595 is fragile.
- [ ] Implement the chosen option in `crates/quarto/tests/integration/json_errors.rs` (test-only; no product code)
- [ ] Update the test's doc comment (`:510-521`) and the stale comment at `:594` so they match the new assertions
- [ ] GREEN: the target test passes on Windows
- [ ] Sibling check: `json_errors::` module green on Windows (the only file touched). Rely on CI for the rest (see § Crate suite snapshot).
- [ ] Sanity check that the new assertions still catch regressions: temporarily point the expectation at a different file and confirm the test fails, then revert. (There is no Unix host locally, and CI's Linux/macOS legs cover Unix.)
- [ ] Close bd-clq56rem with a link to the commit

## Verification

```bash
# target test (crate-scoped; no workspace build)
cargo nextest run -p quarto -E 'test(ipynb_diagnostic_hyperlinks_real_notebook)'

# rest of json_errors
cargo nextest run -p quarto -E 'test(json_errors::)'
```

No full-crate or workspace run locally. CI covers the rest.

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
