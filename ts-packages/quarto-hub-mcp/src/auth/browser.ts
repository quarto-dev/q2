/**
 * Cross-platform browser launcher for the loopback auth flow.
 *
 * The launcher is best-effort and never changes the control flow: the
 * loopback listener is bound *before* the browser is opened and the
 * `authenticate` tool blocks on the listener regardless of whether the
 * launch succeeded. On failure the user can still paste the surfaced
 * authorization URL into a browser themselves.
 */

import { spawn, type ChildProcess } from 'node:child_process';

export interface BrowserOpenSpec {
  readonly command: string;
  readonly args: readonly string[];
}

/**
 * Pure argv construction — exposed for tests. The Windows form is the
 * fiddly one and the reason this is a separate function:
 *
 *   `cmd.exe /c start "" "<url>"`
 *
 * Two gotchas the naive `start <url>` form gets wrong:
 *   1. `cmd.exe` treats `&` as a statement separator and OAuth URLs are
 *      dense with `&`; quoting the URL prevents that interpretation.
 *   2. `start`'s first *quoted* argument is the window title, not the
 *      URL. The empty `""` is a placeholder title so the quoted URL is
 *      parsed as the target.
 */
export function browserOpenSpec(platform: NodeJS.Platform, url: string): BrowserOpenSpec {
  switch (platform) {
    case 'darwin':
      return { command: 'open', args: [url] };
    case 'win32':
      return { command: 'cmd.exe', args: ['/c', 'start', '', url] };
    default:
      // Linux and other Unix-likes.
      return { command: 'xdg-open', args: [url] };
  }
}

export interface OpenBrowserOptions {
  readonly platform?: NodeJS.Platform;
  readonly signal?: AbortSignal;
  /** Injection seam for tests. */
  readonly spawnFn?: typeof spawn;
}

/**
 * Launch the system browser at `url`. Returns the spawned child (so the
 * caller can observe / kill it) or `undefined` if the spawn itself
 * threw synchronously. Errors are intentionally swallowed — the caller
 * relies on the listener, not the launcher, for completion.
 *
 * When an `AbortSignal` is supplied, the child is killed on abort so a
 * host-issued cancellation tears down the browser subprocess instead of
 * leaving it alive until the deadline.
 */
export function openBrowser(url: string, opts: OpenBrowserOptions = {}): ChildProcess | undefined {
  const platform = opts.platform ?? process.platform;
  const spawnFn = opts.spawnFn ?? spawn;
  const { command, args } = browserOpenSpec(platform, url);
  let child: ChildProcess;
  try {
    child = spawnFn(command, args as string[], {
      stdio: 'ignore',
      // Single argv element per arg, no shell — the URL is never passed
      // through a shell that could re-interpret its metacharacters.
      windowsVerbatimArguments: false,
    });
  } catch {
    return undefined;
  }
  // A spawn error (e.g. ENOENT for a missing `xdg-open`) arrives async.
  // Swallow it: the listener-failure / timeout response text is the
  // user-facing fallback, not a thrown error here.
  child.on('error', () => undefined);

  if (opts.signal) {
    const onAbort = (): void => {
      if (child.exitCode === null && child.signalCode === null) {
        child.kill();
      }
    };
    if (opts.signal.aborted) {
      onAbort();
    } else {
      opts.signal.addEventListener('abort', onAbort, { once: true });
      child.once('exit', () => opts.signal?.removeEventListener('abort', onAbort));
    }
  }

  return child;
}
