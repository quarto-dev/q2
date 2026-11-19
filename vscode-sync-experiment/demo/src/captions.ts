/**
 * Where captions, narration, and warnings go (DM3). Everything here produces display text only: nothing is
 * ever written to the PTY's input.
 */

export const ITALIC_DIM = '\x1b[2;3m';
export const RED_ITALIC = '\x1b[31;3m';
export const RESET = '\x1b[0m';

export type Where = 'inline' | 'strip' | 'both' | 'off';
export interface Options {
  captions: Where;
  narration: Where;
}
export interface Message {
  kind: 'caption' | 'narration' | 'warning';
  text: string;
}
export interface Routed {
  inline?: string;
  strip?: string;
}

export function routeMessage(m: Message, o: Options): Routed {
  const style = m.kind === 'warning' ? RED_ITALIC : ITALIC_DIM;
  const text = `${style}${m.text.replace(/[\r\n]+/g, ' ')}${RESET}`;
  const where = m.kind === 'caption' ? o.captions : o.narration;
  const out: Routed = {};
  if (where === 'inline' || where === 'both') out.inline = text;
  if (where === 'strip' || where === 'both') out.strip = text;
  // A warning is never suppressed: with `off` it goes to the strip.
  if (m.kind === 'warning' && where === 'off') out.strip = text;
  return out;
}

/** The strip's last `max` lines. */
export class StripBuffer {
  private buf: string[] = [];
  constructor(private readonly max: number) {}
  push(line: string): void {
    this.buf.push(line);
    if (this.buf.length > this.max) this.buf.splice(0, this.buf.length - this.max);
  }
  lines(): string[] {
    return [...this.buf];
  }
  clear(): void {
    this.buf = [];
  }
}
