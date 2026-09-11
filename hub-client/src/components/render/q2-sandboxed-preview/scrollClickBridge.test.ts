/**
 * @vitest-environment jsdom
 *
 * Tests for the iframe-side scroll/click bridge that lives in the
 * quarto-hub-sandboxed-preview project. In q2-preview the parent reads
 * `contentDocument`/`contentWindow` directly; cross-origin, the same
 * measurements happen inside the iframe and travel as postMessage.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import {
  installScrollClickBridge,
  scrollDocumentToLine,
  currentScrollRatio,
} from '../../../../quarto-hub-sandboxed-preview/src/scrollClickBridge';

function setBody(html: string) {
  document.body.innerHTML = html;
}

describe('currentScrollRatio', () => {
  it('returns 0 for a document too short to scroll', () => {
    // jsdom: scrollHeight 0, innerHeight 768 → maxScroll < 0.
    expect(currentScrollRatio(window, document)).toBe(0);
  });
});

describe('installScrollClickBridge', () => {
  beforeEach(() => {
    setBody('');
  });

  it('posts PREVIEW_SCROLLED with the current ratio on window scroll', () => {
    const post = vi.fn();
    const uninstall = installScrollClickBridge(window, document, post);
    window.dispatchEvent(new Event('scroll'));
    expect(post).toHaveBeenCalledWith({ type: 'PREVIEW_SCROLLED', ratio: 0 });
    uninstall();
    post.mockClear();
    window.dispatchEvent(new Event('scroll'));
    expect(post).not.toHaveBeenCalled();
  });

  it('posts CLICK_AT_LINE with the resolved line and block top on pointerup over a located block', () => {
    setBody('<p data-loc="0:12:1-14:10">hello</p>');
    const block = document.querySelector('p')!;
    vi.spyOn(block, 'getBoundingClientRect').mockReturnValue({ top: 123 } as DOMRect);
    const post = vi.fn();
    const uninstall = installScrollClickBridge(window, document, post);
    block.dispatchEvent(new Event('pointerup', { bubbles: true }));
    expect(post).toHaveBeenCalledWith({ type: 'CLICK_AT_LINE', line: 12, iframeY: 123 });
    uninstall();
  });

  it('stays silent on pointerup with no located ancestor', () => {
    setBody('<p>unlocated</p>');
    const post = vi.fn();
    const uninstall = installScrollClickBridge(window, document, post);
    document.querySelector('p')!.dispatchEvent(new Event('pointerup', { bubbles: true }));
    expect(post).not.toHaveBeenCalled();
    uninstall();
  });

  it('stays silent for included-file content (fileId != 0)', () => {
    setBody('<p data-loc="2:5:1-6:1">included</p>');
    const post = vi.fn();
    const uninstall = installScrollClickBridge(window, document, post);
    document.querySelector('p')!.dispatchEvent(new Event('pointerup', { bubbles: true }));
    expect(post).not.toHaveBeenCalled();
    uninstall();
  });
});

describe('scrollDocumentToLine', () => {
  it('scrolls the best-matching located element into view when off-screen', () => {
    setBody('<p data-loc="0:1:1-3:1">a</p><p data-loc="0:10:1-12:1">b</p>');
    const target = document.querySelectorAll('p')[1]!;
    // jsdom reports rect 0/0 for everything, which counts as "visible"
    // (top >= 0 && bottom <= innerHeight) — push it off-screen.
    vi.spyOn(target, 'getBoundingClientRect').mockReturnValue({ top: -50, bottom: -10 } as DOMRect);
    const scrolled = vi.fn();
    (target as HTMLElement).scrollIntoView = scrolled;
    scrollDocumentToLine(document, window, 11);
    expect(scrolled).toHaveBeenCalledWith({ behavior: 'smooth', block: 'center' });
  });

  it('no-ops when no element is located', () => {
    setBody('<p>nothing located</p>');
    expect(() => scrollDocumentToLine(document, window, 5)).not.toThrow();
  });
});
