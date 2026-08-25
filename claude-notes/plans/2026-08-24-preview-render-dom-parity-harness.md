# Preview ↔ Render DOM Parity Harness — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Created:** 2026-08-24 (revised the same day after a blank-slate review)
**Branch:** `explore/react-parity-harness` (worktree `.worktrees/workspace-1`, off `main` @ `cf9c45cc8`)
**Related strands:** bd-tmb2u5yu (`Math.tsx` drops `math inline|display` —
found while designing this; blocks opting in any math fixture), bd-qn8yi1su
(revealjs analogue — *not* in scope, but should extend this harness),
bd-2yd37vuk, bd-tqijrhsu (open parity gaps opted-in fixtures may surface)
**Origin:** an external "dual-engine parity testing" spec (Gemini) evaluated
against this codebase — see § Assessment.

**Goal:** Automatically assert, for opted-in `smoke-all` fixtures, that
`q2 preview`'s React renderer produces the same article-body DOM as
`q2 render`'s native HTML writer.

**Architecture:** A fourth `smoke-all` runner,
`hub-client/src/services/smokeAllParity.wasm.test.tsx`, renders each opted-in
fixture twice through the same WASM module — `render_page_in_project` (native
writer → full HTML) and `render_page_for_preview` (the *same* Rust function
with `prefer_preview_format: true` → pipeline stopped before
`render-html-body` → Pandoc AST JSON) — mounts the AST read-only with the real
`<Ast registry={previewRegistry}>` under jsdom, and compares the canonical form
of `main#quarto-document-content` from both sides. Canonicalisation is a small
pure module with an explicit, reasoned normalisation table. Fixtures opt in
with `dom-dom-parity: true` under a format entry of the existing `_quarto: tests:` DSL.

**Tech Stack:** vitest 4 (jsdom env via docblock, hub-client
`vitest.wasm.config.ts`), `wasm-quarto-hub-client`, `@testing-library/react`,
`jsdom`. **No new dependencies.**

**Spec:** this file (§ Design). The design was brainstormed, approved, and
then revised after a blank-slate review in the session that wrote this plan;
there is no separate spec document.

## Global Constraints

- **No new npm or cargo dependencies.** jsdom + testing-library are already
  devDeps of `hub-client` and `ts-packages/preview-renderer`; `katex` reaches
  preview-renderer through the root `package.json` hoist (pre-existing).
- **Fresh WASM is a prerequisite.** Every `*.wasm.test.*` runs the checked-in
  `hub-client/wasm-quarto-hub-client/wasm_quarto_hub_client_bg.wasm`. Run
  `cd hub-client && npm run build:wasm` once at the start of the plan and
  again after any Rust change (there is one: Task 2.1 touches `quarto-test`,
  which the WASM does **not** depend on — so no rebuild is needed for it, but
  rebuild anyway if `main` has moved under you).
- **HTML article body only.** Comparison root is `main#quarto-document-content`.
  revealjs, tabsets, engine fixtures, math *content*, and hub-client-only
  chrome are non-goals (§ Non-goals).
- **Curated allowlist, not xfail.** Only fixtures with `dom-dom-parity: true` are
  compared; an opted-in fixture that diverges fails the suite. There is no
  expected-failure mechanism — fix the divergence or don't opt in. (Accepted
  cost: a fixture that *finds* a bug carries no regression guard until the
  bug is fixed and the fixture opts in. That is the chosen trade-off.)
- **Every normalisation rule cites a reason** (and a strand/file where one
  exists). A rule without a reason is a bug hiding a bug.
- **Read-only mount defines the contract.** The preview side is a bare
  `<Ast registry={previewRegistry}>` with no `PreviewContext` /
  `AssetManifestContext` / `IncrementalContext` — the component markup the
  `preview-render-parity` skill's contract is about. Edit chrome, comment
  chrome, and asset blob-URL rewriting are preview-only by design and are
  *not* compared.
- **Zero new CI wiring.** The runner is a `*.wasm.test.tsx` file, picked up by
  `npm run test:wasm` → `test:ci` → `cargo xtask verify` →
  `.github/workflows/ts-test-suite.yml:164`.
- **Test-only Node code stays out of the app build.** Anything importing
  `fs`/`path`/`url` lives under `hub-client/src/test-utils/` (excluded by
  `hub-client/tsconfig.app.json:28-35`) or in a `*.test.*` file.
- **Typechecking scope (pre-existing):** `npm run typecheck` in hub-client
  runs `tsc -p tsconfig.app.json --noEmit`, which *excludes* tests and
  `src/test-utils/`; vitest transpiles test files without typechecking.
  `npx tsc --noEmit -p tsconfig.json` checks **nothing** (solution file) —
  never use it as a gate.
- **Gate per CLAUDE.md:** `cargo clippy -p quarto-test --all-targets -- -D
  warnings` + `cargo nextest run -p quarto-test` after Task 2.1; one
  `cargo nextest run --workspace` at the Phase 2 boundary (the only Rust
  phase); full `cargo xtask verify` (hub-client is touched, so no
  `--skip-hub-*`) before any push.
- Cross-platform (`.claude/rules/cross-platform.md`): build paths with
  `path.join`/`resolve`; the harness runs on Linux CI and macOS dev boxes.
- `.claude/rules/integration-tests.md`: no new Rust test binaries. The
  smoke-all Rust sweep is invoked as
  `cargo nextest run -p quarto --test integration smoke_all`
  (`crates/quarto/tests/integration/main.rs:25`).

---

## Assessment of the originating idea

The external spec proposed a generic "Engine A (Quarto HTML) vs Engine B
(React `renderToString`) → normalise → tree-diff" harness over `smoke-all`.
Evaluated against q2:

- **Sound in its core.** The "React component tree" is real:
  `ts-packages/preview-renderer/src/q2-preview/{blocks,inlines,custom}/` —
  ~40 components mirroring `crates/pampa/src/writers/html.rs` arm-for-arm
  (`blocks/CodeBlock.tsx:71` literally says "mirrors `write_highlighted_body`").
  The repo already states the contract in
  `.claude/skills/preview-render-parity/SKILL.md:7` and polices it *by hand*
  in Chrome, one bug at a time (bd-y1fs3, bd-coffj, bd-nxslt, the revealjs
  audit `claude-notes/plans/2026-06-17-revealjs-preview-render-parity-audit.md`).
  This harness automates an existing, repeatedly-violated contract — and the
  review of this very plan found a fresh violation (bd-tmb2u5yu) by reading
  two files side by side.
- **The data bridge is not `--to json`.** The preview pipeline is
  `build_q2_preview_pipeline_stages` = HTML pipeline minus `math-js`,
  `render-html-body`, `apply-template` (`crates/quarto-core/src/pipeline.rs:394`)
  and minus seven transforms replicated in React (`Q2_PREVIEW_TRANSFORM_EXCLUDED`,
  `pipeline.rs:1523`: `callout-resolve`, `crossref-render`, `title-block`,
  `mermaid-render`, `panel-tabset`, `panel-tabset-resolve`,
  `attribution-viewer`). Render's post-transform AST already has those
  lowered; React expects them as CustomNodes. The correct bridge is the WASM
  export `render_page_for_preview` (`crates/wasm-quarto-hub-client/src/lib.rs:1293`),
  which returns `ast_json`.
- **The two sides are one function.** `render_page_in_project`
  (`lib.rs:1123`) and `render_page_for_preview` both call
  `render_page_in_project_with_attribution` → `ProjectContext::discover` →
  the same `is_single_file` branch into `render_single_doc_to_response` /
  `render_project_active_page_to_response`, differing only in
  `prefer_preview_format` (`lib.rs:1426`). So **every diff the harness
  reports is in `html.rs`/template vs React, by construction.** (`render_qmd`,
  `lib.rs:1001`, is *not* a valid counterpart: it always takes the
  single-doc path, so project fixtures such as `appendix/` would compare
  different pipelines.)
