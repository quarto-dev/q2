/**
 * Phase 7, Defence 2 — namespace-invariant guard.
 *
 * A Monaco theme is global: a rule colours any token whose scope *starts with*
 * the rule's token string. The `qmd.` sentinel super-prefix makes it impossible
 * for a quarto rule to prefix-match a scope another language emits. This test
 * encodes that invariant so a future bare `keyword`/`string` rule fails CI
 * rather than silently recolouring TS/JS/CSS/HTML editor-wide.
 */

import { describe, it, expect } from 'vitest';
import { quartoThemeRules, qmdMonarch } from './quartoTheme';

describe('quartoThemeRules namespace invariant', () => {
  it('every theme rule token carries the qmd. sentinel prefix', () => {
    expect(quartoThemeRules.length).toBeGreaterThan(0);
    for (const rule of quartoThemeRules) {
      expect(
        rule.token.startsWith('qmd.'),
        `theme rule token "${rule.token}" must start with "qmd." — a bare scope would recolour other languages editor-wide`,
      ).toBe(true);
    }
  });

  it('has no duplicate rule tokens', () => {
    const tokens = quartoThemeRules.map((r) => r.token);
    expect(new Set(tokens).size).toBe(tokens.length);
  });
});

describe('qmd Monarch base — bracket symmetry', () => {
  // Regression: a base `content` rule coloured the opening `[` (as `string`)
  // but nothing coloured the closing `]`, so wherever the base shows through
  // (semantic leaves link brackets uncoloured) the two brackets mismatched.
  // The base must treat `[` and `]` identically.
  it('no content rule matches "[" without also matching "]" (or vice versa)', () => {
    const contentRules = qmdMonarch.tokenizer.content as Array<[RegExp, unknown]>;
    for (const rule of contentRules) {
      const re = rule[0];
      expect(re).toBeInstanceOf(RegExp);
      expect(
        re.test('['),
        `rule ${re} treats "[" and "]" asymmetrically — link brackets would mismatch`,
      ).toBe(re.test(']'));
    }
  });
});
