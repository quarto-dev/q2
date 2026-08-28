/**
 * Viewport matrix specs (Phase 4 of the UI/UX modernization plan):
 * graceful narrow viewports. Small windows and split-screen use must
 * degrade gracefully — no horizontal scroll, no clipped controls — down
 * to 320px, which covers the WCAG 1.4.10 reflow requirement (400% zoom
 * at 1280px).
 *
 * Matrix: 1280 / 900 / 700 / 480 / 320px widths over the editor shell,
 * projects home, and every dialog. The full editor can't boot in the
 * no-server harness (Phase 0's known limit), so the shell is covered by
 * the composed `#/dev/editor-shell*` routes: the real ProjectTopBar,
 * DocumentTopBar, SidebarTabs, and `.editor-main view-mode-*` flex rules
 * with placeholder panes for Monaco/iframe.
 *
 * Layout assertions run in the light theme (geometry is
 * theme-independent).
 */

import { test, expect } from '@playwright/test';
import {
  THEMES,
  bootHarness,
  expectNoHorizontalScroll,
  expectInsideViewport,
} from './helpers/harness';

test.setTimeout(60_000);

const WIDTHS = [1280, 900, 700, 480, 320] as const;
const HEIGHT = 720;

async function bootAt(
  page: Parameters<typeof bootHarness>[0],
  width: number,
  route: string,
  selector: string,
  theme: (typeof THEMES)[number] = 'light',
  height: number = HEIGHT,
): Promise<void> {
  await page.setViewportSize({ width, height });
  await bootHarness(page, route, selector, theme);
}

/* ---- projects home ---- */

for (const width of WIDTHS) {
  test(`projects home reflows without clipping at ${width}px`, async ({ page }) => {
    await bootAt(page, width, 'projects-home', '.projects-home');
    await expectNoHorizontalScroll(page, '.projects-home');
    await expectInsideViewport(
      page,
      page.getByPlaceholder('Search projects…'),
      'search input',
    );
    await expectInsideViewport(
      page,
      page.getByRole('button', { name: 'Connect / Import ▾' }),
      'Connect/Import button',
    );
    await expectInsideViewport(
      page,
      page.getByRole('button', { name: '＋ New ▾' }),
      'New menu button',
    );
    await expectInsideViewport(
      page,
      page.getByRole('button', { name: 'Account: Ada Lovelace' }),
      'avatar button',
    );
  });
}

/* ---- menus at narrow widths ---- */

test('project rows keep their identity at 320px', async ({ page }) => {
  await bootAt(page, 320, 'projects-home', '.projects-home');
  // The nowrap metadata plus row actions once squeezed the name button
  // to zero width. The name must keep a usable floor (8ch + padding).
  const firstName = page.locator('.qh-row-name').first();
  await expect(firstName).toHaveText('Research Paper');
  const box = await firstName.boundingBox();
  expect(box, 'first row name has no layout box').not.toBeNull();
  expect(box!.width, 'row name squeezed to zero').toBeGreaterThan(50);
});

test('projects home New menu stays inside the viewport at 320px', async ({ page }) => {
  await bootAt(page, 320, 'projects-home', '.projects-home');
  await page.getByRole('button', { name: '＋ New ▾' }).click();
  const menu = page.locator('[role="menu"]');
  await expect(menu).toBeVisible();
  await expectInsideViewport(page, menu, 'New menu');
});

test('projects home avatar menu stays inside the viewport at 320px', async ({ page }) => {
  await bootAt(page, 320, 'projects-home', '.projects-home');
  await page.getByRole('button', { name: 'Account: Ada Lovelace' }).click();
  const menu = page.locator('.qh-avatar-menu');
  await expect(menu).toBeVisible();
  await expectInsideViewport(page, menu, 'avatar menu');
});

test('file-row context menu stays inside the viewport at 320px', async ({ page }) => {
  await bootAt(page, 320, 'sidebar', '.sidebar-sections');
  const row = page.getByRole('treeitem', { name: /references\.bib/ });
  await row.focus();
  await page.keyboard.press('Shift+F10');
  const menu = page.locator('[role="menu"][aria-label="Actions for references.bib"]');
  await expect(menu).toBeVisible();
  await expectInsideViewport(page, menu, 'context menu');
});

test('peek popover stays inside the viewport at 320px', async ({ page }) => {
  await bootAt(page, 320, 'projects-home', '.projects-home');
  await page
    .getByRole('button', { name: "Peek — see what's inside Research Paper" })
    .click();
  const peek = page.locator('.qh-peek');
  await expect(peek).toBeVisible();
  await expectInsideViewport(page, peek, 'peek popover');
});

