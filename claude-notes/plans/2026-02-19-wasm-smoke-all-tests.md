# WASM Smoke-All Test Runner (TypeScript)

## Overview

Create a vitest-based test runner that exercises the WASM rendering module against the same smoke-all test fixtures used by the native Rust test runner. This verifies that the WASM rendering pipeline (used by hub-client for live preview) produces correct output for all tested scenarios including project metadata, directory metadata, and basic rendering.

**Prerequisite**: The Rust-side changes in `claude-notes/plans/2026-02-18-wasm-project-context.md` are complete (commit `54875cee`). `render_qmd` now discovers `_quarto.yml` and `_metadata.yml` from the VFS.

## Architecture

```
smoke-all test fixtures (shared, checked into repo)
  crates/quarto/tests/smoke-all/**/*.qmd
  crates/quarto/tests/smoke-all/**/_quarto.yml
  crates/quarto/tests/smoke-all/**/_metadata.yml
        │
        ├──→ Native Rust runner (existing)
        │      crates/quarto/tests/smoke_all.rs
        │      calls quarto_test::run_test_file()
        │      uses NativeRuntime + real filesystem
        │
        └──→ WASM vitest runner (THIS PLAN)
               hub-client/src/services/smokeAll.wasm.test.ts
               reads fixtures from disk (Node.js fs)
               populates WASM VFS, calls render_qmd()
               checks assertions against returned HTML
```

## Key Files to Understand

Before implementing, read these files:

### WASM module entry points
- **`crates/wasm-quarto-hub-client/src/lib.rs`** — The WASM module. Key functions:
  - `render_qmd(path: &str) -> Promise<String>` — Renders a QMD file from VFS. Returns JSON: `{ "success": true, "html": "...", "warnings": [...] }` or `{ "success": false, "error": "...", "diagnostics": [...] }`
  - `vfs_add_file(path: &str, content: &str)` — Add a text file to VFS
  - `vfs_clear()` — Clear user files from VFS (preserves embedded resources)
  - `vfs_list_files()` — List all VFS files (returns JSON array)
  - All VFS paths use the `/project/` prefix (e.g., `/project/_quarto.yml`, `/project/chapters/doc.qmd`)

### Existing WASM test patterns
- **`hub-client/src/services/projectContext.wasm.test.ts`** — Best example to follow: loads WASM module, populates VFS with `vfs_add_file`, calls `render_qmd`, and asserts on HTML output. Demonstrates VFS clear/populate/render cycle.
- **`hub-client/src/services/changelogRender.wasm.test.ts`** — Another WASM test example (simpler, no VFS population).
- **`hub-client/vitest.wasm.config.ts`** — Vitest config for WASM tests. Matches `src/**/*.wasm.test.ts`, uses `environment: 'node'`, 30-second timeout. Run with `npm run test:wasm`.

### Smoke-all test fixtures
- **`crates/quarto/tests/smoke-all/`** — The shared test fixtures. Two categories:
  - `quarto-test/` — 7 basic rendering tests (callouts, code blocks, error handling, file existence)
  - `metadata/` — 7 project/directory metadata test directories, each with `_quarto.yml`, possibly `_metadata.yml` files, and `.qmd` test documents

### Test specification format
- **`crates/quarto-test/src/spec.rs`** — Rust parser for `_quarto.tests` specs (reference for the TS implementation)
- **`~/src/quarto-cli/tests/smoke/smoke-all.test.ts`** — Original TS implementation we're drawing from (Deno-based, not directly reusable)

### Existing hub-client dependencies
- **`jsdom`** is already a dev dependency in `hub-client/package.json` — use it for `ensureHtmlElements` CSS selector queries
- **`yaml`** — **not currently in `hub-client/package.json`**. Must be added as a devDependency (`npm install -D yaml` from repo root). Needed for parsing `_quarto.tests` nested YAML from frontmatter. The `yaml` package (v2+) is the standard choice.

## Work Items

### Phase 0: Dependencies

- [x] Add `yaml` package as a devDependency: run `npm install -D yaml` **from the repo root** (npm workspaces). This is needed for parsing `_quarto.tests` nested YAML from frontmatter.

### Phase 1: Test runner infrastructure

File: `hub-client/src/services/smokeAll.wasm.test.ts`

