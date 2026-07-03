/**
 * Helpers for extracting content from the hub-client preview iframe.
 *
 * These run against the live browser page after a project has been loaded
 * and rendered through the full Automerge → VFS → WASM → Preview pipeline.
 */

import type { Page } from '@playwright/test';
import type {} from './testHooks';

/**
 * Wait for the preview iframe to render content.
 *
 * For html-style previews (the default) the DoubleBufferedIframe component
 * mounts an `iframe.preview-active` whose body is populated when render
 * completes. For `format: q2-debug`, the renderer mounts a Q2DebugIframe
 * whose `src` ends in `q2-debug.html`; we wait for that iframe and for
 * its body to receive content from the postMessage flow.
 *
 * If `consoleErrors` is provided, the wait will abort early when a fatal
 * browser error is detected (e.g. WebSocket failure, WASM crash), avoiding
 * a long timeout with no diagnostic info.
 */
export type PreviewIframeKind = 'html' | 'q2-debug' | 'q2-preview';

export function previewIframeSelector(kind: PreviewIframeKind): string {
  if (kind === 'q2-debug') return 'iframe[src*="q2-debug.html"]';
  if (kind === 'q2-preview') return 'iframe[src*="q2-preview.html"]';
  return 'iframe.preview-active';
}

/**
 * Wait until every expected project file has synced into the WASM VFS.
 *
 * The smoke-all pipeline pushes all fixture files to the hub as Automerge
 * docs, then the browser syncs them down into the VFS asynchronously. The
 * spec sorts the render-target QMD *last* server-side as a best-effort hint,
 * but nothing actually gates the first render — or the assertion-time
 * re-render — on the sibling files (extension `_extensions/<x>/<x>.lua`,
 * filters, included partials) having arrived. When they lose that race, a
 * `{{< shortcode >}}` fails to expand or an extension fails to load, and the
 * test fails non-deterministically. This was the dominant source of the
 * concentrated hard-failures on the multi-file extension fixtures
 * (`block-shortcode`, `shortcode-extension`, `quarto-doc-api-extension`).
 *
 * This barrier polls `vfsListFiles()` and returns as soon as EITHER:
 *   (a) all expected project-relative paths are present — the fast path for
 *       well-rooted fixtures (the small multi-file extension fixtures clear
 *       this in a poll or two; single-file fixtures clear it immediately); or
 *   (b) the VFS file count has *quiesced* — stable across two consecutive
 *       polls — i.e. sync has stopped delivering files. This is the escape
 *       hatch for fixtures whose discovered file set is over-broad: a fixture
 *       in a directory with no `_quarto.yml` (several `q2-preview/` fixtures)
 *       roots at the parent and pulls in unrelated sibling-project files
 *       (`with-render-components/`, `multi-element-project/`) that this render
 *       never needs and that may sync slowly or not at all. Without (b) those
 *       fixtures would pay the full timeout on every run — a large wall-clock
 *       regression on a contended CI runner, ×~78 tests.
 *
 * The cap is deliberately short: the barrier is *insurance*. The real
 * completeness guarantee is `waitForPreviewRender` (waits for a render) plus
 * `renderForAssertions`, which reads the current VFS at assertion time — by
 * then any straggler files have synced. So a slightly-early return here is
 * harmless; it just falls back to the prior behavior.
 *
 * `expectedRelPaths` are project-root-relative (the shape of
 * `fixture.projectFiles[].path`); the VFS reports `/project/`-prefixed
 * absolute paths, which we strip before comparing.
 */
