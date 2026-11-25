import { describe, expect, it } from 'vitest';
import { DEFAULT_LINGER_MS, endPlan } from '../../src/ending';

describe('endPlan: what the launcher does when the scripted story has finished', () => {
  it('stays up for free play after a paced story by default, as it always has (Ctrl-C ends it)', () => {
    expect(endPlan({ play: true, fast: false, exit: false })).toEqual({ hold: true, lingerMs: 0 });
  });

  it('--exit ends the run when the story ends, after a short look at the final windows', () => {
    expect(endPlan({ play: true, fast: false, exit: true })).toEqual({ hold: false, lingerMs: DEFAULT_LINGER_MS });
  });

  it('--exit --linger sets how long the windows stay up first; 0 closes them at once', () => {
    expect(endPlan({ play: true, fast: false, exit: true, linger: '12000' })).toEqual({ hold: false, lingerMs: 12000 });
    expect(endPlan({ play: true, fast: false, exit: true, linger: '0' })).toEqual({ hold: false, lingerMs: 0 });
  });

  it('ignores a --linger that is not a non-negative number, and uses the default', () => {
    for (const bad of ['', 'soon', '-5', 'NaN']) {
      expect(endPlan({ play: true, fast: false, exit: true, linger: bad }).lingerMs).toBe(DEFAULT_LINGER_MS);
    }
  });

  it('--fast always ends at once, with no linger, with or without --exit (the tests rely on it)', () => {
    expect(endPlan({ play: true, fast: true, exit: false })).toEqual({ hold: false, lingerMs: 0 });
    expect(endPlan({ play: true, fast: true, exit: true, linger: '9000' })).toEqual({ hold: false, lingerMs: 0 });
  });

  it('free play has no story to finish, so --exit changes nothing there', () => {
    expect(endPlan({ play: false, fast: false, exit: true })).toEqual({ hold: true, lingerMs: 0 });
  });
});
