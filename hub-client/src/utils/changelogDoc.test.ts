/**
 * Tests for the changelog/more-info iframe theming (GH #624).
 *
 * The About tab renders markdown into an iframe whose document sees none
 * of the app's theme classes or CSS variables, and whose canvas lets the
 * modal background show through. The injected styles must therefore set
 * theme-appropriate colors: hardcoded light colors rendered the changelog
 * near-invisible (1.3:1) on the dark modal.
 *
 * These tests pin the actual requirement — WCAG AA contrast (4.5:1) for
 * text and links against the modal background of each theme — rather than
 * specific hex values.
 */

import { describe, it, expect } from 'vitest';
import { changelogStylesForTheme, injectChangelogStyles } from './changelogDoc';

// -- WCAG contrast helpers ---------------------------------------------------

function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const f = (c: number) => (c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4));
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
}

function contrastRatio(a: string, b: string): number {
  let [la, lb] = [luminance(a), luminance(b)];
  if (la < lb) [la, lb] = [lb, la];
  return (la + 0.05) / (lb + 0.05);
}

/** Pull a `color`/`background` declaration out of a `selector { ... }` block. */
function declared(css: string, selector: string, prop: string): string {
  const block = css.match(new RegExp(`${selector.replace(/[.*]/g, '\\$&')}\\s*{([^}]*)}`));
  expect(block, `no rule for ${selector}`).toBeTruthy();
  const decl = block![1].match(new RegExp(`${prop}:\\s*([^;]+)`));
  expect(decl, `no ${prop} in ${selector} rule`).toBeTruthy();
  return decl![1].trim();
}

// Modal backgrounds from theme.css: light --bg-modal, dark --bg-modal
// (--posit-blue-dark-2). The iframe canvas shows the modal through, and the
// injected styles also set the body background to the same value.
const LIGHT_MODAL_BG = '#ffffff';
const DARK_MODAL_BG = '#213d4f';

describe('changelogStylesForTheme', () => {
  it('declares color-scheme so UA painting (scrollbars, canvas) matches', () => {
    expect(changelogStylesForTheme('dark')).toContain('color-scheme: dark');
    expect(changelogStylesForTheme('light')).toContain('color-scheme: light');
  });

  it('dark theme: body text meets AA contrast on the dark modal background', () => {
    const css = changelogStylesForTheme('dark');
    const text = declared(css, 'body', 'color');
    expect(contrastRatio(text, DARK_MODAL_BG)).toBeGreaterThanOrEqual(4.5);
  });

  it('dark theme: links meet AA contrast on the dark modal background', () => {
    const css = changelogStylesForTheme('dark');
    const link = declared(css, 'a', 'color');
    expect(contrastRatio(link, DARK_MODAL_BG)).toBeGreaterThanOrEqual(4.5);
  });

  it('dark theme: code chips keep AA contrast on their own background', () => {
    const css = changelogStylesForTheme('dark');
    const codeBg = declared(css, 'code', 'background');
    const text = declared(css, 'body', 'color'); // code inherits body text color
    expect(contrastRatio(text, codeBg)).toBeGreaterThanOrEqual(4.5);
  });

  it('dark theme: body background matches the dark modal (seamless iframe)', () => {
    const css = changelogStylesForTheme('dark');
    expect(declared(css, 'body', 'background').toLowerCase()).toBe(DARK_MODAL_BG);
  });

  it('light theme: body text and links meet AA contrast on the light modal', () => {
    const css = changelogStylesForTheme('light');
    expect(contrastRatio(declared(css, 'body', 'color'), LIGHT_MODAL_BG)).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(declared(css, 'a', 'color'), LIGHT_MODAL_BG)).toBeGreaterThanOrEqual(4.5);
    expect(declared(css, 'body', 'background').toLowerCase()).toBe(LIGHT_MODAL_BG);
  });

  it('themes differ (a theme flip must restyle the iframe)', () => {
    expect(changelogStylesForTheme('dark')).not.toBe(changelogStylesForTheme('light'));
  });
});

describe('injectChangelogStyles', () => {
  const html = '<!DOCTYPE html>\n<html>\n<head>\n<meta charset="utf-8">\n</head>\n<body><p>x</p></body>\n</html>';

  it('injects the themed styles before </head>', () => {
    const out = injectChangelogStyles(html, 'dark');
    const styleIdx = out.indexOf('<style>');
    expect(styleIdx).toBeGreaterThan(-1);
    expect(out.indexOf('</head>')).toBeGreaterThan(styleIdx);
    expect(out).toContain('color-scheme: dark');
    expect(out).toContain('<p>x</p>');
  });

  it('produces theme-specific documents from the same source HTML', () => {
    expect(injectChangelogStyles(html, 'dark')).not.toBe(injectChangelogStyles(html, 'light'));
  });
});
