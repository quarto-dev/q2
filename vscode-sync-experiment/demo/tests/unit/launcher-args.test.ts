import { describe, expect, it } from 'vitest';
import { parseLauncherArgs } from '../../src/launcher-args';

describe('parseLauncherArgs: the launcher command line', () => {
  it('defaults to free play on hub-browser: windows shown, paced, nothing scripted', () => {
    const a = parseLauncherArgs([]);
    expect(a.preset).toBe('hub-browser');
    expect(a.play).toBe(false);
    expect(a.exit).toBe(false);
    expect(a.options).toMatchObject({ fast: false, headless: false, pace: 'auto', dwellMs: 2500, typedHub: false });
  });

  it('--fast shows every window: it means no pacing and every check, not headless', () => {
    const a = parseLauncherArgs(['--fast']);
    expect(a.options.fast).toBe(true);
    expect(a.options.headless).toBe(false);
    expect(a.play).toBe(true); // --fast implies --play
    expect(a.options.typedHub).toBe(true);
  });

  it('--headless hides the browser windows, on its own or with --fast, and does not imply --play', () => {
    expect(parseLauncherArgs(['--headless']).options).toMatchObject({ headless: true, fast: false });
    expect(parseLauncherArgs(['--headless']).play).toBe(false);
    expect(parseLauncherArgs(['--fast', '--headless']).options).toMatchObject({ headless: true, fast: true });
  });

  it('--play scripts the story; --exit and --linger are passed on for the end of the run', () => {
    const a = parseLauncherArgs(['--play', '--exit', '--linger', '1500']);
    expect(a.play).toBe(true);
    expect(a.exit).toBe(true);
    expect(a.linger).toBe('1500');
  });

  it('reads the preset and the numeric and string options', () => {
    const a = parseLauncherArgs(['--preset', 'vscode-share', '--dwell', '3000', '--seed', '7', '--screen', '1', '--captions', 'off', '--narration', 'both', '--root', '/tmp/d']);
    expect(a.preset).toBe('vscode-share');
    expect(a.options).toMatchObject({ dwellMs: 3000, seed: 7, screen: 1, captions: 'off', narration: 'both', root: '/tmp/d' });
  });

  it('rejects a preset that does not exist', () => {
    expect(() => parseLauncherArgs(['--preset', 'nope'])).toThrow(/nope/);
  });

  it('recognizes --help', () => {
    expect(parseLauncherArgs(['--help']).help).toBe(true);
  });
});
