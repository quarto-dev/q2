/**
 * SC21 / SC23 / SC24 — marimo through `q2 preview`: the POSITIVE splice e2e
 * suite (Phase B of `claude-notes/plans/2026-07-07-plan4c2-marimo-preview-e2e.md`,
 * strand bd-5jxcio5d). The marimo sibling of PC5 (echo) / PC6 (julia).
 *
 * bd-5jxcio5d FIXED (commit 4cfc3b1ae): `derive_cell_outputs`
 * (`crates/quarto-core/src/engine/capture_splice.rs`) now pairs an `A1` engine
 * cell to its lockstep `B1` block even when that block is not a `::: {.cell}`
 * Div wrapper — `is_engine_output_block(block) = is_cell_wrapper(block) ||
 * matches!(block, Block::RawBlock(_))`. Marimo's executed cells are bare
 * `{=html}` RawBlock islands (`<marimo-island>`/`<marimo-cell-output>`), zero
 * `.cell` Divs, so before the fix `is_cell_wrapper` was false for every B1
 * block and the splice fell through to raw source (pinned by this file's
 * former SC21-NEG canary). SC22 (`capture_splice_seam.rs`,
 * `marimo_shaped_capture_splices`) binds the fix at the native/AST tier
 * unconditionally; **this file is the real-engine, real-browser proof that
 * SC22's synthetic `B1` was faithful** — the executed island actually reaches
 * the live preview pane, via a real `uv`-spawned marimo subprocess and a real
 * `q2 preview` server + chromium.
 *
 * THREE SEAMS in this file:
 *   - **SC21** (the flip) — minimal `40 + 2` doc: pane shows the executed
 *     island (`marimo-cell-output` + `42`) WITHOUT a reload.
 *   - **SC23** — widget MARKUP delivery: `mo.ui.slider(...)` doc → the
 *     widget's raw `{=html}` island markup (`<marimo-island>` +
 *     `<marimo-ui-element>`/`<marimo-slider>`) reaches the pane. Asserts
 *     DELIVERY only, not interactivity — see the hydration note below.
 *   - **SC24** — sql-interop delivery: `{python .marimo}` + bare `{sql}` doc
 *     (with the pyproject deps block) → the executed sql island
 *     (`marimo-table` carrying the computed value) reaches the pane.
 *
 * WIDGET HYDRATION IS OUT OF SCOPE (open decision, left for Gordon). Marimo
 * widgets hydrate client-side via an EXTERNAL
 * `cdn.jsdelivr.net/npm/@marimo-team/islands@.../main.js` script. Static
 * executed output (SC21's `42`, SC24's sql table) is baked into the
 * server-rendered island markup and shows without JS — robust regardless of
 * that script's fate. An interactive widget (SC23) additionally needs that
 * external script to load and run INSIDE the preview pane's sandboxed
 * iframe, which may be CSP/external-host-blocked — a separate question from
 * the splice this file proves. SC23 therefore asserts markup delivery only;
 * do not read it as proof that interactive widgets work in `q2 preview`.
 *
 * ISOLATION RATIONALE (frozen row, carried over from the former SC21-NEG):
 * unlike julia (PC6), marimo has NO shared daemon and NO shared transport
 * file — each render spawns a self-contained `uv`/marimo subprocess. So
 * there is no `isolateJuliaProject`/HOME-override here: a plain temp project
 * copy (fresh per test, via `stageMarimoProject`) fully isolates state.
 *
 * OPT-IN (user directive 2026-07-03, mirroring PC6's `QUARTO_PC6_LIVE`). Every
 * test in this file is gated behind `QUARTO_SC21_LIVE=1` (skips by default,
 * so the normal `npm run test:e2e` suite never runs it) because it spawns a
 * REAL marimo subprocess (`uv run --with marimo==0.23.13 ...`). Run on demand:
 *   QUARTO_SC21_LIVE=1 npx playwright test engine-capture-splice-marimo --project=chromium
 *
 * PREVIEW-BINARY FRESHNESS (frozen precondition): the pane render runs in the
 * embedded SPA/WASM baked into `target/debug/q2` via `include_dir!` (see
 * claude-notes/instructions/preview-spa-rebuild.md — the three-cache trap:
 * WASM → q2-preview-spa/dist → q2 binary). This file was run AFTER the fix
 * commit (4cfc3b1ae) with a binary already rebuilt fresh at that commit — no
 * rebuild was needed for Phase B (verified via `ls -la target/debug/q2` mtime
 * vs. the fix commit's timestamp).
 *
 * NAMED REVERT (SC21). The same fix hunk SC22 binds
 * (`is_engine_output_block` in `capture_splice.rs`): reverting it means the
 * marimo island never reaches the pane again → `marimo-cell-output`/`42`
 * absent from the pane body RED. `applyMarimoLegRevert` below is a SEPARATE,
 * secondary revert on the RECORDING half of the chain (kept from the former
 * SC21-NEG canary): it prevents the capture from ever being recorded
 * server-side (assertion (a) below), which also reddens this test but for a
 * different reason (no capture to splice, rather than a capture that fails to
 * splice). Both are legitimate RED paths; the primary named revert for the
 * bd-5jxcio5d fix itself is the `capture_splice.rs` hunk.
 */

