/**
 * The app-wide typst compiler: one loader (so one resident `Module`), one fonts loader and
 * one runner (so one live compile) per page. The PDF chain (host phase H8) goes through here.
 */
import { createTypstLoader, TypstFontsLoader } from './typstAssets';
import { TypstRunner, type WorkerLike } from './typstRunner';
import { cacheApiTarballs } from './typstPackageCache';
import type { PandocLoader } from '../pandoc/pandocLoader';

/** `typst.worker.ts` is named deliberately; see its header. */
export function createTypstBrowserWorker(): WorkerLike {
  return new Worker(new URL('./typst.worker.ts', import.meta.url), { type: 'module' }) as unknown as WorkerLike;
}

let shared: { loader: PandocLoader; fonts: TypstFontsLoader; runner: TypstRunner } | undefined;

export function getTypst(): { loader: PandocLoader; fonts: TypstFontsLoader; runner: TypstRunner } {
  if (!shared) {
    const loader = createTypstLoader();
    const fonts = new TypstFontsLoader();
    shared = { loader, fonts, runner: new TypstRunner({ loader, fonts, createWorker: createTypstBrowserWorker, cache: cacheApiTarballs() }) };
  }
  return shared;
}

let previewRunner: TypstRunner | undefined;

/**
 * The PDF preview's own typst runner (H10b): the shared loader and fonts, but serialized, so a newer preview job
 * never aborts a running one and a Download as PDF and a preview never cancel each other's compile.
 */
export function getPreviewTypstRunner(): TypstRunner {
  const { loader, fonts } = getTypst();
  previewRunner ??= new TypstRunner({ loader, fonts, createWorker: createTypstBrowserWorker, cache: cacheApiTarballs(), serialized: true });
  return previewRunner;
}
