- **Engine-channel tests** that need to exercise jupyter / knitr / a custom Jupyter kernel without requiring those runtimes: use the **replay engine** (bd-45yw). Record a trace once with `trace: true` on a development machine, check it in, and replay it via `RenderToFileOptions.replay_capture`. See `claude-notes/instructions/replay-engine.md`.
- **CRITICAL - TEST FIRST**: When fixing bugs using tests, you MUST run the failing test BEFORE implementing any fix. This is non-negotiable. Verify the test fails in the expected way, then implement the fix, then verify the test passes.
- Always strive for minimal test documents as small as possible. Create many small test documents instead of a few large test documents.
- You are encouraged to spend time and tokens on thinking about good tests.
- If writing tests is taking a lot of time, decompose the writing of tests into subtasks. Good tests are important!
- Precise tests are good tests. **bad**: testing for the presence of a field in an object. **good** testing if the value of the field is correct.
- When choosing hex colors for CSS test assertions (`ensureCssRegexMatches`), use **non-condensable** 6-digit hex values. CSS minifiers shorten `#RRGGBB` to `#RGB` when each pair is a repeated digit (e.g., `#cc5500` → `#c50`). Break at least one pair to prevent this: `#cc5501` instead of `#cc5500`.
- Do not write tests that expect known-bad inputs. Instead, add a failing test, and create a beads task to handle the problem.

## Native vs WASM Lua Testing

Native tests (`cargo nextest run`) use `Lua::new()` with the full C stdlib on all platforms.
This is the standard Lua environment — tests can use `io.open`, `os.time`, and all standard
library functions.

WASM-specific code paths (restricted Lua stdlib, synthetic io/os modules) are tested
separately on the real `wasm32-unknown-unknown` target in CI. See `dev-docs/wasm.md` for
the WASM architecture and build details.

**Never add `test` to the `#[cfg(target_arch = "wasm32")]` guard.** This was a prior pattern
that caused Windows test failures. WASM coverage is provided by dedicated WASM tests in CI.

## End-to-End Testing for WASM Features

**CRITICAL**: When implementing features that involve the WASM module (`wasm-quarto-hub-client`), you MUST write and run end-to-end tests BEFORE claiming the feature works.

### Why This Matters

The WASM module is a separate compilation target with its own:
- `Cargo.toml` (excluded from workspace)
- Runtime environment (browser or Node.js)
- Dependencies (must be added separately)

Changes that compile in the Rust workspace may NOT work in WASM. Always verify with actual WASM execution.

### How to Test WASM Features

1. **Build the WASM module**:
   ```bash
   cd hub-client && npm run build:wasm
   ```

2. **Create a Node.js test script** (`hub-client/test-wasm.mjs`):
   ```javascript
   import { readFile } from 'fs/promises';
   import { dirname, join } from 'path';
   import { fileURLToPath } from 'url';

   const __dirname = dirname(fileURLToPath(import.meta.url));

   // Import from the built pkg directory
   const wasm = await import('./node_modules/wasm-quarto-hub-client/wasm_quarto_hub_client.js');
   const wasmPath = join(__dirname, 'node_modules/wasm-quarto-hub-client/wasm_quarto_hub_client_bg.wasm');
   const wasmBytes = await readFile(wasmPath);
   await wasm.default(wasmBytes);

   // Test your feature
   const content = '# Hello\n\nWorld';
   const result = JSON.parse(wasm.render_qmd_content(content, ''));
   console.log('Success:', result.success);
   console.log('HTML:', result.html);

   // Verify expected output
   if (!result.html.includes('data-loc')) {
     console.error('FAIL: Expected data-loc attributes in HTML');
     process.exit(1);
   }
   ```

3. **Run the test**:
   ```bash
   cd hub-client && node test-wasm.mjs
   ```

### What to Verify

For any WASM feature, the test should verify:
1. The WASM function is callable (no missing exports)
2. The function returns expected data structure
3. The actual content/behavior is correct (not just "no errors")

