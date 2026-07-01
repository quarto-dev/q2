/**
 * SC21-NEG — marimo through `q2 preview`: a LIMITATION-PINNING CANARY
 * (Phase 4cH of claude-notes/plans/2026-07-02-plan4c-marimo-validation.md;
 * architectural FINDING #5, strand bd-5jxcio5d).
 *
 * This is the marimo sibling of PC5 (echo) / PC6 (julia), but it does NOT
 * assert a positive splice — because marimo cannot splice into the preview
 * pane today, and that is the exact fact this spec pins.
 *
 * WHY marimo does not splice (FINDING #5). The preview capture-splice
 * (`crates/quarto-core/src/engine/capture_splice.rs`,
 * `derive_cell_outputs`/`is_cell_wrapper`) maps each source engine cell to the
 * next `::: {.cell}` (Div with class `"cell"`) block in the executed markdown.
 * Echo and julia emit that wrapper (julia via the engine-host's
 * `mdFromCodeCell`; the echo fixture's own source comment calls the wrapper
 * "load-bearing... a bare paragraph → no splice"). Marimo does NOT: its engine
 * returns each executed cell as a bare ```` ```{=html} ```` block carrying
 * `<marimo-island>`/`<marimo-cell-output>` custom elements (by design — that is
 * what makes it render correctly at the `q2 render` tier), with ZERO
 * `class="cell"`. So the marimo capture records server-side but has no wrapper
 * to map, and the pane shows the inert source regardless of the delivery
 * chain's health. `q2 render` produces the executed island fine; `q2 preview`
 * does not. Strand bd-5jxcio5d tracks closing the gap.
 *
 * CONJUNCTIVE assertions (the corrected SC21-NEG row):
 *   (a) the preview server log records the marimo capture — a
 *       `recorded engine capture(s)` line with `engines=marimo`. Proves the
 *       marimo engine ran and the RECORDING half of the chain works (the half
 *       that is NOT severed). Server-log access is via `previewServer.ts`'s
 *       additive `serverLog()` accessor; `RUST_LOG` is raised for the
 *       `quarto_preview::capture_driver` target so the INFO line is visible.
 *   (b) after a bounded settle the pane STILL contains the literal inert
 *       `40 + 2` AND does NOT contain `marimo-cell-output` — pins the
 *       limitation exactly (the island never reaches the pane). Non-vacuous
 *       because `page.goto` runs only AFTER (a) has confirmed the capture is
 *       recorded, so the SPA connects with the capture already present and its
 *       first render ATTEMPTS (and fails) the splice — the inert pane is due to
 *       the missing wrapper, not to a capture that never arrived.
 *
 * CANARY / TRIPWIRE. This spec is EXPECTED TO REDDEN when bd-5jxcio5d's fix
 * lands: any fix that makes the marimo island reach the pane breaks assertion
 * (b) (`marimo-cell-output` appears / the literal `40 + 2` is replaced). At
 * that point the fixer should FLIP this into the positive splice test:
 *   - assert `marimo-cell-output` AND the evaluated `42` are present;
 *   - scope the literal-absent check to the pane BODY EXCLUDING the head
 *     `notebookCode:` script — because the literal source ALSO survives there,
 *     not only URL-encoded (see the secondary-finding note below). The original
 *     SC21 assertion (b) premise ("source only URL-encoded inside
 *     `<marimo-code hidden>`") was wrong; the corrected premise scopes to the
 *     body.
 *
 * SECONDARY FINDING (why the original (b) premise was wrong). `q2 render` of
 * this doc carries the literal, spaced `40 + 2` inside a `notebookCode:`
 * JS-string in the `__MARIMO_EXPORT_CONTEXT__` include-in-header script — so
 * the literal is NOT confined to an unexecuted cell. The NEG form sidesteps
 * this (it asserts the literal is PRESENT in the inert pane), but the flip
 * sketch above must account for it.
 *
 * ISOLATION RATIONALE (frozen row): unlike julia (PC6), marimo has NO shared
 * daemon and NO shared transport file — each render spawns a self-contained
 * `uv`/marimo subprocess. So there is no `isolateJuliaProject`/HOME-override
 * here: a plain temp project copy (fresh per test) fully isolates state.
 *
 * OPT-IN (user directive 2026-07-03, mirroring PC6's `QUARTO_PC6_LIVE`). Gated
 * behind `QUARTO_SC21_LIVE=1`; SKIPS by default so the CI suite stays fast (a
 * real marimo run spawns a `uv`-managed python subprocess, marimo==0.23.13).
 * Run on demand:
 *   QUARTO_SC21_LIVE=1 npx playwright test engine-capture-splice-marimo --project=chromium
 *
 * PREVIEW-BINARY FRESHNESS (frozen precondition): the pane render runs in the
 * embedded SPA/WASM baked into `target/debug/q2` via `include_dir!` (see
 * claude-notes/instructions/preview-spa-rebuild.md — the three-cache trap:
 * WASM → q2-preview-spa/dist → q2 binary). This spec was authored and run AFTER
 * a full rebuild chain (npm run build:wasm → cargo xtask build-q2-preview-spa →
 * cargo build --bin q2). A stale binary would silently render pre-change WASM.
 *
 * NAMED REVERT HUNKS → RED.
 *   1. Chain hunk (`set_capture`, capture_driver.rs:193): MOOT for the NEG
 *      form. In PC5/PC6 reverting it severs the delivery tail; here the tail is
 *      already severed (that is the whole point), so it does not bind this
 *      canary. Not run.
 *   2. Marimo-leg hunk (SC8's ratified TWO-part fixture revert, proven RED once
 *      below): applied to the spec's TEMP project copy only via
 *      `applyMarimoLegRevert` under the default-off `QUARTO_SC21_REVERT` knob —
 *      remove ONLY the `python:` claim ENTRY from `_extension.yml` (keeping the
 *      `claims:` key + the other entries) AND add `claims-files: []`. With the
 *      static map still present, `ts_engine`'s static short-circuit resolves the
 *      missing `python` key to a static None (never reaching the dynamic
 *      `claimsLanguage` wire, which fires only when there is NO static map); so
 *      no engine owns `{python .marimo}` → it falls through to jupyter
 *      (unavailable here) → render fails → NO marimo capture is recorded →
 *      assertion (a) reddens. This binds the MARIMO ENGINE's participation (the
 *      recording half). The committed FIXTURE stays `git diff`-clean throughout.
 *      (See `applyMarimoLegRevert`'s IMPLEMENTER WARNING: dropping the WHOLE
 *      `claims:` map — 4cB2's claims-less variant, NOT SC8's revert — would
 *      instead AWAKEN the dynamic `claimsLanguage`, which re-claims the cell and
 *      does not redden.) Verbatim RED (`QUARTO_SC21_LIVE=1 QUARTO_SC21_REVERT=1`,
 *      2026-07-03):
 *
 *        Error: server must record a marimo capture ('recorded engine
 *        capture(s)' with engines=marimo) — proves the marimo engine ran; got
 *        no such line within 90s.
 *        server log tail:
 *        2026-07-03T16:17:38.450021Z  WARN preview::diagnostics: Engine capture
 *          failed page=index.qmd code=Some("Q-PREVIEW-CAP-1")
 *        Expected: true
 *        Received: false
 *
 * MISSING-TEST PASS (logged, not silent): marimo bare-sql-interop through
 * preview is ACCEPTED-UNTESTED for 4cH — minimal-doc parity is the scope.
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
 * distinctive literal `40 + 2` (asserted PRESENT in the inert pane) and its
 * distinctive result `42` (what a future splice would surface) keep the flip
 * sketch's assertions non-ambient. No `engine:` front-matter — ownership flows
 * through the static `whenClass: marimo` claim on `python`, exactly as the
 * render-tier SC8 fixture (`minimal.qmd`) does.
 */