test('row menu submenu stays inside the viewport at 320px', async ({ page }) => {
  await bootAt(page, 320, 'projects-home', '.projects-home');
  await page.getByRole('button', { name: 'Actions for Research Paper' }).click();
  const menu = page.locator('[role="menu"]');
  await expect(menu).toBeVisible();
  // Submenus open on hover (pointer parity); keyboard uses ArrowRight.
  await menu.locator('.qh-submenu-parent', { hasText: 'Move to collection' }).hover();
  const submenu = page.locator('.qh-submenu');
  await expect(submenu).toBeVisible();
  await expectInsideViewport(page, submenu, 'Move to collection submenu');
});

/* ---- dialogs ---- */

const DIALOG_ROUTES: { route: string; selector: string; label: string }[] = [
  { route: 'dialog-new-file', selector: '.new-file-dialog', label: 'new-file dialog' },
  { route: 'dialog-share', selector: '.share-dialog', label: 'share dialog' },
  { route: 'dialog-new-asset', selector: '.new-asset-dialog', label: 'new-asset dialog' },
];

for (const { route, selector, label } of DIALOG_ROUTES) {
  for (const width of WIDTHS) {
    test(`${label} fits inside the viewport at ${width}px`, async ({ page }) => {
      await bootAt(page, width, route, selector);
      await expectNoHorizontalScroll(page);
      await expectInsideViewport(page, page.locator(selector), label);
    });
  }
}

test('new-asset dialog stays reachable in a short viewport (320×400)', async ({ page }) => {
  await bootAt(page, 320, 'dialog-new-asset', '.new-asset-dialog', 'light', 400);
  await expectInsideViewport(page, page.locator('.new-asset-dialog'), 'new-asset dialog');
  // The footer actions must remain reachable — the dialog scrolls its
  // content internally rather than clipping past the viewport bottom.
  await expectInsideViewport(
    page,
    page.locator('.new-asset-dialog .dialog-actions'),
    'dialog actions',
  );
});

test('projects-home form dialog fits inside the viewport at 320px', async ({ page }) => {
  await bootAt(page, 320, 'projects-home', '.projects-home');
  await page.getByRole('button', { name: '＋ New ▾' }).click();
  await page.locator('[role="menu"] [role="menuitem"]').first().click();
  const dialog = page.locator('[role="dialog"]');
  await expect(dialog).toBeVisible();
  await expectInsideViewport(page, dialog, 'New project dialog');
});

/* ---- editor shell ---- */

const SHELL_ROUTES = [
  { route: 'editor-shell', mode: 'both' },
  { route: 'editor-shell-markup', mode: 'markup' },
  { route: 'editor-shell-preview', mode: 'preview' },
] as const;

for (const { route, mode } of SHELL_ROUTES) {
  for (const width of WIDTHS) {
    test(`editor shell (${mode} mode) reflows without clipping at ${width}px`, async ({
      page,
    }) => {
      await bootAt(page, width, route, '.editor-main');
      // .editor-main (overflow: hidden) reports internal flex clipping
      // via scrollWidth; .editor-container never sees it.
      await expectNoHorizontalScroll(page, '.editor-main');
      // .header-left can collapse to zero flex width while its buttons
      // paint on, ending up *under* .header-right — an internal overlap
      // no viewport assertion catches. Its own scrollWidth reports it.
      await expectNoHorizontalScroll(page, '.header-left');
      // Header controls stay reachable (the preview button stays inline
      // at every width — Phase 5 review kept it out of any overflow).
      await expectInsideViewport(
        page,
        page.getByRole('button', { name: 'Fullscreen preview' }),
        'preview button',
      );
      // No pane is clipped past the viewport's right edge. At ≤900px the
      // sidebar is an off-canvas drawer by design (Phase 5) — the drawer
      // specs below own its geometry.
      if (width > 900) {
        await expectInsideViewport(page, page.locator('.sidebar-sections'), 'sidebar');
      }
      await expectInsideViewport(page, page.locator('.editor-pane'), 'editor pane');
      // Split view collapses below 700px (Phase 5): the preview pane is
      // display:none there, not clipped.
      if (width > 700) {
        await expectInsideViewport(page, page.locator('.preview-pane'), 'preview pane');
      }
    });
  }
}

/* ---- sidebar (fixed-width surface) ---- */

test('sidebar rows truncate without internal scroll at 320px', async ({ page }) => {
  await bootAt(page, 320, 'sidebar', '.sidebar-sections');
  await expectNoHorizontalScroll(page, '.sidebar-sections');
});

/* ---- notifications ---- */

for (const width of [480, 320] as const) {
  test(`notifications stay inside the viewport at ${width}px`, async ({ page }) => {
    await bootAt(page, width, 'notifications', '.ephemeral-session-banner');
    await expectInsideViewport(page, page.locator('.toast'), 'toast');
    await expectInsideViewport(
      page,
      page.locator('.update-available-toast'),
      'update toast',
    );
  });
}

