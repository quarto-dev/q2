/**
 * Cross-page navigation + Bootstrap JS (bd-kw93.14, Phase F.1).
 *
 * Pins three behaviours:
 *
 *   1. Body link clicks to other project pages route through the SPA
 *      (no full-page reload), with browser back/forward walking the
 *      in-SPA history. The artifact-rooted `.html` href emitted by
 *      `LinkRewriteTransform` gets reverse-mapped to the source
 *      `.qmd` by the iframe's link handler.
 *   2. Anchor links (`about.qmd#intro`, after rewrite
 *      `/.quarto/project-artifacts/about.html#intro`) scroll the
 *      iframe to the named heading after the cross-page render
 *      commits.
 *   3. A click on a `.qmd` link to a file the project doesn't have
 *      surfaces the D.4 render-error overlay rather than blanking
 *      the iframe.
 *   4. Bootstrap 5's bundled JS is loaded in the iframe so chrome /
 *      authored `data-bs-toggle` elements actually toggle. F.2
 *      relies on this for the navbar/sidebar/etc. chrome.
 *
 * Multi-page fixture:
 *   index.qmd  → links to about.qmd, about.qmd#intro, posts/first.qmd, missing.qmd,
 *                plus a manually-authored Bootstrap collapse button.
 *   about.qmd  → has an `## Intro {#intro}` section.
 *   posts/first.qmd → just a heading so we can pin the cross-page render.
 */

import { test, expect, type Page } from '@playwright/test';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

const INDEX_QMD = `# Index Home

- [About](about.qmd)
- [About intro](about.qmd#intro)
- [Posts](posts/first.qmd)
- [Missing](missing.qmd)

<button id="bs-toggle" class="btn btn-primary" type="button" data-bs-toggle="collapse" data-bs-target="#bs-collapse">Toggle</button>

<div class="collapse" id="bs-collapse">
The collapsed content lives here.
</div>
`;

// Padded with enough content above #intro that the section starts
// well below the viewport top — otherwise scrollY can stay at 0
// even after a successful scroll because everything fits on screen.
const ABOUT_QMD = `# About

${Array.from({ length: 60 }, (_, i) => `Pre-section paragraph ${i + 1}, ipsum text to push the heading down.`).join('\n\n')}

## Intro {#intro}

Intro section content.

${Array.from({ length: 40 }, (_, i) => `Post-section paragraph ${i + 1}.`).join('\n\n')}

## Other

More content.
`;

const FIRST_POST_QMD = `# First post

First post content.
`;

async function waitForInnerHeading(page: Page, text: string) {
  await page.waitForFunction(
    (expected) => {
      const outer = document.querySelector('iframe');
      const innerDoc = outer?.contentDocument;
      const h1 = innerDoc?.querySelector('h1');
      return h1 != null && h1.textContent === expected;
    },
    text,
    { timeout: 30_000 },
  );
}

async function clickInnerLinkByText(page: Page, text: string) {
  await page.evaluate((label: string) => {
    const outer = document.querySelector('iframe') as HTMLIFrameElement;
    const innerDoc = outer.contentDocument!;
    const link = Array.from(innerDoc.querySelectorAll('a')).find(
      (a) => (a.textContent ?? '').trim() === label,
    );
    if (!link) {
      throw new Error(`No link with text ${JSON.stringify(label)} in inner doc`);
    }
    link.click();
  }, text);
}

let server: PreviewServerHandle;

test.beforeEach(async () => {
  server = await startPreviewServer({
    fixtureFiles: [
      // `_quarto.yml` is what flips the WASM renderer from
      // single-file mode (where `LinkRewriteTransform` is a no-op
      // because there's no `ProjectIndex`) to project mode. Phase
      // F.1's whole link-rewrite path is gated on this file
      // existing — without it, body links emerge from the WASM
      // render unchanged and the iframe link handler never sees
      // the artifact-rooted `.html` form.
      { path: '_quarto.yml', content: 'project:\n  type: website\n' },
      { path: 'index.qmd', content: INDEX_QMD },
      { path: 'about.qmd', content: ABOUT_QMD },
      { path: 'posts/first.qmd', content: FIRST_POST_QMD },
    ],
  });
});

test.afterEach(async () => {
  await server?.stop();
});

test('Bootstrap JS is loaded in the iframe', async ({ page }) => {
  // The CLI auto-appends `?page=index.qmd` to `server.url` when the
  // project has an `index.qmd` at root (Phase D.2), so plain `goto`
  // is enough to land on the index page.
  await page.goto(server.url);
  await waitForInnerHeading(page, 'Index Home');

  // The bundled Bootstrap UMD attaches the `bootstrap` global on
  // `window` once it executes. Module-top inline script in
  // entry.tsx (Phase F.1) runs before any AST renders, so the
  // global must be present by the time the H1 is visible.
  const bootstrapPresent = await page.evaluate(() => {
    const outer = document.querySelector('iframe') as HTMLIFrameElement;
    const w = outer.contentWindow as unknown as { bootstrap?: unknown };
    return typeof w.bootstrap !== 'undefined';
  });
  expect(bootstrapPresent).toBe(true);
});

