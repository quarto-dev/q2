/**
 * Keyboard-interaction spec for the sidebar: the file tree (APG treeview),
 * the search-results listbox, the outline panel, and the section
 * accordion. Driven against the `#/dev/sidebar` harness route in the
 * no-server visual config (it needs the DevHarness, not screenshots).
 *
 * Covers: tree roles + roving tabindex, arrow/Home/End navigation,
 * Right/Left expand/collapse/parent, type-ahead, Enter activation,
 * Shift+F10 context menu with focus return, search-results listbox
 * navigation, outline activation + collapse toggles, section accordion
 * semantics, and the visible focus ring on keyboard focus.
 *
 * Phase 2 deliverable of the UI/UX modernization plan.
 */

import { test, expect, type Page } from '@playwright/test';
import { bootHarness } from './helpers/visual';

// bootHarness does two page loads (identity pinning) against a shared dev
// server; under full parallelism the default 30s budget is too tight.
test.setTimeout(60_000);

const TREE = '[role="tree"][aria-label="Files"]';
const LAST_ACTION = 'sidebar-last-action';

function treeitem(page: Page, name: string | RegExp) {
  return page.locator(`${TREE} [role="treeitem"]`, { hasText: name });
}

async function lastAction(page: Page) {
  return page.getByTestId(LAST_ACTION).textContent();
}

test.beforeEach(async ({ page }) => {
  await bootHarness(page, 'sidebar', '.sidebar-sections', 'light');
});

test('file tree: APG roles with a single tab stop on the active file', async ({ page }) => {
  const tree = page.locator(TREE);
  await expect(tree).toBeVisible();
  // FAKE_FILES: folders data/, figures/ (collapsed), then files
  // _quarto.yml, analysis.qmd, index.qmd, references.bib.
  await expect(tree.locator('[role="treeitem"]')).toHaveCount(6);
  // Roving tabindex: exactly one tabbable item — the active file.
  await expect(tree.locator('[role="treeitem"][tabindex="0"]')).toHaveCount(1);
  const active = tree.locator('[role="treeitem"][aria-selected="true"]');
  await expect(active).toHaveCount(1);
  await expect(active).toHaveText(/index\.qmd/);
  await expect(active).toHaveAttribute('tabindex', '0');
  // Both folders expose their (collapsed) expansion state.
  await expect(tree.locator('[role="treeitem"][aria-expanded="false"]')).toHaveCount(2);
});

test('file tree: arrows move focus; Right expands, Left collapses or parents', async ({
  page,
}) => {
  const tree = page.locator(TREE);
  await tree.locator('[role="treeitem"][tabindex="0"]').focus();

  await page.keyboard.press('ArrowUp');
  await expect(treeitem(page, 'analysis.qmd')).toBeFocused();
  await page.keyboard.press('ArrowUp');
  await expect(treeitem(page, '_quarto.yml')).toBeFocused();
  await page.keyboard.press('ArrowUp');
  await expect(treeitem(page, 'figures')).toBeFocused();

  // Right expands the collapsed folder; its child appears.
  await page.keyboard.press('ArrowRight');
  await expect(treeitem(page, 'figures')).toHaveAttribute('aria-expanded', 'true');
  await expect(tree.locator('[role="treeitem"]')).toHaveCount(7);

  // Right again moves into the folder's first child.
  await page.keyboard.press('ArrowRight');
  await expect(treeitem(page, 'plot.png')).toBeFocused();

  // Left from a child returns to the parent folder.
  await page.keyboard.press('ArrowLeft');
  await expect(treeitem(page, 'figures')).toBeFocused();

  // Left on the expanded folder collapses it.
  await page.keyboard.press('ArrowLeft');
  await expect(treeitem(page, 'figures')).toHaveAttribute('aria-expanded', 'false');
  await expect(tree.locator('[role="treeitem"]')).toHaveCount(6);
});

test('file tree: Home/End jump to first/last visible item', async ({ page }) => {
  const tree = page.locator(TREE);
  await tree.locator('[role="treeitem"][tabindex="0"]').focus();
  await page.keyboard.press('Home');
  await expect(treeitem(page, 'data')).toBeFocused();
  await page.keyboard.press('End');
  await expect(treeitem(page, 'references.bib')).toBeFocused();
});

