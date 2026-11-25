import { describe, expect, it } from 'vitest';
import { childArgs, DEFAULT_SEQUENCE, formatDuration, parseSequence, summarize } from '../../src/sequence';

describe('parseSequence: the runner that plays several stories one after another', () => {
  it('defaults to hub-vscode-clone, then hub-browser, then vscode-share', () => {
    expect(DEFAULT_SEQUENCE).toEqual(['hub-vscode-clone', 'hub-browser', 'vscode-share']);
    expect(parseSequence([]).presets).toEqual(['hub-vscode-clone', 'hub-browser', 'vscode-share']);
  });

  it('--presets takes any list, in the order given, as one comma-separated value or with =', () => {
    expect(parseSequence(['--presets', 'vscode-share,hub-browser']).presets).toEqual(['vscode-share', 'hub-browser']);
    expect(parseSequence(['--presets=hub-vscode-host']).presets).toEqual(['hub-vscode-host']);
  });

  it('rejects a preset that does not exist, naming it and listing the real ones', () => {
    expect(() => parseSequence(['--presets', 'hub-browser,nope'])).toThrow(/nope.*hub-browser/s);
  });

  it('rejects an empty list', () => {
    expect(() => parseSequence(['--presets', ''])).toThrow(/at least one/);
  });

  it('rejects --preset (one launcher run is `npm run demo`), pointing at --presets', () => {
    expect(() => parseSequence(['--preset', 'hub-browser'])).toThrow(/--presets/);
  });

  it('stops at the first failure unless --keep-going', () => {
    expect(parseSequence([]).keepGoing).toBe(false);
    expect(parseSequence(['--keep-going']).keepGoing).toBe(true);
  });

  it('passes every other flag through untouched and in order, values included', () => {
    const plan = parseSequence(['--fast', '--dwell', '3000', '--presets', 'hub-browser', '--seed=7', '--keep-going']);
    expect(plan.passthrough).toEqual(['--fast', '--dwell', '3000', '--seed=7']);
  });

  it('treats --root as a parent directory: each story gets its own subdirectory', () => {
    const plan = parseSequence(['--root', '/tmp/demos']);
    expect(plan.rootParent).toBe('/tmp/demos');
    expect(childArgs('hub-browser', plan)).toContain('/tmp/demos/hub-browser');
    expect(parseSequence(['--root=/tmp/x']).rootParent).toBe('/tmp/x');
    expect(parseSequence([]).rootParent).toBeUndefined();
  });
});

describe('childArgs: the launcher command line for one story', () => {
  it('plays the story and ends when it ends, then adds the pass-through flags', () => {
    const plan = parseSequence(['--fast', '--dwell', '3000']);
    expect(childArgs('hub-vscode-clone', plan)).toEqual(['--preset', 'hub-vscode-clone', '--play', '--exit', '--fast', '--dwell', '3000']);
  });

  it('adds --root only when a parent directory was given', () => {
    expect(childArgs('vscode-share', parseSequence([])).includes('--root')).toBe(false);
    expect(childArgs('vscode-share', parseSequence(['--root', '/tmp/d'])).slice(-2)).toEqual(['--root', '/tmp/d/vscode-share']);
  });
});

describe('formatDuration', () => {
  it('shows seconds under a minute, and minutes with two-digit seconds above', () => {
    expect(formatDuration(42)).toBe('42s');
    expect(formatDuration(60)).toBe('1m00s');
    expect(formatDuration(125.4)).toBe('2m05s');
  });
});

describe('summarize: one line per story and a total', () => {
  it('marks passes, failures with their exit code, and stories that never ran', () => {
    const out = summarize([
      { preset: 'hub-vscode-clone', code: 0, seconds: 125 },
      { preset: 'hub-browser', code: 1, seconds: 100 },
      { preset: 'vscode-share', code: undefined, seconds: 0 },
    ]);
    expect(out).toMatch(/pass\s+hub-vscode-clone\s+2m05s/);
    expect(out).toMatch(/FAIL\s+hub-browser\s+1m40s\s+\(exit 1\)/);
    expect(out).toMatch(/skip\s+vscode-share/);
    expect(out).toMatch(/total\s+3m45s/);
  });
});
