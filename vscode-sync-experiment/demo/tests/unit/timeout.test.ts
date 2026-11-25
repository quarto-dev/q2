import { describe, expect, it } from 'vitest';
import { timeoutDue } from '../../src/timeout';
import { parseLauncherArgs } from '../../src/launcher-args';

describe('timeoutDue: --timeout ends a run N seconds after the first editor window is up', () => {
  it('is never due without a timeout, or before any editor is up (setup time does not count)', () => {
    expect(timeoutDue(undefined, 1_000_000, 10)).toBe(false);
    expect(timeoutDue(1000, 1_000_000, undefined)).toBe(false);
  });

  it('is due once the seconds have passed since the editor came up, and not before', () => {
    expect(timeoutDue(1000, 1000 + 9_999, 10)).toBe(false);
    expect(timeoutDue(1000, 1000 + 10_000, 10)).toBe(true);
    expect(timeoutDue(1000, 1000 + 60_000, 10)).toBe(true);
  });
});

describe('parseLauncherArgs: --timeout <seconds>', () => {
  it('is off by default and reads a positive number of seconds', () => {
    expect(parseLauncherArgs([]).timeoutSeconds).toBeUndefined();
    expect(parseLauncherArgs(['--timeout', '10']).timeoutSeconds).toBe(10);
    expect(parseLauncherArgs(['--timeout', '2.5']).timeoutSeconds).toBe(2.5);
  });

  it('rejects anything that is not a positive number', () => {
    for (const bad of ['0', '-3', 'soon', '']) expect(() => parseLauncherArgs(['--timeout', bad])).toThrow(/--timeout/);
  });
});