test('Bootstrap data-bs-toggle="collapse" actually toggles when clicked', async ({ page }) => {
  // The CLI auto-appends `?page=index.qmd` to `server.url` when the
  // project has an `index.qmd` at root (Phase D.2), so plain `goto`
  // is enough to land on the index page.
  await page.goto(server.url);
  await waitForInnerHeading(page, 'Index Home');

  // Wait for the collapse div to render in the inner doc.
  await page.waitForFunction(() => {
    const outer = document.querySelector('iframe') as HTMLIFrameElement;
    return outer.contentDocument!.getElementById('bs-collapse') != null;
  });

  // Initially Bootstrap's `.collapse` class is set (collapsed) and
  // `.show` is NOT.
  const initiallyCollapsed = await page.evaluate(() => {
    const outer = document.querySelector('iframe') as HTMLIFrameElement;
    const div = outer.contentDocument!.getElementById('bs-collapse')!;
    return div.classList.contains('collapse') && !div.classList.contains('show');
  });
  expect(initiallyCollapsed).toBe(true);

  // Click the button. Bootstrap's delegated click handler should
  // toggle the collapse, ending in `.show` on the target div.
  await page.evaluate(() => {
    const outer = document.querySelector('iframe') as HTMLIFrameElement;
    const btn = outer.contentDocument!.getElementById('bs-toggle')!;
    btn.click();
  });

  // Bootstrap's collapse animation transitions through `.collapsing`
  // before settling on `.show`. Wait up to a couple of seconds.
  await page.waitForFunction(
    () => {
      const outer = document.querySelector('iframe') as HTMLIFrameElement;
      const div = outer.contentDocument!.getElementById('bs-collapse')!;
      return div.classList.contains('show');
    },
    null,
    { timeout: 5_000 },
  );
});

test('Body link click switches activeFile and updates the URL', async ({ page }) => {
  // The CLI auto-appends `?page=index.qmd` to `server.url` when the
  // project has an `index.qmd` at root (Phase D.2), so plain `goto`
  // is enough to land on the index page.
  await page.goto(server.url);
  await waitForInnerHeading(page, 'Index Home');

  await clickInnerLinkByText(page, 'About');

  // Cross-page render lands.
  await waitForInnerHeading(page, 'About');

  // pushState updated the URL: ?page=about.qmd.
  expect(page.url()).toContain('page=about.qmd');
});

test('Browser back button restores the previous page', async ({ page }) => {
  // The CLI auto-appends `?page=index.qmd` to `server.url` when the
  // project has an `index.qmd` at root (Phase D.2), so plain `goto`
  // is enough to land on the index page.
  await page.goto(server.url);
  await waitForInnerHeading(page, 'Index Home');

  await clickInnerLinkByText(page, 'About');
  await waitForInnerHeading(page, 'About');

  await page.goBack();
  await waitForInnerHeading(page, 'Index Home');
});

test('Cross-page anchor link scrolls to the named section', async ({ page }) => {
  // The CLI auto-appends `?page=index.qmd` to `server.url` when the
  // project has an `index.qmd` at root (Phase D.2), so plain `goto`
  // is enough to land on the index page.
  await page.goto(server.url);
  await waitForInnerHeading(page, 'Index Home');

  await clickInnerLinkByText(page, 'About intro');
  await waitForInnerHeading(page, 'About');

  // Wait for #intro to land in the DOM (SectionizeTransform wraps
  // `## Intro {#intro}` in `<div id="intro" class="section level2">`).
  await page.waitForFunction(
    () => {
      const outer = document.querySelector('iframe') as HTMLIFrameElement;
      return outer.contentDocument!.getElementById('intro') != null;
    },
    null,
    { timeout: 5_000 },
  );

  // After the new doc commits, the iframe should have scrolled so
  // that #intro is at (or near) the top of the viewport. The scroll
  // happens on `#root` (q2-preview.html's `<div id="root">` carries
  // `height: 100vh; overflow: auto`), so check both window.scrollY
  // and the root element's scrollTop.
  await page.waitForFunction(
    () => {
      const outer = document.querySelector('iframe') as HTMLIFrameElement;
      const innerWin = outer.contentWindow!;
      const innerDoc = outer.contentDocument!;
      const rootScroll = innerDoc.getElementById('root')?.scrollTop ?? 0;
      return innerWin.scrollY > 0 || rootScroll > 0;
    },
    null,
    { timeout: 5_000 },
  );
});

test('Missing-page link surfaces the render-error overlay', async ({ page }) => {
  // The CLI auto-appends `?page=index.qmd` to `server.url` when the
  // project has an `index.qmd` at root (Phase D.2), so plain `goto`
  // is enough to land on the index page.
  await page.goto(server.url);
  await waitForInnerHeading(page, 'Index Home');

  await clickInnerLinkByText(page, 'Missing');

  // PreviewErrorOverlay renders the word "Render Error" (collapsed
  // mode shows the affordance; expanded shows the message). Pin the
  // collapsed-mode "Error" button text appearing in the SPA chrome.
  await page.waitForFunction(
    () => /error/i.test(document.body.textContent ?? ''),
    null,
    { timeout: 5_000 },
  );
});