const INDEX_QMD =
  '---\ntitle: "SC21 marimo capture"\n---\n\n# SC21 heading\n\n```{python .marimo}\nimport marimo as mo\n40 + 2\n```\n';

function onPath(bin: string): boolean {
  try {
    execFileSync(bin, ['--version'], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

/**
 * Marimo-leg named revert (hunk 2), applied to a TEMP-COPY of the fixture only
 * — never the committed fixture. Guarded by the scratch `QUARTO_SC21_REVERT`
 * knob so it fires only when proving RED.
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
 * (`_extensions/marimo/` + `_quarto.yml`) and the `40 + 2` `index.qmd`, then
 * return the source dir for `startPreviewServer({ copyFromDir })`.
 * (previewServer copies again into its own fresh tempdir, so this dir is a
 * private staging area — never the committed fixture.)
 */
async function stageMarimoProject(): Promise<string> {
  const projSrc = await mkdtemp(path.join(tmpdir(), 'q2-sc21-marimo-src-'));
  await cp(
    path.join(MARIMO_FIXTURE_ROOT, '_extensions', 'marimo'),
    path.join(projSrc, '_extensions', 'marimo'),
    { recursive: true },
  );
  await cp(path.join(MARIMO_FIXTURE_ROOT, '_quarto.yml'), path.join(projSrc, '_quarto.yml'));
  await writeFile(path.join(projSrc, 'index.qmd'), INDEX_QMD);
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

test('SC21-NEG: marimo capture records server-side but never splices into the preview pane', async ({
  page,
}) => {
  // The preview engine-host drives marimo via `uv`; a warm-cache capture is
  // ~2s but a cold pubgrub resolution can be much slower. Raise the test-level
  // timeout so the bounded waits below (90s capture + 30s pane + settle) are
  // the real bound and a cold resolution still passes.
  test.setTimeout(150_000);

  test.skip(
    process.env.QUARTO_SC21_LIVE !== '1',
    'SC21-NEG is opt-in (set QUARTO_SC21_LIVE=1) — spawns a real uv/marimo subprocess; see file header',
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

  // Assertion (a): the marimo engine ran and its capture was RECORDED
  // server-side (the un-severed half of the chain). Bounded wait on the server
  // log — this also guarantees the capture EXISTS before we inspect the pane,
  // so (b) below is non-vacuous.
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
  // attempts the splice with the capture already present (anti-vacuity).
  await page.goto(server.url);

  // Wait for the pane to render (inert source in today's world, or the island
  // in a future fixed world), then settle so any splice attempt has completed.
  await page
    .waitForFunction(
      () => {
        const outer = document.querySelector('iframe');
        const html = outer?.contentDocument?.body?.innerHTML ?? '';
        return html.includes('40 + 2') || html.includes('marimo-cell-output');
      },
      null,
      { timeout: 30_000 },
    )
    .catch(() => {
      /* fall through to the assertions so we can attach the pane state */
    });
  // Settle: give a hypothetical fixed-world splice ample time to land, so this
  // canary reddens promptly when bd-5jxcio5d's fix arrives.
  await page.waitForTimeout(5_000);

  const html = await paneHtml(page);
  const diag = `\n\npane HTML was:\n${html}\n\nconsole:\n${consoleLines.join('\n')}`;

  // (b) LIMITATION PINNED: the pane still shows the inert source and the marimo
  // island never spliced in. Either expectation flipping is the tripwire that a
  // fix (bd-5jxcio5d) landed — see the flip sketch in the file header.
  expect(
    html?.includes('40 + 2') ?? false,
    `CANARY: the inert source "40 + 2" must STILL be in the pane — marimo does ` +
      `not splice into q2 preview (FINDING #5). If this failed, the island now ` +
      `reaches the pane: flip this spec to the positive splice test (bd-5jxcio5d);${diag}`,
  ).toBe(true);
  expect(
    html?.includes('marimo-cell-output') ?? false,
    `CANARY: "marimo-cell-output" must be ABSENT from the pane — the marimo ` +
      `island is never spliced by the .cell-anchored capture-splice (FINDING #5). ` +
      `If this failed, the splice now works: flip this spec to positive (bd-5jxcio5d);${diag}`,
  ).toBe(false);
});