export async function waitForVfsFiles(
  page: Page,
  expectedRelPaths: string[],
  opts: { timeout?: number; consoleErrors?: string[] } = {},
): Promise<void> {
  const timeout = opts.timeout ?? 10000;
  const consoleErrors = opts.consoleErrors;
  const pollInterval = 200;
  const start = Date.now();
  const deadline = start + timeout;
  // The quiesce escape hatch (b) must not preempt the exact-match path (a) for
  // a fixture whose files are still arriving. Extension fixtures push their
  // files target-last, so the VFS count climbs 0→…→N; a brief sync lull at an
  // intermediate count must NOT be read as "done". So only allow quiesce after
  // a grace window (giving fast fixtures time to exact-match) and require a
  // longer stable run (~600ms) before trusting it.
  const quiesceGraceMs = 3000;
  const quiesceStableHits = 3;
  const VFS_PROJECT_PREFIX = '/project/';
  // Normalize to project-relative with no leading slash (matches the
  // stripped VFS form below).
  const expected = expectedRelPaths.map((p) => p.replace(/^\/+/, ''));

  let missing: string[] = expected;
  let prevCount = -1;
  let stableHits = 0;
  while (Date.now() < deadline) {
    // Fail fast on an unrecoverable WASM trap rather than waiting out the
    // full timeout with no diagnostic (mirrors waitForPreviewRender).
    if (consoleErrors && consoleErrors.length > 0) {
      const fatal = consoleErrors.find(
        (e) => e.includes('unreachable') || e.includes('RuntimeError'),
      );
      if (fatal) {
        throw new Error(
          `Fatal browser error during VFS sync wait: ${fatal}\nAll console errors:\n${consoleErrors.join('\n')}`,
        );
      }
    }

    const result = await page.evaluate(
      async ({ expected, prefix }) => {
        await window.__quartoTestReady;
        const hooks = window.__quartoTest;
        if (!hooks) return { missing: expected, count: -1 }; // hooks not installed yet
        // This barrier runs *before* the first render completes, so the WASM
        // module may not be initialized yet — the Preview component calls
        // initWasm() on mount, and `vfsListFiles()` throws
        // "WASM module not initialized" until then. Treat that (and any other
        // transient access error) as "not ready, keep polling" rather than
        // letting it abort the wait.
        let listing: { success: boolean; files?: string[] };
        try {
          listing = hooks.wasmRenderer.vfsListFiles();
        } catch {
          return { missing: expected, count: -1 };
        }
        if (!listing.success) return { missing: expected, count: -1 };
        const files = listing.files ?? [];
        const present = new Set(
          files.map((f: string) => (f.startsWith(prefix) ? f.slice(prefix.length) : f)),
        );
        return {
          missing: expected.filter((p) => !present.has(p)),
          count: files.length,
        };
      },
      { expected, prefix: VFS_PROJECT_PREFIX },
    );
    missing = result.missing;

    // (a) Fast path: everything we expected is present.
    if (missing.length === 0) return;

    // (b) Quiesce path: the VFS file count has stopped growing. Only trusted
    // after the grace window and a sustained stable run, so it can't preempt a
    // still-arriving extension fixture (see the constants above).
    if (result.count > 0 && result.count === prevCount) {
      stableHits += 1;
      if (Date.now() - start >= quiesceGraceMs && stableHits >= quiesceStableHits) {
        return;
      }
    } else {
      stableHits = 0;
    }
    prevCount = result.count;

    await page.waitForTimeout(pollInterval);
  }

  // Best-effort barrier: if neither condition was met in time, proceed anyway.
  // The barrier only *reduces* the sync race; it never introduces a hard
  // failure (see the doc comment). Log what was still missing for diagnostics.
  // eslint-disable-next-line no-console
  console.warn(
    `[waitForVfsFiles] proceeding after ${timeout}ms with ${missing.length} ` +
      `project file(s) not yet synced into the VFS: ${missing.join(', ')}`,
  );
}

/**
 * Snapshot of *where* the boot→render pipeline is stalled, captured at the
 * moment a render wait gives up.
 *
 * Every smoke-all failure in CI surfaces as the same opaque
 * "Timed out after 75000ms waiting for preview iframe to render" — a single
 * symptom with several distinct underlying causes (project-load stall, WASM
 * init stall, render stall, …) that the raw job log cannot tell apart. This
 * reads stable DOM markers + the WASM test hooks to classify the failure into
 * a `stage`, and emits a single parseable `[smoke-diag] ...` line into the
 * timeout error so the offline analyzer can bucket nightly failures by cause
 * instead of guessing one at a time.
 *
 * No app changes / rebuild: it relies only on existing class names
 * (`.project-selector` / `.connecting` / `.error` from ProjectSelector,
 * `.editor-container` from Editor), the "Loading preview..." text from
 * PreviewRouter, the preview iframe, and `window.__quartoTest.wasmRenderer`.
 */