### DO NOT

- Claim a WASM feature is complete based only on `cargo check` or `npm run build`
- Assume TypeScript type declarations match actual WASM exports
- Test only in the browser when a Node.js test would be faster and more reliable

## Smoke-All Tests

Smoke-all test fixtures live in `crates/quarto/tests/smoke-all/`. Each `.qmd` file embeds assertions in `_quarto.tests` frontmatter. There are **three independent runners** that exercise the same fixtures through different pipelines:

### 1. Rust (native renderer)
```bash
cargo nextest run -p quarto --test smoke_all
```
Fastest (~1s). Renders via `quarto-core` directly. Runs all assertion types including `ensureHtmlElements` (CSS selectors via `scraper`), `ensureCssRegexMatches`, `ensureFileRegexMatches`, etc.

### 2. WASM Vitest (jsdom)
```bash
cd hub-client && npm run test:wasm
```
~3s. Renders via WASM module in Node.js with jsdom for HTML assertions. Runs the full smoke-all suite plus other WASM tests.

### 3. Playwright E2E (browser)
```bash
cd hub-client && npx playwright test e2e/smoke-all.spec.ts
```
~12s. Full pipeline: Automerge sync → hub server → browser → WASM render → preview iframe. Tests the complete hub-client integration.

**CRITICAL prerequisites for Playwright tests:**

1. **Build with `VITE_E2E=1`** before running any Playwright test:
   ```bash
   cd hub-client
   VITE_E2E=1 npm run build   # compiles test hooks into the bundle
   ```
   Without this flag, `window.__quartoTest` is tree-shaken out and every
   `page.evaluate` call that reads it fails with "E2E test hooks not found".
   `bootstrapProjectSet` checks for this and throws a clear error, but the
   root cause is always a missing `VITE_E2E=1` build.

2. **No conflicting hub server on port 3031**: `globalSetup` starts its own
   `cargo run --bin hub` on port 3031. If a dev hub is already bound there,
   the test hub fails to start. Dev hubs should use port 3030 (the default).
   Stop any running hub before running Playwright tests.

3. **No conflicting Vite server on port 5174**: `playwright.config.ts` serves
   the built app via `npm run preview -- --port 5174`. Port 5173 is left for
   the dev server (`npm run dev`). The two can coexist.

The full e2e command handles the build automatically:
```bash
cd hub-client && npm run test:e2e   # build:wasm + VITE_E2E=1 build + playwright
```

### Writing Fixtures

Each fixture is a `.qmd` file with test assertions in frontmatter. The project must have a `_quarto.yml`.

**IMPORTANT**: `ensureFileRegexMatches`, `ensureCssRegexMatches`, and
`ensureHtmlElements` all use the same two-array format. The outer array
has exactly **two positional elements** (second is optional):

1. First element: array of patterns that **must match**
2. Second element: array of patterns that **must NOT match**

Each element is an array of patterns, not a single pattern. Put **all**
must-match patterns together in the first array. A common mistake is
listing patterns as separate array elements — the second one becomes
a must-NOT-match list:

```yaml
# WRONG — "kbd\\.js" becomes a must-NOT-match pattern!
ensureFileRegexMatches:
  - ["kbd\\.css"]
  - ["kbd\\.js"]

# CORRECT — both patterns must match
ensureFileRegexMatches:
  - ["kbd\\.css", "kbd\\.js"]
```

Full example:

```yaml
_quarto:
  tests:
    html:
      ensureCssRegexMatches:
        - ["#170229", "my-custom-rule"]   # patterns that must appear in CSS
        - ["unwanted-pattern"]             # patterns that must NOT appear
      ensureHtmlElements:
        - ["nav#TOC", "div.callout"]       # CSS selectors that must match
        - ["div.should-not-exist"]         # selectors that must NOT match
      ensureFileRegexMatches:
        - ["expected-pattern", "another-expected"]  # must match
        - ["should-not-appear"]                     # must NOT match
      noErrors: true
```

