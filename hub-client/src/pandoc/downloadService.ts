/**
 * The app-wide download controller and the menu's format list (pandoc-host H5).
 *
 * Builds the controller once with the real dependencies: pandoc.wasm through the shared
 * runner, or, in the `q2 preview` embed, the preview server's native render (D7).
 */
import {
  classifyPandocCompletion,
  getPandocFormats,
  getPandocShareTree,
  getPandocShareTreeVersion,
  getTypstAssets,
  getTypstAssetsVersion,
  initWasm,
  renderPandocRequest,
  typstDatePrelude,
  type PandocFormatInfo,
} from '@quarto/preview-runtime';
import type { ShareTree } from '@quarto/pandoc-host';
import { DownloadController, type DownloadDeps, type DownloadFormat, type PdfInfo, type TraceEvent } from './downloadController';
import { pandocWasmEnabled, isPreviewEmbed } from './featureFlag';
import { renderNatively } from './nativeRender';
import { fetchChapterCaptures } from './captureFetch';
import { getPandoc, getPreviewPandocRunner } from './pandocService';
import { saveBlob } from './saveBlob';
import { getPreviewTypstRunner, getTypst } from '../typst/typstService';
import { splitTypstAssets } from '../typst/typstAssetSplit';
import { TYPST_FONTS_SHA256 } from '../typst/typstAssets';
import { createFontMemo } from '../typst/fontMemo';
import type { FontListOutcome, TypstJob, TypstRunFailure, TypstRunOptions, TypstRunOutcome } from '../typst/typstRunner';

/**
 * Formats the menu offers. The table in Rust says what *can* be produced; this list says
 * what the UI has been reviewed for. Every row the table has is wired (H5 plan, Close-out).
 */
export const MENU_FORMATS: readonly string[] = ['docx', 'pptx', 'epub', 'typst', 'pdf'];

/** What the preview server's `POST /api/preview/render` accepts (H4b); `pdf` is browser-only (H8). */
const NATIVE_FORMATS: readonly string[] = ['docx', 'pptx', 'epub'];

/** "Download as" is offered at all: pandoc.wasm is shipped, or the embed has the native route. */
export function downloadAvailable(): boolean {
  return pandocWasmEnabled() || isPreviewEmbed();
}

const toFormat = (f: PandocFormatInfo): DownloadFormat => ({ key: f.key, label: f.label, extension: f.extension, mime: f.mime });

/**
 * The menu entries from the Rust table's rows, in its order: available, not hidden, in
 * `allowed`; and in the embed only what the native route accepts (typst's native render
 * compiles to PDF, so its `.typ`-only download is not offered there).
 */
export function selectMenuFormats(rows: PandocFormatInfo[], embed: boolean, allowed: readonly string[] = MENU_FORMATS): DownloadFormat[] {
  return rows
    .filter((f) => f.available && !f.hidden && allowed.includes(f.key) && (!embed || NATIVE_FORMATS.includes(f.key)))
    .map(toFormat);
}

/** The menu entries. Needs the hub wasm to be initialised. */
export function menuFormats(): DownloadFormat[] {
  if (!downloadAvailable()) return [];
  return selectMenuFormats(getPandocFormats(), isPreviewEmbed());
}

/** The row for one format key (a document's own format), or undefined. */
export function formatByKey(key: string): DownloadFormat | undefined {
  const row = getPandocFormats().find((f) => f.key === key && f.available);
  return row ? toFormat(row) : undefined;
}

let cachedTree: ShareTree | undefined;
/** The share tree, re-read from Rust only when its version changes. */
function shareTree(): ShareTree {
  const version = getPandocShareTreeVersion();
  if (cachedTree?.share_tree_version !== version) cachedTree = getPandocShareTree();
  return cachedTree;
}

export type PdfListener = (pdf: Uint8Array, info: PdfInfo) => void;
const pdfListeners = new Set<PdfListener>();
/** Each compiled PDF, as the "Download as PDF" chain produces it (the viewer's feed). Returns an unsubscribe. */
export function onPdfCompiled(listener: PdfListener): () => void {
  pdfListeners.add(listener);
  return () => pdfListeners.delete(listener);
}

/** The runners the PDF chain uses; the preview supplies its own (H10b), Download uses the app-wide ones. */
export interface ChainRunners {
  pandoc?: NonNullable<DownloadDeps['runner']>;
  typst?: {
    run(job: TypstJob, options?: TypstRunOptions): Promise<TypstRunOutcome>;
    listFonts(fonts?: Uint8Array[], options?: TypstRunOptions): Promise<FontListOutcome | TypstRunFailure>;
  };
}

