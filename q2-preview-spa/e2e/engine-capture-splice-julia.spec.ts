/**
 * PC6 — engine-capture delivery to the browser pane, REAL julia engine
 * (bd-h4rhohhy, Bug B). The julia sibling of PC5 (echo). Where PC5 isolates the
 * delivery chain from Bug C with the echo fixture, PC6 is the real-engine
 * evidence row: a `{julia}` cell executes server-side (QuartoNotebookRunner via
 * the Deno host), its `.cell`-wrapped output is recorded as a capture, delivered
 * to the SPA, and spliced into the pane — the executed value `2` appears WITHOUT
 * a reload.
 *
 * OPT-IN (bd-h4rhohhy, P3 2026-07-02). This spec PASSES — a green run is on
 * record (the julia `{1+1}` cell's `.cell`-wrapped `2` splices into the pane
 * without a reload, in 6.5s on this machine). It is gated behind
 * `QUARTO_PC6_LIVE=1` (skips by default, so the normal `npm run test:e2e` suite
 * never runs it) because it spawns a REAL julia server process — an opt-in
 * julia-gated tier, consistent with PC4a's `#[ignore]` + `QUARTO_PC4A_LIVE=1`
 * gate on the Rust side, not because isolation is broken. As of P3, BOTH the
 * temp-`HOME` override (transport file / server, unchanged since P1/P2) AND the
 * julia PROJECT are isolated: `isolateJuliaProject()` below recursively copies
 * the ambient `QUARTO_JULIA_PROJECT` directory (incl. `Project.toml`/
 * `Manifest.toml`) into a per-test temp
 * dir and points `QUARTO_JULIA_PROJECT` at the copy, so the spawned server never
 * runs `--project=<the shared, real directory>` (the depot, `JULIA_DEPOT_PATH`,
 * stays shared — no re-instantiation). See task-p3-report.md for the live-run
 * evidence (shared transport file mtime/existence unchanged across the run) that
 * grounds keeping this opt-in rather than folding it into the default suite: a
 * real julia spawn is still slow/environment-dependent (network-installed julia,
 * multi-second server boot) — the same reason PC4a stays `#[ignore]` — not an
 * isolation gap. Run it on demand with:
 *   QUARTO_PC6_LIVE=1 npx playwright test engine-capture-splice-julia --project=chromium
 *
 * The native-tier proof that julia splices cleanly is unconditional and lives in
 * `crates/quarto-core/tests/integration/capture_splice_seam.rs` reasoning +
 * the julia leg recorded in `.superpowers/sdd/task-p2-report.md`.
 *
 * No inert-first assertion (see PC5's block comment): the binding assertion is
 * that `2` appears in the pane without a reload. Julia's multi-second execution
 * means the capture may arrive AFTER the browser connects — if an inert phase is
 * observed it is noted as evidence, never frozen as an assertion (it's a race).
 */

import { test, expect, type Page } from '@playwright/test';
import { cp, mkdtemp, mkdir, writeFile, readFile, stat } from 'node:fs/promises';
import { tmpdir, homedir } from 'node:os';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

/** Repo root, relative to this file (`q2-preview-spa/e2e/`). */
const REPO_ROOT = path.resolve(import.meta.dirname, '..', '..');
/** The committed julia-engine fixture root (a project dir; engine under `_extensions/`). */
const JULIA_FIXTURE_ROOT = path.join(
  REPO_ROOT,
  'crates',
  'quarto-core',
  'tests',
  'fixtures',
  'extensions',
  'julia-engine',
);

/** Minimal julia doc: `daemon: false`, one `{julia}` cell whose value is 2. */
const INDEX_QMD =
  '---\nengine: julia\nexecute:\n  daemon: false\n---\n\n# PC6 heading\n\n```{julia}\n1 + 1\n```\n';