### Running a Single Fixture

Use `SMOKE_FILTER` to run only fixtures whose relative path contains the
given string. This works for both the Rust and WASM runners:

```bash
# Rust — run only kbd fixture
SMOKE_FILTER=kbd cargo nextest run -p quarto -E 'test(smoke_all)'

# WASM Vitest — run only kbd fixture
SMOKE_FILTER=kbd npm run test:wasm

# Playwright — use native --grep (no env var needed)
npx playwright test smoke-all --grep kbd
```

To render a fixture directly and inspect its output:
```bash
cargo run --bin q2 -- render crates/quarto/tests/smoke-all/path/to/doc.qmd -v
```

## Pipeline traces (`quarto-trace`)

Setting `trace: true` in a document's metadata enables the
`JsonTraceObserver`, which writes a typed snapshot of every pipeline
stage to `.quarto/trace/<stem>/latest.json.gz`. Traces are useful as
regression-test fixtures (small enough to check in and load offline)
and as user-attached bug-report artifacts. See
`claude-notes/plans/2026-05-03-trace-size-for-replay.md` (bd-5qnj) for
the design and budget rationale.

### On-disk format

- **Compressed compact JSON** at `.quarto/trace/<stem>/latest.json.gz`.
  Pretty-print accounted for ~80% of bytes on real traces; gzip on top
  collapses what's left.
- **Schema version 2** with content-addressed AST dedup: every unique
  AST is stored once in a top-level `asts` map, and pipeline entries
  refer to it via `{ "$ref": "<hash>" }` sentinels. The reader
  rehydrates these into inline AST values, so consumers see a v1-shaped
  in-memory `TraceDocument`. Hand-written v1 traces (no `asts`, no
  `$ref`) still parse via the rehydration no-op path.
- See `crates/quarto-trace/src/lib.rs` for the schema struct and
  `claude-notes/plans/5qnj-trace-size-investigation/measurements.md`
  for size measurements on real fixtures.

### Inspecting a trace

```bash
# List traces under ./.quarto/trace/
q2 trace list

# Show the full trace for a doc, pretty-printed (rehydrated AST refs)
q2 trace show --doc <stem>

# Show one stage's entry only
q2 trace show --doc <stem> --stage parse

# Open the trace-viewer SPA (auto-binds a free port, prints URL)
q2 trace view

# Raw on-disk bytes (compact gzipped JSON; use jq after gunzip)
gunzip -c .quarto/trace/<stem>/latest.json.gz | jq '.schema_version, (.asts | length)'
```

### Writing tests against a trace fixture

When writing a regression test that consumes a trace, prefer
`quarto_trace::read::read_trace` over hand-parsing JSON — it handles
both v1 and v2 inputs uniformly, and rehydrates `$ref` sentinels. For
hand-authored fixtures, write valid v1 JSON (no `asts` map, inline
ASTs); the reader accepts them.

```rust
use quarto_trace::read::read_trace;

let doc = read_trace(&fixture_path).expect("parse trace");
assert_eq!(doc.pipeline.len(), expected);
// `data.ast` (or bare `data` for transform: entries) is fully inlined
// regardless of on-disk format.
```

### Size budgets

Provisional ceilings, not hard targets:

- **Checked-in CI fixture**: ≤ 100 KB (compressed). Keeps `.git/`
  manageable as fixtures accumulate.
- **User-attached bug-report artifact**: ≤ 1 MB (compressed). Fits a
  GitHub issue attachment without effort.

A 6.1 KB qmd fixture currently produces a 62 KB compressed trace —
well within both budgets. If you find a fixture exceeding the CI
budget, capture the measurement and file an issue rather than checking
in the oversize artifact; the dedup pass may need extension (e.g. for
new stage types that emit large non-AST payloads).