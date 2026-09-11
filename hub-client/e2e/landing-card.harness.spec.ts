/**
 * Layout, type, and alignment contract for the quarto-hub.com landing
 * card (bd-g0uyp2v1).
 *
 * None of this is visible to the component tests — jsdom does not load
 * LoginScreen.css, so alignment, the type scale, and line breaking are
 * all initial values there. It is also the class of detail a later
 * "let's make this consistent" edit flattens, which is why it is pinned
 * here rather than left to the screenshots.
 *
 * Where it can, this spec measures *rendered geometry* — glyph
 * positions, line-box counts, box ratios — rather than the CSS
 * properties that produce them. That survives a change of mechanism
 * (text-align vs align-items vs margin auto) and fails on the thing a
 * reader would actually notice.
 */

import { test, expect, type Page } from '@playwright/test';
import { bootHarness, THEMES } from './helpers/harness';

test.setTimeout(60_000);

/** Sub-pixel slack: layout centering lands within a pixel either way. */
const SLACK = 2;

/**
 * How to reduce a container's contents to the one rectangle worth
 * measuring:
 *
 * - `last-line` — the final line box of wrapped text. The *short* line:
 *   the earlier lines fill the measure and so read as centered whether
 *   they are or not.
 * - `leftmost-line` — the line that starts furthest left. This is what
 *   left-alignment means across a whole block, and unlike `last-line`
 *   it is immune to a trailing inline child: `.ls-what` ends with the
 *   "Learn more" link, whose rect starts mid-line, so the last line's
 *   left gap is ~77px even though every line hangs on the left edge.
 * - `row` — the union of every child box, for a single-line flex row.
 *   A Range over such a row reports a rect per text run and skips
 *   replaced content entirely, so the logo would be left out and the
 *   wordmark alone would look off-center by the logo's width.
 * - an explicit selector — measure that descendant's box.
 */
type Ink = 'last-line' | 'leftmost-line' | 'row' | (string & {});

/** Horizontal gaps between a container's content box and the ink in it. */
async function inkGaps(
  page: Page,
  container: string,
  ink: Ink = 'last-line',
): Promise<{ left: number; right: number }> {
  return page.evaluate(
    ([containerSel, inkMode]) => {
      const el = document.querySelector(containerSel!);
      if (!(el instanceof HTMLElement)) throw new Error(`no element at ${containerSel}`);
      const box = el.getBoundingClientRect();
      const cs = getComputedStyle(el);
      const contentLeft = box.left + parseFloat(cs.paddingLeft);
      const contentRight = box.right - parseFloat(cs.paddingRight);

      let left: number;
      let right: number;
      if (inkMode === 'last-line' || inkMode === 'leftmost-line') {
        const range = document.createRange();
        range.selectNodeContents(el);
        const rects = Array.from(range.getClientRects());
        if (rects.length === 0) throw new Error(`no text rects in ${containerSel}`);
        const rect =
          inkMode === 'last-line'
            ? rects[rects.length - 1]!
            : rects.reduce((a, b) => (b.left < a.left ? b : a));
        [left, right] = [rect.left, rect.right];
      } else if (inkMode === 'row') {
        const boxes = Array.from(el.children).map((c) => c.getBoundingClientRect());
        if (boxes.length === 0) throw new Error(`no child boxes in ${containerSel}`);
        left = Math.min(...boxes.map((b) => b.left));
        right = Math.max(...boxes.map((b) => b.right));
      } else {
        const target = el.querySelector(inkMode!);
        if (!(target instanceof HTMLElement)) {
          throw new Error(`no element at ${containerSel} ${inkMode}`);
        }
        ({ left, right } = target.getBoundingClientRect());
      }

      return { left: left - contentLeft, right: contentRight - right };
    },
    [container, ink] as const,
  );
}