export interface PreviewStageDiagnostics {
  stage: string;
  wasmReady: boolean;
  vfsCount: number;
  projectSelector: boolean;
  connecting: boolean;
  connError: boolean;
  editor: boolean;
  loadingPreview: boolean;
  iframePresent: boolean;
  bodyLen: number;
  /**
   * The on-screen "Render Error" message, if the preview pane is showing one
   * (Preview's ErrorView or ReactPreview's inline error view). This is the
   * *actual* failure reason — e.g. "Failed to read file: Path not found:
   * /project/<target>.qmd" — which otherwise lives only in the page DOM and
   * never reaches the CI log, where the lone visible signal is the benign
   * mocked-auth 401. Surfacing it stops failure logs misleading the next
   * investigator toward a phantom connectivity/auth cause. Empty when no
   * render error is shown.
   */
  renderError: string;
  /**
   * Sync-layer view of the stall (from the sync client's
   * `getSyncDiagnostics()` test hook): for every index-referenced file that
   * never loaded, the underlying automerge-repo DocHandle state
   * (`loading` / `requesting` / `unavailable` / `nohandle`) and whether the
   * client's own unavailable marker was set, plus connected-peer count and
   * the unavailable-retry poll's tick counter / timer state. This is the
   * field that distinguishes the remaining stall *mechanisms* behind the
   * uniform "Path not found: <target>" symptom — a request lost in flight
   * (`requesting`), automerge-repo's terminal cached verdict
   * (`unavailable`), or a storage load that never finished (`loading`).
   * Null when the hook is unavailable (older bundle) or capture threw.
   */
  sync: {
    connectedPeers: number;
    unavailableRetryTicks: number;
    retryTimerActive: boolean;
    stranded: Array<{
      path: string;
      docId: string;
      handleState: string | null;
      unavailableMarker: boolean;
    }>;
  } | null;
  /** Single-line, key=value, grep-friendly form for the failure message. */
  line: string;
}