import { test, expect, type Page } from '@playwright/test';
import { cp, mkdtemp, writeFile, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

/** Repo root, relative to this file (`q2-preview-spa/e2e/`). */
const REPO_ROOT = path.resolve(import.meta.dirname, '..', '..');
/** The committed marimo-engine fixture root (a project dir; engine under `_extensions/`). */
const MARIMO_FIXTURE_ROOT = path.join(
  REPO_ROOT,
  'crates',
  'quarto-core',
  'tests',
  'fixtures',
  'extensions',
  'marimo',
);

/**
 * Minimal marimo doc: one `{python .marimo}` cell whose value is 42. The
 * distinctive literal `40 + 2` (asserted absent from the executed pane body)
 * and its distinctive result `42` (what the splice surfaces) keep the
 * assertions non-ambient. No `engine:` front-matter — ownership flows through
 * the static `whenClass: marimo` claim on `python`, exactly as the render-tier
 * SC8 fixture (`minimal.qmd`) does.
 */
const INDEX_QMD =
  '---\ntitle: "SC21 marimo capture"\n---\n\n# SC21 heading\n\n```{python .marimo}\nimport marimo as mo\n40 + 2\n```\n';

/**
 * SC23 widget doc — same shape as the render-tier `WIDGET_DOC`
 * (`crates/quarto-core/tests/integration/marimo_engine_e2e.rs`): a single
 * `mo.ui.slider` cell. Its raw `{=html}` island output is
 * `<marimo-island>...<marimo-ui-element>...<marimo-slider .../>...</marimo-ui-element>...</marimo-island>`.
 */
const WIDGET_QMD =
  '---\ntitle: "SC23 marimo widget"\n---\n\n```{python .marimo}\nimport marimo as mo\nmo.ui.slider(1, 10, value=5)\n```\n';

/**
 * SC24 sql-interop doc — same shape as the render-tier
 * `SQL_INTEROP_STATIC_DOC` (`marimo_engine_e2e.rs`): a `{python .marimo}`
 * primary cell plus a bare `{sql}` cell, with the `pyproject:` deps block so
 * marimo's `uv`-managed subprocess resolves `duckdb`/`sqlglot`/`polars`/
 * `pyarrow` (the STATIC claims path — the committed fixture's `claims:` map,
 * unmodified, same configuration SC21/SC23 use).
 */
const SQL_INTEROP_QMD =
  '---\ntitle: "SC24 marimo sql interop"\npyproject: |\n    dependencies = ["duckdb", "sqlglot", "polars", "pyarrow"]\n---\n\n```{python .marimo}\nimport marimo as mo\n1 + 1\n```\n\n```{sql}\nSELECT 1 + 1 AS x\n```\n';

function onPath(bin: string): boolean {
  try {
    execFileSync(bin, ['--version'], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

/**
 * Marimo-leg named revert, applied to a TEMP-COPY of the fixture only —
 * never the committed fixture. Guarded by the scratch `QUARTO_SC21_REVERT`
 * knob so it fires only when re-verifying the RECORDING half of the chain
 * (see the file header — this is a SECONDARY revert, not the fix's primary
 * named revert, which is the `capture_splice.rs` hunk).
 *
 * This is SC8's ratified TWO-part revert, applied verbatim: remove ONLY the
 * `python:` claim ENTRY from the `_extension.yml` `claims:` map (keeping the
 * `claims:` key itself and the `"python.marimo"`/`sql`/`"sql.marimo"` entries)
 * AND add `claims-files: []`. With the static `claims:` map still PRESENT,
 * `ts_engine`'s static short-circuit answers `claims_language` from the map
 * alone — the now-missing `python` key resolves to a static None and the
 * cell is NOT claimed; the dynamic wire call to the engine's `claimsLanguage`
 * fires ONLY when `self.claims` is `None` (i.e. no static map at all), so it is
 * never consulted here. `claims-files: []` likewise disables the whole-file
 * dynamic `claimsFile` short-circuit (SC8 BLOCKING FINDING #3). Net: no engine
 * owns `{python .marimo}` → it falls through to jupyter (unavailable here) →
 * render fails → NO marimo capture → assertion (a) reddens.
 *
 * IMPLEMENTER WARNING: do NOT drop the WHOLE `claims:` map (that is 4cB2's
 * claims-LESS variant, not SC8's revert). With the map absent, `self.claims`
 * is `None`, so the dynamic `claimsLanguage` wire call FIRES and re-claims
 * `{python .marimo}` (`language==="python" && firstClass==="marimo" → 2`) — the
 * revert then does NOT redden (the render still emits `marimo-cell-output` +
 * `42`). The entry-only form and the whole-map form engage different resolution
 * paths (static-map-lookup vs dynamic wire); only the entry-only form is SC8's.
 */
async function applyMarimoLegRevert(projSrc: string): Promise<void> {
  const ymlPath = path.join(projSrc, '_extensions', 'marimo', '_extension.yml');
  const lines = (await readFile(ymlPath, 'utf8')).split('\n');
  const idx = lines.findIndex((l) => l.trim() === 'python:');
  if (idx < 0) throw new Error('temp _extension.yml has no `python:` claim entry to remove');
  // Remove the `python:` key line and its deeper-indented child list item(s),
  // stopping at the next sibling key (`"python.marimo":`, same indent).
  const indent = lines[idx].length - lines[idx].trimStart().length;
  let end = idx + 1;
  while (end < lines.length) {
    const l = lines[end];
    const li = l.length - l.trimStart().length;
    if (l.trim() !== '' && li <= indent) break;
    end++;
  }
  lines.splice(idx, end - idx);
  let out = lines.join('\n');
  if (!out.endsWith('\n')) out += '\n';
  out += '      claims-files: []\n'; // engine-entry-level sibling of `claims:`
  await writeFile(ymlPath, out);
}

/**
 * Assemble a temp project dir with the committed marimo extension
 * (`_extensions/marimo/` + `_quarto.yml`) and the given `index.qmd` content,
 * then return the source dir for `startPreviewServer({ copyFromDir })`.
 * (previewServer copies again into its own fresh tempdir, so this dir is a
 * private staging area — never the committed fixture.) Defaults to the SC21
 * minimal doc; SC23/SC24 pass their own doc content so each test stages an
 * independent project.
 */
async function stageMarimoProject(docContent: string = INDEX_QMD): Promise<string> {
  const projSrc = await mkdtemp(path.join(tmpdir(), 'q2-marimo-preview-src-'));
  await cp(
    path.join(MARIMO_FIXTURE_ROOT, '_extensions', 'marimo'),
    path.join(projSrc, '_extensions', 'marimo'),
    { recursive: true },
  );
  await cp(path.join(MARIMO_FIXTURE_ROOT, '_quarto.yml'), path.join(projSrc, '_quarto.yml'));
  await writeFile(path.join(projSrc, 'index.qmd'), docContent);
  if (process.env.QUARTO_SC21_REVERT === '1') await applyMarimoLegRevert(projSrc);
  return projSrc;
}

/** Read the sandboxed renderer iframe's `<body>` innerHTML (markup + text). */
async function paneHtml(page: Page): Promise<string | null> {
  return page.evaluate(() => {
    const outer = document.querySelector('iframe');
    return outer?.contentDocument?.body?.innerHTML ?? null;
  });
}

/**
 * Strip ANSI SGR escapes. `q2 preview`'s tracing layer emits colour codes even
 * to a pipe, so a field like `engines=marimo` reaches us as
 * `engines\x1b[0m\x1b[2m=\x1b[0mmarimo` — the literal substring is absent until
 * the codes are removed. Match against the stripped form.
 */
// eslint-disable-next-line no-control-regex
const stripAnsi = (s: string): string => s.replace(/\x1b\[[0-9;]*m/g, '');

/** Poll the (ANSI-stripped) server log until it contains every `needle`, or time out. */
async function waitForServerLog(
  srv: PreviewServerHandle,
  needles: string[],
  timeoutMs: number,
): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const log = stripAnsi(srv.serverLog());
    if (needles.every((n) => log.includes(n))) return true;
    if (Date.now() > deadline) return false;
    await new Promise((r) => setTimeout(r, 200));
  }
}

let server: PreviewServerHandle | undefined;
const consoleLines: string[] = [];

test.afterEach(async () => {
  await server?.stop();
  server = undefined;
});

test('SC21: recorded marimo capture splices the executed island into the pane without reload', async ({
  page,
}) => {
  // The preview engine-host drives marimo via `uv`; a warm-cache capture is
  // ~2s but a cold pubgrub resolution can be much slower. Raise the test-level
  // timeout so the bounded waits below (90s capture + 30s pane + settle) are
  // the real bound and a cold resolution still passes.
  test.setTimeout(150_000);

  test.skip(
    process.env.QUARTO_SC21_LIVE !== '1',
    'SC21 is opt-in (set QUARTO_SC21_LIVE=1) — spawns a real uv/marimo subprocess; see file header',
  );
  test.skip(!onPath('deno') || !onPath('uv'), 'deno/uv not on PATH — marimo engine unavailable');

  consoleLines.length = 0;
  page.on('console', (msg) => consoleLines.push(`[${msg.type()}] ${msg.text()}`));
  page.on('pageerror', (err) => consoleLines.push(`[pageerror] ${err.message}`));

  const projSrc = await stageMarimoProject();
  // Raise the capture-driver target to INFO so the `recorded engine capture(s)`
  // line (assertion a) is visible; keep everything else at `warn`.
  server = await startPreviewServer({
    copyFromDir: projSrc,
    extraEnv: { RUST_LOG: 'warn,quarto_preview::capture_driver=info' },
  });

  // Assertion (a) — PRECONDITION: the marimo engine ran and its capture was
  // RECORDED server-side. Bounded wait on the server log — this also
  // guarantees the capture EXISTS before we connect the browser, so the
  // first render attempts the splice with the capture already present
  // (anti-vacuity: a fixed-world splice isn't just "eventually correct after
  // some later edit").
  const captureRecorded = await waitForServerLog(
    server,
    ['recorded engine capture(s)', 'engines=marimo'],
    90_000,
  );
  expect(
    captureRecorded,
    "server must record a marimo capture ('recorded engine capture(s)' with " +
      `engines=marimo) — proves the marimo engine ran; got no such line within 90s.\n` +
      `server log tail:\n${stripAnsi(server.serverLog()).slice(-2000)}`,
  ).toBe(true);

  // Connect the SPA only AFTER the capture is recorded, so the first render
  // attempts the splice with the capture already present.
  await page.goto(server.url);

  // Wait for the executed island to appear, then settle so the splice has
  // fully applied.
  await page
    .waitForFunction(
      () => {
        const outer = document.querySelector('iframe');
        const html = outer?.contentDocument?.body?.innerHTML ?? '';
        return html.includes('marimo-cell-output') && html.includes('42');
      },
      null,
      { timeout: 30_000 },
    )
    .catch(() => {
      /* fall through to the assertions so we can attach the pane state */
    });
  await page.waitForTimeout(2_000);

  const html = await paneHtml(page);
  const diag = `\n\npane HTML was:\n${html}\n\nconsole:\n${consoleLines.join('\n')}`;

  // Assertion (b) — POSITIVE SPLICE: the executed marimo island reached the
  // pane WITHOUT a reload.
  expect(
    html?.includes('marimo-cell-output') ?? false,
    `pane must contain "marimo-cell-output" — the marimo island must splice ` +
      `into the preview pane (bd-5jxcio5d fixed);${diag}`,
  ).toBe(true);
  expect(
    html?.includes('42') ?? false,
    `pane must contain the executed value "42" — the marimo cell's evaluated ` +
      `output must reach the pane;${diag}`,
  ).toBe(true);
  // The literal, unexecuted source must be ABSENT from the pane BODY. Scoped
  // to the body (not the head) because `q2 render` of this doc carries the
  // literal, spaced `40 + 2` inside a `notebookCode:` JS-string in the head's
  // `__MARIMO_EXPORT_CONTEXT__` include-in-header script — `paneHtml()` above
  // already reads only the iframe `<body>` innerHTML (never `<head>`), so a
  // plain substring check here is correctly scoped.
  expect(
    html?.includes('40 + 2') ?? true,
    `pane BODY must NOT contain the literal unexecuted source "40 + 2" — the ` +
      `splice must have replaced the source cell with the executed island;${diag}`,
  ).toBe(false);
});

test('SC23: recorded marimo widget capture delivers island markup to the pane (markup only, not hydration)', async ({
  page,
}) => {
  test.setTimeout(150_000);

  test.skip(
    process.env.QUARTO_SC21_LIVE !== '1',
    'SC23 is opt-in (set QUARTO_SC21_LIVE=1) — spawns a real uv/marimo subprocess; see file header',
  );
  test.skip(!onPath('deno') || !onPath('uv'), 'deno/uv not on PATH — marimo engine unavailable');

  consoleLines.length = 0;
  page.on('console', (msg) => consoleLines.push(`[${msg.type()}] ${msg.text()}`));
  page.on('pageerror', (err) => consoleLines.push(`[pageerror] ${err.message}`));

  const projSrc = await stageMarimoProject(WIDGET_QMD);
  server = await startPreviewServer({
    copyFromDir: projSrc,
    extraEnv: { RUST_LOG: 'warn,quarto_preview::capture_driver=info' },
  });

  const captureRecorded = await waitForServerLog(
    server,
    ['recorded engine capture(s)', 'engines=marimo'],
    90_000,
  );
  expect(
    captureRecorded,
    "server must record a marimo capture ('recorded engine capture(s)' with " +
      `engines=marimo) — proves the marimo engine ran; got no such line within 90s.\n` +
      `server log tail:\n${stripAnsi(server.serverLog()).slice(-2000)}`,
  ).toBe(true);

  await page.goto(server.url);

  await page
    .waitForFunction(
      () => {
        const outer = document.querySelector('iframe');
        const html = outer?.contentDocument?.body?.innerHTML ?? '';
        return html.includes('<marimo-island') && html.includes('<marimo-ui-element');
      },
      null,
      { timeout: 30_000 },
    )
    .catch(() => {
      /* fall through to the assertion so we can attach the pane state */
    });
  await page.waitForTimeout(2_000);

  const html = await paneHtml(page);
  const diag = `\n\npane HTML was:\n${html}\n\nconsole:\n${consoleLines.join('\n')}`;

  // MARKUP DELIVERY ONLY — do NOT assert the slider is interactive/hydrated.
  // Marimo widgets hydrate client-side via an external cdn.jsdelivr.net
  // islands script that may be CSP-blocked in the preview sandbox; whether it
  // loads is a separate question (see file header), left as a Gordon
  // decision. This assertion only proves the splice delivered the widget's
  // raw `{=html}` island markup to the pane.
  expect(
    html?.includes('<marimo-island') ?? false,
    `pane must contain "<marimo-island" — the widget's island markup must ` +
      `splice into the preview pane;${diag}`,
  ).toBe(true);
  expect(
    (html?.includes('<marimo-ui-element') ?? false) || (html?.includes('<marimo-slider') ?? false),
    `pane must contain "<marimo-ui-element" and/or "<marimo-slider" — the ` +
      `slider widget's markup must be present inside the island;${diag}`,
  ).toBe(true);
});

test('SC24: recorded marimo sql-interop capture delivers the executed sql island to the pane', async ({
  page,
}) => {
  // Cold `uv`/pubgrub resolution of duckdb+sqlglot+polars+pyarrow can be
  // considerably slower than the minimal/widget docs' bare marimo dep — the
  // render-tier SC9/SC14 rows confirmed these deps resolve on this machine,
  // but budget generously for a cold cache.
  test.setTimeout(240_000);

  test.skip(
    process.env.QUARTO_SC21_LIVE !== '1',
    'SC24 is opt-in (set QUARTO_SC21_LIVE=1) — spawns a real uv/marimo subprocess; see file header',
  );
  test.skip(!onPath('deno') || !onPath('uv'), 'deno/uv not on PATH — marimo engine unavailable');

  consoleLines.length = 0;
  page.on('console', (msg) => consoleLines.push(`[${msg.type()}] ${msg.text()}`));
  page.on('pageerror', (err) => consoleLines.push(`[pageerror] ${err.message}`));

  const projSrc = await stageMarimoProject(SQL_INTEROP_QMD);
  server = await startPreviewServer({
    copyFromDir: projSrc,
    extraEnv: { RUST_LOG: 'warn,quarto_preview::capture_driver=info' },
  });

  const captureRecorded = await waitForServerLog(
    server,
    ['recorded engine capture(s)', 'engines=marimo'],
    180_000,
  );
  expect(
    captureRecorded,
    "server must record a marimo capture ('recorded engine capture(s)' with " +
      `engines=marimo) — proves the marimo engine ran (incl. cold uv dep ` +
      `resolution); got no such line within 180s.\n` +
      `server log tail:\n${stripAnsi(server.serverLog()).slice(-2000)}`,
  ).toBe(true);

  await page.goto(server.url);

  await page
    .waitForFunction(
      () => {
        const outer = document.querySelector('iframe');
        const html = outer?.contentDocument?.body?.innerHTML ?? '';
        return html.includes('marimo-table');
      },
      null,
      { timeout: 30_000 },
    )
    .catch(() => {
      /* fall through to the assertion so we can attach the pane state */
    });
  await page.waitForTimeout(2_000);

  const html = await paneHtml(page);
  const diag = `\n\npane HTML was:\n${html}\n\nconsole:\n${consoleLines.join('\n')}`;

  // The render-tier SC14 test asserts the raw-HTML entity-escaped form
  // (`data-data=` + `x&#92;&quot;:2`). OBSERVED in a live run here: the pane's
  // `<marimo-table>` carries `data-data="&quot;[{\&quot;x\&quot;:2}]&quot;"`
  // — innerHTML re-serializes the attribute's embedded quotes as `&quot;`
  // (standard attribute-value serialization, not a decode), so the same
  // `x":2` payload is present, just re-escaped rather than raw-decoded as
  // this comment originally assumed.
  //
  // SCOPED, not ambient: a bare `html.includes('2')` over the WHOLE pane
  // body would pass even if the sql island's data were wrong, because this
  // doc's OTHER cell (`{python .marimo}` evaluating `1 + 1`) independently
  // renders `<pre class="text-xs">2</pre>` elsewhere in the pane — that stray
  // "2" would keep the old assertion green regardless of what the
  // `marimo-table` actually carries. Extract the `<marimo-table ...>` opening
  // tag itself and require the computed value to be INSIDE it, next to the
  // `x` column, so the check fails if the sql payload is wrong even though
  // the python cell's "2" still renders.
  const marimoTableTag = html?.match(/<marimo-table[^>]*>/)?.[0];
  expect(
    marimoTableTag,
    `pane must contain a "<marimo-table ...>" element — the bare {sql} cell's ` +
      `executed island must splice into the preview pane;${diag}`,
  ).toBeTruthy();
  expect(
    (marimoTableTag?.includes('x') ?? false) && (marimoTableTag?.includes('2') ?? false),
    `the <marimo-table> element's own attributes must carry the "x" column ` +
      `and its computed value "2" (SELECT 1 + 1 AS x) — checked WITHIN the ` +
      `marimo-table tag itself, not the whole pane (which also contains an ` +
      `unrelated "2" from the python cell's 1 + 1 output);${diag}\n\n` +
      `marimo-table tag: ${marimoTableTag}`,
  ).toBe(true);
});

test('SC25: marimo widget include-in-header reaches the pane <head> AND executes (hydration)', async ({
  page,
}) => {
  // bd-5oyk1xce: the widget-hydration question SC23 left OUT of scope. Two
  // fixes together make this pass:
  //   1. Rust (CaptureSpliceStage.fold_capture_includes_into_meta): re-inject
  //      the engine's include-in-header (islands <script>, the inline
  //      __MARIMO_EXPORT_CONTEXT__ marker, <marimo-code>) into
  //      meta.rendered.includes.header — the q2-preview pipeline excludes
  //      ApplyTemplateStage, so nothing else delivers engine header includes.
  //   2. TS (HeaderIncludesEffect.rematerializeScript): re-materialize
  //      injected <script> nodes so the browser EXECUTES them (innerHTML-parsed
  //      scripts are inert).
  //
  // Network-INDEPENDENT assertions (do NOT depend on the external
  // cdn.jsdelivr.net islands module actually loading):
  //   (a) the islands <script src=…jsdelivr…islands…> is present in the iframe
  //       <head>              → Rust delivery fix
  //   (b) <marimo-code> is present in the iframe   → Rust delivery fix
  //   (c) window.__MARIMO_EXPORT_CONTEXT__ is DEFINED on the iframe window
  //       → the INLINE `Object.defineProperty(window,"__MARIMO_EXPORT_CONTEXT__",…)`
  //         marker script executed → TS re-materialization fix. This is the
  //         exact marker that was `undefined` in the pre-fix repro.
  // Full slider interactivity additionally needs the CDN module to load, which
  // is marimo+network's job, outside these two fixes — not asserted here.
  test.setTimeout(150_000);

  test.skip(
    process.env.QUARTO_SC21_LIVE !== '1',
    'SC25 is opt-in (set QUARTO_SC21_LIVE=1) — spawns a real uv/marimo subprocess; see file header',
  );
  test.skip(!onPath('deno') || !onPath('uv'), 'deno/uv not on PATH — marimo engine unavailable');

  consoleLines.length = 0;
  page.on('console', (msg) => consoleLines.push(`[${msg.type()}] ${msg.text()}`));
  page.on('pageerror', (err) => consoleLines.push(`[pageerror] ${err.message}`));

  const projSrc = await stageMarimoProject(WIDGET_QMD);
  server = await startPreviewServer({
    copyFromDir: projSrc,
    extraEnv: { RUST_LOG: 'warn,quarto_preview::capture_driver=info' },
  });

  const captureRecorded = await waitForServerLog(
    server,
    ['recorded engine capture(s)', 'engines=marimo'],
    90_000,
  );
  expect(
    captureRecorded,
    `server must record a marimo capture; log tail:\n${stripAnsi(server.serverLog()).slice(-2000)}`,
  ).toBe(true);

  await page.goto(server.url);

  // Wait for the island markup (delivery) to land, then settle so the injected
  // head scripts have run.
  await page
    .waitForFunction(
      () => {
        const f = document.querySelector('iframe');
        return !!f?.contentDocument?.querySelector('marimo-island');
      },
      null,
      { timeout: 30_000 },
    )
    .catch(() => {});
  await page.waitForTimeout(3_000);

  const probe = await page.evaluate(() => {
    const outer = document.querySelector('iframe');
    const doc = outer?.contentDocument ?? null;
    const headHtml = doc?.head?.innerHTML ?? '';
    const w = outer?.contentWindow as unknown as Record<string, unknown> | null;
    return {
      islandsScriptInHead:
        !!doc?.head?.querySelector(
          'script[src*="jsdelivr.net/npm/@marimo-team/islands"]',
        ),
      marimoCodePresent: !!doc?.querySelector('marimo-code'),
      exportContextDefined: !!w && '__MARIMO_EXPORT_CONTEXT__' in w,
      headHtml,
    };
  });

  const diag =
    `\n\niframe <head> was:\n${probe.headHtml}\n\nconsole:\n${consoleLines.join('\n')}`;

  // (a) Rust delivery: islands module <script> reached the pane <head>.
  expect(
    probe.islandsScriptInHead,
    `iframe <head> must contain the islands <script src=…jsdelivr…@marimo-team/islands…> ` +
      `— proves the engine include-in-header was re-injected into ` +
      `meta.rendered.includes.header and delivered to the pane;${diag}`,
  ).toBe(true);

  // (b) Rust delivery: the hidden notebook source reached the pane.
  expect(
    probe.marimoCodePresent,
    `iframe must contain <marimo-code> (the hidden notebook source, another ` +
      `include-in-header artifact);${diag}`,
  ).toBe(true);

  // (c) TS execution: the INLINE __MARIMO_EXPORT_CONTEXT__ marker script ran.
  // This is the marker that was `undefined` in the pre-fix repro; its presence
  // proves HeaderIncludesEffect re-materialized the injected <script> so it
  // executed (an innerHTML-parsed script would stay inert → undefined).
  expect(
    probe.exportContextDefined,
    `window.__MARIMO_EXPORT_CONTEXT__ must be defined on the iframe window — ` +
      `proves the injected inline marker <script> EXECUTED (the ` +
      `rematerializeScript fix). It was undefined before the fix;${diag}`,
  ).toBe(true);
});
