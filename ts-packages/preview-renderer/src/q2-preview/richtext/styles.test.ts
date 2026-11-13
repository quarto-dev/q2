// @vitest-environment jsdom
//
// Edit-chrome colors are overridable CSS custom properties (see the header comment
// in styles.ts). jsdom does not resolve var() reliably, so this pins the contract
// on the injected stylesheet text: every documented variable is READ with a
// fallback, none is ever SET by the sheet (which would shadow a :root override),
// and no toolbar / edit-box rule hard-codes a color outside a var() fallback.
import { describe, it, expect } from 'vitest';
import { ensureRichTextStyles } from './styles';

const VARS = [
  '--q2-edit-toolbar-color',
  '--q2-edit-toolbar-bg',
  '--q2-edit-toolbar-border',
  '--q2-edit-toolbar-shadow',
  '--q2-edit-toolbar-hover-bg',
  '--q2-edit-toolbar-active-bg',
  '--q2-edit-toolbar-separator',
  '--q2-edit-toolbar-notice-color',
  '--q2-edit-toolbar-input-border',
  '--q2-edit-ring',
  '--q2-edit-tint',
];

function sheet(): string {
  ensureRichTextStyles();
  const el = document.head.querySelector('style[data-q2-richtext]');
  expect(el, 'rich-text stylesheet injected').not.toBeNull();
  // Drop comments so the documentation block doesn't count as usage.
  return (el!.textContent ?? '').replace(/\/\*[\s\S]*?\*\//g, '');
}

/** Remove every `var(...)` call (paren-balanced), fallbacks included. */
function stripVars(css: string): string {
  let out = '';
  let i = 0;
  while (i < css.length) {
    if (css.startsWith('var(', i)) {
      let depth = 0;
      for (; i < css.length; i++) {
        if (css[i] === '(') depth++;
        else if (css[i] === ')' && --depth === 0) { i++; break; }
      }
    } else {
      out += css[i++];
    }
  }
  return out;
}

describe('edit-chrome color variables', () => {
  const css = sheet();

  it.each(VARS)('%s is read with a fallback', (name) => {
    expect(css).toMatch(new RegExp(`var\\(${name},\\s*[^)\\s]`));
  });

  it('never sets a --q2-edit-* variable (so :root / theme overrides win)', () => {
    expect(css).not.toMatch(/--q2-edit-[a-z-]+\s*:/);
  });

  it('toolbar and edit-box rules have no hard-coded colors outside var() fallbacks', () => {
    const chromeRules = css
      .split('}')
      .filter((rule) => /\.q2-rt-toolbar|\.q2-rt-tb-|\.q2-rt-link-input|\.ProseMirror\s*\{/.test(rule));
    expect(chromeRules.length).toBeGreaterThan(0);
    for (const rule of chromeRules) {
      expect(stripVars(rule), rule.trim()).not.toMatch(/#[0-9a-fA-F]{3,8}\b|rgba?\(/);
    }
  });
});
