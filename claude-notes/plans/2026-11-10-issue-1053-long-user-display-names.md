---
title: "Issue 1053: long user display names"
---

## Overview

GitHub issue #1053 asks `updateUserName` to accept real full names longer than 50 characters and requires presence and attribution UI to remain usable for those names. The fix will use test-driven development and preserve the existing public API.

## Checklist

- [x] Inspect name validation, storage constraints, presence and attribution rendering, and existing tests; choose the supported limit and minimal layout changes.
- [x] Add focused regression tests for long-name acceptance and UI overflow behavior; run them before implementation and confirm the expected failures.
- [x] Implement the validation and layout fix with minimal changes.
- [x] Run focused tests and the required hub-client build and test suites.
- [x] Exercise long names through the real hub-client UI and inspect presence and attribution rendering, or explicitly record any verification limitation.
- [x] Keep attribution badges and tooltips inside narrow viewports, including attributed text near the right edge; add regression coverage.
- [x] Review the final diff and report changed files and verification results.

### Connect name-limit alignment

- [x] Confirm Connect's first/last-name limits and display-name composition from the implementation.
- [x] Add regression tests for the 513-code-point combined boundary and Unicode code-point counting; verify they fail first.
- [x] Align validation with Connect's two 256-code-point fields plus their separating space.
- [x] Rerun focused and required hub-client verification.

### Review follow-ups

- [x] Anchor the attribution badge to the hovered text's right edge near the viewport edge, instead of shifting it left by a fixed 320px (a short badge landed far from its text).
- [x] Keep each collaborator `<li>` a direct child of its `<ul>` in the Status tab; the tooltip now wraps the name inside the item.
- [x] Reuse each peer's cursor style element across presence updates and rebuild it only when the name or color changes.
- [x] Count code points in `isValidUserName` only when the UTF-16 length is between the limit and twice the limit.
- [x] Drop `min-width: 0` declarations made redundant by `.qh-truncate`, and the badge's CSS `max-width` (the inline value always applies).
- [x] Rerun the hub-client build and test suites on the follow-ups (`npm run build:all`, `npm run test:ci`, preview-renderer `npm test`, and `cargo nextest run -p quarto-core attribution_viewer` passed; not re-inspected in a browser).

## Details

- Source issue: https://github.com/quarto-dev/q2/issues/1053
- The Connect user API's `MaxNameLength` is 256 for each of `first_name` and `last_name`, counted with Go's `utf8.RuneCountInString`. Quarto Hub joins the fields with one space, so `MAX_USER_NAME_LENGTH` is 513 Unicode code points.
- Work is isolated in the `fix/issue-1053-long-user-names` branch from `origin/main`; the pre-existing checkout and its dirty plan file are untouched.
- No commit or push will be created without explicit user approval.

## Verification

- The Connect boundary tests failed before the validator change because both valid 513-code-point names were rejected. All six focused storage-consumer tests passed afterward.
- Final Connect-alignment checks passed: `npm run preflight`; `npm run test:ci` (1,769 unit, 240 integration, and 450 WASM tests passed, with 28 WASM tests skipped); `npm run build:all`; focused ESLint; and `cargo xtask lint --quiet`.
- The full strict `claude-notes` render passed (1,706 files).
- The new regression tests failed before implementation for the 50-character validation limit and missing layout behavior. Further tooltip-wrapping, narrow-viewport, and right-edge attribution tests also failed before their fixes.
- Focused hub-client regressions: 85 tests passed. Focused preview-renderer attribution tests: 11 tests passed.
- `npm run test:ci` in `hub-client`: passed on the final code.
- `npm test` in `ts-packages/preview-renderer`: 660 passed, 40 skipped.
- `cargo nextest run -p quarto-core attribution_viewer`: 8 passed.
- `npm run build:all`, `npm run lint:css`, `cargo xtask lint --quiet`, TypeScript checks, and ESLint on the changed files passed. The changed `usePresence.ts` retains one pre-existing warning at line 339. Repository-wide `npm run lint` reports its existing 190 errors in unrelated files.

### End-to-end browser inspection

A temporary Playwright scenario used two real browser contexts and a running hub. It saved two 513-character names through the classic identity UI, made an edit as the remote collaborator, and inspected cursor, status, replay, and attribution surfaces in Chromium. The scenario was moved out of the repository after passing.

Exact test invocation (after the E2E-enabled production build):

```sh
npx playwright test --config=playwright.config.ts \
  e2e/long-display-names.verification.spec.ts \
  --project=chromium --workers=1 --retries=0
```

Observed output (the cursor's full 513-character CSS content is omitted here):

```text
LONG_NAME_E2E {"statusLayout":{"clientWidth":154,"scrollWidth":4196,"textOverflow":"ellipsis"},"replayLayout":{"clientWidth":189,"scrollWidth":3397,"textOverflow":"ellipsis","chipMaxWidth":"200px"},"badgeLayout":{"left":8,"right":240,"width":232,"height":186,"whiteSpace":"normal","overflowWrap":"anywhere","viewportWidth":248}}
1 passed
```

The cursor's full 513-character content was also present in its generated CSS. Screenshots of status, replay, and attribution were captured and visually inspected. They show ellipsized cursor/status/replay labels, a bounded tooltip wrapping the full name, and an attribution badge wrapping the full name at a narrow iframe width.