/**
 * PC6-FIG — figure-bearing julia doc. The cell carries `#| label: fig-pc6`,
 * which pre-engine sugaring wraps in a `::: {#fig-pc6}` float Div — so the
 * engine cell is NESTED, not top-level. This is the shape the top-level-only
 * splice walk silently missed (julia figure preview bug: capture recorded,
 * pane shows raw source, no plot).
 *
 * The cell's value displays via `MIME"text/html"` with an inline data-URI
 * `<img>` — the same shape Plots.jl emits (its HTML display embeds the PNG
 * as a data URI), so the capture carries the image bytes and no asset
 * delivery is involved. A cell whose value only offers `MIME"image/png"`
 * instead goes through `mdImageOutput` (quarto-api jupyter/to-markdown.ts),
 * which writes `index_files/figure-html/...` on the server and emits a path
 * ref — those files are NOT served by `q2 preview`, a separate defect
 * (naturalWidth stays 0); see the strand filed off bd-h4rhohhy.
 */
const FIG_INDEX_QMD = [
  '---',
  'engine: julia',
  'execute:',
  '  daemon: false',
  '---',
  '',
  '# PC6 figure heading',
  '',
  '```{julia}',
  '#| label: fig-pc6',
  'struct InlineImg end',
  'Base.show(io::IO, ::MIME"text/html", ::InlineImg) =',
  '    print(io, "<img src=\\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC\\" />")',
  'InlineImg()',
  '```',
  '',
].join('\n');

