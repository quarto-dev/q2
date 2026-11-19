/** In-memory logger and notifier: for the tests and the headless peer. */
import type { Logger, Notifier } from './host';

export class RecordingNotifier implements Notifier {
  readonly messages: { level: string; message: string; actions: string[] }[] = [];
  /** What the next warnings answer (the action the user clicked); empty means the user ignored it. */
  readonly choices: string[] = [];
  readonly statuses: (string | null)[] = [];
  async info(message: string, ...actions: string[]): Promise<string | undefined> {
    this.messages.push({ level: 'info', message, actions });
    return undefined;
  }
  async warn(message: string, ...actions: string[]): Promise<string | undefined> {
    this.messages.push({ level: 'warn', message, actions });
    return this.choices.shift();
  }
  async error(message: string, ...actions: string[]): Promise<string | undefined> {
    this.messages.push({ level: 'error', message, actions });
    return undefined;
  }
  status(text: string | null): void {
    this.statuses.push(text);
  }
}

export class MemoryLog implements Logger {
  readonly lines: string[] = [];
  appendLine(line: string): void {
    this.lines.push(line);
  }
}