- **Corrections to the spec:** no cheerio/htmlparser2 (jsdom + `scraper`
  exist); no `renderToString` (components use contexts/hooks — mount with
  testing-library exactly like `q2-preview.integration.test.tsx:80`); compare
  `<main>` only, not whole pages (`apply-template` is excluded; navbar,
  sidebar and footer live outside `<main>` and are Rust HTML strings on both
  sides anyway); **do not** strip ids or generic `data-*` (ids are part of
  the contract — the reveal audit's F1 was "section id dropped"); no custom
  tree-diff (line-oriented canonical text + vitest's diff is enough);
  `glob(**/*.qmd)` day-one is a wall of red (tabsets have no React
  implementation) — hence the allowlist.
- **Existing tooling reused:** `hub-client/src/services/smokeAll.wasm.test.ts`
  already loads the WASM in vitest, walks `smoke-all/`, parses the DSL, and
  populates the VFS — the harness is its sibling. Prior art on normalisation:
  `crates/pampa/tests/integration/test.rs:691` (`normalize_html`, tag-per-line),
  `hub-client/e2e/helpers/smokeAllAssertions.ts:42` (`stripSourceTrackingSpans`).

## Design

### Contract

For an opted-in fixture `F` with a `html` format entry:

```
canon(main#quarto-document-content of render_page_in_project(F).html)
  ===
canon(main#quarto-document-content of mount(<Ast astJson={render_page_for_preview(F).ast_json} registry={previewRegistry}/>))
```

where `canon` is `canonicalize()` from § Normalisation and `mount` is a
bare read-only mount (§ Global Constraints). React-replicated transforms
(title-block, callouts, crossref, theorems, footnotes) **are** compared —
that is the point. So are `toc-body` and page-navigation when a fixture
enables them: the template puts both *inside* `<main>`
(`crates/quarto-core/src/template.rs:318-331`) and `PreviewDocument.tsx`
must mirror that placement.

### Components

| File | Responsibility |
|---|---|
| `ts-packages/preview-renderer/src/test-utils/domParity.ts` | Pure DOM → canonical text. `canonicalize`, `extractParityRoot`, `compareParity`, `PARITY_RULES`. No WASM, no fixtures, no fs. The directory already exists (`setup.ts` lives there) and is excluded from the package's `tsconfig.json`, so this never reaches `dist/` — consumers import it through vitest's alias only. |
| `ts-packages/preview-renderer/src/test-utils/domParity.test.ts` | Unit tests for the above (jsdom via docblock). |
| `hub-client/src/test-utils/smokeAllFixtures.ts` | Fixture discovery, front-matter reading, `_quarto.tests` block access, skip logic, project-root discovery, VFS population, user-grammar handle, WASM loader — **extracted** from `smokeAll.wasm.test.ts`, which then imports it. Under `test-utils/` so its Node-builtin imports stay out of the app tsconfig. |
| `hub-client/src/services/smokeAllParity.wasm.test.tsx` | The runner: per opted-in fixture, render both ways, mount, compare, report. Plus the "harness catches an injected divergence" self-test. `.tsx` because it mounts JSX. |
| `hub-client/vitest.wasm.config.ts` | `include` widened to `*.wasm.test.{ts,tsx}`. |
| `crates/quarto-test/src/spec.rs` | Accept `dom-parity` key into `TestSpec.dom_parity: bool` (runner ignores it). |
| `hub-client/src/services/smokeAll.wasm.test.ts`, `hub-client/e2e/helpers/smokeAllDiscovery.ts` | Accept and ignore `dom-parity`. |
| Opted-in fixtures under `crates/quarto/tests/smoke-all/` | `dom-dom-parity: true` added under `html:`. |
| `claude-notes/instructions/testing.md`, `.claude/skills/preview-render-parity/SKILL.md` | Document the fourth runner and the harness-first workflow. |

### Normalisation rules (`PARITY_RULES` + the canonical serialiser)

| Rule | Why |
|---|---|
| Strip attributes `data-loc`, `data-sid` | Preview-only source tracking. The writer emits them only when `include_source_locations` is on (`html.rs` doc block ~L743-748) — off for `q2 render`; the preview AST is written with `include_inline_locations: true` (`PreviewAstOutput.ast_json`, `pipeline.rs:195`) and React forwards them via `dataLocProps`. |
| Replace the children of `span.math` with one opaque text node `⟨opaque⟩` | `math-js` (excluded from preview) leaves TeX in `\(…\)` delimiters for MathJax; React `inlines/Math.tsx:24` emits KaTeX HTML. Divergent by design; the `<span>` and its classes still compare. **Today `Math.tsx` emits no class at all** (bd-tmb2u5yu), so on the preview side the selector matches nothing and the class diverges — no fixture with math can opt in until that strand closes. The rule is written for the fixed state on purpose. |
| **Unwrap any `<div>` that has no attributes left after the strip rule, on both sides** (added by the Task 0.2 spike) | React cannot inject raw HTML without a host element, so `blocks/RawBlock.tsx:44` wraps every `RawBlock(format: "html")` in a `<div>` (`dangerouslySetInnerHTML`) that the writer (`html.rs`, `Block::RawBlock`) never emits — most visibly the code-copy button. Must be symmetric: a preview-only variant would false-positive on a `Div` block with an empty `Attr` (render `<div>` vs preview `<div data-loc=…>`); `title-block/simple-default.qmd` has two such divs on both sides. **Accepted cost:** a missing/extra attribute-less `<div>` is invisible to the runner (such a div matches no id/class selector; the render side's `ensureHtmlElements` assertions still cover `div.quarto-title-meta > div`-style structure). The alternative — excluding every fixture with a code block — would gut the corpus. Ordering: after the attribute strip (the wrapper carries `data-loc`), before `normalize()` and the whitespace pass. |
| **Fail** if `data-hl-spans` is present on either side | Consumed attribute — the writer (`write_code_container_attr`, `html.rs:539`; the "reserved" comment at `:517`) and `blocks/CodeBlock.tsx` both decode it into `<span class="hl-…">` and must not forward it. Leakage is a bug (bd-nxslt); the harness must not normalise it away. |
| Sort attributes by name | Serialisation order is not semantic. |
| **Do not** sort class tokens; **do** collapse whitespace inside `class` | Class *order* is part of the mirroring contract (`sourceCode` prepend, bd-y1fs3). |
| Keep `id` and every `data-*` not listed above | Ids are contract (reveal audit F1; heading anchors). `data-qf-*`, `data-cites`, etc. are writer output that React must mirror. |
| Merge adjacent text nodes before walking (`Node.normalize()` on a clone) | React emits one text node per `Str`/`Space`; the parser emits one per run. Without merging, `"hi"`,`" "`,`"there"` vs `"hi there"` would be a false positive. |
| Inside `<pre>`: text verbatim (JSON-escaped on one line) | Collapsing would hide code-indentation bugs — the exact bug class `CodeBlock.tsx` exists to mirror. |
| Outside `<pre>`: collapse whitespace runs to one space; keep a leading/trailing space **only if the neighbouring sibling on that side is inline** (a non-whitespace text node, or an element in `INLINE_TAGS`); drop the node if nothing remains | Distinguishes `<em>a</em> <em>b</em>` from `<em>a</em><em>b</em>` and `hi <em>x</em>` from `hi<em>x</em>` (real preview bugs), while dropping the writer's pretty-printing newlines between block elements (React emits none). |
| Drop comment nodes | Not rendered. |
| Lower-case tag names; ignore void-element self-closing | Parser noise. |

Deliberately **not** a rule: `data-block-pool-id` (React edit chrome). Under
the read-only mount it is never emitted (`usePreviewEdit.ts:21` returns no
resolver without a `PreviewContext`), so a strip rule would be dead code with
a false reason. If the mount configuration ever changes, add it then.

### Canonical form

One line per node, two-space indent per depth, no closing lines; text lines
are JSON string literals:

```
<main class="content" id="quarto-document-content">
  <header class="quarto-title-block default" id="title-block-header">
    <div class="quarto-title">
      <h1 class="title">
        "A Simple Document"
  <p>
    "hi "
    <em>
      "there"
```

Line-oriented on purpose: vitest's `toBe` diff on two such strings shows the
divergent node with context, and a `diff` of the two `.norm.txt` artifacts
reads the same way. No tree-diff engine (YAGNI; `quarto-ast-reconcile`'s
`find_first_divergence`, `hash.rs:677`, exists if we ever want "first
divergent node").

### Failure output

On mismatch the runner (a) records `<rel-path> [html]: parity mismatch` with
vitest's diff text in the aggregated failure list (same single-`it` pattern
as the sibling runner), and (b) writes
`hub-client/test-results/parity/<rel-path-with-separators-as-__>/{render,preview}.norm.txt`
(`test-results/` is already gitignored, `hub-client/.gitignore:18`). Per-fixture
wall time is logged so runtime growth is visible (the sweep shares the
sibling's 120 s hang-detection timeout, `vitest.wasm.config.ts`).

### Non-goals (explicit)

- revealjs decks (`.reveal .slides`) — bd-qn8yi1su; should reuse
  `domParity.ts` when it lands.
- Tabsets — no React implementation at all (`pipeline.rs:1556`); a fixture
  containing a tabset cannot be opted in.
- Engine fixtures (`run.requires: [knitr|jupyter]`, one today:
  `includes/code-cell/code-cell.qmd`) — WASM has no engines. The WASM
  `RunConfig` does not model `requires`; the allowlist makes this moot:
  do not opt them in.
- Math *content* (opaque by rule), hub-client-only chrome outside `<main>`
  (navbar/sidebar/footer), attribution badges and edit/comment chrome
  (preview-only), Q1 ↔ Q2 parity.
- Computed-style / CSS parity — `crates/quarto-core/tests/integration/preview_render_css_parity.rs`
  covers theme CSS byte-equality already.

---

## Checklist

### Phase 0 — Groundwork + spike
- [x] 0.1 Extract `hub-client/src/test-utils/smokeAllFixtures.ts` from `smokeAll.wasm.test.ts` (pure refactor, identical counts)
- [x] 0.2 Throwaway spike over 4 fixtures using the extracted helpers; research note with divergence classes + initial allowlist

### Phase 1 — `domParity.ts`
- [x] 1.1 `canonicalize` — shape, attribute sorting, text/whitespace rules, `<pre>` verbatim, text-node merging
- [x] 1.2 `PARITY_RULES` — strip list, opaque `span.math`, forbidden `data-hl-spans`
- [x] 1.3 `extractParityRoot` + `compareParity`

### Phase 2 — `dom-parity:` DSL key
- [x] 2.1 Rust `quarto-test`: parse into `TestSpec.dom_parity`, runner ignores
- [x] 2.2 Both TS parsers (WASM sibling, Playwright discovery) ignore `dom-parity`; Phase-2 boundary workspace nextest

### Phase 3 — Runner
- [x] 3.1 `smokeAllParity.wasm.test.tsx` + first opt-in in one green commit
- [x] 3.2 Opt in the remaining fixtures the spike showed green; all four runners green

### Phase 4 — Triage + docs + gate
- [x] 4.1 File a strand per real divergence found (bd-tmb2u5yu already filed)
- [x] 4.2 Update `testing.md` and the `preview-render-parity` skill
- [x] 4.3 Full `cargo xtask verify`; reconcile this checklist; commit

---

## Phase 0 — Groundwork + spike

### Task 0.1: Extract `smokeAllFixtures.ts` (pure refactor)

**Files:**
- Create: `hub-client/src/test-utils/smokeAllFixtures.ts`
- Modify: `hub-client/src/services/smokeAll.wasm.test.ts` (remove the moved code, import it)

Done first so the spike (0.2) and the runner (3.1) use the *same* fixture →
VFS semantics as the existing sweep (project-root discovery, `/project/`
prefix relative to the project root, binary files, user grammars). A spike
that hand-rolls VFS loading would report divergences the real runner never
sees.

**Interfaces:**
- Produces (moved verbatim from `smokeAll.wasm.test.ts` unless noted; line
  numbers are the pre-move locations):
  ```ts
  export const SMOKE_ALL_DIR: string;                                   // L88 (test-utils/ and services/ are siblings under src/, so the relative path is unchanged)
  export interface JsUserGrammarsHandle { … }                           // L24
  export interface WasmModule {                                          // L32, two methods added:
    …;
    render_page_in_project: (path: string, user_grammars?: unknown) => Promise<string>;
    render_page_for_preview: (path: string, user_grammars?: unknown, capture_gz_json?: Uint8Array) => Promise<string>;
  }
  export interface RunConfig { … }                                      // L59
  export interface FileEntry { … }                                      // L365
  export async function loadSmokeWasm(): Promise<WasmModule>;           // NEW: body of the beforeAll at L103-133
  export async function discoverTestFiles(dir: string): Promise<string[]>;       // L139
  export function readFrontmatter(content: string): Record<string, unknown>;      // L164
  export function readTestsBlock(metadata: Record<string, unknown>): { run: RunConfig | null; formats: Record<string, Record<string, unknown>> } | null;  // NEW
  export function parseTwoArraySpec(value: unknown): { matches: string[]; noMatches: string[] };  // L181
  export function shouldSkip(runConfig: RunConfig | null): string | null;         // L293
  export async function findProjectRoot(qmdDir: string): Promise<string>;         // L322
  export async function readAllFiles(dir: string, projectRoot: string): Promise<FileEntry[]>;  // L372
  export async function populateVfs(wasm: WasmModule, qmdPath: string): Promise<{ vfsPath: string; projectFiles: FileEntry[] }>;  // L404, wasm param added
  export async function buildUserGrammarsHandle(wasm: WasmModule, files: readonly FileEntry[]): Promise<JsUserGrammarsHandle | undefined>;  // L435, wasm param added
  ```
  `parseTestSpecs`/`parseFormatSpec`, `WasmRenderResult`, `JsonDiagnostic`,
  `FormatSpec`, `AssertionFn` and the assertion factories **stay** in
  `smokeAll.wasm.test.ts` — they are that runner's assertion model.
  `readTestsBlock` is the small shared accessor both runners use.

- [x] **Step 1: Fresh WASM, then record the baseline**

```bash
cd hub-client && npm run build:wasm
cd hub-client && npm run test:wasm 2>&1 | tee /private/tmp/claude-502/-Users-gordon-src-q2/6f5c0c8f-a359-437a-87d6-879b0e289c0e/scratchpad/wasm-baseline.log; grep -E "Smoke-all WASM results|Tests " /private/tmp/claude-502/-Users-gordon-src-q2/6f5c0c8f-a359-437a-87d6-879b0e289c0e/scratchpad/wasm-baseline.log
```
Record the `N passed, M skipped, 0 failed` line — the refactor must reproduce it exactly.

- [x] **Step 2: Create `smokeAllFixtures.ts`**

Move the listed items. `__dirname`-relative paths: `SMOKE_ALL_DIR` was
`resolve(__dirname, '../../../crates/quarto/tests/smoke-all')` from
`src/services/`; from `src/test-utils/` it is the same depth, so the
expression is unchanged. The two new functions:

```ts
/**
 * Initialise the WASM module from the checked-in build and wire the
 * dart-sass VFS callbacks. Shared by every `*.wasm.test.*` smoke-all
 * runner; call once from `beforeAll`.
 */
export async function loadSmokeWasm(): Promise<WasmModule> {
  const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
  const wasmBytes = await readFile(join(wasmDir, 'wasm_quarto_hub_client_bg.wasm'));
  const wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
  await wasm.default(wasmBytes);
  const sassModule = await import('/src/wasm-js-bridge/sass.js');
  sassModule.setVfsCallbacks(
    (path: string): string | null => {
      try {
        const result = JSON.parse(wasm.vfs_read_file(path)) as { success: boolean; content?: string };
        return result.success && result.content !== undefined ? result.content : null;
      } catch {
        return null;
      }
    },
    (path: string): boolean => {
      try {
        const result = JSON.parse(wasm.vfs_read_file(path)) as { success: boolean; content?: string };
        return result.success && result.content !== undefined;
      } catch {
        return false;
      }
    },
  );
  return wasm;
}

/**
 * The `_quarto.tests` block split into its `run` config and its
 * per-format raw mappings. Returns null when the fixture has no tests
 * block. A format entry that is not a mapping (e.g. `html: default`) is
 * normalised to `{}`, matching the Rust parser
 * (crates/quarto-test/src/spec.rs `parse_format_spec`: `value.as_mapping()`
 * optional). Runners parse the per-format mapping themselves (their
 * assertion models differ); this keeps the *shape* of the DSL in one
 * place.
 */
export function readTestsBlock(
  metadata: Record<string, unknown>,
): { run: RunConfig | null; formats: Record<string, Record<string, unknown>> } | null {
  const quarto = metadata['_quarto'] as Record<string, unknown> | undefined;
  const tests = quarto?.['tests'] as Record<string, unknown> | undefined;
  if (!tests) return null;
  const formats: Record<string, Record<string, unknown>> = {};
  for (const [key, value] of Object.entries(tests)) {
    if (key === 'run') continue;
    formats[key] =
      value !== null && typeof value === 'object' && !Array.isArray(value)
        ? (value as Record<string, unknown>)
        : {};
  }
  return { run: (tests['run'] as RunConfig) ?? null, formats };
}
```

`populateVfs` and `buildUserGrammarsHandle` take `wasm: WasmModule` as
their first parameter instead of closing over a module-level `let wasm`.

- [x] **Step 3: Rewire `smokeAll.wasm.test.ts`**

Delete the moved definitions; add
`import { SMOKE_ALL_DIR, loadSmokeWasm, discoverTestFiles, readFrontmatter, readTestsBlock, parseTwoArraySpec, shouldSkip, populateVfs, buildUserGrammarsHandle, type WasmModule, type RunConfig } from '../test-utils/smokeAllFixtures';`.
`beforeAll` becomes `wasm = await loadSmokeWasm();`. `parseTestSpecs` becomes:

```ts
function parseTestSpecs(metadata: Record<string, unknown>, options: ParseOptions = {}) {
  const block = readTestsBlock(metadata);
  if (!block) return { runConfig: null, formatSpecs: [] };
  const formatSpecs = Object.entries(block.formats).map(([format, value]) =>
    parseFormatSpec(format, value, options),
  );
  return { runConfig: block.run, formatSpecs };
}
```

Call sites: `populateVfs(wasm, testFile)`, `buildUserGrammarsHandle(wasm, projectFiles)`.
Do **not** add a `dom-parity` case yet — this task is behaviour-preserving;
Task 2.2 does that.

- [x] **Step 4: Verify identical results**

```bash
cd hub-client && npm run test:wasm 2>&1 | tee /private/tmp/claude-502/-Users-gordon-src-q2/6f5c0c8f-a359-437a-87d6-879b0e289c0e/scratchpad/wasm-after.log; grep -E "Smoke-all WASM results|Tests " /private/tmp/claude-502/-Users-gordon-src-q2/6f5c0c8f-a359-437a-87d6-879b0e289c0e/scratchpad/wasm-after.log
cd hub-client && npm run typecheck
```
Expected: the `passed/skipped/failed` line is byte-identical to Step 1;
`typecheck` clean (it proves nothing about the moved file — that is the
point: the app project must not have grown a Node import).

- [x] **Step 5: Commit**

```bash
git add hub-client/src/test-utils/smokeAllFixtures.ts hub-client/src/services/smokeAll.wasm.test.ts
git commit -m "Extract smoke-all fixture/VFS helpers for reuse by a parity runner"
```

### Task 0.2: Spike — dump both sides for four fixtures (throwaway)

**Files:**
- Create (throwaway, deleted at the end of the task, never committed):
  `hub-client/src/services/paritySpike.wasm.test.tsx`
- Modify (kept — Task 3.1 needs it too): `hub-client/vitest.wasm.config.ts:16`
  → `include: ['src/**/*.wasm.test.{ts,tsx}']`
- Create: `claude-notes/research/2026-08-24-preview-render-parity-spike.md`

**Purpose:** answer, cheaply and before building anything:
- **Q1** does the WASM module initialise under `// @vitest-environment jsdom`?
- **Q2** does `render_page_for_preview` succeed on plain smoke-all fixtures,
  and does `render_page_in_project` return `html` for them?
- **Q3** does `<Ast>` from `@quarto/preview-renderer/framework` mount from a
  hub-client test with no setup file (no `matchMedia`/`ResizeObserver` shims
  — preview-renderer's non-test sources reference none, but confirm)?
- **Q4** what do the raw `<main>` subtrees actually differ by, so the rules
  table in § Design is confirmed or amended *before* Phase 1?

Fixtures: `markdown/heading-auto-id.qmd`, `highlighting/01-builtin-python.qmd`,
`appendix/footnotes-heading.qmd` (a project — `appendix/_quarto.yml`
exists, so this exercises the project branch on both sides),
`title-block/simple-default.qmd`.

- [x] **Step 1: Widen the include glob** (positional file arguments only
  *filter within* `include`; a `.tsx` outside the glob is "no test files
  found")

```ts
      include: ['src/**/*.wasm.test.{ts,tsx}'],
```

- [x] **Step 2: Write the spike file**

```tsx
// @vitest-environment jsdom
/**
 * THROWAWAY spike for claude-notes/plans/2026-08-24-preview-render-dom-parity-harness.md
 * Task 0.2. Deleted at the end of the task — never committed.
 */
import { describe, it, beforeAll } from 'vitest';
import { mkdir, writeFile } from 'fs/promises';
import { dirname, join, resolve } from 'path';
import { fileURLToPath } from 'url';
import { JSDOM } from 'jsdom';
import { render, cleanup } from '@testing-library/react';
import { Ast } from '@quarto/preview-renderer/framework';
import { previewRegistry } from '@quarto/preview-renderer';
import {
  SMOKE_ALL_DIR,
  loadSmokeWasm,
  populateVfs,
  buildUserGrammarsHandle,
  type WasmModule,
} from '../test-utils/smokeAllFixtures';

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT_DIR = resolve(__dirname, '../../test-results/parity-spike');

const FIXTURES = [
  'markdown/heading-auto-id.qmd',
  'highlighting/01-builtin-python.qmd',
  'appendix/footnotes-heading.qmd',
  'title-block/simple-default.qmd',
];

let wasm: WasmModule;

beforeAll(async () => {
  wasm = await loadSmokeWasm();
});

describe('parity spike', () => {
  it('dumps <main> from both sides', async () => {
    await mkdir(OUT_DIR, { recursive: true });
    for (const rel of FIXTURES) {
      wasm.vfs_clear();
      cleanup();
      const { vfsPath, projectFiles } = await populateVfs(wasm, join(SMOKE_ALL_DIR, rel));
      const grammars = await buildUserGrammarsHandle(wasm, projectFiles);

      const renderRes = JSON.parse(await wasm.render_page_in_project(vfsPath, grammars));
      const previewRes = JSON.parse(await wasm.render_page_for_preview(vfsPath, grammars, undefined));

      const slug = rel.replace(/[\\/]/g, '__');
      const renderMain = new JSDOM(renderRes.html ?? '').window.document
        .querySelector('main#quarto-document-content');
      const { container } = render(
        <Ast
          astJson={previewRes.ast_json ?? '{"pandoc-api-version":[1,23,0],"meta":{},"blocks":[]}'}
          currentFilePath={vfsPath}
          onNavigateToDocument={() => {}}
          setAst={() => {}}
          registry={previewRegistry}
        />,
      );
      const previewMain = container.querySelector('main#quarto-document-content');

      await writeFile(join(OUT_DIR, `${slug}.render.html`),
        renderMain?.outerHTML ?? `NO MAIN — success=${renderRes.success} error=${renderRes.error}`);
      await writeFile(join(OUT_DIR, `${slug}.preview.html`),
        previewMain?.outerHTML ?? `NO MAIN — success=${previewRes.success} error=${previewRes.error}`);
    }
  });
});
```

- [x] **Step 3: Run the spike**

```bash
cd hub-client && npx vitest run --config vitest.wasm.config.ts src/services/paritySpike.wasm.test.tsx
```

Expected: the test passes (it only dumps) and eight files appear under
`hub-client/test-results/parity-spike/`. If it fails, that is a Q1–Q3
answer — record what and how it was worked around; that changes Task 3.1.

- [x] **Step 4: Inspect and diff each pair**

```bash
cd hub-client/test-results/parity-spike
for f in *.render.html; do
  b=${f%.render.html}
  echo "=== $b"
  diff <(sed 's/></>\n</g' "$f") <(sed 's/></>\n</g' "$b.preview.html") | head -80
done
```

Read every diff. Classify each hunk as one of: **(a)** serialiser noise the
§ Normalisation table already covers; **(b)** a *deliberate* divergence not
yet in the table (needs a new rule with a reason); **(c)** a real bug (React
or writer — bd-tmb2u5yu is a known (c) if a fixture has math).

- [x] **Step 5: Write the research note**

`claude-notes/research/2026-08-24-preview-render-parity-spike.md` with:
the exact command run; answers to Q1–Q4; a table
`fixture | (a) hunks | (b) hunks | (c) hunks | opt-in candidate?`; verbatim
snippets for every (b) and (c); the amended rules table if any (b) was
found; and the initial allowlist (fixtures whose only diffs are (a)).

- [x] **Step 6: Delete the spike, commit the note + the glob change**

```bash
rm hub-client/src/services/paritySpike.wasm.test.tsx
rm -rf hub-client/test-results/parity-spike
git add claude-notes/research/2026-08-24-preview-render-parity-spike.md hub-client/vitest.wasm.config.ts
git commit -m "Record preview/render DOM parity spike findings; accept .tsx wasm tests"
```

---

## Phase 1 — `domParity.ts`

All three tasks touch the same two files; each ends green and committed.
`ts-packages/preview-renderer/vitest.config.ts` is `environment: 'node'`
(line 67) and includes `src/**/*.test.ts`; the test file uses a jsdom
docblock. The `test-utils/` directory already exists and is excluded from
the package `tsconfig.json`, so nothing here ships in `dist/`.

### Task 1.1: `canonicalize` — shape, attributes, text and whitespace

**Files:**
- Create: `ts-packages/preview-renderer/src/test-utils/domParity.ts`
- Test: `ts-packages/preview-renderer/src/test-utils/domParity.test.ts`

**Interfaces:**
- Produces:
  ```ts
  export interface ParityRules {
    stripAttrs: ReadonlySet<string>;
    forbidAttrs: ReadonlySet<string>;
    opaqueSelectors: readonly string[];
    unwrapTags: ReadonlySet<string>;                 // added after the Task 0.2 spike
  }
  export const PARITY_RULES: ParityRules;            // empty until Task 1.2
  export const OPAQUE_MARKER = '⟨opaque⟩';
  export const INLINE_TAGS: ReadonlySet<string>;
  export class ParityRuleViolation extends Error {}
  export function canonicalize(el: Element, rules?: ParityRules): string;
  ```

- [x] **Step 1: Write the failing tests**

```ts
// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { canonicalize } from './domParity';

function el(html: string): Element {
  const host = document.createElement('div');
  host.innerHTML = html;
  return host.firstElementChild!;
}

describe('canonicalize — shape', () => {
  it('emits one line per node, indented by depth, no closing lines', () => {
    const out = canonicalize(el('<main id="x"><p>hi <em>there</em></p></main>'));
    expect(out).toBe(
      ['<main id="x">', '  <p>', '    "hi "', '    <em>', '      "there"'].join('\n'),
    );
  });

  it('sorts attributes by name and lower-cases tag names', () => {
    const out = canonicalize(el('<DIV title="t" class="c" id="i"></DIV>'));
    expect(out).toBe('<div class="c" id="i" title="t">');
  });

  it('drops pretty-printing whitespace between block elements and drops comments', () => {
    const out = canonicalize(el('<div>\n  <!-- c -->\n  <p>  a \n b  </p>\n</div>'));
    expect(out).toBe(['<div>', '  <p>', '    "a b"'].join('\n'));
  });

  it('collapses whitespace inside class but preserves token order', () => {
    const out = canonicalize(el('<pre class="  sourceCode   python ">x</pre>'));
    expect(out).toBe(['<pre class="sourceCode python">', '  "x"'].join('\n'));
  });

  it('escapes quotes in attribute values so lines stay single-line', () => {
    const out = canonicalize(el('<a title=\'say "hi"\'></a>'));
    expect(out).toBe('<a title="say &quot;hi&quot;">');
  });
});

describe('canonicalize — inline whitespace is significant', () => {
  it('distinguishes <em>a</em> <em>b</em> from <em>a</em><em>b</em>', () => {
    const spaced = canonicalize(el('<p><em>a</em> <em>b</em></p>'));
    const tight = canonicalize(el('<p><em>a</em><em>b</em></p>'));
    expect(spaced).not.toBe(tight);
    expect(spaced).toBe(['<p>', '  <em>', '    "a"', '  " "', '  <em>', '    "b"'].join('\n'));
  });

  it('keeps a trailing space before an inline sibling but trims before a block sibling', () => {
    expect(canonicalize(el('<p>hi <em>x</em></p>'))).toContain('"hi "');
    expect(canonicalize(el('<div>hi <p>x</p></div>'))).toContain('"hi"');
  });

  it('keeps text verbatim inside <pre>', () => {
    const out = canonicalize(el('<pre><code>x\n  y\n</code></pre>'));
    expect(out).toBe(['<pre>', '  <code>', '    "x\\n  y\\n"'].join('\n'));
  });

  it('merges adjacent text nodes (React emits one per Str/Space)', () => {
    const p = document.createElement('p');
    p.appendChild(document.createTextNode('hi'));
    p.appendChild(document.createTextNode(' '));
    p.appendChild(document.createTextNode('there'));
    expect(canonicalize(p)).toBe(canonicalize(el('<p>hi there</p>')));
    expect(p.childNodes.length).toBe(3); // input not mutated
  });
});
```

- [x] **Step 2: Run to verify failure**

```bash
cd ts-packages/preview-renderer && npx vitest run src/test-utils/domParity.test.ts
```
Expected: FAIL — cannot resolve `./domParity`.

- [x] **Step 3: Implement**

```ts
/**
 * DOM canonicalisation for preview ↔ render parity.
 *
 * Plan: claude-notes/plans/2026-08-24-preview-render-dom-parity-harness.md
 *
 * Converts an Element subtree to a line-oriented canonical text so two
 * independently-produced DOMs (the native HTML writer's, parsed by
 * jsdom; the React renderer's, mounted by testing-library) can be
 * compared with a plain string diff. Every normalisation applied here
 * is listed in the plan's § Normalisation table with its reason; do not
 * add a rule without one.
 *
 * Test-only: this directory is excluded from the package build, and
 * hub-client reaches it through vitest's `@quarto/preview-renderer`
 * source alias, not the package exports map.
 */

export interface ParityRules {
  /** Attribute names removed from every element before comparison. */
  stripAttrs: ReadonlySet<string>;
  /** Attribute names whose presence on either side is an error. */
  forbidAttrs: ReadonlySet<string>;
  /**
   * CSS selectors whose matching elements keep their tag + attributes
   * but have their children replaced by a single OPAQUE_MARKER line.
   */
  opaqueSelectors: readonly string[];
  /**
   * Tag names whose elements are replaced by their children when no
   * attribute survives `stripAttrs`. Applied on the clone before text
   * nodes are merged, so the unwrapped children take part in the
   * whitespace pass as siblings of the wrapper's neighbours.
   */
  unwrapTags: ReadonlySet<string>;
}

export const OPAQUE_MARKER = '⟨opaque⟩';

/** Thrown when a forbidden attribute is found (see PARITY_RULES.forbidAttrs). */
export class ParityRuleViolation extends Error {}

export const PARITY_RULES: ParityRules = {
  // Populated in Task 1.2. Empty rules == pure serialiser normalisation.
  stripAttrs: new Set<string>(),
  forbidAttrs: new Set<string>(),
  opaqueSelectors: [],
  unwrapTags: new Set<string>(),
};

/**
 * Elements whose adjacency to a text node makes that text node's edge
 * whitespace significant. Whitespace next to anything else (block
 * elements, or nothing) is the writer's pretty-printing and is dropped.
 */
export const INLINE_TAGS: ReadonlySet<string> = new Set([
  'a', 'abbr', 'b', 'br', 'cite', 'code', 'del', 'em', 'i', 'img', 'ins',
  'kbd', 'mark', 'q', 's', 'samp', 'small', 'span', 'strong', 'sub', 'sup',
  'time', 'u', 'var',
]);

const ELEMENT_NODE = 1;
const TEXT_NODE = 3;

function escapeAttr(value: string): string {
  return value.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/\n/g, ' ');
}

function isInlineNeighbor(n: Node | null): boolean {
  if (!n) return false;
  if (n.nodeType === TEXT_NODE) return (n.textContent ?? '').trim().length > 0;
  if (n.nodeType === ELEMENT_NODE) return INLINE_TAGS.has((n as Element).tagName.toLowerCase());
  return false;
}

/** Whitespace-normalised text outside <pre>; '' means "drop this node". */
function normalizeText(node: Node): string {
  let s = (node.textContent ?? '').replace(/\s+/g, ' ');
  if (!isInlineNeighbor(node.previousSibling)) s = s.replace(/^ /, '');
  if (!isInlineNeighbor(node.nextSibling)) s = s.replace(/ $/, '');
  return s;
}

function openTagLine(el: Element, rules: ParityRules): string {
  const attrs: Array<{ name: string; line: string }> = [];
  for (const { name, value } of Array.from(el.attributes)) {
    if (rules.forbidAttrs.has(name)) {
      throw new ParityRuleViolation(
        `forbidden attribute '${name}' on <${el.tagName.toLowerCase()}> — see PARITY_RULES.forbidAttrs`,
      );
    }
    if (rules.stripAttrs.has(name)) continue;
    const v = name === 'class' ? value.replace(/\s+/g, ' ').trim() : value;
    attrs.push({ name, line: `${name}="${escapeAttr(v)}"` });
  }
  attrs.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  const tag = el.tagName.toLowerCase();
  return attrs.length ? `<${tag} ${attrs.map((a) => a.line).join(' ')}>` : `<${tag}>`;
}

function walk(node: Node, depth: number, rules: ParityRules, out: string[], inPre: boolean): void {
  const pad = '  '.repeat(depth);
  if (node.nodeType === TEXT_NODE) {
    const text = inPre ? (node.textContent ?? '') : normalizeText(node);
    if (text) out.push(pad + JSON.stringify(text));
    return;
  }
  if (node.nodeType !== ELEMENT_NODE) return; // comments, processing instructions
  const el = node as Element;
  out.push(pad + openTagLine(el, rules));
  if (rules.opaqueSelectors.some((sel) => el.matches(sel))) {
    out.push(`${'  '.repeat(depth + 1)}${OPAQUE_MARKER}`);
    return;
  }
  const childInPre = inPre || el.tagName.toLowerCase() === 'pre';
  for (const child of Array.from(el.childNodes)) walk(child, depth + 1, rules, out, childInPre);
}

/** True when every attribute on `el` is in `rules.stripAttrs`. */
function hasNoSurvivingAttrs(el: Element, rules: ParityRules): boolean {
  return Array.from(el.attributes).every((a) => rules.stripAttrs.has(a.name));
}

/**
 * Replace every `rules.unwrapTags` element that has no surviving
 * attributes by its children (document order, so nested wrappers unwrap
 * too). Mutates `root`, which must be the private clone.
 */
function unwrapBareElements(root: Element, rules: ParityRules): void {
  for (const tag of rules.unwrapTags) {
    for (const el of Array.from(root.querySelectorAll(tag))) {
      if (hasNoSurvivingAttrs(el, rules)) el.replaceWith(...Array.from(el.childNodes));
    }
  }
}

/**
 * Canonical, line-oriented rendering of `el` and its subtree. Works on a
 * clone so the caller's DOM is untouched. Order on the clone: unwrap bare
 * wrapper elements (rules.unwrapTags, judged after the strip list) →
 * merge adjacent text nodes with `normalize()` (React emits one text
 * node per Str/Space) → walk (strip/forbid/sort attributes, opaque
 * subtrees, whitespace).
 */
export function canonicalize(el: Element, rules: ParityRules = PARITY_RULES): string {
  const clone = el.cloneNode(true) as Element;
  unwrapBareElements(clone, rules);
  clone.normalize();
  const out: string[] = [];
  walk(clone, 0, rules, out, false);
  return out.join('\n');
}
```

- [x] **Step 4: Run to verify pass**

```bash
cd ts-packages/preview-renderer && npx vitest run src/test-utils/domParity.test.ts
```
Expected: 9 passed.

- [x] **Step 5: Commit**

```bash
git add ts-packages/preview-renderer/src/test-utils/domParity.ts ts-packages/preview-renderer/src/test-utils/domParity.test.ts
git commit -m "Add DOM canonicaliser for preview/render parity"
```

### Task 1.2: `PARITY_RULES` — strip list, opaque math, forbidden `data-hl-spans`

**Files:**
- Modify: `ts-packages/preview-renderer/src/test-utils/domParity.ts` (`PARITY_RULES`)
- Test: `ts-packages/preview-renderer/src/test-utils/domParity.test.ts`

**Interfaces:**
- Consumes: `canonicalize`, `OPAQUE_MARKER`, `ParityRuleViolation` from Task 1.1.
- Produces: the populated `PARITY_RULES` used by the runner.

- [x] **Step 1: Write the failing tests**

Extend the existing import line to
`import { canonicalize, OPAQUE_MARKER, ParityRuleViolation } from './domParity';`
and append:

```ts
describe('PARITY_RULES', () => {
  it('strips preview-only source-tracking attributes', () => {
    const out = canonicalize(el('<p data-loc="f:1:1-1:5" data-sid="7" class="k">x</p>'));
    expect(out).toBe(['<p class="k">', '  "x"'].join('\n'));
  });

  it('keeps id and every other data-* attribute', () => {
    const out = canonicalize(el('<section id="intro" data-qf-ref-type="fig"></section>'));
    expect(out).toBe('<section data-qf-ref-type="fig" id="intro">');
  });

  it('makes span.math contents opaque but keeps the span and its classes', () => {
    const a = canonicalize(el('<span class="math inline">\\(x^2\\)</span>'));
    const b = canonicalize(el('<span class="math inline"><span class="katex">…</span></span>'));
    expect(a).toBe(b);
    expect(a).toBe(['<span class="math inline">', `  ${OPAQUE_MARKER}`].join('\n'));
  });

  it('throws ParityRuleViolation when data-hl-spans leaks', () => {
    expect(() => canonicalize(el('<pre data-hl-spans="[]"><code>x</code></pre>'))).toThrow(
      ParityRuleViolation,
    );
  });

  it("unwraps React's attribute-less RawBlock <div> wrapper (data-loc does not count)", () => {
    // preview: RawBlock.tsx host element carrying only data-loc; render: the raw HTML inline.
    const preview = canonicalize(
      el('<main><div data-loc="f:1:1-2:1"><button class="code-copy-button">c</button></div> <p>x</p></main>'),
    );
    const render = canonicalize(el('<main><button class="code-copy-button">c</button>\n<p>x</p></main>'));
    expect(preview).toBe(render);
    expect(preview).not.toContain('<div');
  });

  it('keeps a <div> that has any surviving attribute, and unwraps nested bare divs', () => {
    expect(canonicalize(el('<div class="k"><p>x</p></div>'))).toBe(['<div class="k">', '  <p>', '    "x"'].join('\n'));
    expect(canonicalize(el('<main><div><div><p>x</p></div></div></main>'))).toBe(
      canonicalize(el('<main><p>x</p></main>')),
    );
  });
});
```

- [x] **Step 2: Run to verify failure**

```bash
cd ts-packages/preview-renderer && npx vitest run src/test-utils/domParity.test.ts
```
Expected: the six new tests FAIL (attributes present / no opaque marker / no throw / `<div` still present).

- [x] **Step 3: Populate the rules**

Replace the `PARITY_RULES` constant:

```ts
export const PARITY_RULES: ParityRules = {
  stripAttrs: new Set<string>([
    // Source tracking: emitted only when `include_source_locations` is on
    // (crates/pampa/src/writers/html.rs, `data-sid`/`data-loc` doc block
    // ~L743-748). Off for `q2 render`; on for the preview AST
    // (`PreviewAstOutput.ast_json` is written with
    // `include_inline_locations: true`, crates/quarto-core/src/pipeline.rs
    // ~L195) and forwarded by React via `dataLocProps`. Preview-only by
    // construction.
    //
    // NOT listed: `data-block-pool-id` (React edit chrome). The parity
    // runner mounts read-only (no PreviewContext), so it is never emitted;
    // add it here only if the mount configuration changes.
    'data-loc',
    'data-sid',
  ]),
  forbidAttrs: new Set<string>([
    // Consumed by both writers — `write_code_container_attr`
    // (crates/pampa/src/writers/html.rs ~L539) and
    // q2-preview/blocks/CodeBlock.tsx decode it into <span class="hl-…">
    // markup and must NOT forward it. Leakage is a bug (bd-nxslt), so the
    // harness errors instead of normalising it away.
    'data-hl-spans',
  ]),
  opaqueSelectors: [
    // `math-js` is excluded from the preview pipeline
    // (Q2_PREVIEW_STAGE_EXCLUDED, crates/quarto-core/src/pipeline.rs ~L394):
    // render leaves TeX in \( \) delimiters for MathJax; React
    // (q2-preview/inlines/Math.tsx) emits KaTeX HTML. Divergent by design.
    // The <span> itself and its `math inline|display` classes still
    // compare — which is why bd-tmb2u5yu (Math.tsx emits no class) must
    // close before any fixture containing math can opt in.
    'span.math',
  ],
  unwrapTags: new Set<string>([
    // React cannot inject raw HTML without a host element:
    // q2-preview/blocks/RawBlock.tsx wraps every RawBlock(format:"html")
    // in a <div dangerouslySetInnerHTML> (carrying only data-loc) that the
    // native writer (crates/pampa/src/writers/html.rs, Block::RawBlock)
    // never emits — most visibly the code-copy button. Symmetric on
    // purpose: a Div block with an empty Attr is a bare <div> on BOTH
    // sides, so a preview-only unwrap would false-positive. Accepted
    // cost: a missing/extra bare <div> is invisible to the runner. Found
    // by the Task 0.2 spike
    // (claude-notes/research/2026-08-24-preview-render-parity-spike.md).
    'div',
  ]),
};
```

- [x] **Step 4: Run to verify pass**

```bash
cd ts-packages/preview-renderer && npx vitest run src/test-utils/domParity.test.ts
```
Expected: 15 passed.

- [x] **Step 5: Commit**

```bash
git add ts-packages/preview-renderer/src/test-utils/domParity.ts ts-packages/preview-renderer/src/test-utils/domParity.test.ts
git commit -m "Define preview/render parity normalisation rules with reasons"
```

### Task 1.3: `extractParityRoot` + `compareParity`

**Files:**
- Modify: `ts-packages/preview-renderer/src/test-utils/domParity.ts`
- Test: `ts-packages/preview-renderer/src/test-utils/domParity.test.ts`

**Interfaces:**
- Produces:
  ```ts
  export const PARITY_ROOT_SELECTOR = 'main#quarto-document-content';
  export function extractParityRoot(scope: ParentNode, label: string): Element;  // throws if absent
  export interface ParityResult { equal: boolean; render: string; preview: string; }
  export function compareParity(renderRoot: Element, previewRoot: Element, rules?: ParityRules): ParityResult;
  ```

- [x] **Step 1: Write the failing tests**

Extend the import line with `extractParityRoot, compareParity` and append:

```ts
describe('extractParityRoot / compareParity', () => {
  it('finds main#quarto-document-content and names the side on failure', () => {
    const host = document.createElement('div');
    host.innerHTML =
      '<div id="quarto-content"><main class="content" id="quarto-document-content"><p>x</p></main></div>';
    expect(extractParityRoot(host, 'render').tagName).toBe('MAIN');
    const empty = document.createElement('div');
    expect(() => extractParityRoot(empty, 'preview')).toThrow(/preview.*main#quarto-document-content/);
  });

  it('reports equal for identical subtrees modulo rules', () => {
    const r = compareParity(
      el('<main id="quarto-document-content" class="content"><p>a</p></main>'),
      el('<main class="content" id="quarto-document-content"><p data-loc="x">a</p></main>'),
    );
    expect(r.equal).toBe(true);
    expect(r.render).toBe(r.preview);
  });

  it('reports unequal and exposes both canonical texts for a class divergence', () => {
    const r = compareParity(
      el('<main id="quarto-document-content"><pre class="sourceCode python"><code>x</code></pre></main>'),
      el('<main id="quarto-document-content"><pre class="python"><code>x</code></pre></main>'),
    );
    expect(r.equal).toBe(false);
    expect(r.render).toContain('class="sourceCode python"');
    expect(r.preview).toContain('class="python"');
  });
});
```

- [x] **Step 2: Run to verify failure**

```bash
cd ts-packages/preview-renderer && npx vitest run src/test-utils/domParity.test.ts
```
Expected: 3 new FAIL (exports missing).

- [x] **Step 3: Implement**

Append to `domParity.ts`:

```ts
/** The subtree both pipelines are contractually required to agree on. */
export const PARITY_ROOT_SELECTOR = 'main#quarto-document-content';

/**
 * Locate the parity root inside a document / container. `label` names
 * the side ("render" / "preview") so a missing root is attributable.
 */
export function extractParityRoot(scope: ParentNode, label: string): Element {
  const root = scope.querySelector(PARITY_ROOT_SELECTOR);
  if (!root) {
    throw new Error(`${label}: no element matches ${PARITY_ROOT_SELECTOR}`);
  }
  return root;
}

export interface ParityResult {
  equal: boolean;
  /** Canonical text of the render side. */
  render: string;
  /** Canonical text of the preview side. */
  preview: string;
}

/**
 * Canonicalise both roots and compare. Never throws for a mismatch; does
 * throw (ParityRuleViolation) on a forbidden attribute.
 */
export function compareParity(
  renderRoot: Element,
  previewRoot: Element,
  rules: ParityRules = PARITY_RULES,
): ParityResult {
  const render = canonicalize(renderRoot, rules);
  const preview = canonicalize(previewRoot, rules);
  return { equal: render === preview, render, preview };
}
```

- [x] **Step 4: Run to verify pass**

```bash
cd ts-packages/preview-renderer && npx vitest run src/test-utils/domParity.test.ts
cd ts-packages/preview-renderer && npm test && npm run test:integration
```
Expected: 18 passed in the new file; the package suites green with the same
counts as before plus 18.

- [x] **Step 5: Commit**

```bash
git add ts-packages/preview-renderer/src/test-utils/domParity.ts ts-packages/preview-renderer/src/test-utils/domParity.test.ts
git commit -m "Add parity root extraction and comparison"
```

---

## Phase 2 — `dom-parity:` DSL key

### Task 2.1: Rust `quarto-test` accepts `parity`

**Files:**
- Modify: `crates/quarto-test/src/spec.rs:124-133` (`TestSpec`), `:180-265`
  (`parse_format_spec`; the only `Ok(TestSpec {` constructor is at `:259`)
- Test: `crates/quarto-test/src/spec.rs` tests module (`:472`+), next to
  `test_unknown_assertion_fails` (`:575`)

**Interfaces:**
- Consumes: `parse_test_specs(metadata: &Value, input_path: &Path) -> Result<(Option<RunConfig>, Vec<TestSpec>)>` (`spec.rs:139`) — note it returns a **tuple**.
- Produces: `TestSpec.dom_parity: bool` — `true` when the fixture opts into the
  preview↔render DOM parity runner. The native runner ignores it; the field
  exists so the DSL stays a single grammar across all four runners.

- [x] **Step 1: Write the failing tests**

```rust
    #[test]
    fn test_parity_key_is_accepted_and_recorded() {
        let yaml: Value = serde_yaml::from_str(
            r#"
            _quarto:
              tests:
                html:
                  noErrors: true
                  dom-parity: true
            "#,
        )
        .unwrap();

        let (_run, specs) = parse_test_specs(&yaml, std::path::Path::new("test.qmd")).unwrap();
        let html = specs.iter().find(|s| s.format == "html").expect("html spec");
        assert!(html.dom_parity, "dom-parity: true must be recorded");
        // Only noErrors produced an assertion; parity is not one.
        assert_eq!(html.assertions.len(), 1);
    }

    #[test]
    fn test_parity_defaults_false_and_rejects_non_bool() {
        let yaml: Value = serde_yaml::from_str(
            r#"
            _quarto:
              tests:
                html:
                  noErrors: true
            "#,
        )
        .unwrap();
        let (_run, specs) = parse_test_specs(&yaml, std::path::Path::new("test.qmd")).unwrap();
        assert!(!specs[0].dom_parity);

        let bad: Value = serde_yaml::from_str(
            r#"
            _quarto:
              tests:
                html:
                  dom-parity: "yes"
            "#,
        )
        .unwrap();
        let err = format!(
            "{:#}",
            parse_test_specs(&bad, std::path::Path::new("test.qmd")).unwrap_err()
        );
        assert!(err.contains("dom-parity must be a boolean"), "got: {err}");
    }
```

- [x] **Step 2: Run to verify failure**

```bash
cargo nextest run -p quarto-test test_parity
```
Expected: compile error — no field `parity` on `TestSpec`.

- [x] **Step 3: Implement**

In `TestSpec` add:

```rust
    /// `dom-dom-parity: true` — opt this format into the preview ↔ render DOM
    /// parity runner (`hub-client/src/services/smokeAllParity.wasm.test.tsx`).
    /// The native runner ignores it; the field exists so all four
    /// smoke-all runners share one DSL grammar. Plan:
    /// claude-notes/plans/2026-08-24-preview-render-dom-parity-harness.md
    pub dom_parity: bool,
```

In `parse_format_spec`, add `let mut dom_parity = false;` beside
`check_warnings`, a match arm before `other =>`:

```rust
                "dom-parity" => {
                    dom_parity = assertion_value
                        .as_bool()
                        .context("dom-parity must be a boolean")?;
                }
```

and `dom_parity,` in the `Ok(TestSpec { … })` constructor at `:259`.

- [x] **Step 4: Run to verify pass**

```bash
cargo clippy -p quarto-test --all-targets -- -D warnings
cargo nextest run -p quarto-test
cargo nextest run -p quarto --test integration smoke_all
```
Expected: all green; the smoke_all sweep is unchanged (no fixture opts in yet).

- [x] **Step 5: Commit**

```bash
git add crates/quarto-test/src/spec.rs
git commit -m "Accept a parity key in the smoke-all test DSL (native runner ignores it)"
```

### Task 2.2: Both TS parsers ignore `dom-parity`; Phase-2 boundary

**Files:**
- Modify: `hub-client/src/services/smokeAll.wasm.test.ts` — `parseFormatSpec`,
  the filesystem no-op group (~L270-276 after Task 0.1's move) + `default:`
  which today throws `Unknown assertion type` (was `:277` before the move)
- Modify: `hub-client/e2e/helpers/smokeAllDiscovery.ts:234-240` (the
  `fileExists` no-op group + `default: throw`)

Both parsers hard-error on unknown keys, so a fixture with `dom-dom-parity: true`
would break the WASM sweep and the Playwright sweep until this lands.

- [x] **Step 1: Check for an existing unit test of either parser**

```bash
ls hub-client/e2e/helpers/*.test.ts 2>/dev/null; grep -rln "smokeAllDiscovery" hub-client/e2e hub-client/src | head
```

If a test file for `smokeAllDiscovery` exists, add there and watch it fail:

```ts
it('accepts dom-parity: true as a non-assertion', () => {
  const { formatSpecs } = parseTestSpecs({ _quarto: { tests: { html: { noErrors: true, dom-parity: true } } } });
  expect(formatSpecs[0].assertions.map((a) => a.type)).toEqual(['noErrors']);
});
```

If none exists, do not create one for a one-line case; Task 3.1's opt-in
plus `npx playwright test --config=playwright.smoke-all.config.ts e2e/smoke-all.spec.ts --list` is the check.

- [x] **Step 2: Implement — same three lines in both files**

```ts
        case 'dom-parity':
          // Opt-in flag for the preview <-> render DOM parity runner
          // (hub-client/src/services/smokeAllParity.wasm.test.tsx). Not an
          // assertion here.
          break;
```

- [x] **Step 3: Verify**

```bash
cd hub-client && npm run test:wasm      # counts identical to Task 0.1 Step 1
cd hub-client && npx playwright test --config=playwright.smoke-all.config.ts e2e/smoke-all.spec.ts --list | tail -3   # base config's testIgnore excludes this spec
```
Expected: WASM counts unchanged; Playwright lists the same number of tests
as on `main` (`--list` does not launch a browser or the web server).

- [x] **Step 4: Phase-2 boundary — workspace nextest**

```bash
cargo nextest run --workspace 2>&1 | tee /private/tmp/claude-502/-Users-gordon-src-q2/6f5c0c8f-a359-437a-87d6-879b0e289c0e/scratchpad/nextest-phase2.log; grep -E "Summary|passed|failed|skipped" /private/tmp/claude-502/-Users-gordon-src-q2/6f5c0c8f-a359-437a-87d6-879b0e289c0e/scratchpad/nextest-phase2.log | tail -3
```
Expected: green; delta vs the live baseline on `main` @ `cf9c45cc8` is
exactly +2 passed (Task 2.1's two tests). If no baseline log exists, run the
same command on `main` first and record it.

- [x] **Step 5: Commit**

```bash
git add hub-client/src/services/smokeAll.wasm.test.ts hub-client/e2e/helpers/smokeAllDiscovery.ts
git commit -m "Ignore the parity key in the WASM and Playwright smoke-all parsers"
```

---

## Phase 3 — Runner

### Task 3.1: `smokeAllParity.wasm.test.tsx` + first opt-in (one green commit)

**Files:**
- Create: `hub-client/src/services/smokeAllParity.wasm.test.tsx`
- Modify: the first opt-in candidate from the Task 0.2 note:
  `crates/quarto/tests/smoke-all/title-block/simple-default.qmd` (byte-identical
  under the rules even without the unwrap rule) — add `dom-dom-parity: true` under
  `_quarto.tests.html`. (`markdown/heading-auto-id.qmd`, the pre-spike guess,
  is blocked by c3 + c4 — see § Findings.)

The runner and its first fixture land together so no commit on the branch
is red.

**Interfaces:**
- Consumes: everything exported by `smokeAllFixtures.ts` (Task 0.1);
  `extractParityRoot`, `compareParity`, `ParityRuleViolation` from
  `@quarto/preview-renderer/test-utils/domParity` — resolved by the
  `@quarto/preview-renderer` → `../ts-packages/preview-renderer/src` alias
  in `vitest.wasm.config.ts` (prefix match, the same mechanism the sibling
  uses for `@quarto/preview-runtime/userGrammar/Discovery`). **vitest-only**:
  this path is not in the package exports map and `tsc` never sees this
  file (§ Global Constraints). `Ast` from
  `@quarto/preview-renderer/framework` (it is **not** re-exported from the
  package root — `src/index.ts` re-exports `./q2-preview` only; the sibling
  test imports it from `../framework`, `q2-preview.integration.test.tsx:35`);
  `previewRegistry` from `@quarto/preview-renderer`.

- [x] **Step 1: Write the runner with its two tests (the second is the self-check)**

```tsx
// @vitest-environment jsdom
/**
 * Preview <-> render DOM parity runner (fourth smoke-all runner).
 *
 * For every smoke-all fixture whose `_quarto.tests.html` carries
 * `dom-dom-parity: true`, render it twice through the same WASM module:
 *   - `render_page_in_project`   -> native HTML writer -> full page HTML
 *   - `render_page_for_preview`  -> the same Rust function with
 *                                   `prefer_preview_format: true`: pipeline
 *                                   stopped before `render-html-body`
 *                                   -> Pandoc AST JSON
 * Mount the AST read-only with the real q2-preview React registry under
 * jsdom, and require the canonical form of `main#quarto-document-content`
 * to be identical on both sides. Normalisation rules and their reasons
 * live in `ts-packages/preview-renderer/src/test-utils/domParity.ts`.
 *
 * Opt-in is curated: an opted-in fixture that diverges FAILS. There is
 * no expected-failure list — fix the divergence or remove the opt-in.
 *
 * Plan: claude-notes/plans/2026-08-24-preview-render-dom-parity-harness.md
 * Manual predecessor: .claude/skills/preview-render-parity/SKILL.md
 */
import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile, mkdir, writeFile } from 'fs/promises';
import { join, relative, resolve, dirname } from 'path';
import { fileURLToPath } from 'url';
import { JSDOM } from 'jsdom';
import { render, cleanup } from '@testing-library/react';
import { Ast } from '@quarto/preview-renderer/framework';
import { previewRegistry } from '@quarto/preview-renderer';
import {
  extractParityRoot,
  compareParity,
  ParityRuleViolation,
} from '@quarto/preview-renderer/test-utils/domParity';
import {
  SMOKE_ALL_DIR,
  loadSmokeWasm,
  discoverTestFiles,
  readFrontmatter,
  readTestsBlock,
  shouldSkip,
  populateVfs,
  buildUserGrammarsHandle,
  type WasmModule,
} from '../test-utils/smokeAllFixtures';

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT_DIR = resolve(__dirname, '../../test-results/parity');

let wasm: WasmModule;

beforeAll(async () => {
  wasm = await loadSmokeWasm();
});

beforeEach(() => {
  wasm.vfs_clear();
  cleanup();
});

interface Sides {
  renderMain: Element;
  previewMain: Element;
}

/** Render one fixture both ways and return the two parity roots. */
async function renderBothSides(testFile: string): Promise<Sides> {
  const { vfsPath, projectFiles } = await populateVfs(wasm, testFile);
  const grammars = await buildUserGrammarsHandle(wasm, projectFiles);

  const renderRes = JSON.parse(await wasm.render_page_in_project(vfsPath, grammars)) as {
    success: boolean; html?: string; error?: string;
  };
  if (!renderRes.success || !renderRes.html) {
    throw new Error(`render_page_in_project failed: ${renderRes.error ?? 'no html'}`);
  }

  const previewRes = JSON.parse(
    await wasm.render_page_for_preview(vfsPath, grammars, undefined),
  ) as { success: boolean; ast_json?: string; error?: string };
  if (!previewRes.success || !previewRes.ast_json) {
    throw new Error(`render_page_for_preview failed: ${previewRes.error ?? 'no ast_json'}`);
  }

  const renderDoc = new JSDOM(renderRes.html).window.document;
  // Read-only mount: no PreviewContext / AssetManifestContext /
  // IncrementalContext on purpose (plan § Global Constraints).
  const { container } = render(
    <Ast
      astJson={previewRes.ast_json}
      currentFilePath={vfsPath}
      onNavigateToDocument={() => {}}
      setAst={() => {}}
      registry={previewRegistry}
    />,
  );
  return {
    renderMain: extractParityRoot(renderDoc, 'render'),
    previewMain: extractParityRoot(container, 'preview'),
  };
}

async function writeArtifacts(relPath: string, renderText: string, previewText: string) {
  const dir = join(OUT_DIR, relPath.replace(/[\\/]/g, '__'));
  await mkdir(dir, { recursive: true });
  await writeFile(join(dir, 'render.norm.txt'), renderText);
  await writeFile(join(dir, 'preview.norm.txt'), previewText);
  return dir;
}

/** Produce vitest's own diff text for two strings without throwing. */
function diffText(expected: string, actual: string): string {
  try {
    expect(actual).toBe(expected);
    return '';
  } catch (e) {
    return (e as Error).message;
  }
}

async function optedInFixtures(): Promise<string[]> {
  const files = await discoverTestFiles(SMOKE_ALL_DIR);
  const out: string[] = [];
  for (const f of files) {
    const block = readTestsBlock(readFrontmatter(await readFile(f, 'utf-8')));
    if (!block || shouldSkip(block.run)) continue;
    if (block.formats['html']?.['dom-parity'] === true) out.push(f);
  }
  return out;
}

describe('smoke-all preview <-> render DOM parity', () => {
  // Same single-`it` shape as smokeAll.wasm.test.ts: vitest collects tests
  // synchronously, and discovery is async.
  it('every opted-in fixture has identical <main> DOM on both sides', async () => {
    const fixtures = await optedInFixtures();
    const smokeFilter = process.env.SMOKE_FILTER || '';
    const failures: string[] = [];
    let compared = 0;

    for (const testFile of fixtures) {
      const relPath = relative(SMOKE_ALL_DIR, testFile);
      if (smokeFilter && !relPath.includes(smokeFilter)) continue;
      wasm.vfs_clear();
      cleanup();
      const started = performance.now();
      try {
        const { renderMain, previewMain } = await renderBothSides(testFile);
        const result = compareParity(renderMain, previewMain);
        compared++;
        if (!result.equal) {
          const dir = await writeArtifacts(relPath, result.render, result.preview);
          failures.push(
            `${relPath} [html]: parity mismatch (artifacts: ${dir})\n${diffText(result.render, result.preview)}`,
          );
        }
      } catch (e) {
        const kind = e instanceof ParityRuleViolation ? 'rule violation' : 'error';
        failures.push(`${relPath} [html]: ${kind}: ${(e as Error).message}`);
      }
      console.log(`  parity ${relPath}: ${Math.round(performance.now() - started)} ms`);
    }

    console.log(`\nParity results: ${compared} compared, ${failures.length} failed, ${fixtures.length} opted in`);
    expect(fixtures.length, 'at least one fixture must opt in (dom-parity: true)').toBeGreaterThan(0);
    expect(failures, `${failures.length} parity failure(s):\n${failures.join('\n\n')}`).toHaveLength(0);
  });

  it('reports an injected divergence (harness self-check)', async () => {
    const [first] = await optedInFixtures();
    expect(first, 'needs one opted-in fixture').toBeDefined();
    const { renderMain, previewMain } = await renderBothSides(first);
    expect(compareParity(renderMain, previewMain).equal).toBe(true);

    // Inject: add a class to the first element on the preview side.
    const victim = previewMain.querySelector('p, h1, h2, pre, div, section');
    expect(victim).not.toBeNull();
    victim!.classList.add('injected-divergence');

    const result = compareParity(renderMain, previewMain);
    expect(result.equal).toBe(false);
    expect(result.preview).toContain('injected-divergence');
    expect(result.render).not.toContain('injected-divergence');
  });
});
```

- [x] **Step 2: Run before opting anything in — verify the right failure**

```bash
cd hub-client && npm run test:wasm
```
Expected: the sibling runner still green; the new file FAILS on
`at least one fixture must opt in` and `needs one opted-in fixture`. Any
*other* failure (module resolution, JSX transform, WASM under jsdom,
`render` complaining) is a harness bug — fix it here. (No setup file is
loaded for this config; Task 0.2's Q3 established none is needed. If that
turns out wrong, add `setupFiles` to `vitest.wasm.config.ts` pointing at a
new `src/test-utils/parity-setup.ts` with only the shim that is missing.)

- [x] **Step 3: Opt in the first fixture**

```yaml
_quarto:
  tests:
    html:
      noErrors: true
      dom-parity: true
```

```bash
cd hub-client && SMOKE_FILTER=simple-default npm run test:wasm
```
Expected: `Parity results: 1 compared, 0 failed, 1 opted in`; self-check passes.
(vitest 4 hides `console.log` from passing tests by default — append
`-- --reporter=verbose` to see the parity summary on a green run.)

If it fails with a *mismatch*, the spike note was wrong or a rule is
missing: read the artifacts under `hub-client/test-results/parity/`, classify
(a)/(b)/(c) as in Task 0.2 Step 4. (b) → add the rule with a reason in
`domParity.ts` + a unit test (a Task-1.2-shaped mini-step). (c) → pick the
next candidate from the note; record the divergence for Task 4.1.

- [x] **Step 4: Commit (green)**

```bash
git add hub-client/src/services/smokeAllParity.wasm.test.tsx crates/quarto/tests/smoke-all/title-block/simple-default.qmd
git commit -m "Add preview/render DOM parity runner over opted-in smoke-all fixtures"
```

### Task 3.2: Opt in the remaining green fixtures

**Files:**
- Modify: each remaining opt-in candidate from the Task 0.2 note — only
  `highlighting/01-builtin-python.qmd` (green under the amended rules).
  `appendix/footnotes-heading.qmd` (c1, c2) and `markdown/heading-auto-id.qmd`
  (c3, c4) stay out until their strands close — see § Findings.

- [x] **Step 1: Opt in one at a time, same loop as Task 3.1 Step 3**

- [x] **Step 2: All four runners green**

```bash
cargo nextest run -p quarto --test integration smoke_all           # Rust: parity key accepted
cd hub-client && npm run test:wasm                                 # WASM sibling + parity
cd hub-client && npx playwright test --config=playwright.smoke-all.config.ts e2e/smoke-all.spec.ts --list  # discovery parses the key
```
Expected: Rust sweep unchanged; WASM sibling counts unchanged from Task 0.1
Step 1; parity `N compared, 0 failed, N opted in`; Playwright lists the same
tests as before.

- [x] **Step 3: Commit**

```bash
git add crates/quarto/tests/smoke-all
git commit -m "Opt further smoke-all fixtures into preview/render DOM parity"
```

---

## Phase 4 — Triage, docs, gate

### Task 4.1: File strands for real divergences

**Files:** none (braid). bd-tmb2u5yu (`Math.tsx` classes) is already filed.

For each further (c)-class divergence recorded in the Task 0.2 note or found
in Phase 3, create one strand. The spike found three new ones (c1–c3 in
`claude-notes/research/2026-08-24-preview-render-parity-spike.md`; c4 is
bd-tmb2u5yu): c1 `inlines/Link.tsx` drops kv attributes outside a
`data-*`/`rel`/`target` allowlist (loses `role="doc-noteref"`/`doc-backlink`);
c2 `blocks/OrderedList.tsx` emits no `type="1"` for `Decimal` (the writer and
Pandoc do); c3 `inlines/Strikeout.tsx` renders `<s>` where the writer (and
Pandoc) render `<del>`. These are out-of-plan bugs (they are
*findings* of this plan, not work items of it):

```bash
braid create "preview parity: <one-line symptom>" -t bug -p 2 -l preview-parity \
  -d "$(cat <<EOF
Found by the preview<->render DOM parity harness
(hub-client/src/services/smokeAllParity.wasm.test.tsx) on fixture
crates/quarto/tests/smoke-all/<path>.qmd.

render:  <canonical snippet>
preview: <canonical snippet>

Native writer: crates/pampa/src/writers/html.rs:<line>
React: ts-packages/preview-renderer/src/q2-preview/<component>.tsx

Fix should end with \`dom-parity: true\` on the fixture.
EOF
)" --json
```

- [x] Create one strand per divergence; list their ids in this plan under a
  new § Findings section and in the research note.

### Task 4.2: Documentation

**Files:**
- Modify: `claude-notes/instructions/testing.md:116-140`:
  - line 116 "three independent runners" → "four";
  - line 120's stale `cargo nextest run -p quarto --test smoke_all` →
    `cargo nextest run -p quarto --test integration smoke_all`;
  - add `### 4. Preview ↔ render DOM parity (WASM + jsdom)` after runner 3:
    command (`cd hub-client && npm run test:wasm`, `SMOKE_FILTER=` works),
    what it compares (`main#quarto-document-content`, read-only mount),
    how to opt in (`dom-dom-parity: true` under `html:`), the no-xfail policy,
    where artifacts land (`hub-client/test-results/parity/`), and that
    fixtures with math wait on bd-tmb2u5yu;
  - add `dom-parity` to the DSL reference (search `ensureCssRegexMatches` for the list).
- Modify: `.claude/skills/preview-render-parity/SKILL.md`:
  - in "Diagnosis workflow" insert a step 0: *reproduce with the harness
    first* — write a minimal fixture under `smoke-all/q2-preview/` (or opt in
    an existing one), run `SMOKE_FILTER=<name> npm run test:wasm`, read
    `test-results/parity/…`; go to Chrome only for computed-style symptoms;
  - in "TDD workflow", the regression test for a parity fix is the fixture's
    `dom-dom-parity: true` opt-in when the fixture can be made minimal;
  - replace the integration-branch guidance (lines ~140, 224, 228:
    `feature/q2-preview-command`, parent epic bd-kw93): bd-kw93 is closed and
    the branch merged via PR #214 (`git log --grep "q2 preview command"` on
    `main`); parity strands branch off `main` and carry the `preview-parity`
    label instead of a parent-child dep.

- [x] Make both edits; commit:

```bash
git add claude-notes/instructions/testing.md .claude/skills/preview-render-parity/SKILL.md
git commit -m "Document the preview/render DOM parity runner and harness-first workflow"
```

### Task 4.3: Gate and reconcile

- [x] `cargo clippy --workspace --all-targets -- -D warnings` (clean at 1ce54e55e)
- [x] `cargo xtask verify` (full — hub-client and preview-renderer changed;
  this reruns the workspace nextest, so report its pass/skip counts against
  the Phase-2 baseline: the delta must still be exactly +2).
  **Result (58a8d22e6):** all steps passed; workspace nextest 13217 passed /
  199 skipped vs baseline 13215 / 199 on `main` @ cf9c45cc8 (= +2, Task 2.1's
  tests). Final whole-branch review found two one-line Importants
  (`INLINE_TAGS` missing `label`/`input`/`button`/`svg`; stale Playwright
  command in testing.md), fixed in 58a8d22e6; `domParity.test.ts` is 19 tests.
- [x] Re-read this checklist; verify every `[x]` against the commits; fix
  stale marks; commit the plan file.
- [x] Report; **do not push** without explicit approval (CLAUDE.md).

---

## Findings

Real divergences the harness (spike + runner) surfaced. Out-of-plan bugs,
one strand each (label `preview-parity`); filed by Task 4.1.
Details and verbatim snippets: `claude-notes/research/2026-08-24-preview-render-parity-spike.md`.

| # | Symptom | Fixture | Strand |
|---|---|---|---|
| c1 | `Link.tsx` drops `role=` (and every kv attr outside `data-*`/`rel`/`target`) | `appendix/footnotes-heading.qmd` | bd-294mbrcx |
| c2 | `OrderedList.tsx` omits `type="1"` for `Decimal` | `appendix/footnotes-heading.qmd` | bd-q88zinyv |
| c3 | `Strikeout.tsx` renders `<s>`, writer `<del>` | `markdown/heading-auto-id.qmd` | bd-qzwlhrlv |
| c4 | `Math.tsx` emits no `math inline|display` class | `markdown/heading-auto-id.qmd` | bd-tmb2u5yu |
| s1 | Crossref floats not rendered as `div.quarto-float > figure.quarto-float-*` (figures and tables) | `includes/crossref/crossref.qmd`, `localization/lang-es-crossref.qmd`, `localization/language-inline-override.qmd` | bd-d96axq4a |
| s2 | Localized UI strings not applied (callout title, theorem "Proof") | `localization/lang-es-callout.qmd`, `localization/lang-es-theorem.qmd` | bd-hamxar01 |
| s3 | Callout drops `title=` / `data-appearance=` | `quarto-test/callout-title-attribute.qmd`, `quarto-test/callouts-matrix.qmd` | bd-p2cd2ssg |
| s4 | Callout body heading sectionized on render, not on preview | `toc-containers/callout-body-heading-not-in-toc.qmd` | bd-bg0jze2i |
| s5 | Inline `<code>` forwards `data-hl-spans` (forbid rule trips) | `highlighting/02-inline-code.qmd` | bd-bda2mbnl |
| s6 | Included list item wrapped in `<p>` on preview | `includes/nested/nested.qmd` | bd-nrywksil |

All of the above are children of the epic **bd-j3764r9a** "React <-> HTML DOM
parity (q2 preview vs q2 render)", alongside **bd-xa4vv9tt** (this branch's
harness work; close on merge), **bd-00iveh46** (`data-filename` dropped from
`<pre>`, `includes/in-code-fence`), and four earlier-filed strands that are
specifically about preview/render parity and were promoted to children:
bd-e3m3rkik (mermaid chrome), bd-47afd5ro (tabsets), bd-2yd37vuk
(`#quarto-header`), bd-tqijrhsu (toc-location). bd-1tl09 (native code-block
decorations epic) is `related` only — it does not cover the React mirror.

## Addendum (2026-08-24, after the plan closed)

Full write-up: **`claude-notes/research/2026-08-24-preview-render-parity-survey.md`**
(survey method and results, rename, bulk opt-in, strand/epic map, harness
limitations). Summary:

- [x] **Rename the DSL key `parity` → `dom-parity`** everywhere it is a key,
  field (`TestSpec.dom_parity`), case label, assertion message or doc
  reference (commit `d750318b7`). File names and the `PARITY_RULES` /
  `compareParity` / `smokeAllParity` identifiers keep their names.
- [x] **Whole-corpus survey + bulk opt-in.** A throwaway sweep of all 164
  fixtures (`hub-client/test-results/parity-survey/`, gitignored, not
  committed) found **106 byte-identical, 18 mismatching, 29 unreachable
  (their `_quarto.tests` key is `q2-preview` or an extension format, not
  `html`), 5 other** (2 intentional — `shouldError`, `run.skip`; 1
  survey-environment artefact; 1 harness limitation — `theme: none` has no
  `<main>`; 1 real bug — inline `Code.tsx` forwards `data-hl-spans`). The
  106 minus the engine fixture `includes/code-cell/code-cell.qmd` were opted
  in (commit `04bd3c2cf`): **`Parity results: 105 compared, 0 failed, 105
  opted in`**, ~16.5 s of the sweep's 120 s hang-detection budget. 15 of
  those fixtures have a `format: html:` block ahead of `_quarto: tests:
  html:`; the opt-in script had to scope to the `_quarto → tests → html`
  chain (the first attempt mis-inserted under `format:` and showed up as
  `90 compared`).

The mismatch → strand map and the harness limitations the survey exposed
(`dom-parity` only read under `html:`; minimal-template docs have no `<main>`;
`#main` excludes the page frame) are in the research note above — recorded
there as notes, deliberately not as strands.

## Follow-ups (not in this plan)

- bd-tmb2u5yu: fix `Math.tsx` classes, then opt in a math fixture.
- bd-qn8yi1su: revealjs deck parity — extend `domParity.ts` with a
  `.reveal .slides` root selector and a `render_page_in_project` vs
  `RevealDeck` mount.
- Widen the allowlist as parity strands close: each closing PR should end
  with `dom-dom-parity: true` on the fixture that reproduced it.
- Reconsider the mount configuration once edit chrome stabilises: comparing
  a `PreviewRoot` mount would need strip rules for `data-block-pool-id`,
  `tabIndex`, comment anchors, and asset blob URLs.
- A lint (`cargo xtask lint`) that a fixture under `smoke-all/q2-preview/`
  without `dom-dom-parity: true` must cite a reason — only once the allowlist is
  large enough that omissions are the exception.
