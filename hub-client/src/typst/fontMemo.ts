/**
 * The PDF chain's font list, memoized (H10b Task 2). Every preview run needs the family names for the pandoc
 * request, and the answer depends only on the font assets, so it is computed once per asset version rather than
 * once per run: that also keeps the first stage of an overlapping run from queueing a typst job behind others.
 *
 * The shared job has its own `AbortController` and no stage callback, so one run's abort or detach cannot resolve
 * it for the others; each waiter detaches on its own signal. Only an `ok` outcome is cached. A waiter whose
 * shared job came back `superseded` or `aborted` while its own run is still current asks again, once.
 */
import type { LoadProgress } from '../pandoc/pandocLoader';
import type { FontListOutcome, TypstRunFailure, TypstRunOptions } from './typstRunner';

export type FontListResult = FontListOutcome | TypstRunFailure;

export interface FontMemoDeps {
  runner: { listFonts(fonts?: Uint8Array[], options?: TypstRunOptions): Promise<FontListResult> };
  /** The buffers to load (each call returns fresh copies, since the runner transfers them). */
  fonts: () => Uint8Array[];
  /** What the answer depends on; evaluated before `fonts()` is called. */
  key: () => string;
}

interface Flight {
  key: string;
  promise: Promise<FontListResult>;
  progress: Set<(p: LoadProgress) => void>;
}

const aborted = (): TypstRunFailure => ({ ok: false, kind: 'aborted', diagnostics: [], notices: [] });

export function createFontMemo(deps: FontMemoDeps): (options?: TypstRunOptions) => Promise<FontListResult> {
  let cached: { key: string; outcome: FontListOutcome } | undefined;
  let flight: Flight | undefined;

  const join = (key: string): Flight => {
    if (flight && flight.key === key) return flight;
    const progress = new Set<(p: LoadProgress) => void>();
    const f: Flight = {
      key,
      progress,
      promise: deps.runner.listFonts(deps.fonts(), { signal: new AbortController().signal, onLoadProgress: (p) => progress.forEach((l) => l(p)) }).then((o) => {
        if (o.ok) cached = { key, outcome: o };
        if (flight === f) flight = undefined;
        return o;
      }),
    };
    flight = f;
    return f;
  };

  const get = async (options: TypstRunOptions, retried: boolean): Promise<FontListResult> => {
    const key = deps.key();
    if (cached?.key === key) return cached.outcome;
    const signal = options.signal;
    if (signal?.aborted) return aborted();
    const f = join(key);
    options.onStage?.('loading');
    if (options.onLoadProgress) f.progress.add(options.onLoadProgress);
    let off: (() => void) | undefined;
    const detached = new Promise<FontListResult>((resolve) => {
      const onAbort = () => {
        if (options.onLoadProgress) f.progress.delete(options.onLoadProgress);
        resolve(aborted());
      };
      signal?.addEventListener('abort', onAbort, { once: true });
      off = () => signal?.removeEventListener('abort', onAbort);
    });
    let outcome: FontListResult;
    try {
      outcome = await Promise.race([f.promise, detached]);
    } finally {
      off?.();
      if (options.onLoadProgress) f.progress.delete(options.onLoadProgress);
    }
    if (!outcome.ok && (outcome.kind === 'superseded' || outcome.kind === 'aborted') && !signal?.aborted && !retried) return get(options, true);
    return outcome;
  };

  return (options = {}) => get(options, false);
}
