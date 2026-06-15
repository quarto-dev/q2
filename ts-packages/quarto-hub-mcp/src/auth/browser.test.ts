/**
 * Browser launcher — argv construction (the Windows form is the fiddly
 * one) and abort-kills-subprocess behaviour.
 */

import { spawnSync } from 'node:child_process';
import { EventEmitter } from 'node:events';
import { describe, it, expect, vi } from 'vitest';

import { browserOpenSpec, openBrowser } from './browser.js';

const URL_WITH_AMP =
  'https://accounts.google.com/o/oauth2/v2/auth?response_type=code&client_id=x&redirect_uri=http://127.0.0.1:5000/callback&state=abc';

describe('browserOpenSpec', () => {
  it('uses `open` on macOS', () => {
    expect(browserOpenSpec('darwin', URL_WITH_AMP)).toEqual({
      command: 'open',
      args: [URL_WITH_AMP],
      windowsVerbatimArguments: false,
    });
  });

  it('uses `xdg-open` on Linux', () => {
    expect(browserOpenSpec('linux', URL_WITH_AMP)).toEqual({
      command: 'xdg-open',
      args: [URL_WITH_AMP],
      windowsVerbatimArguments: false,
    });
  });

  it('quotes the URL and passes argv verbatim on Windows', () => {
    // The URL must be wrapped in literal double quotes so `cmd.exe`
    // leaves `&` alone; Node would not quote it otherwise (no spaces),
    // and a bare `&` splits the command, dropping `redirect_uri`.
    expect(browserOpenSpec('win32', URL_WITH_AMP)).toEqual({
      command: 'cmd.exe',
      args: ['/c', 'start', '""', `"${URL_WITH_AMP}"`],
      windowsVerbatimArguments: true,
    });
  });
});

// Regression guard for the "Missing required parameter: redirect_uri"
// bug: an OAuth URL pushed through `cmd.exe` must survive its `&`
// statement-separator parsing intact. Runs only on Windows, where the
// real `cmd.exe` is available.
describe.runIf(process.platform === 'win32')('Windows cmd.exe arg passing', () => {
  it('preserves the full URL through cmd.exe (no & truncation)', () => {
    const url = new URL('https://accounts.google.com/o/oauth2/v2/auth');
    url.searchParams.set('response_type', 'code');
    url.searchParams.set('client_id', 'x.apps.googleusercontent.com');
    url.searchParams.set('redirect_uri', 'http://127.0.0.1:53017/callback');
    url.searchParams.set('scope', 'openid email profile');
    url.searchParams.set('state', 'abc');
    const full = url.toString();

    const spec = browserOpenSpec('win32', full);
    // Echo the trailing argv (the `""` placeholder title + the quoted
    // URL) that the real command passes to `start`, instead of launching
    // a browser. If `&` split the command, stderr would carry
    // "'client_id' is not recognized" and stdout would be truncated; if
    // the URL weren't quoted, the placeholder/URL token boundary would
    // collapse. We assert both survive intact.
    const trailing = spec.args.slice(2); // ['""', '"<url>"']
    const r = spawnSync('cmd.exe', ['/c', 'echo', ...trailing], {
      encoding: 'utf8',
      windowsVerbatimArguments: spec.windowsVerbatimArguments,
    });
    expect(r.stderr).toBe('');
    expect(r.stdout.trim()).toBe(`"" "${full}"`);
  });
});

interface FakeChild extends EventEmitter {
  exitCode: number | null;
  signalCode: NodeJS.Signals | null;
  kill: ReturnType<typeof vi.fn>;
}

function fakeChild(): FakeChild {
  const child = new EventEmitter() as FakeChild;
  child.exitCode = null;
  child.signalCode = null;
  child.kill = vi.fn(() => true);
  return child;
}

describe('openBrowser', () => {
  it('spawns the platform command with the constructed argv', () => {
    const child = fakeChild();
    const spawnFn = vi.fn(() => child) as never;
    openBrowser(URL_WITH_AMP, { platform: 'linux', spawnFn });
    expect(spawnFn).toHaveBeenCalledTimes(1);
    const [command, args] = (spawnFn as unknown as { mock: { calls: unknown[][] } }).mock
      .calls[0]!;
    expect(command).toBe('xdg-open');
    expect(args).toEqual([URL_WITH_AMP]);
  });

  it('kills the subprocess when the abort signal fires', () => {
    const child = fakeChild();
    const spawnFn = vi.fn(() => child) as never;
    const ac = new AbortController();
    openBrowser(URL_WITH_AMP, { platform: 'linux', spawnFn, signal: ac.signal });
    expect(child.kill).not.toHaveBeenCalled();
    ac.abort();
    expect(child.kill).toHaveBeenCalledTimes(1);
  });

  it('returns undefined when spawn throws synchronously', () => {
    const spawnFn = vi.fn(() => {
      throw new Error('ENOENT');
    }) as never;
    const result = openBrowser(URL_WITH_AMP, { platform: 'linux', spawnFn });
    expect(result).toBeUndefined();
  });
});
