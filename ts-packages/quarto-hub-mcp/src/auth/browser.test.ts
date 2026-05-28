/**
 * Browser launcher — argv construction (the Windows form is the fiddly
 * one) and abort-kills-subprocess behaviour.
 */

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
    });
  });

  it('uses `xdg-open` on Linux', () => {
    expect(browserOpenSpec('linux', URL_WITH_AMP)).toEqual({
      command: 'xdg-open',
      args: [URL_WITH_AMP],
    });
  });

  it('uses cmd.exe /c start "" "<url>" on Windows', () => {
    // The empty placeholder title and the single quoted URL argv element
    // are what keep `&` from being treated as a statement separator.
    expect(browserOpenSpec('win32', URL_WITH_AMP)).toEqual({
      command: 'cmd.exe',
      args: ['/c', 'start', '', URL_WITH_AMP],
    });
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