function onPath(bin: string): boolean {
  try {
    execFileSync(bin, ['--version'], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

/** Real julia bindir (through the juliaup shim), or null if julia is unavailable. */
function juliaBindir(): string | null {
  try {
    return execFileSync('julia', ['-e', 'print(Sys.BINDIR)'], {
      encoding: 'utf8',
    }).trim();
  } catch {
    return null;
  }
}

/** The instantiated quarto julia project (Project.toml + Manifest.toml), or null. */
function quartoJuliaProject(): string | null {
  const p =
    process.env.QUARTO_JULIA_PROJECT ??
    path.join(homedir(), 'Library', 'Caches', 'quarto', 'julia');
  try {
    execFileSync('test', ['-f', path.join(p, 'Manifest.toml')]);
    return p;
  } catch {
    return null;
  }
}

/**
 * P3 (bd-h4rhohhy) — copy the shared, instantiated quarto julia project
 * (`Project.toml` + `Manifest.toml`; package code lives in the shared
 * `JULIA_DEPOT_PATH`, untouched here) into a fresh temp dir. The caller
 * points `QUARTO_JULIA_PROJECT` at the returned path instead of the
 * shared directory, so the server this test spawns runs `--project=`
 * against a private copy — never the same directory the developer's
 * own preview sessions resolve against.
 */
async function isolateJuliaProject(sharedProject: string): Promise<string> {
  const dst = await mkdtemp(path.join(tmpdir(), 'q2-pc6-project-'));
  await cp(sharedProject, dst, { recursive: true });
  return dst;
}

/**
 * Snapshot of the SHARED (non-isolated) julia transport file's mtime,
 * for proving "the shared transport was untouched during the run"
 * after — the isolation-fix acceptance criterion. Mirrors
 * `SharedTransportSentinel` in `julia_engine_e2e.rs`.
 */
async function captureSharedTransportMtime(): Promise<number | null> {
  const p = path.join(homedir(), 'Library', 'Caches', 'quarto', 'julia', 'julia_transport.txt');
  try {
    return (await stat(p)).mtimeMs;
  } catch {
    return null;
  }
}

/** Read all `<body>` text inside the sandboxed renderer iframe. */
async function paneText(page: Page): Promise<string | null> {
  return page.evaluate(() => {
    const outer = document.querySelector('iframe');
    return outer?.contentDocument?.body?.textContent ?? null;
  });
}

/** Kill the isolated julia server's process group, read from the temp transport file. */
async function killIsolatedServer(isolatedHome: string): Promise<void> {
  const transport = path.join(
    isolatedHome,
    'Library',
    'Caches',
    'quarto',
    'julia',
    'julia_transport.txt',
  );
  try {
    const text = await readFile(transport, 'utf8');
    const m = text.match(/"pid"\s*:\s*(\d+)/);
    if (m) {
      const pid = Number(m[1]);
      try {
        process.kill(-pid, 'SIGTERM');
      } catch {
        /* group signalling may be unavailable; fall through */
      }
      try {
        process.kill(pid, 'SIGTERM');
      } catch {
        /* already gone */
      }
    }
  } catch {
    /* no transport file (server closed after oneShot, or never wrote one) */
  }
}

let server: PreviewServerHandle | undefined;
let isolatedHome: string | undefined;
const consoleLines: string[] = [];

test.afterEach(async () => {
  await server?.stop();
  if (isolatedHome) await killIsolatedServer(isolatedHome);
  server = undefined;
  isolatedHome = undefined;
});

test('PC6: recorded julia capture splices the executed value into the pane without reload', async ({
  page,
}) => {
  test.skip(
    process.env.QUARTO_PC6_LIVE !== '1',
    'PC6 is opt-in (set QUARTO_PC6_LIVE=1) — julia transport is not HOME-isolated; see file header',
  );
  test.skip(!onPath('deno') || !onPath('julia'), 'deno/julia not on PATH');
  const bindir = juliaBindir();
  const project = quartoJuliaProject();
  test.skip(
    bindir === null || project === null,
    'julia bindir unresolved or quarto julia project not instantiated',
  );

  consoleLines.length = 0;
  page.on('console', (msg) => consoleLines.push(`[${msg.type()}] ${msg.text()}`));
  page.on('pageerror', (err) => consoleLines.push(`[pageerror] ${err.message}`));

  // P3 acceptance evidence: snapshot the shared transport's mtime BEFORE
  // spawning anything, so we can prove afterward that it was never touched.
  const sharedTransportMtimeBefore = await captureSharedTransportMtime();

  // Assemble a temp project with the committed julia-engine extension.
  const projSrc = await mkdtemp(path.join(tmpdir(), 'q2-pc6-julia-src-'));
  await cp(
    path.join(JULIA_FIXTURE_ROOT, '_extensions', 'julia-engine'),
    path.join(projSrc, '_extensions', 'julia-engine'),
    { recursive: true },
  );
  await writeFile(path.join(projSrc, 'index.qmd'), INDEX_QMD);

  // Isolated HOME so the julia transport/server live in a throwaway cache dir.
  isolatedHome = await mkdtemp(path.join(tmpdir(), 'q2-pc6-home-'));
  await mkdir(path.join(isolatedHome, 'Library', 'Caches', 'quarto', 'julia'), {
    recursive: true,
  });

  // P3: isolate the julia PROJECT too (on top of HOME) — see file header.
  const isolatedProject = await isolateJuliaProject(project!);

  server = await startPreviewServer({
    copyFromDir: projSrc,
    extraEnv: {
      HOME: isolatedHome,
      JULIA_DEPOT_PATH: process.env.JULIA_DEPOT_PATH ?? path.join(homedir(), '.julia'),
      QUARTO_JULIA_PROJECT: isolatedProject,
      // Real julia bindir first so the launcher doesn't hit the juliaup shim
      // (which drifts under the temp HOME).
      PATH: `${bindir}${path.delimiter}${process.env.PATH ?? ''}`,
    },
  });

  await page.goto(server.url);

  // Binding assertion: the executed value `2` appears in the pane via the live
  // capture→splice path (NO reload). Julia's first render can be slow (server
  // spawn + package load), so allow a generous timeout.
  await page
    .waitForFunction(
      () => {
        const outer = document.querySelector('iframe');
        const body = outer?.contentDocument?.body;
        const text = body?.textContent ?? '';
        // The cell source is `1 + 1`; the EXECUTED output adds a standalone `2`.
        return /(^|[^+\d])2([^+\d]|$)/.test(text);
      },
      null,
      { timeout: 90_000 },
    )
    .catch(() => {
      /* fall through to the assertion so we can attach the pane state */
    });

  const text = await paneText(page);
  expect(
    text && /(^|[^+\d])2([^+\d]|$)/.test(text),
    `pane must show the julia-executed value 2 after the capture splices in; ` +
      `pane text was:\n${text}\n\nconsole:\n${consoleLines.join('\n')}`,
  ).toBe(true);

  // P3 acceptance: the shared, real transport file's mtime is unchanged —
  // this run's (isolated) server never touched it.
  const sharedTransportMtimeAfter = await captureSharedTransportMtime();
  expect(
    sharedTransportMtimeAfter,
    'shared julia_transport.txt mtime changed during the run — isolation leaked onto the shared server',
  ).toBe(sharedTransportMtimeBefore);
});

test('PC6-FIG: a figure-labeled julia cell shows a VISIBLE image in the pane', async ({
  page,
}) => {
  // Cold julia (server spawn + first execute) plus the 90s image wait —
  // same budget the marimo sibling uses.
  test.setTimeout(150_000);
  test.skip(
    process.env.QUARTO_PC6_LIVE !== '1',
    'PC6 is opt-in (set QUARTO_PC6_LIVE=1) — spawns a real julia server; see file header',
  );
  test.skip(!onPath('deno') || !onPath('julia'), 'deno/julia not on PATH');
  const bindir = juliaBindir();
  const project = quartoJuliaProject();
  test.skip(
    bindir === null || project === null,
    'julia bindir unresolved or quarto julia project not instantiated',
  );

  consoleLines.length = 0;
  page.on('console', (msg) => consoleLines.push(`[${msg.type()}] ${msg.text()}`));
  page.on('pageerror', (err) => consoleLines.push(`[pageerror] ${err.message}`));

  const sharedTransportMtimeBefore = await captureSharedTransportMtime();

  const projSrc = await mkdtemp(path.join(tmpdir(), 'q2-pc6-fig-src-'));
  await cp(
    path.join(JULIA_FIXTURE_ROOT, '_extensions', 'julia-engine'),
    path.join(projSrc, '_extensions', 'julia-engine'),
    { recursive: true },
  );
  await writeFile(path.join(projSrc, 'index.qmd'), FIG_INDEX_QMD);

  isolatedHome = await mkdtemp(path.join(tmpdir(), 'q2-pc6-fig-home-'));
  await mkdir(path.join(isolatedHome, 'Library', 'Caches', 'quarto', 'julia'), {
    recursive: true,
  });
  const isolatedProject = await isolateJuliaProject(project!);

  server = await startPreviewServer({
    copyFromDir: projSrc,
    extraEnv: {
      HOME: isolatedHome,
      JULIA_DEPOT_PATH: process.env.JULIA_DEPOT_PATH ?? path.join(homedir(), '.julia'),
      QUARTO_JULIA_PROJECT: isolatedProject,
      PATH: `${bindir}${path.delimiter}${process.env.PATH ?? ''}`,
    },
  });

  await page.goto(server.url);

  // Binding assertion: a LOADED, laid-out <img> — naturalWidth > 0 AND a
  // non-collapsed bounding box — appears inside the pane iframe. Text-level
  // assertions (PC5/PC6) cannot catch the nested-cell splice miss: the doc
  // text renders fine while the figure silently stays raw source.
  await page
    .waitForFunction(
      () => {
        const outer = document.querySelector('iframe');
        const doc = outer?.contentDocument;
        if (!doc) return false;
        return [...doc.querySelectorAll('img')].some((img) => {
          const rect = img.getBoundingClientRect();
          return img.naturalWidth > 0 && rect.width > 0 && rect.height > 0;
        });
      },
      null,
      { timeout: 90_000 },
    )
    .catch(() => {
      /* fall through to the assertion so we can attach the pane state */
    });

  const imgState = await page.evaluate(() => {
    const outer = document.querySelector('iframe');
    const doc = outer?.contentDocument;
    const imgs = [...(doc?.querySelectorAll('img') ?? [])].map((img) => ({
      src: img.src.slice(0, 64),
      naturalWidth: img.naturalWidth,
      rect: img.getBoundingClientRect().toJSON(),
    }));
    return { imgs, bodyText: doc?.body?.textContent?.slice(0, 500) ?? null };
  });
  expect(
    imgState.imgs.some(
      (i) => i.naturalWidth > 0 && i.rect.width > 0 && i.rect.height > 0,
    ),
    `pane must show a visible, loaded figure image after the capture splices in; ` +
      `imgs: ${JSON.stringify(imgState.imgs)}\npane text:\n${imgState.bodyText}\n\n` +
      `console:\n${consoleLines.join('\n')}`,
  ).toBe(true);

  const sharedTransportMtimeAfter = await captureSharedTransportMtime();
  expect(
    sharedTransportMtimeAfter,
    'shared julia_transport.txt mtime changed during the run — isolation leaked onto the shared server',
  ).toBe(sharedTransportMtimeBefore);
});
