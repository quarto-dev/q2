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
  /**
   * Whether to spawn with `windowsVerbatimArguments`. Only the Windows
   * spec sets this `true`: it has already wrapped the URL in literal
   * double quotes, so Node must pass argv through verbatim rather than
   * re-quoting it.
   */
  readonly windowsVerbatimArguments: boolean;
}

/**
 * Pure argv construction — exposed for tests. The Windows form is the
 * fiddly one and the reason this is a separate function:
 *
 *   `cmd.exe /c start "" "<url>"`
 *
 * Two gotchas the naive `start <url>` form gets wrong:
 *   1. `cmd.exe` treats `&` as a statement separator and OAuth URLs are
 *      dense with `&`. The URL must reach `cmd.exe` wrapped in double
 *      quotes so the `&` stays literal. Node's default
 *      (`windowsVerbatimArguments: false`) only quotes an argv element
 *      when it contains a space, tab, or `"` — an OAuth URL contains
 *      none of those, so Node leaves it bare and `cmd.exe` splits the
 *      command at the first `&` (dropping `redirect_uri` and everything
 *      after it). We therefore quote the URL ourselves and pass argv
 *      verbatim. Double quotes neutralise `&`, `|`, `<`, `>`, `(`, `)`
 *      but NOT `%`: `cmd.exe` still performs `%VAR%` expansion inside
 *      quotes. The percent-encoded URL (`%3A`, `%2F`, …) survives only
 *      because no environment variable is named like a two-hex-digit
 *      token (`3A`, `2F`, …). That holds for every real Windows
 *      environment; the dynamic URL fields (`state`, `code_challenge`)
 *      are base64url and never contain `%` at all.
 *   2. `start`'s first *quoted* argument is the window title, not the
 *      URL. The empty `""` is a placeholder title so the quoted URL is
 *      parsed as the target.
 */
export function browserOpenSpec(platform: NodeJS.Platform, url: string): BrowserOpenSpec {
  switch (platform) {
    case 'darwin':
      return { command: 'open', args: [url], windowsVerbatimArguments: false };
    case 'win32':
      return {
        command: 'cmd.exe',
        args: ['/c', 'start', '""', `"${url}"`],
        windowsVerbatimArguments: true,
      };
    default:
      // Linux and other Unix-likes.
      return { command: 'xdg-open', args: [url], windowsVerbatimArguments: false };
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
  const { command, args, windowsVerbatimArguments } = browserOpenSpec(platform, url);
  let child: ChildProcess;
  try {
    child = spawnFn(command, args as string[], {
      stdio: 'ignore',
      // No shell is involved on any platform. On Windows the URL is
      // pre-quoted in the argv (see browserOpenSpec) and must be passed
      // verbatim so `cmd.exe` sees the literal double quotes that keep
      // `&` from splitting the command.
      windowsVerbatimArguments,
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
