/**
 * PC5 — engine-capture delivery to the browser pane (bd-h4rhohhy, Bug B).
 *
 * The full delivery chain under test (server-side up to the sidecar write, then
 * browser-side through the WASM render):
 *
 *   record_eager_captures → write_capture_doc + IndexDocument::set_capture
 *   (capture_driver.rs) → samod sync → quarto-sync-client capture-sidecar diff
 *   → PreviewApp onCapturesChange → render effect → getBinaryDocById
 *   → WASM render_page_for_preview → the q2-preview pipeline's CaptureSplice
 *     stage (crates/quarto-core/src/engine/capture_splice.rs).
 *
 * NOTE: the preview path does NOT use ReplayEngine and has NO staleness check —
 * that was the P0-era hypothesis, refuted by the P2 diagnosis. The splice
 * (`derive_cell_outputs` / `is_cell_wrapper`) maps each engine cell to the next
 * `::: {.cell}` wrapper in the executed markdown.
 *
 * Engine: the committed `echo-engine` TS fixture (deno-gated). It is used
 * INSTEAD of julia on purpose — echo spawns NO child julia server, so the
 * Bug C engine-host stdout-corruption path CANNOT occur here, which isolates
 * the delivery chain from Bug C.
 *
 * The echo engine transforms every ```{echo}``` block into a `::: {.cell}`
 * wrapper containing `**ECHO_EXECUTED**` (the same `.cell` shape real engines
 * emit — load-bearing for the splice, which only matches `.cell` wrappers).
 * The binding assertions are that the executed marker `ECHO_EXECUTED` appears
 * in the pane WITHOUT a reload, and that the raw source token is gone.
 *
 * STATUS (P2): the echo fixture was fixed to emit the `.cell` wrapper (the
 * evidenced defect — the old bare-paragraph output had no wrapper for the
 * splice to match). The amended assertion is controller-ratified and this spec
 * is un-skipped (see the per-test block comment). PC5's binding to the
 * server-side sidecar write is fail-on-revert proven against `set_capture`
 * (transcript in .superpowers/sdd/task-p2-report.md).
 */