for (const theme of THEMES) {
  test(`landing page (${theme}): header block is centered`, async ({ page }) => {
    await bootHarness(page, 'landing', '.ls-card', theme);

    // The logo + wordmark row and the tagline read as one masthead, so
    // they center together; centering only one of them reads as a
    // mistake rather than a choice.
    const lockup = await inkGaps(page, '.ls-lockup', 'row');
    expect(
      Math.abs(lockup.left - lockup.right),
      `the lockup is off-center (left ${lockup.left}px, right ${lockup.right}px)`,
    ).toBeLessThanOrEqual(SLACK);
    expect(lockup.left, 'the lockup has no room to be centered in').toBeGreaterThan(SLACK);

    const tagline = await inkGaps(page, '.ls-tagline', '.ls-tagline-line:last-child');
    expect(
      Math.abs(tagline.left - tagline.right),
      `the tagline's second line is off-center (left ${tagline.left}px, right ${tagline.right}px)`,
    ).toBeLessThanOrEqual(SLACK);
  });

  test(`landing page (${theme}): the sign-in affordance is centered`, async ({ page }) => {
    // The default state has no status line at all (the button already
    // says "Continue with Google"), so the centering of that slot is
    // asserted on the expiry state, which does render one.
    await bootHarness(page, 'landing-expired', '.ls-card', theme);

    const note = await inkGaps(page, '.ls-note');
    expect(
      Math.abs(note.left - note.right),
      `the status line is off-center (left ${note.left}px, right ${note.right}px)`,
    ).toBeLessThanOrEqual(SLACK);
    expect(note.left, 'the status line has no room to be centered in').toBeGreaterThan(
      SLACK,
    );

    // The button is full-width, so its own gaps prove nothing; what
    // matters is that the row would center a narrower button too.
    const justify = await page
      .locator('.ls-actions')
      .evaluate((el) => getComputedStyle(el).justifyContent);
    expect(justify, 'the CTA row does not center its button').toBe('center');
  });

  test(`landing page (${theme}): the prose blocks stay left-aligned`, async ({ page }) => {
    await bootHarness(page, 'landing', '.ls-card', theme);

    // A left-aligned block has at least one line starting on the
    // content edge; a centered one has none.
    for (const selector of ['.ls-what', '.ls-footnote']) {
      const { left } = await inkGaps(page, selector, 'leftmost-line');
      expect(
        left,
        `no line of ${selector} reaches the left edge (closest is ${left}px) — it reads as centered`,
      ).toBeLessThanOrEqual(SLACK);
    }
  });

  test(`landing page (${theme}): the headline sits on the type scale and does not wrap`, async ({
    page,
  }) => {
    await bootHarness(page, 'landing', '.ls-card', theme);

    // theme.css's scale is 13 / 18 / 24. The headline was set at a
    // hand-picked 21px, which is exactly the drift the four-layer token
    // scheme exists to prevent.
    const { fontSize, scaleStep } = await page.locator('.ls-tagline').evaluate((el) => {
      const cs = getComputedStyle(el);
      return {
        fontSize: cs.fontSize,
        scaleStep: cs.getPropertyValue('--text-xl').trim(),
      };
    });
    expect(scaleStep, 'the type scale has no --text-xl step').not.toBe('');
    expect(fontSize, `the headline is off the type scale (${fontSize})`).toBe(scaleStep);

    // Each sentence is its own line by construction; the card has to be
    // wide enough that neither one *also* wraps, or the deliberate
    // two-line break turns into a ragged four-line block.
    for (const nth of [1, 2]) {
      const lines = await page
        .locator(`.ls-tagline .ls-tagline-line:nth-child(${nth})`)
        .evaluate((el) => el.getClientRects().length);
      expect(lines, `headline sentence ${nth} wraps — the card is too narrow`).toBe(1);
    }
  });

  test(`landing page (${theme}): Learn more sits under the description, unwrapped`, async ({
    page,
  }) => {
    await bootHarness(page, 'landing', '.ls-card', theme);

    // The link lives inside the description, on a line of its own by
    // construction rather than by wrapping. Pinning the *mechanism* is
    // the point: as an inline run, whether it landed on the last line
    // or dangled under a full one was decided by the paragraph's
    // character count, so every copy edit re-litigated the layout.
    const { display, gap, lines, linkSize, paraSize } = await page
      .locator('.ls-what')
      .evaluate((el) => {
        const link = el.querySelector('.ls-learn-more')!;
        const range = document.createRange();
        range.selectNodeContents(el.firstChild!);
        const lead = Array.from(range.getClientRects());
        const last = lead[lead.length - 1]!;
        const cs = getComputedStyle(link);
        return {
          display: cs.display,
          gap: link.getBoundingClientRect().top - last.bottom,
          lines: link.getClientRects().length,
          linkSize: cs.fontSize,
          paraSize: getComputedStyle(el).fontSize,
        };
      });

    expect(display, 'the link is an inline run again').toBe('block');
    // A bare wrap at this leading gives ~3px; the deliberate margin adds 4.
    expect(gap, `the link is jammed against the paragraph (${gap}px)`).toBeGreaterThan(5);
    // Its own line means *one* line — the label must not itself wrap.
    expect(lines, 'the link label wraps across lines').toBe(1);
    expect(linkSize, 'the link is not set at the description size').toBe(paraSize);
  });

}
