/**
 * Theming contract for the invite landing and the editor welcome banner
 * (bd-fxdcxbpq).
 *
 * Two defects shipped on this branch and neither was catchable by the
 * component tests (jsdom does not resolve color-mix, and every capture
 * was light-mode):
 *
 * 1. The payload preview box drew its border, row dividers, thumbnail
 *    frame, ruled texture, and filename chip from the brand *primitive*
 *    `--posit-blue-light-1`. Primitives do not flip with the theme, so
 *    in dark mode they resolved to #D1DBE5 against a #213D4F card —
 *    near-white grid lines instead of hairline dividers.
 * 2. Surfaces were painted with `--bg-subtle`, a translucent legacy
 *    token, against `.claude/rules/hub-client-theme.md`.
 *
 * These specs pin the two invariants that were violated, theme-neutrally:
 * dividers stay subtle against the surface they sit on, and every surface
 * these components paint is fully opaque.
 */

import { test, expect, type Locator } from '@playwright/test';
import { bootHarness, THEMES } from './helpers/harness';

test.setTimeout(60_000);

/**
 * Parse a computed color into 0-255 channels plus alpha.
 *
 * Two formats matter here: `rgb()/rgba()` with 0-255 channels, and the
 * modern `color(srgb r g b[ / a])` with 0-1 floats — which is what
 * Chromium reports for anything resolved from `color-mix()`, i.e. every
 * derived surface on these components.
 */
function parseColor(value: string): { r: number; g: number; b: number; a: number } {
  const nums = value.match(/[\d.]+/g);
  if (!nums || nums.length < 3) throw new Error(`unparseable color: ${value}`);
  const scale = value.includes('color(srgb') ? 255 : 1;
  const [r, g, b] = nums.slice(0, 3).map((n) => Number(n) * scale);
  return { r, g, b, a: nums.length > 3 ? Number(nums[3]) : 1 };
}

/** WCAG relative luminance. */
function luminance(value: string): number {
  const { r, g, b } = parseColor(value);
  const channel = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

/** WCAG contrast ratio between two computed colors. */
function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const styleOf = (locator: Locator, prop: string): Promise<string> =>
  locator.evaluate(
    (el, p) => getComputedStyle(el).getPropertyValue(p),
    prop,
  );

/**
 * A divider is decoration, not content: it must read as a hairline
 * against the surface behind it in every theme. The shipped defect put
 * a near-white line on a dark card (~7:1) — an order of magnitude past
 * this ceiling, which both correct themes clear at ~1.3:1.
 */
const MAX_DIVIDER_CONTRAST = 2.5;

/**
 * A centered card needs to read as raised off the surface behind it.
 * The quarto-hub.com landing derived its page surface as
 * `color-mix(--border-color 14%, --bg-modal)`, which only works in
 * light mode: in dark mode `--border-color` (--posit-blue-dark-1) is
 * *lighter* than `--bg-modal` (--posit-blue-dark-2), so the page came
 * out marginally lighter than the card it framed — elevation inverted,
 * and at 1.06:1 the card was findable only by its border. It now takes
 * the per-theme `--bg-page-recessed` instead.
 *
 * Both halves matter, and the direction is the one that caught the bug:
 * 1.06 clears any sane magnitude floor.
 *
 * Scoped to the landing page on purpose. `.il-wrap` still derives its
 * own surface the border-mix way and so still inverts in dark mode —
 * a merged surface, deliberately left alone here.
 */
const MIN_CARD_SEPARATION = 1.05;

for (const theme of THEMES) {
  test(`landing page (${theme}): the card reads as raised off its page`, async ({ page }) => {
    await bootHarness(page, 'landing', '.ls-card', theme);

    const pageBg = await styleOf(page.locator('.ls-wrap'), 'background-color');
    const cardBg = await styleOf(page.locator('.ls-card'), 'background-color');

    expect(
      luminance(cardBg),
      `the page (${pageBg}) is lighter than the card (${cardBg}) — elevation is inverted`,
    ).toBeGreaterThan(luminance(pageBg));
    expect(
      contrast(cardBg, pageBg),
      `the card (${cardBg}) and its page (${pageBg}) are the same surface`,
    ).toBeGreaterThan(MIN_CARD_SEPARATION);
  });

  test(`invite landing (${theme}): payload dividers stay subtle against the card`, async ({
    page,
  }) => {
    // The collection card is the richer payload: an outer box plus row
    // dividers, so it exercises every derived divider on these components.
    await bootHarness(page, 'invite-landing-collection-signed-in', '.il-card', theme);

    const surface = await styleOf(page.locator('.il-card'), 'background-color');
    const payloadBorder = await styleOf(page.locator('.il-payload'), 'border-top-color');
    const rowDivider = await styleOf(
      page.locator('.il-payload-row').nth(1),
      'border-top-color',
    );
    const footerDivider = await styleOf(page.locator('.il-payload-footer'), 'border-top-color');

    for (const [label, color] of [
      ['payload box', payloadBorder],
      ['payload row', rowDivider],
      ['payload footer', footerDivider],
    ] as const) {
      expect(
        contrast(color, surface),
        `${label} divider (${color}) is too loud on the card surface (${surface})`,
      ).toBeLessThan(MAX_DIVIDER_CONTRAST);
    }
  });

  test(`invite landing (${theme}): every painted surface is opaque`, async ({ page }) => {
    await bootHarness(page, 'invite-landing-collection-signed-in', '.il-card', theme);

    // A surface either paints nothing (alpha 0 — .il-payload deliberately
    // shows the card through) or paints opaquely. A fractional alpha is
    // the composited-tint defect this pins.
    for (const selector of ['.il-wrap', '.il-card', '.il-payload', '.il-explainer']) {
      const bg = await styleOf(page.locator(selector), 'background-color');
      const { a } = parseColor(bg);
      expect(
        a === 0 || a === 1,
        `${selector} paints a translucent surface (${bg})`,
      ).toBe(true);
    }
  });

  // The quarto-hub.com landing page (bd-g0uyp2v1) borrows this family's
  // surfaces, so it inherits the same invariants.
  test(`landing page (${theme}): surfaces opaque, divider subtle`, async ({ page }) => {
    await bootHarness(page, 'landing', '.ls-card', theme);

    const surface = await styleOf(page.locator('.ls-card'), 'background-color');
    for (const selector of ['.ls-wrap', '.ls-card']) {
      const bg = await styleOf(page.locator(selector), 'background-color');
      expect(parseColor(bg).a, `${selector} paints a translucent surface (${bg})`).toBe(1);
    }
    const divider = await styleOf(page.locator('.ls-footnote'), 'border-top-color');
    expect(
      contrast(divider, surface),
      `the footnote divider (${divider}) is too loud on the card (${surface})`,
    ).toBeLessThan(MAX_DIVIDER_CONTRAST);
  });

  test(`welcome banner (${theme}): tint is opaque`, async ({ page }) => {
    await bootHarness(page, 'invite-welcome-banner', '.ewb', theme);

    const bg = await styleOf(page.locator('.ewb'), 'background-color');
    expect(parseColor(bg).a, `banner tint is translucent (${bg})`).toBe(1);
  });
}