import { test, expect, type Page } from '@playwright/test';
import { cp, mkdtemp, mkdir, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

/** Repo root, relative to this file (`q2-preview-spa/e2e/`). */
const REPO_ROOT = path.resolve(import.meta.dirname, '..', '..');
/** The committed echo-engine TS fixture (native tests share it). */
const ECHO_ENGINE_FIXTURE = path.join(
  REPO_ROOT,
  'crates',
  'quarto-core',
  'tests',
  'fixtures',
  'extensions',
  'echo-engine',
);

/** A distinctive source token so the inert-state check can't be faked. */
const SOURCE_TOKEN = 'PC5_ECHO_SOURCE_TOKEN';
const INDEX_QMD = `---\ntitle: PC5 echo capture\n---\n\n# PC5 heading\n\n\`\`\`{echo}\n${SOURCE_TOKEN}\n\`\`\`\n`;

/** `true` when `deno` is on PATH (the echo engine-host runtime). */
function denoAvailable(): boolean {
  try {
    execFileSync('deno', ['--version'], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

/**
 * Assemble a temp project dir containing `index.qmd` + the echo-engine
 * extension under `_extensions/`, then start `q2 preview` against it.
 */
async function startEchoPreview(): Promise<PreviewServerHandle> {
  const projSrc = await mkdtemp(path.join(tmpdir(), 'q2-pc5-echo-src-'));
  await mkdir(path.join(projSrc, '_extensions'), { recursive: true });
  await cp(ECHO_ENGINE_FIXTURE, path.join(projSrc, '_extensions', 'echo-engine'), {
    recursive: true,
  });
  await writeFile(path.join(projSrc, 'index.qmd'), INDEX_QMD);
  return startPreviewServer({ copyFromDir: projSrc });
}

/** Read all `<body>` text inside the sandboxed renderer iframe. */
async function paneText(page: Page): Promise<string | null> {
  return page.evaluate(() => {
    const outer = document.querySelector('iframe');
    const innerDoc = outer?.contentDocument;
    return innerDoc?.body?.textContent ?? null;
  });
}

let server: PreviewServerHandle;
const consoleLines: string[] = [];

test.beforeEach(async () => {
  consoleLines.length = 0;
  server = await startEchoPreview();
});

test.afterEach(async () => {
  await server?.stop();
});

// PC5 (bd-h4rhohhy, controller-amended). The echo fixture now emits a `::: {.cell}`
// wrapper (the shape real engines produce via the engine-host's `mdFromCodeCell`),
// so the recorded capture splices into the pane via the live delivery chain — no
// reload. The binding assertions are:
//   (a) `ECHO_EXECUTED` appears in the pane without a reload, and
//   (b) the raw source token is ABSENT from the final pane (the splice REPLACED the
//       cell rather than appending to it).
//
// Why NOT an inert-source-first assertion: the eager capture is recorded at server
// startup, BEFORE the browser connects, so the SPA's first render already has the
// capture and splices immediately (P0 measured renderTicks=1). There is no
// observable inert→executed transition to assert on. That guard was only ever
// satisfiable while the splice was BROKEN (the source stayed forever).
//
// Why (a) is non-vacuous without the inert guard: `ECHO_EXECUTED` exists ONLY in the
// recorded capture bytes — the document source is `PC5_ECHO_SOURCE_TOKEN` — so its
// presence proves the live capture→splice chain fired. And q2 preview serves the
// CLIENT-SIDE SPA (rendered in-browser via WASM), not a server-pre-rendered HTML
// page, so the "stale full render" vacuity the inert guard once targeted cannot
// occur here. Fail-on-revert proof: reverting `set_capture` (capture_driver.rs)
// makes this RED by timeout (recorded in .superpowers/sdd/task-p2-report.md).
test('PC5: recorded echo capture splices into the pane without reload', async ({ page }) => {
  test.skip(!denoAvailable(), 'deno not on PATH — echo engine-host unavailable');

  page.on('console', (msg) => consoleLines.push(`[${msg.type()}] ${msg.text()}`));
  page.on('pageerror', (err) => consoleLines.push(`[pageerror] ${err.message}`));

  await page.goto(server.url);

  // Binding assertion (a): the executed marker appears in the pane via the live
  // capture→splice path (NO reload). If the delivery chain is broken this times out.
  await page
    .waitForFunction(
      () => {
        const outer = document.querySelector('iframe');
        const body = outer?.contentDocument?.body;
        return body?.textContent?.includes('ECHO_EXECUTED') ?? false;
      },
      null,
      { timeout: 30_000 },
    )
    .catch(() => {
      /* fall through to the assertion so we can attach the pane state */
    });

  const text = await paneText(page);
  // Sync-client state harvest. `__renderTicks` is a production counter
  // (PreviewApp.tsx bumps it per completed render). `__pc5CaptureLog` is only
  // populated when the quarantined onCapturesChange instrumentation is present
  // (see the P0 report's fix-wave §Bug B) — it is `null` in the committed build.
  const syncState = await page.evaluate(() => {
    const w = window as unknown as { __pc5CaptureLog?: unknown[]; __renderTicks?: number };
    return { captureLog: w.__pc5CaptureLog ?? null, renderTicks: w.__renderTicks ?? 0 };
  });
  const diag =
    `\n\npane text was:\n${text}\n\nsync-client state:\n` +
    `${JSON.stringify(syncState, null, 2)}\n\nconsole:\n${consoleLines.join('\n')}`;
  // (a) executed marker present via the live splice.
  expect(
    text?.includes('ECHO_EXECUTED'),
    `pane must show the executed echo marker after the capture splices in;${diag}`,
  ).toBe(true);
  // (b) the source cell was REPLACED, not appended: the raw source token is gone.
  expect(
    text?.includes(SOURCE_TOKEN),
    `the spliced pane must NOT still show the raw source token ${SOURCE_TOKEN} — ` +
      `the capture must REPLACE the cell, not append to it;${diag}`,
  ).toBe(false);
});
