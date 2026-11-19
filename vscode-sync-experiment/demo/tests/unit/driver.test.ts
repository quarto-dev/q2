import { describe, expect, it } from 'vitest';
import { runStory, StoryFailure, type DriverEnv, type Step } from '../../src/driver';

function fakeEnv() {
  const calls: string[] = [];
  const log: any[] = [];
  const env: DriverEnv = {
    caption: (t) => void calls.push(`caption:${t}`),
    type: async (c) => void calls.push(`type:${c}`),
    fail: (t) => void calls.push(`fail:${t}`),
    sleep: async (ms) => void calls.push(`sleep:${ms}`),
    waitKey: async () => void calls.push('key'),
    log: (e) => void log.push(e),
  };
  return { env, calls, log };
}
const story = (checks: (() => Promise<void>)[] = []): Step[] => [
  { id: 's1', caption: 'one', command: 'git switch feature', check: checks[0] ?? (async () => {}) },
  { id: 's2', caption: 'two', action: async () => void 0, check: checks[1] ?? (async () => {}) },
];

describe('driver (DM4)', () => {
  it('auto holds the dwell after each passing check', async () => {
    const { env, calls } = fakeEnv();
    await runStory(story(), env, { pace: 'auto', dwellMs: 1500, fast: false });
    expect(calls).toEqual(['caption:one', 'type:git switch feature', 'sleep:1500', 'caption:two', 'sleep:1500']);
  });

  it('key waits for a key after each passing check', async () => {
    const { env, calls } = fakeEnv();
    await runStory(story(), env, { pace: 'key', dwellMs: 1500, fast: false });
    expect(calls).toEqual(['caption:one', 'type:git switch feature', 'key', 'caption:two', 'key']);
  });

  it('--fast skips captions, dwell, and key wait but still runs every command and check', async () => {
    const { env, calls } = fakeEnv();
    let checked = 0;
    await runStory(story([async () => void checked++, async () => void checked++]), env, { pace: 'auto', dwellMs: 1500, fast: true });
    expect(calls).toEqual(['type:git switch feature']);
    expect(checked).toBe(2);
  });

  it('the first failed check stops the run and writes the failure to the terminal', async () => {
    const { env, calls, log } = fakeEnv();
    const s = story([async () => { throw new Error('guest never swapped'); }]);
    await expect(runStory(s, env, { pace: 'auto', dwellMs: 10, fast: false })).rejects.toBeInstanceOf(StoryFailure);
    expect(calls).toContain('fail:s1: guest never swapped');
    expect(calls.some((c) => c.startsWith('caption:two'))).toBe(false);
    expect(log.at(-1)).toMatchObject({ id: 's1', result: 'fail' });
  });

  it('the step log matches the story: id, caption, command, result, in order', async () => {
    const { env, log } = fakeEnv();
    await runStory(story(), env, { pace: 'auto', dwellMs: 0, fast: true });
    expect(log).toEqual([
      expect.objectContaining({ id: 's1', caption: 'one', command: 'git switch feature', result: 'pass' }),
      expect.objectContaining({ id: 's2', caption: 'two', result: 'pass' }),
    ]);
  });
});