test('file tree: type-ahead focuses the matching item', async ({ page }) => {
  const tree = page.locator(TREE);
  await tree.locator('[role="treeitem"][tabindex="0"]').focus();
  await page.keyboard.press('r');
  await expect(treeitem(page, 'references.bib')).toBeFocused();
  // The harness mocks timers; advance past the type-ahead window so the
  // next keystroke starts a fresh buffer.
  await page.clock.runFor(600);
  await page.keyboard.press('f');
  await expect(treeitem(page, 'figures')).toBeFocused();
});

test('file tree: Enter opens the focused file and moves selection', async ({ page }) => {
  const tree = page.locator(TREE);
  await tree.locator('[role="treeitem"][tabindex="0"]').focus();
  await page.keyboard.press('ArrowUp'); // analysis.qmd
  await page.keyboard.press('Enter');
  expect(await lastAction(page)).toBe('select:analysis.qmd');
  const active = tree.locator('[role="treeitem"][aria-selected="true"]');
  await expect(active).toHaveText(/analysis\.qmd/);
  // The roving tab stop follows the new active file.
  await expect(active).toHaveAttribute('tabindex', '0');
});

test('file tree: Enter on a folder toggles expansion', async ({ page }) => {
  const tree = page.locator(TREE);
  await tree.locator('[role="treeitem"][tabindex="0"]').focus();
  await page.keyboard.press('Home'); // data folder
  await page.keyboard.press('Enter');
  await expect(treeitem(page, 'data')).toHaveAttribute('aria-expanded', 'true');
  await expect(treeitem(page, 'survey.csv')).toBeVisible();
  await page.keyboard.press('Enter');
  await expect(treeitem(page, 'data')).toHaveAttribute('aria-expanded', 'false');
});

test('file tree: Shift+F10 opens the context menu; Escape returns focus to the row', async ({
  page,
}) => {
  const tree = page.locator(TREE);
  await tree.locator('[role="treeitem"][tabindex="0"]').focus();
  await page.keyboard.press('End'); // references.bib
  await page.keyboard.press('Shift+F10');

  const menu = page.locator('[role="menu"][aria-label="Actions for references.bib"]');
  await expect(menu).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('[role="menu"]')).toHaveCount(0);
  await expect(treeitem(page, 'references.bib')).toBeFocused();
});

test('file tree: context menu activates an item and returns focus to the row', async ({
  page,
}) => {
  const tree = page.locator(TREE);
  await tree.locator('[role="treeitem"][tabindex="0"]').focus();
  await page.keyboard.press('Shift+F10'); // menu for index.qmd
  const menu = page.locator('[role="menu"]');
  await expect(menu).toBeVisible();
  await page.keyboard.press('c'); // type-ahead: Copy Link
  await page.keyboard.press('Enter');
  await expect(page.locator('[role="menu"]')).toHaveCount(0);
  expect(await lastAction(page)).toBe('copy:index.qmd');
  await expect(treeitem(page, 'index.qmd')).toBeFocused();
});

test('search results: listbox semantics, arrow navigation, Enter selects', async ({ page }) => {
  const input = page.getByRole('searchbox', { name: 'Search files' });
  await input.fill('qmd');
  await page.clock.runFor(200); // debounce

  const listbox = page.locator('[role="listbox"][aria-label="Search results"]');
  await expect(listbox).toBeVisible();
  const options = listbox.locator('[role="option"]');
  await expect(options).toHaveCount(2); // index.qmd, analysis.qmd

  // ArrowDown from the input moves into the first result.
  await input.press('ArrowDown');
  await expect(options.first()).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(options.nth(1)).toBeFocused();
  await page.keyboard.press('ArrowUp');
  await expect(options.first()).toBeFocused();

  await page.keyboard.press('Enter');
  expect(await lastAction(page)).toBe('select:index.qmd');
});

test('search results: Escape clears the query and returns focus to the input', async ({
  page,
}) => {
  const input = page.getByRole('searchbox', { name: 'Search files' });
  await input.fill('qmd');
  await page.clock.runFor(200);
  await input.press('ArrowDown');
  await expect(page.locator('[role="option"]').first()).toBeFocused();

  await page.keyboard.press('Escape');
  await expect(input).toBeFocused();
  await expect(input).toHaveValue('');
  await expect(page.locator(TREE)).toBeVisible();
});