/* ---- Phase 5: narrow-viewport layout design ----
   The designed narrow layouts deferred from Phase 4: the sidebar becomes
   an overlay drawer with scrim at ≤900px, split view collapses to the
   editor pane at ≤700px, and the header's secondary actions collapse
   into an overflow menu at ≤700px. */

/* ---- sidebar drawer (≤900px) ---- */

test('sidebar is an off-canvas drawer at 800px, static at 1280px', async ({ page }) => {
  await bootAt(page, 800, 'editor-shell', '.editor-main');
  await expect(page.getByRole('button', { name: 'Toggle sidebar' })).toBeVisible();
  // Off-canvas via transform — present in the DOM, outside the viewport.
  await expect(page.locator('.sidebar-drawer')).not.toBeInViewport();
  await expectNoHorizontalScroll(page, '.editor-main');

  await bootAt(page, 1280, 'editor-shell', '.editor-main');
  // The toggle is permanent chrome (Phase 5 review feedback): visible at
  // every width, and the sidebar starts visible at 1280px.
  const toggle = page.getByRole('button', { name: 'Toggle sidebar' });
  await expect(toggle).toBeVisible();
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  await expect(page.locator('.sidebar-sections')).toBeVisible();
});

test('sidebar toggle is a distinct chip in both states', async ({ page }) => {
  await bootAt(page, 1280, 'editor-shell', '.editor-main');
  const toggle = page.getByRole('button', { name: 'Toggle sidebar' });
  const bare = page.getByRole('button', { name: 'Switch project' });
  const styleOf = (el: HTMLElement) => {
    const cs = getComputedStyle(el);
    return { bg: cs.backgroundColor, border: cs.borderTopColor };
  };
  // The toggle wears the sidebar's own grey tint in both states — never
  // bare/transparent like the title-bar buttons. Open deepens the tint.
  const on = await toggle.evaluate(styleOf);
  const bareStyle = await bare.evaluate(styleOf);
  expect(on.bg).not.toBe(bareStyle.bg);
  expect(on.bg).not.toBe('rgba(0, 0, 0, 0)');
  // The switch-project button is the header's teal one: it exits the
  // editor for the projects view. The toggle stays grey.
  const switchColor = await bare.evaluate((el) => getComputedStyle(el).color);
  expect(switchColor).toBe('rgb(65, 149, 153)'); // --posit-teal
  // Sidebar off: still a visible chip (background + border), just greyer.
  await toggle.click();
  await expect(page.locator('.sidebar-sections')).toBeHidden();
  // The chip's background settles asynchronously after the state flip
  // (attribute change → style recalc → the pointer's hover style under
  // the just-clicked toggle). A single immediate read can land mid-settle
  // and still show the on-state value, so poll until it moves.
  await expect
    .poll(async () => (await toggle.evaluate(styleOf)).bg, { timeout: 2000 })
    .not.toBe(on.bg);
  const off = await toggle.evaluate(styleOf);
  expect(off.bg).not.toBe('rgba(0, 0, 0, 0)');
  expect(off.border).not.toBe('rgba(0, 0, 0, 0)');
  expect(off.bg).not.toBe(on.bg);
});

test('sidebar toggle hides and restores the sidebar at 1280px', async ({ page }) => {
  await bootAt(page, 1280, 'editor-shell', '.editor-main');
  const toggle = page.getByRole('button', { name: 'Toggle sidebar' });
  await toggle.click();
  await expect(page.locator('.sidebar-sections')).toBeHidden();
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expectNoHorizontalScroll(page, '.editor-main');
  await toggle.click();
  await expect(page.locator('.sidebar-sections')).toBeVisible();
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
});

test('sidebar hidden at 1280px stays closed as a drawer at 800px', async ({ page }) => {
  await bootAt(page, 1280, 'editor-shell', '.editor-main');
  const toggle = page.getByRole('button', { name: 'Toggle sidebar' });
  await toggle.click();
  await expect(page.locator('.sidebar-sections')).toBeHidden();
  // Narrowing across the breakpoint must not pop the sidebar back: the
  // drawer opens only when the user asks.
  await page.setViewportSize({ width: 800, height: 720 });
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expect(page.locator('.sidebar-drawer')).not.toBeInViewport();
  await toggle.click();
  await expect(page.locator('.sidebar-drawer')).toBeInViewport();
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
});

test('sidebar drawer opens with scrim and moves focus in', async ({ page }) => {
  await bootAt(page, 800, 'editor-shell', '.editor-main');
  await page.getByRole('button', { name: 'Toggle sidebar' }).click();
  const drawer = page.locator('.sidebar-drawer');
  await expect(drawer).toBeInViewport();
  await expect(page.locator('.drawer-scrim')).toBeVisible();
  // Focus moved into the drawer on open (modal drawer pattern).
  const focusInside = await page.evaluate(() =>
    document.querySelector('.sidebar-drawer')?.contains(document.activeElement),
  );
  expect(focusInside).toBe(true);
});

