/**
 * Reads the facts `judgePreviewStyle` needs out of a preview iframe, and keeps what a page said and fetched from the
 * moment it opened, so a bad first render leaves evidence. The playwright-facing parts are thin; the filtering of
 * network traffic is pure (`worthRecording`).
 */
import type { Frame, Page } from '@playwright/test';
import type { PreviewFacts } from './preview-style';

export interface NetEntry {
  time: string;
  url: string;
  status: number | null;
  type: string;
  failure?: string;
}
export interface ConsoleEntry {
  time: string;
  type: string;
  text: string;
}

const MAX = 500;

/** A request is evidence when it failed, answered outside 2xx (a 304 is a cache hit), or fetched a stylesheet. */
export function worthRecording(status: number | null, type: string, failure?: string): boolean {
  if (failure) return true;
  if (type === 'stylesheet') return true;
  return status !== null && (status < 200 || status >= 300) && status !== 304;
}

/** The console and network of one page, from the moment it is attached (attach before the first `goto`). */
export class PageLog {
  readonly console: ConsoleEntry[] = [];
  readonly net: NetEntry[] = [];

  constructor(page: Page) {
    const now = () => new Date().toISOString();
    const push = <T,>(a: T[], e: T) => void (a.length < MAX && a.push(e));
    page.on('console', (m) => push(this.console, { time: now(), type: m.type(), text: m.text().slice(0, 1000) }));
    page.on('pageerror', (e) => push(this.console, { time: now(), type: 'pageerror', text: String(e).slice(0, 1000) }));
    page.on('response', (r) => {
      const type = r.request().resourceType();
      if (worthRecording(r.status(), type)) push(this.net, { time: now(), url: r.url().slice(0, 300), status: r.status(), type });
    });
    page.on('requestfailed', (r) => push(this.net, { time: now(), url: r.url().slice(0, 300), status: null, type: r.resourceType(), failure: r.failure()?.errorText }));
  }
}

/**
 * The facts of one frame, or null when it is not the visible preview (the main page, an unmounted frame, or the hidden
 * buffer of the double-buffered iframe: only what the person sees is judged).
 */
export async function collectPreviewFacts(frame: Frame): Promise<PreviewFacts | null> {
  if (frame.parentFrame() === null) return null;
  const el = await frame.frameElement().catch(() => null);
  if (!el || !(await el.isVisible().catch(() => false))) return null;
  return frame
    .evaluate(() => {
      const body = document.body;
      if (!body) return null;
      const cs = getComputedStyle(body);
      const h1 = document.querySelector('h1');
      const sheets = Array.from(document.styleSheets).map((s) => {
        let rules: number | null = null;
        try {
          rules = s.cssRules.length;
        } catch {
          /* cross-origin or not loaded */
        }
        return { href: s.href, rules };
      });
      return {
        hasContent: (body.innerText ?? '').trim() !== '',
        bodyFontFamily: cs.fontFamily,
        bodyFontSizePx: parseFloat(cs.fontSize),
        bodyMarginPx: parseFloat(cs.marginTop),
        h1FontSizePx: h1 ? parseFloat(getComputedStyle(h1).fontSize) : null,
        sheets,
      };
    })
    .catch(() => null);
}

/** What the iframe's head holds (the links and style elements a theme should have produced), size-capped. */
export async function frameHead(frame: Frame): Promise<string> {
  return frame.evaluate(() => document.head?.outerHTML.slice(0, 60_000) ?? '').catch((e) => `unreadable: ${e}`);
}