export async function capturePreviewDiagnostics(
  page: Page,
  kind: PreviewIframeKind = 'html',
): Promise<PreviewStageDiagnostics> {
  const iframeSelector = previewIframeSelector(kind);
  const raw = await page.evaluate(async (selector) => {
    const has = (sel: string) => !!document.querySelector(sel);
    const text = document.body?.innerText ?? '';
    let wasmReady = false;
    let vfsCount = -1;
    try {
      await window.__quartoTestReady;
      const r = window.__quartoTest?.wasmRenderer;
      if (r) {
        try {
          const listing = r.vfsListFiles();
          wasmReady = true;
          vfsCount = listing.success ? (listing.files?.length ?? 0) : 0;
        } catch {
          wasmReady = false; // vfsListFiles throws until initWasm() resolves
        }
      }
    } catch {
      /* hooks not ready */
    }
    const iframe = document.querySelector(selector) as HTMLIFrameElement | null;
    let bodyLen = -1;
    if (iframe?.contentDocument?.body) {
      bodyLen = iframe.contentDocument.body.innerHTML.length;
    }

    // Capture the on-screen "Render Error" message if the preview is showing
    // one. Both error views (Preview's ErrorView and ReactPreview's inline
    // view) render `<strong>Render Error</strong>` followed by a `<pre>` with
    // the message, so match on that shared structure rather than a class
    // (both use inline styles). The message is the genuine failure reason that
    // otherwise never leaves the DOM. Collapse whitespace + cap length to keep
    // the single-line diag tidy.
    let renderError = '';
    const errStrong = Array.from(document.querySelectorAll('strong')).find(
      (s) => s.textContent?.trim() === 'Render Error',
    );
    if (errStrong) {
      const pre = errStrong.parentElement?.querySelector('pre');
      const msg = (pre?.textContent ?? errStrong.parentElement?.textContent ?? '').trim();
      renderError = msg.replace(/\s+/g, ' ').slice(0, 300);
    }

    // Sync-layer stall mechanism (see the `sync` field doc on
    // PreviewStageDiagnostics). Optional-called so a bundle predating the
    // hook degrades to null instead of throwing inside the diag capture.
    let sync: {
      connectedPeers: number;
      unavailableRetryTicks: number;
      retryTimerActive: boolean;
      stranded: Array<{
        path: string;
        docId: string;
        handleState: string | null;
        unavailableMarker: boolean;
      }>;
    } | null = null;
    try {
      const renderer = window.__quartoTest?.wasmRenderer as
        | { getSyncDiagnostics?: () => NonNullable<typeof sync> }
        | undefined;
      sync = renderer?.getSyncDiagnostics?.() ?? null;
    } catch {
      /* not connected yet, or hook absent */
    }

    return {
      wasmReady,
      vfsCount,
      projectSelector: has('.project-selector'),
      connecting: has('.project-selector .connecting'),
      connError: has('.project-selector .error'),
      editor: has('.editor-container'),
      loadingPreview: text.includes('Loading preview'),
      iframePresent: !!iframe,
      bodyLen,
      renderError,
      sync,
    };
  }, iframeSelector);

  // Classify by the furthest-reached stage (most-downstream marker wins).
  let stage: string;
  if (raw.iframePresent && raw.bodyLen > 0) stage = 'RENDERED_RACED'; // body filled but our poll missed it
  else if (raw.iframePresent) stage = 'RENDER_STALL'; // preview mounted, body never populated
  else if (raw.loadingPreview) stage = 'PREVIEW_ROUTER_STALL'; // WASM-init / format-check stuck
  else if (raw.editor) stage = 'EDITOR_NO_PREVIEW'; // Editor mounted, PreviewRouter not showing a preview
  else if (raw.connError) stage = 'CONNECT_ERROR'; // project load errored out
  else if (raw.projectSelector) stage = 'CONNECT_STALL'; // project never loaded (sync/findDoc hang)
  else if (!raw.wasmReady) stage = 'WASM_NOT_READY';
  else stage = 'UNKNOWN';

  // Compact sync-mechanism suffix, e.g.
  //   syncPeers=1 syncRetryTicks=12 stranded="test.qmd=requesting+marker"
  // `stranded` lists each never-loaded file as <path>=<handleState>, with
  // `+marker` when the sync client's unavailable marker is set and
  // `nohandle` when the repo holds no cached handle. Only emitted when the
  // hook returned data; kept after renderError so existing parsers are
  // unaffected.
  let syncSuffix = '';
  if (raw.sync) {
    const stranded = raw.sync.stranded
      .map(
        (s) =>
          `${s.path}=${s.handleState ?? 'nohandle'}${s.unavailableMarker ? '+marker' : ''}`,
      )
      .join(',');
    syncSuffix =
      ` syncPeers=${raw.sync.connectedPeers}` +
      ` syncRetryTicks=${raw.sync.unavailableRetryTicks}` +
      ` syncRetryTimer=${raw.sync.retryTimerActive ? 1 : 0}` +
      (stranded ? ` stranded="${stranded}"` : '');
  }

  const line =
    `[smoke-diag] stage=${stage} wasmReady=${raw.wasmReady} vfs=${raw.vfsCount} ` +
    `projectSelector=${raw.projectSelector ? 1 : 0} connecting=${raw.connecting ? 1 : 0} ` +
    `connError=${raw.connError ? 1 : 0} editor=${raw.editor ? 1 : 0} ` +
    `loadingPreview=${raw.loadingPreview ? 1 : 0} iframe=${raw.iframePresent ? 1 : 0} ` +
    `bodyLen=${raw.bodyLen}` +
    // The real failure reason, when the preview is showing a render error.
    // Appended after the fixed fields so the analyzer's `stage=` parse is
    // unaffected.
    (raw.renderError ? ` renderError="${raw.renderError}"` : '') +
    syncSuffix;

  return { stage, ...raw, line };
}