const fontMemos = new WeakMap<object, ReturnType<typeof createFontMemo>>();

/** The font list of the preview's typst runner, computed once per font asset version. */
function previewFontFamilies(runner: NonNullable<ChainRunners['typst']>) {
  let memo = fontMemos.get(runner);
  if (!memo) {
    memo = createFontMemo({
      runner,
      fonts: () => splitTypstAssets(getTypstAssets().files).fonts,
      key: () => `${TYPST_FONTS_SHA256}|${getTypstAssetsVersion()}`,
    });
    fontMemos.set(runner, memo);
  }
  return memo;
}

/**
 * The browser-side dependencies shared by the download controller and the PDF preview's. `scope` is
 * the default for what a book chapter requests: `'chapter'` is the page alone, `'auto'` the whole
 * book for typst, pdf and epub (R9). A click that carries its own scope (the menu's "Download book
 * as" and "This chapter only") overrides it through `buildRequest`'s sixth argument.
 */
function wasmDeps(scope: 'auto' | 'chapter', runners: ChainRunners = {}): Omit<DownloadDeps, 'save'> {
  return {
    buildRequest: async (path, format, sourceDateEpoch, signal, typstAvailableFonts, extra) => {
      await initWasm();
      return renderPandocRequest(path, format, {
        sourceDateEpoch,
        signal,
        typstAvailableFonts,
        scope: extra?.scope ?? scope,
        capturesByPath: extra?.capturesByPath,
        onProgress: extra?.onProgress,
      });
    },
    fetchCaptures: (docIds, signal) => fetchChapterCaptures(docIds, signal),
    getShareTree: shareTree,
    runner: runners.pandoc ?? getPandoc().runner,
    typst: {
      runner: runners.typst ?? getTypst().runner,
      ...(runners.typst ? { fontFamilies: previewFontFamilies(runners.typst) } : {}),
      assets: () => splitTypstAssets(getTypstAssets().files),
      datePrelude: typstDatePrelude,
    },
    classify: classifyPandocCompletion,
  };
}

/** A `pdf` document gets the viewer when the browser chain is shipped (never in the native embed). */
export function pdfPreviewAvailable(): boolean {
  return pandocWasmEnabled() && !isPreviewEmbed();
}

let previewTrace: ((event: TraceEvent) => void) | undefined;
/** The E2E harness's observer of the preview controllers' runs (`DownloadDeps.trace`); production never sets it. */
export function setPreviewTrace(listener: ((event: TraceEvent) => void) | undefined): void {
  previewTrace = listener;
}

/**
 * A controller for the PDF preview pane: the same chain as "Download as PDF" but it saves nothing and
 * hands each compiled PDF to `onPdf`. It is separate from the app-wide controller so a preview
 * compile never shows in, or supersedes, a download's status.
 */
export function createPdfPreviewController(
  onPdf: (pdf: Uint8Array, info: PdfInfo) => void,
  options: { warm?: boolean; runners?: ChainRunners } = {},
): DownloadController {
  // The pane shows one page, so it stays chapter-alone for good (R9 Q-9-5).
  // `warm` selects the warm pandoc runner, the preview's serialized typst runner and the overlapping controller;
  // without it the preview is the fresh path, as before H10b. `runners` overrides either runner (tests).
  const warmRunner = options.warm ? getPreviewPandocRunner() : undefined;
  const runners: ChainRunners = warmRunner ? { pandoc: warmRunner, typst: getPreviewTypstRunner(), ...options.runners } : { ...options.runners };
  return new DownloadController({ ...wasmDeps('chapter', runners), save: () => {}, onPdf, overlap: options.warm === true, pool: warmRunner, trace: (e) => previewTrace?.(e) });
}

let controller: DownloadController | undefined;

export function getDownloadController(): DownloadController {
  if (!controller) {
    controller = isPreviewEmbed()
      ? new DownloadController({ native: (request, opts) => renderNatively(request, opts), save: saveBlob })
      : new DownloadController({
          // Chapter-alone unless the click says otherwise: only the book chapter's "Download book as"
          // entry asks for 'auto' (R9 Q-9-5), so every other entry keeps one page per file.
          ...wasmDeps('chapter'),
          save: saveBlob,
          onPdf: (pdf, info) => pdfListeners.forEach((l) => l(pdf, info)),
        });
  }
  return controller;
}
