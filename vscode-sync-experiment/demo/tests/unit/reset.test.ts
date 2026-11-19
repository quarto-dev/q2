import { describe, expect, it } from 'vitest';
import { resetDemo, type ResetEnv } from '../../src/reset';

describe('demo reset (DM12)', () => {
  it('stops, rebuilds, keeps the engine environments, restarts, renavigates, and announces, in order', async () => {
    const calls: string[] = [];
    const rec = (n: string) => async () => void calls.push(n);
    const env: ResetEnv = {
      stopProcesses: rec('stop-hub-and-peers'),
      rebuildJournal: rec('rebuild-journal'),
      rebuildGuestClone: rec('rebuild-clone'),
      startHub: async () => (calls.push('start-hub'), 'NEWID'),
      navigateBrowsers: async (id) => void calls.push(`navigate:${id}`),
      rejoinVscode: async (id) => void calls.push(`rejoin:${id}`),
      clearObserver: rec('clear-observer'),
      clearStrip: rec('clear-strip'),
      announce: (t) => void calls.push(`announce:${t}`),
    };
    await resetDemo(env, { hasClone: true, hasVscode: true });
    expect(calls).toEqual([
      'stop-hub-and-peers',
      'rebuild-journal',
      'rebuild-clone',
      'start-hub',
      'navigate:NEWID',
      'rejoin:NEWID',
      'clear-observer',
      'clear-strip',
      'announce:the journal has been restored to its starting state',
    ]);
  });

  it('skips the clone and VS Code steps when the preset has none', async () => {
    const calls: string[] = [];
    const rec = (n: string) => async () => void calls.push(n);
    await resetDemo(
      {
        stopProcesses: rec('stop'), rebuildJournal: rec('journal'), rebuildGuestClone: rec('clone'),
        startHub: async () => 'X', navigateBrowsers: rec('nav'), rejoinVscode: rec('rejoin'),
        clearObserver: rec('obs'), clearStrip: rec('strip'), announce: () => void calls.push('say'),
      },
      { hasClone: false, hasVscode: false },
    );
    expect(calls).toEqual(['stop', 'journal', 'nav', 'obs', 'strip', 'say']);
  });
});