export async function waitForPreviewRender(
  page: Page,
  opts: {
    timeout?: number;
    consoleErrors?: string[];
    kind?: PreviewIframeKind;
  } = {},
): Promise<void> {
  const timeout = opts.timeout ?? 30000;
  const consoleErrors = opts.consoleErrors;
  const iframeSelector = previewIframeSelector(opts.kind ?? 'html');

  // Poll for render completion, but also check for fatal console errors
  // so we can fail fast with a useful message instead of timing out.
  const pollInterval = 250;
  const deadline = Date.now() + timeout;

  while (Date.now() < deadline) {
    // Check if any fatal console errors have been collected
    if (consoleErrors && consoleErrors.length > 0) {
      // Only treat unrecoverable WASM traps as immediately fatal.
      // Lua panics are expected control flow on wasm32 (LUAI_THROW is
      // rewired to a Rust panic caught by rust_lua_protected_call) and
      // are suppressed by a custom panic hook in wasm-quarto-hub-client
      // (see claude-notes/plans/2026-04-16-suppress-lua-panic-noise.md),
      // so they should not reach consoleErrors at all. Network errors
      // (500s) and out-of-date builds may still produce transient noise.
      const fatal = consoleErrors.find(
        (e) =>
          e.includes('unreachable') ||
          e.includes('RuntimeError'),
      );
      if (fatal) {
        throw new Error(
          `Fatal browser error during render wait: ${fatal}\nAll console errors:\n${consoleErrors.join('\n')}`,
        );
      }
    }

    // Check if the preview iframe has rendered content
    const rendered = await page.evaluate((selector) => {
      const iframe = document.querySelector(selector) as HTMLIFrameElement | null;
      if (!iframe?.contentDocument?.body) return false;
      return iframe.contentDocument.body.innerHTML.length > 0;
    }, iframeSelector);

    if (rendered) return;

    await page.waitForTimeout(pollInterval);
  }

  // Timed out — classify *where* it stalled so nightly failures stop looking
  // identical (see capturePreviewDiagnostics). The `[smoke-diag] ...` line is
  // parsed by the offline analyzer to bucket failures by cause.
  let diagLine = '[smoke-diag] stage=DIAG_FAILED';
  try {
    const diag = await capturePreviewDiagnostics(page, opts.kind ?? 'html');
    diagLine = diag.line;
  } catch (err) {
    diagLine = `[smoke-diag] stage=DIAG_FAILED err=${err instanceof Error ? err.message : String(err)}`;
  }
  const errorContext = consoleErrors?.length
    ? `\nConsole errors:\n${consoleErrors.join('\n')}`
    : '\nNo console errors captured.';
  throw new Error(
    `Timed out after ${timeout}ms waiting for preview iframe to render.\n${diagLine}${errorContext}`,
  );
}

/**
 * Get the raw rendered HTML by re-rendering the document via WASM.
 *
 * The browser's DOM serialization loses DOCTYPE and wraps inline text
 * in data-sid spans, so we can't reliably match raw HTML patterns from
 * the iframe content. Instead we do a fresh WASM render (VFS is already
 * populated) and return the raw HTML string.
 */