test('sidebar drawer: Escape closes and focus returns to the toggle', async ({ page }) => {
  await bootAt(page, 800, 'editor-shell', '.editor-main');
  const toggle = page.getByRole('button', { name: 'Toggle sidebar' });
  await toggle.click();
  await expect(page.locator('.drawer-scrim')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.drawer-scrim')).toHaveCount(0);
  await expect(toggle).toBeFocused();
});

test('sidebar drawer closes on scrim click', async ({ page }) => {
  await bootAt(page, 800, 'editor-shell', '.editor-main');
  await page.getByRole('button', { name: 'Toggle sidebar' }).click();
  const scrim = page.locator('.drawer-scrim');
  await expect(scrim).toBeVisible();
  await scrim.click({ position: { x: 400, y: 400 } });
  await expect(scrim).toHaveCount(0);
});

test('sidebar drawer traps Tab within itself while open', async ({ page }) => {
  await bootAt(page, 800, 'editor-shell', '.editor-main');
  await page.getByRole('button', { name: 'Toggle sidebar' }).click();
  const drawer = page.locator('.sidebar-drawer');
  await expect(drawer).toBeInViewport();
  // Tab well past the drawer's focusable count; focus must stay inside.
  for (let i = 0; i < 20; i++) await page.keyboard.press('Tab');
  const focusInside = await page.evaluate(() =>
    document.querySelector('.sidebar-drawer')?.contains(document.activeElement),
  );
  expect(focusInside).toBe(true);
});

/* ---- split-view collapse (≤700px) ---- */

test('split view collapses to the editor pane at 700px', async ({ page }) => {
  await bootAt(page, 700, 'editor-shell', '.editor-main');
  await expect(page.locator('.preview-pane')).toBeHidden();
  await expect(page.locator('.pane-divider')).toBeHidden();
  await expect(page.locator('.editor-pane')).toBeVisible();
  await expectNoHorizontalScroll(page, '.editor-main');
});

test('split view intact at 900px (counter-check)', async ({ page }) => {
  await bootAt(page, 900, 'editor-shell', '.editor-main');
  await expect(page.locator('.preview-pane')).toBeVisible();
  await expect(page.locator('.pane-divider')).toBeVisible();
});

/* ---- smallest-header composition (≤700px, Phase 5 review) ----
   Review feedback: Share + Preview stay inline — the kebab overflow
   menu is retired. (The view toggle was later removed entirely; the
   drag divider owns the editor/preview split.) */

test('header at 700px: inline share + preview, no kebab', async ({ page }) => {
  await bootAt(page, 700, 'editor-shell', '.editor-main');
  await expect(page.locator('.document-top-bar .preview-btn')).toBeVisible();
  // Share lives in the project top bar and stays inline at all widths.
  await expect(page.locator('.project-top-bar .header-share-btn')).toBeVisible();
  await expect(page.getByRole('button', { name: 'More actions' })).toHaveCount(0);
  // The inline actions still fire.
  await page.locator('.project-top-bar .header-share-btn').click();
  await expect(page.getByTestId('header-last-action')).toHaveText('share');
});

test('header at 320px: same composition, nothing clipped', async ({ page }) => {
  await bootAt(page, 320, 'editor-shell', '.editor-main');
  await expect(page.locator('.document-top-bar .preview-btn')).toBeVisible();
  await expect(page.locator('.project-top-bar .header-share-btn')).toBeVisible();
  await expectNoHorizontalScroll(page, '.document-top-bar');
  await expectNoHorizontalScroll(page, '.project-top-bar');
});

test('header actions inline at 1280px (counter-check)', async ({ page }) => {
  await bootAt(page, 1280, 'editor-shell', '.editor-main');
  await expect(page.locator('.document-top-bar .preview-btn')).toBeVisible();
  await expect(page.locator('.project-top-bar .header-share-btn')).toBeVisible();
  await expect(page.getByRole('button', { name: 'More actions' })).toHaveCount(0);
});

/* ---- fullscreen preview at narrow widths (regression: the ≤700px
   split-collapse rule must not hide the fullscreen preview pane) ---- */

for (const width of [700, 320] as const) {
  test(`fullscreen preview shows the preview pane at ${width}px`, async ({ page }) => {
    await bootAt(page, width, 'editor-shell-fullscreen', '.editor-main');
    await expect(page.locator('.preview-pane.fullscreen')).toBeVisible();
    await expectNoHorizontalScroll(page, '.editor-main');
  });
}
