/** In-memory logger and notifier: for the tests and the headless peer. */
import type { Logger, Notifier } from './host';

export class RecordingNotifier implements Notifier {
  readonly messages: { level: string; message: string }[] = [];
  readonly statuses: (string | null)[] = [];
  async info(message: string): Promise<undefined> {
    this.messages.push({ level: 'info', message });
    return undefined;
  }
  async warn(message: string): Promise<undefined> {
    this.messages.push({ level: 'warn', message });
    return undefined;
  }
  async error(message: string): Promise<undefined> {
    this.messages.push({ level: 'error', message });
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
