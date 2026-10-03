/**
 * The app-wide pandoc runner: one loader (and so one resident `Module`) and one runner
 * (so one live render) per page. UI code (host phase H3 onward) goes through here.
 */
import { PandocLoader } from './pandocLoader';
import { PandocRunner, type WorkerLike } from './pandocRunner';
import { WarmPandocRunner } from './warmPandocRunner';

/** `pandoc.worker.ts` is named deliberately; see its header. */
export function createBrowserWorker(): WorkerLike {
  return new Worker(new URL('./pandoc.worker.ts', import.meta.url), { type: 'module' }) as unknown as WorkerLike;
}

let shared: { loader: PandocLoader; runner: PandocRunner } | undefined;

export function getPandoc(): { loader: PandocLoader; runner: PandocRunner } {
  if (!shared) {
    const loader = new PandocLoader();
    shared = { loader, runner: new PandocRunner({ loader, createWorker: createBrowserWorker }) };
  }
  return shared;
}

/** WebKit stops finding files after about 47 renders on one compiled module (H10b 5b); recompile well before that. */
export const WEBKIT_RECYCLE_AFTER = 30;

const isWebKit = () => typeof navigator !== 'undefined' && /AppleWebKit/.test(navigator.userAgent) && !/Chrome|Chromium|Edg\/|Android/.test(navigator.userAgent);

let previewRunner: WarmPandocRunner | undefined;

/** The preview's warm runner: shares the app-wide loader (so one resident `Module`) but has its own workers. */
export function getPreviewPandocRunner(): WarmPandocRunner {
  previewRunner ??= new WarmPandocRunner({ loader: getPandoc().loader, createWorker: createBrowserWorker, recycleAfter: isWebKit() ? WEBKIT_RECYCLE_AFTER : undefined });
  return previewRunner;
}
