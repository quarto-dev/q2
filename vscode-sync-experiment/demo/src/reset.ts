/** `demo reset` (DM12): restore the starting state without closing any window. The engine environments stay. */

export interface ResetEnv {
  stopProcesses(): Promise<void>;
  rebuildJournal(): Promise<void>;
  rebuildGuestClone(): Promise<void>;
  /** Starts a fresh hub and returns its (new) project id. */
  startHub(): Promise<string>;
  navigateBrowsers(projectId: string): Promise<void>;
  /** Join session again, accepting the G19 rebind confirm. */
  rejoinVscode(projectId: string): Promise<void>;
  clearObserver(): Promise<void>;
  clearStrip(): Promise<void>;
  announce(text: string): void;
}

export async function resetDemo(env: ResetEnv, p: { hasClone: boolean; hasVscode: boolean }): Promise<void> {
  await env.stopProcesses();
  await env.rebuildJournal();
  if (p.hasClone) await env.rebuildGuestClone();
  const id = await env.startHub();
  await env.navigateBrowsers(id);
  if (p.hasVscode) await env.rejoinVscode(id);
  await env.clearObserver();
  await env.clearStrip();
  env.announce('the journal has been restored to its starting state');
}