export async function getPreviewHtml(
  page: Page,
  documentPath: string,
): Promise<string> {
  return page.evaluate(async (docPath) => {
    await window.__quartoTestReady;
    const hooks = window.__quartoTest;
    if (!hooks) throw new Error('__quartoTest missing — rebuild with VITE_E2E=1');
    const renderer = hooks.wasmRenderer;

    // Discover user grammars from the VFS so the re-render matches the
    // in-Preview render (Preview.tsx forwards a project-file list +
    // resolvers from automergeSync; bd-izfv made the project-render
    // path actually honor that handle). Without this, fixtures that
    // depend on `_quarto/grammars/<lang>/` render unhighlighted here
    // even though the live iframe renders them correctly.
    //
    // `vfs_list_files` returns the VFS-absolute form (`/project/...`);
    // `discoverUserGrammars` expects project-relative paths with no
    // leading slash, and the resolver callbacks below mirror that
    // shape (matches `Preview.tsx` → `automergeSync` wiring).
    const VFS_PROJECT_PREFIX = '/project/';
    const stripPrefix = (vfsPath: string): string | null =>
      vfsPath.startsWith(VFS_PROJECT_PREFIX)
        ? vfsPath.slice(VFS_PROJECT_PREFIX.length)
        : null;
    const toVfsPath = (relPath: string): string =>
      relPath.startsWith('/') ? relPath : `${VFS_PROJECT_PREFIX}${relPath}`;

    const listing = renderer.vfsListFiles();
    const projectFilePaths: string[] = listing.success
      ? (listing.files ?? [])
          .map(stripPrefix)
          .filter((p): p is string => p !== null)
      : [];

    const result = await renderer.renderToHtml({
      documentPath: docPath,
      userGrammars: {
        files: projectFilePaths,
        getBinaryContent: async (path: string) => {
          const r = renderer.vfsReadBinaryFile(toVfsPath(path));
          if (!r.success || typeof r.content !== 'string') return null;
          // Decode base64 → Uint8Array on the page (Buffer isn't available).
          const binary = atob(r.content);
          const bytes = new Uint8Array(binary.length);
          for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
          return bytes;
        },
        getTextContent: async (path: string) => {
          const r = renderer.vfsReadFile(toVfsPath(path));
          return r.success && typeof r.content === 'string' ? r.content : null;
        },
      },
    });
    return result.html ?? '';
  }, documentPath);
}

/**
 * Get combined CSS from all local stylesheets referenced in the preview.
 *
 * Parses <link rel="stylesheet"> tags from the preview HTML, reads each
 * local stylesheet from VFS via the wasmRenderer module, and returns
 * the concatenated CSS.
 */
export async function getPreviewCss(page: Page): Promise<string> {
  return page.evaluate(async () => {
    const iframe = document.querySelector('iframe.preview-active') as HTMLIFrameElement | null;
    if (!iframe?.contentDocument) {
      throw new Error('No active preview iframe found');
    }

    await window.__quartoTestReady;
    const hooks = window.__quartoTest;
    if (!hooks) throw new Error('__quartoTest missing — rebuild with VITE_E2E=1');
    const renderer = hooks.wasmRenderer;
    const links = iframe.contentDocument.querySelectorAll('link[rel="stylesheet"]');
    let combinedCss = '';

    for (const link of links) {
      const href = link.getAttribute('href');
      if (!href || href.startsWith('http://') || href.startsWith('https://') || href.startsWith('//')) {
        continue;
      }

      // Handle data: URIs (CSS is inlined by iframePostProcessor)
      if (href.startsWith('data:')) {
        // Extract CSS from data URI: data:text/css;base64,... or data:text/css,...
        const commaIdx = href.indexOf(',');
        if (commaIdx === -1) continue;
        const meta = href.slice(0, commaIdx);
        const data = href.slice(commaIdx + 1);
        if (meta.includes('base64')) {
          combinedCss += atob(data) + '\n';
        } else {
          combinedCss += decodeURIComponent(data) + '\n';
        }
        continue;
      }

      // Try reading from VFS
      const vfsPath = href.startsWith('/') ? href : `/project/${href}`;
      try {
        const result = renderer.vfsReadFile(vfsPath);
        if (result.success && result.content) {
          combinedCss += result.content + '\n';
        }
      } catch {
        // CSS file not readable from VFS — may be post-processed
      }
    }

    return combinedCss;
  });
}

/**
 * Diagnostic info from a render result.
 */
export interface RenderDiagnostic {
  kind: string;
  title: string;
}

/**
 * Get render diagnostics by re-rendering the document via page.evaluate.
 *
 * Since the Preview component doesn't expose its last render result to
 * the global scope, we perform a fresh render to capture diagnostics.
 * The VFS is already populated from the Automerge sync, so this is fast.
 */
