/** The scripted driver (DM4): caption, type, check, pace; stop at the first failed check. */

export interface Step {
  id: string;
  caption: string;
  /** Typed into a terminal at a human pace (the host's unless `who` says otherwise). */
  command?: string | (() => string);
  who?: 'host' | 'guest';
  /** A non-terminal action (a click in an editor, a command palette entry). */
  action?: () => Promise<void>;
  /** A retrying check; throws when the effect does not appear in time. */
  check: () => Promise<void>;
}

export interface DriverEnv {
  caption(text: string): void;
  type(command: string, who?: 'host' | 'guest'): Promise<void>;
  /** Show a failure in the terminal. */
  fail(text: string): void;
  sleep(ms: number): Promise<void>;
  waitKey(): Promise<void>;
  log(entry: { id: string; caption: string; command?: string; result: 'pass' | 'fail'; error?: string; at: string }): void;
}

export interface DriverOptions {
  pace: 'auto' | 'key';
  dwellMs: number;
  /** No captions, no pacing; every command and check still runs. */
  fast: boolean;
}

export class StoryFailure extends Error {
  constructor(readonly stepId: string, message: string) {
    super(`${stepId}: ${message}`);
  }
}

export async function runStory(steps: Step[], env: DriverEnv, o: DriverOptions): Promise<void> {
  for (const s of steps) {
    const command = typeof s.command === 'function' ? s.command() : s.command;
    const entry = { id: s.id, caption: s.caption, command, at: new Date().toISOString() };
    if (!o.fast) env.caption(s.caption);
    try {
      if (command !== undefined) await env.type(command, s.who ?? 'host');
      if (s.action) await s.action();
      await s.check();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      env.log({ ...entry, result: 'fail', error: msg });
      env.fail(`${s.id}: ${msg}`);
      throw new StoryFailure(s.id, msg);
    }
    env.log({ ...entry, result: 'pass' });
    if (!o.fast) {
      if (o.pace === 'key') await env.waitKey();
      else await env.sleep(o.dwellMs);
    }
  }
}
