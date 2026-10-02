/**
 * The app-wide pandoc runner: one loader (and so one resident `Module`) and one runner
 * (so one live render) per page. UI code (host phase H3 onward) goes through here.
 */
import { PandocLoader } from './pandocLoader';
import { PandocRunner, type WorkerLike } from './pandocRunner';

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