export async function getRenderDiagnostics(
  page: Page,
  documentPath: string,
): Promise<{
  success: boolean;
  error?: string;
  diagnostics: RenderDiagnostic[];
  warnings: RenderDiagnostic[];
}> {
  return page.evaluate(async (docPath) => {
    await window.__quartoTestReady;
    const hooks = window.__quartoTest;
    if (!hooks) throw new Error('__quartoTest missing — rebuild with VITE_E2E=1');
    const result = await hooks.wasmRenderer.renderToHtml({ documentPath: docPath });
    return {
      success: result.success,
      error: result.error,
      diagnostics: (result.diagnostics ?? []).map((d: { kind: string; title: string }) => ({
        kind: d.kind,
        title: d.title,
      })),
      warnings: (result.warnings ?? []).map((d: { kind: string; title: string }) => ({
        kind: d.kind,
        title: d.title,
      })),
    };
  }, documentPath);
}

/**
 * Combined render result: HTML *and* diagnostics from a single WASM render.
 */
export interface AssertionRender {
  success: boolean;
  error?: string;
  html: string;
  diagnostics: RenderDiagnostic[];
  warnings: RenderDiagnostic[];
}

/**
 * Render the document once for assertions, returning both the raw HTML and
 * the diagnostics from the same render.
 *
 * The smoke-all assertion runner previously called {@link getRenderDiagnostics}
 * *and* {@link getPreviewHtml} separately — two full WASM renders per test, on
 * top of the live Preview render (three total). For the shortcode/extension
 * fixtures the render runs the wasm32 Lua interpreter (the slowest, most
 * contention-sensitive path), so that redundant rendering is exactly what
 * pushed those fixtures past the preview-render deadline on a 2-core CI runner.
 * Rendering once and reading both outputs removes ~1 full render per test.
 *
 * Like {@link getPreviewHtml}, this discovers user grammars from the VFS so a
 * grammar-dependent fixture renders identically to the live Preview, and the
 * diagnostics come from that same grammar-aware render.
 */
export async function renderForAssertions(
  page: Page,
  documentPath: string,
): Promise<AssertionRender> {
  return page.evaluate(async (docPath) => {
    await window.__quartoTestReady;
    const hooks = window.__quartoTest;
    if (!hooks) throw new Error('__quartoTest missing — rebuild with VITE_E2E=1');
    const renderer = hooks.wasmRenderer;

    // Discover user grammars from the VFS (mirrors getPreviewHtml / Preview.tsx).
    const VFS_PROJECT_PREFIX = '/project/';
    const stripPrefix = (vfsPath: string): string | null =>
      vfsPath.startsWith(VFS_PROJECT_PREFIX)
        ? vfsPath.slice(VFS_PROJECT_PREFIX.length)
        : null;
    const toVfsPath = (relPath: string): string =>
      relPath.startsWith('/') ? relPath : `${VFS_PROJECT_PREFIX}${relPath}`;

    const listing = renderer.vfsListFiles();
    const projectFilePaths: string[] = listing.success
      ? (listing.files ?? [])
          .map(stripPrefix)
          .filter((p): p is string => p !== null)
      : [];

    const result = await renderer.renderToHtml({
      documentPath: docPath,
      userGrammars: {
        files: projectFilePaths,
        getBinaryContent: async (path: string) => {
          const r = renderer.vfsReadBinaryFile(toVfsPath(path));
          if (!r.success || typeof r.content !== 'string') return null;
          const binary = atob(r.content);
          const bytes = new Uint8Array(binary.length);
          for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
          return bytes;
        },
        getTextContent: async (path: string) => {
          const r = renderer.vfsReadFile(toVfsPath(path));
          return r.success && typeof r.content === 'string' ? r.content : null;
        },
      },
    });

    return {
      success: result.success,
      error: result.error,
      html: result.html ?? '',
      diagnostics: (result.diagnostics ?? []).map((d: { kind: string; title: string }) => ({
        kind: d.kind,
        title: d.title,
      })),
      warnings: (result.warnings ?? []).map((d: { kind: string; title: string }) => ({
        kind: d.kind,
        title: d.title,
      })),
    };
  }, documentPath);
}
