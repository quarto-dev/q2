/**
 * Config-file edit propagation (bd-mrx1, Phase B.4).
 *
 * Pins the two DOM-observable plan acceptance criteria for q2
 * preview's Phase B:
 *
 *   1. Editing `_quarto.yml` (project-level metadata) re-renders
 *      the active page with the new metadata applied.
 *   2. Editing `posts/_metadata.yml` (section-level metadata)
 *      re-renders pages under `posts/` with the new metadata
 *      applied.
 *
 * Mechanism (already covered end-to-end by Phase A + B.1):
 * watcher (with `WatchFilter::PreviewBroad`) → samod sync →
 * SPA `onFileContent` → `vfsAddFile` → `contentTick` bump →
 * render useEffect re-fires → WASM `render_page_for_preview`
 * re-reads VFS state, MetadataMergeStage re-merges, rendered AST
 * picks up the new title/subtitle.
 *
 * The relaxed-contract criterion 3 ("editing an unrelated sibling
 * re-renders the active page only when there's a dep edge")
 * lives in `bd-0mji` — it needs an SPA render-event hook to be
 * testable when the edit doesn't change rendered output.
 *
 * Fixture design.
 *
 *   _quarto.yml             title:    "Initial Title"
 *   posts/_metadata.yml     subtitle: "Initial Subtitle"
 *   posts/post1.qmd         (no frontmatter, no body title)
 *
 * `posts/post1.qmd` is the only `.qmd` — unambiguous active page.
 * With no own title, it inherits `title:` from `_quarto.yml`.
 * With no own subtitle, it inherits `subtitle:` from
 * `posts/_metadata.yml`. Both surface in the rendered title-block:
 * `<h1 class="title">` and `<p class="subtitle">`.
 *
 * Each edit test changes one layer's knob and asserts:
 *   - the new value appears in the inner iframe within 5 s;
 *   - the OTHER layer's knob is unchanged (guards against an
 *     over-eager edit clobbering both layers and trivially passing).
 *
 * Per plan §B.4: if a spec fails, stop and report — most likely
 * indicates `_quarto.yml` or `_metadata.yml` aren't reaching the
 * WASM render via the preview's VFS sync. That would be a real
 * Phase B gap, not something to patch over.
 */

import { test, expect, type Page } from '@playwright/test';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

const INITIAL_TITLE = 'Initial Title';
const EDITED_TITLE = 'Edited Title From Project';

const INITIAL_SUBTITLE = 'Initial Subtitle';
const EDITED_SUBTITLE = 'Edited Subtitle From Section';

function quartoYml(title: string): string {
  return `title: "${title}"\n`;
}

function metadataYml(subtitle: string): string {
  return `subtitle: "${subtitle}"\n`;
}

const POST_QMD = `Body paragraph for post1; no frontmatter, no body title — both come from the inherited metadata layers.\n`;

/**
 * Wait for the inner iframe's `<h1 class="title">` to read `text`.
 * Same shape as the helpers in `basic-preview.spec.ts` and
 * `include-shortcode.spec.ts`, but matches on the class to
 * disambiguate from any future body H1.
 */
async function waitForTitle(page: Page, text: string) {
  await page.waitForFunction(
    (expected) => {
      const outer = document.querySelector('iframe');
      const innerDoc = outer?.contentDocument;
      const h1 = innerDoc?.querySelector('h1.title');
      return h1 != null && h1.textContent === expected;
    },
    text,
    { timeout: 30_000 },
  );
}

async function waitForSubtitle(page: Page, text: string) {
  await page.waitForFunction(
    (expected) => {
      const outer = document.querySelector('iframe');
      const innerDoc = outer?.contentDocument;
      const p = innerDoc?.querySelector('p.subtitle');
      return p != null && p.textContent === expected;
    },
    text,
    { timeout: 5_000 },
  );
}

/** Read both layers' current rendered values from the iframe. */
async function readTitleBlock(page: Page): Promise<{
  title: string | null;
  subtitle: string | null;
}> {
  return page.evaluate(() => {
    const outer = document.querySelector('iframe');
    const innerDoc = outer?.contentDocument;
    return {
      title: innerDoc?.querySelector('h1.title')?.textContent ?? null,
      subtitle: innerDoc?.querySelector('p.subtitle')?.textContent ?? null,
    };
  });
}

let server: PreviewServerHandle;

test.beforeEach(async () => {
  server = await startPreviewServer({
    fixtureFiles: [
      { path: '_quarto.yml', content: quartoYml(INITIAL_TITLE) },
      { path: 'posts/_metadata.yml', content: metadataYml(INITIAL_SUBTITLE) },
      { path: 'posts/post1.qmd', content: POST_QMD },
    ],
  });
});

test.afterEach(async () => {
  await server?.stop();
});

test('initial render inherits title from _quarto.yml AND subtitle from posts/_metadata.yml', async ({
  page,
}) => {
  await page.goto(server.url);
  await waitForTitle(page, INITIAL_TITLE);

  const { title, subtitle } = await readTitleBlock(page);
  expect(title).toBe(INITIAL_TITLE);
  expect(subtitle).toBe(INITIAL_SUBTITLE);
});

test('editing _quarto.yml title re-renders the active page within 5s', async ({ page }) => {
  await page.goto(server.url);
  await waitForTitle(page, INITIAL_TITLE);

  // Sanity-check the *other* layer's current value so we can assert
  // it's unchanged post-edit.
  const before = await readTitleBlock(page);
  expect(before.subtitle).toBe(INITIAL_SUBTITLE);

  await writeFile(path.join(server.projectDir, '_quarto.yml'), quartoYml(EDITED_TITLE));

  await page.waitForFunction(
    (expected) => {
      const outer = document.querySelector('iframe');
      const innerDoc = outer?.contentDocument;
      const h1 = innerDoc?.querySelector('h1.title');
      return h1 != null && h1.textContent === expected;
    },
    EDITED_TITLE,
    { timeout: 5_000 },
  );

  const after = await readTitleBlock(page);
  expect(after.title).toBe(EDITED_TITLE);
  // Section-level metadata must not have moved.
  expect(after.subtitle).toBe(INITIAL_SUBTITLE);
});

test('editing posts/_metadata.yml subtitle re-renders the active page within 5s', async ({
  page,
}) => {
  await page.goto(server.url);
  await waitForTitle(page, INITIAL_TITLE);

  const before = await readTitleBlock(page);
  expect(before.title).toBe(INITIAL_TITLE);

  await writeFile(
    path.join(server.projectDir, 'posts', '_metadata.yml'),
    metadataYml(EDITED_SUBTITLE),
  );

  await waitForSubtitle(page, EDITED_SUBTITLE);

  const after = await readTitleBlock(page);
  expect(after.subtitle).toBe(EDITED_SUBTITLE);
  // Project-level metadata must not have moved.
  expect(after.title).toBe(INITIAL_TITLE);
});