test('outline: chevrons expose aria-expanded and toggle via keyboard', async ({ page }) => {
  const outline = page.locator('.outline-panel');
  const introToggle = outline.getByRole('button', { name: 'Collapse Introduction' });
  await expect(introToggle).toHaveAttribute('aria-expanded', 'true');
  await expect(outline.getByRole('button', { name: 'Background' })).toBeVisible();

  await introToggle.press('Enter');
  await expect(
    outline.getByRole('button', { name: 'Expand Introduction' }),
  ).toHaveAttribute('aria-expanded', 'false');
  await expect(outline.getByRole('button', { name: 'Background' })).toHaveCount(0);

  await outline.getByRole('button', { name: 'Expand Introduction' }).press('Space');
  await expect(
    outline.getByRole('button', { name: 'Collapse Introduction' }),
  ).toHaveAttribute('aria-expanded', 'true');
  await expect(outline.getByRole('button', { name: 'Background' })).toBeVisible();
});

test('outline: symbol rows activate via Enter', async ({ page }) => {
  const outline = page.locator('.outline-panel');
  await outline.getByRole('button', { name: 'setup' }).press('Enter');
  expect(await lastAction(page)).toBe('symbol:setup');
});

test('sidebar sections: headers control labelled regions, toggle via keyboard', async ({
  page,
}) => {
  const filesHeader = page.getByRole('button', { name: 'FILES' });
  await expect(filesHeader).toHaveAttribute('aria-expanded', 'true');
  const contentId = await filesHeader.getAttribute('aria-controls');
  expect(contentId).toBeTruthy();
  const region = page.locator(`#${contentId}`);
  await expect(region).toHaveAttribute('role', 'region');
  await expect(region).toHaveAttribute('aria-labelledby', await filesHeader.getAttribute('id'));

  await filesHeader.press('Enter');
  await expect(filesHeader).toHaveAttribute('aria-expanded', 'false');
  await expect(page.locator(`#${contentId}`)).toHaveCount(0);

  await filesHeader.press('Space');
  await expect(filesHeader).toHaveAttribute('aria-expanded', 'true');
  await expect(page.locator(`#${contentId}`)).toBeVisible();
});

test('tab order: header buttons, search, one tree stop, then the next section', async ({
  page,
}) => {
  // Keyboard-only walkthrough from the top of the sidebar harness.
  await page.keyboard.press('Tab');
  const filesHeader = page.getByRole('button', { name: 'FILES' });
  await expect(filesHeader).toBeFocused();

  await page.keyboard.press('Tab');
  await expect(page.getByRole('button', { name: 'New file' })).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(page.getByRole('button', { name: 'Upload asset' })).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(
    page.getByRole('button', { name: 'Open printable version in a new tab' }),
  ).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(page.getByRole('searchbox', { name: 'Search files' })).toBeFocused();

  // The whole file tree is a single tab stop (roving tabindex); the row
  // kebab menus are reached via Shift+F10, not Tab.
  await page.keyboard.press('Tab');
  const active = page.locator(`${TREE} [role="treeitem"][tabindex="0"]`);
  await expect(active).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(page.getByRole('button', { name: 'OUTLINE' })).toBeFocused();
});

test('keyboard focus shows a visible ring on section headers and tree rows', async ({
  page,
}) => {
  const outlineStyleOf = (locator: ReturnType<Page['locator']>) =>
    locator.evaluate((el) => {
      const s = getComputedStyle(el);
      return `${s.outlineStyle} ${s.outlineWidth}`;
    });

  await page.keyboard.press('Tab'); // FILES section header
  const filesHeader = page.getByRole('button', { name: 'FILES' });
  await expect(filesHeader).toBeFocused();
  expect(await outlineStyleOf(filesHeader)).not.toBe('none 0px');

  // Tab through to the tree's tab stop.
  for (let i = 0; i < 5; i++) await page.keyboard.press('Tab');
  const active = page.locator(`${TREE} [role="treeitem"][tabindex="0"]`);
  await expect(active).toBeFocused();
  expect(await outlineStyleOf(active)).not.toBe('none 0px');
});
