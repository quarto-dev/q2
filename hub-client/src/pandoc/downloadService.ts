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
  initWasm,
  renderPandocRequest,
  typstDatePrelude,
  type PandocFormatInfo,
} from '@quarto/preview-runtime';
import type { ShareTree } from '@quarto/pandoc-host';
import { DownloadController, type DownloadFormat } from './downloadController';
import { pandocWasmEnabled, isPreviewEmbed } from './featureFlag';
import { renderNatively } from './nativeRender';
import { getPandoc } from './pandocService';
import { saveBlob } from './saveBlob';
import { getTypst } from '../typst/typstService';
import { splitTypstAssets } from '../typst/typstAssetSplit';

/**
 * Formats the menu offers. The table in Rust says what *can* be produced; this list says
 * what the UI has been reviewed for. Each request phase that lands (R3-R7) adds its entry
 * here in its own commit (H5 plan, Close-out), after the H5 demo STOP.
 */
export const MENU_FORMATS: readonly string[] = ['docx', 'pdf'];

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

let controller: DownloadController | undefined;

export function getDownloadController(): DownloadController {
  if (!controller) {
    controller = isPreviewEmbed()
      ? new DownloadController({ native: (request, opts) => renderNatively(request, opts), save: saveBlob })
      : new DownloadController({
          buildRequest: async (path, format, sourceDateEpoch, signal, typstAvailableFonts) => {
            await initWasm();
            return renderPandocRequest(path, format, { sourceDateEpoch, signal, typstAvailableFonts });
          },
          getShareTree: shareTree,
          runner: getPandoc().runner,
          typst: {
            runner: getTypst().runner,
            assets: () => splitTypstAssets(getTypstAssets().files),
            datePrelude: typstDatePrelude,
          },
          classify: classifyPandocCompletion,
          save: saveBlob,
        });
  }
  return controller;
}