- [x] WASM module initialization in `beforeAll` (follow `projectContext.wasm.test.ts` pattern — it's more relevant than `changelogRender` since it uses VFS)
- [x] `discoverTestFiles()`: use Node.js `fs` + `path` to walk `../../../crates/quarto/tests/smoke-all/**/*.qmd` relative to the test file (3 levels up from `src/services/` → `hub-client/` → repo root). Return array of absolute paths. Skip files starting with `_`.
- [x] `readFrontmatter(qmdContent: string)`: extract YAML frontmatter from QMD content (text between first `---` and second `---`). Parse with a YAML library.
- [x] `parseTestSpecs(metadata)`: extract test specs from `metadata._quarto.tests` (two levels: `metadata["_quarto"]["tests"]`). Return `{ runConfig, formatSpecs }` where each formatSpec has `{ format, assertions, checkWarnings, expectsError }`. Track `checkWarnings` as a boolean that starts `true` and is set to `false` when `noErrors`, `noErrorsOrWarnings`, or `shouldError` is encountered (matching the Rust `spec.rs` pattern). Must handle:
  - `fileExists` uses object format `{ outputPath?: string, supportPath?: string }` (not bare strings)
  - `printsMessage` can be a single object or an array of objects
  - Unknown assertion keys must throw an error (matching both TS Quarto and Rust behavior)
  - `ensureHtmlElements` uses the same two-array YAML format as `ensureFileRegexMatches`: first array = must-match selectors, second array (optional) = must-not-match selectors. Example: `ensureHtmlElements: - ["nav#TOC"]` or `ensureHtmlElements: - [] - ["nav#TOC"]`. Reuse the same array parsing logic.
- [x] `shouldSkip(runConfig)`: check `skip`, `ci`, `os`, `not_os` conditions. For WASM tests, we run on all OSes (it's platform-independent), so `os`/`not_os` checks may not apply — but implement them for compatibility.
- [x] `populateVfs(testDir, wasm)`: given a test directory (e.g., `smoke-all/metadata/project-inherits/`), find the project root (the directory containing `_quarto.yml`, or the test directory itself if no `_quarto.yml`), then recursively read all files as UTF-8 text and add them to VFS with `vfs_add_file()`. Note: `vfs_add_file` is text-only (`&str` content) — binary files in fixtures would need different handling, but current fixtures are all text.
  - **Path mapping**: if the project root on disk is `/abs/path/to/smoke-all/metadata/project-inherits/`, map it to `/project/` in VFS. A file at `.../project-inherits/chapters/doc.qmd` becomes `/project/chapters/doc.qmd`.
  - Call `vfs_clear()` before each test to reset state.
  - Use `vfs_list_files()` as a diagnostic tool when debugging VFS population failures.

### Phase 2: Assertion implementations

Each assertion is a function that takes the parsed WASM render result and throws on failure.

The WASM render result JSON has this shape (from `RenderResponse` and `JsonDiagnostic` in `lib.rs`):
```typescript
interface JsonDiagnostic {
  kind: string;           // Serde-serialized DiagnosticKind (verify actual casing at runtime!)
  title: string;          // The message text
  code?: string;
  problem?: string;
  hints: string[];        // Always present (may be empty array)
  start_line?: number;    // 1-based, for Monaco
  start_column?: number;
  end_line?: number;
  end_column?: number;
  details: Array<{ kind: string; content: string; start_line?: number; start_column?: number; end_line?: number; end_column?: number; }>;
}

interface WasmRenderResult {
  success: boolean;
  html?: string;          // Present when success === true
  error?: string;         // Present when success === false
  warnings?: JsonDiagnostic[];   // Present on success (when there are warnings)
  diagnostics?: JsonDiagnostic[];  // Present on failure (parse errors)
}
```

**Important**: `diagnostics` and `warnings` are mutually exclusive — `diagnostics` only appears when `success === false`, `warnings` only when `success === true`. They are never both present.

The `diagnostics[].title` is the message text, and `diagnostics[].kind` maps to log levels for `printsMessage` matching. The Rust runner uses `DiagnosticKind::{Error, Warning, Info, Note}` → `LogLevel::{Error, Warn, Info, Debug}` (see `runner.rs:270-275`). **Verify the exact serialized string casing** of `kind` at runtime (e.g., is it `"error"` or `"Error"`?) — the spec YAML uses uppercase (`ERROR`, `WARN`) so the mapping must be case-insensitive or normalized.

- [x] **ensureFileRegexMatches(result, matches: string[], noMatches?: string[])**
  - Assert `result.success === true` and `result.html` exists
  - For each pattern in `matches`: `new RegExp(pattern, 'm').test(result.html)` must be true
  - For each pattern in `noMatches`: `new RegExp(pattern, 'm').test(result.html)` must be false
  - Use multiline flag (`m`) to match TS Quarto behavior

- [x] **ensureHtmlElements(result, selectors: string[], noMatchSelectors?: string[])**
  - Parsed from the same two-array YAML format as `ensureFileRegexMatches`: `[[selectors...], [noMatchSelectors...]]`
  - Assert `result.success === true` and `result.html` exists
  - Parse HTML with `new JSDOM(result.html)`
  - For each selector in first array: `document.querySelector(selector) !== null`
  - For each selector in second array: `document.querySelector(selector) === null`

- [x] **noErrors(result)**
  - Assert `result.success === true`
  - If failed, include `result.error` and diagnostic titles in the error message

- [x] **noErrorsOrWarnings(result)**
  - Assert `result.success === true`
  - Assert `result.warnings` is empty or absent
  - Report any warning titles in the error message

- [x] **shouldError(result)**
  - Assert `result.success === false`

- [x] **printsMessage(result, { level, regex, negate? })**
  - Collect messages from whichever is present: `result.diagnostics` (on failure) or `result.warnings` (on success) — they are mutually exclusive, never both present. Map each to `{ level, message }` where `level` is derived from `kind` (case-insensitive) and `message` is `title`.
  - Filter by `level`
  - Check if any `message` matches `new RegExp(regex)`
  - If `negate`, assert none match; otherwise assert at least one matches

- [x] **fileExists / folderExists / pathDoesNotExist** — always-passing no-ops
  - Parse from spec so the test file still exercises rendering
  - `fileExists` spec uses `{ outputPath?: string, supportPath?: string }` object format — parse it but don't check anything
  - `pathDoesNotExist` / `pathDoNotExists` (both spellings) and `folderExists` take bare string paths — parse them but don't check anything
  - These filesystem assertions are meaningless in the WASM VFS context

- [x] **Default assertion**: after running all explicit assertions, if `checkWarnings` is still `true` (i.e., none of `noErrors`, `noErrorsOrWarnings`, or `shouldError` were encountered during spec parsing), run `noErrorsOrWarnings` as a default check. This mirrors the Rust runner (`runner.rs:192-201`) and TS Quarto's `smoke-all.test.ts`.

### Phase 3: Test execution loop

- [x] For each discovered test file:
  - Read content, parse frontmatter, extract test specs
  - Check skip conditions
  - For each format spec: **only test `html` format** (WASM only renders HTML)
  - Register a vitest `it()` test case with descriptive name (relative path + format)
  - In the test body: clear VFS, populate VFS, call `render_qmd(vfsPath)`, parse JSON result, run assertions
- [x] Handle the project root detection: some test directories have `_quarto.yml` at the root (metadata tests), others don't (quarto-test/ basic tests). The VFS population logic needs to find the right root.
  - For metadata tests: the project root is the directory containing `_quarto.yml` (e.g., `smoke-all/metadata/project-inherits/`)
  - For basic tests: there's no `_quarto.yml`, so the QMD file's directory is the project root
  - Walk upward from the QMD file's directory to find `_quarto.yml`, stopping at `smoke-all/` (don't walk above the test fixture tree)

### Phase 4: Verification

- [x] Run `npm run test:wasm` from hub-client — new smoke-all tests pass
- [x] Verify timeout is sufficient (WASM init + multiple renders; may need to increase from 30s in vitest config)
- [x] If any tests fail due to WASM rendering differences (not bugs, just unsupported features), add appropriate skip annotations in the test runner (not in the shared fixture files). Likely candidate: `expected-error.qmd` — the WASM error path (`QuartoError::Parse`) may format the error message differently than native `render_to_file`, so the `printsMessage` regex `"unexpected character"` may not match.

## Notes

- **VFS `/project/` prefix**: The hub-client WASM module uses `/project/` as the VFS root by convention. All paths passed to `render_qmd()` and `vfs_add_file()` must use this prefix.
- **Format filtering**: WASM only produces HTML. Any test spec for non-HTML formats (pdf, docx, etc.) should be silently skipped. Currently all smoke-all tests specify `format: html`.
- **`ensureHtmlElements`**: The native Rust runner recognizes `ensureHtmlElements` but treats it as a no-op (not yet implemented). Unknown assertion types now cause a hard error in both Rust and TS. The WASM runner will be the first to actually check CSS selectors via jsdom. If a test fails because the HTML structure differs from expectations, that's a real finding.
- **WASM module build**: The WASM module must be built before running tests. `npm run test:wasm` should handle this, or the test should fail clearly if the WASM binary is missing. Check how existing `.wasm.test.ts` files handle this.
- **No file output**: Unlike native rendering which writes HTML to disk, WASM `render_qmd` returns HTML in the JSON response. Assertions that would read an output file instead check `result.html` directly.
- **Concurrent test isolation**: Each test must call `vfs_clear()` before populating VFS to ensure isolation. The WASM module is a singleton (single global VFS state), so tests cannot run in parallel. Vitest runs tests within a file sequentially by default, which is sufficient. Do not add `.concurrent` to describe/it blocks. If vitest config changes in the future, may need explicit `sequence: { concurrent: false }` in the WASM vitest config.
