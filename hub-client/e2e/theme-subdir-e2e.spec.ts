/**
 * Theme + Custom SCSS E2E Tests
 *
 * Tests theme rendering through the full Automerge pipeline with various
 * project structures. Motivated by a bug where chapters/chapter2.qmd with
 * theme: [vapor, custom.scss] shows no theme at all in interactive hub use.
 */

import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  createProjectOnServer,
  seedProjectInBrowser,
  getServerUrl,
  type ProjectFile,
} from './helpers/projectFactory';
import {
  waitForPreviewRender,
  getPreviewCss,

  getRenderDiagnostics,
} from './helpers/previewExtraction';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Navigate to a project file and wait for render. */
async function loadProjectFile(
  page: import('@playwright/test').Page,
  serverUrl: string,
  files: ProjectFile[],
  targetFile: string,
) {
  const indexDocId = await createProjectOnServer(serverUrl, files);
  await page.goto('/');
  await expect(page.locator('body')).toBeVisible();
  const localId = await seedProjectInBrowser(page, indexDocId, serverUrl);
  await page.goto(`/#/project/${localId}/file/${encodeURIComponent(targetFile)}`);
  await waitForPreviewRender(page, { timeout: 60000 });
  return localId;
}

// ---------------------------------------------------------------------------
// Test: Reproduce the exact interactive project structure
// ---------------------------------------------------------------------------

test.describe('Theme with metadata layers (hub-metadata-test pattern)', () => {
  // This replicates ~/docs/hub-metadata-test exactly:
  // - _quarto.yml: project-level theme (darkly)
  // - chapters/_metadata.yml: directory-level theme (flatly) + author
  // - chapters/chapter2.qmd: document-level theme override [vapor, custom.scss]
  // - chapters/custom.scss: custom SCSS in the subdirectory

  const metadataTestFiles: ProjectFile[] = [
    {
      path: '_quarto.yml',
      content: 'title: "Test Project"\nformat:\n  html:\n    theme: darkly\n',
      contentType: 'text',
    },
    {
      path: 'custom1.scss',
      content: '/*-- scss:defaults --*/\n$body-bg: #274871ff;',
      contentType: 'text',
    },
    {
      path: 'index.qmd',
      content: [
        '---',
        'title: Hello',
        'theme:',
        '  - vapor',
        '  - ./custom1.scss',
        '---',
        '',
        '# Welcome',
        '',
        'Some paragraph text here.',
      ].join('\n'),
      contentType: 'text',
    },
    {
      path: 'chapters/_metadata.yml',
      content: 'author: "Metadata Author"\ntheme: flatly\n',
      contentType: 'text',
    },
    {
      path: 'chapters/chapter1.qmd',
      content: [
        '---',
        'title: "Chapter One"',
        '---',
        '',
        '## Here we begin',
        'Content in a subdirectory.',
      ].join('\n'),
      contentType: 'text',
    },
    {
      path: 'chapters/chapter2.qmd',
      content: [
        '---',
        'title: Chapter Two',
        'theme:',
        '  - vapor',
        '  - custom.scss',
        '---',
        '',
        '## Here we continue',
        'Content in a subdirectory.',
      ].join('\n'),
      contentType: 'text',
    },
    {
      path: 'chapters/custom.scss',
      content: '/*-- scss:defaults --*/\n$body-bg: #274871ff;',
      contentType: 'text',
    },
  ];

  test('index.qmd with theme [vapor, ./custom1.scss] should render vapor theme', async ({ page }) => {
    const serverUrl = getServerUrl();
    await loadProjectFile(page, serverUrl, metadataTestFiles, 'index.qmd');

    const css = await getPreviewCss(page);
    // Vapor theme color
    expect(css, 'Expected vapor theme color #170229 in CSS').toMatch(/#170229/);
    // Custom SCSS body-bg
    expect(css, 'Expected custom body-bg #274871 in CSS').toMatch(/#274871/);
  });

  test('chapters/chapter1.qmd should inherit flatly theme from _metadata.yml', async ({ page }) => {
    const serverUrl = getServerUrl();
    await loadProjectFile(page, serverUrl, metadataTestFiles, 'chapters/chapter1.qmd');

    const diag = await getRenderDiagnostics(page, 'chapters/chapter1.qmd');
    expect(diag.success, `Render failed: ${diag.error}`).toBe(true);

    const css = await getPreviewCss(page);
    // Flatly uses a distinctive color
    expect(css.length, 'Expected non-trivial CSS from flatly theme').toBeGreaterThan(1000);
  });

  test('chapters/chapter2.qmd with theme [vapor, custom.scss] should render vapor theme', async ({ page }) => {
    const serverUrl = getServerUrl();
    await loadProjectFile(page, serverUrl, metadataTestFiles, 'chapters/chapter2.qmd');

    const diag = await getRenderDiagnostics(page, 'chapters/chapter2.qmd');
    expect(diag.success, `Render failed: ${diag.error}`).toBe(true);

    const css = await getPreviewCss(page);
    // Vapor theme color — this is the key assertion that fails interactively
    expect(css, 'Expected vapor theme color #170229 in CSS').toMatch(/#170229/);
    // Custom SCSS body-bg
    expect(css, 'Expected custom body-bg #274871 in CSS').toMatch(/#274871/);
  });
});

// ---------------------------------------------------------------------------
// Test: Simpler variations to isolate what breaks
// ---------------------------------------------------------------------------

test.describe('Theme custom SCSS in subdirectory (isolated variations)', () => {

  test('subdir qmd + subdir custom.scss, NO _metadata.yml, NO project theme', async ({ page }) => {
    // Simplest possible case: just a subdirectory with a custom theme array
    const serverUrl = getServerUrl();
    await loadProjectFile(page, serverUrl, [
      {
        path: '_quarto.yml',
        content: 'project:\n  type: default\n',
        contentType: 'text',
      },
      {
        path: 'subdir/doc.qmd',
        content: [
          '---',
          'title: Subdir Doc',
          'theme:',
          '  - vapor',
          '  - custom.scss',
          '---',
          '',
          '## Hello',
          'Test content.',
        ].join('\n'),
        contentType: 'text',
      },
      {
        path: 'subdir/custom.scss',
        content: '/*-- scss:rules --*/\n.my-custom-rule { color: #aabbcc; }',
        contentType: 'text',
      },
    ], 'subdir/doc.qmd');

    const css = await getPreviewCss(page);
    expect(css, 'Expected vapor theme color #170229').toMatch(/#170229/);
    expect(css, 'Expected custom rule .my-custom-rule').toMatch(/my-custom-rule/);
  });

  test('subdir qmd + subdir custom.scss, WITH project theme in _quarto.yml', async ({ page }) => {
    // Add a project-level theme — does the override still work?
    const serverUrl = getServerUrl();
    await loadProjectFile(page, serverUrl, [
      {
        path: '_quarto.yml',
        content: 'title: "Test"\nformat:\n  html:\n    theme: darkly\n',
        contentType: 'text',
      },
      {
        path: 'subdir/doc.qmd',
        content: [
          '---',
          'title: Subdir Doc',
          'theme:',
          '  - vapor',
          '  - custom.scss',
          '---',
          '',
          '## Hello',
          'Test content.',
        ].join('\n'),
        contentType: 'text',
      },
      {
        path: 'subdir/custom.scss',
        content: '/*-- scss:rules --*/\n.my-custom-rule { color: #aabbcc; }',
        contentType: 'text',
      },
    ], 'subdir/doc.qmd');

    const css = await getPreviewCss(page);
    expect(css, 'Expected vapor (not darkly) theme color #170229').toMatch(/#170229/);
    expect(css, 'Expected custom rule .my-custom-rule').toMatch(/my-custom-rule/);
  });

  test('subdir qmd + subdir custom.scss, WITH _metadata.yml theme override', async ({ page }) => {
    // Add _metadata.yml in the subdirectory — this is the closest to the bug report
    const serverUrl = getServerUrl();
    await loadProjectFile(page, serverUrl, [
      {
        path: '_quarto.yml',
        content: 'title: "Test"\nformat:\n  html:\n    theme: darkly\n',
        contentType: 'text',
      },
      {
        path: 'chapters/_metadata.yml',
        content: 'author: "Dir Author"\ntheme: flatly\n',
        contentType: 'text',
      },
      {
        path: 'chapters/doc.qmd',
        content: [
          '---',
          'title: Chapter Doc',
          'theme:',
          '  - vapor',
          '  - custom.scss',
          '---',
          '',
          '## Hello',
          'Test content.',
        ].join('\n'),
        contentType: 'text',
      },
      {
        path: 'chapters/custom.scss',
        content: '/*-- scss:rules --*/\n.my-custom-rule { color: #aabbcc; }',
        contentType: 'text',
      },
    ], 'chapters/doc.qmd');

    const css = await getPreviewCss(page);
    expect(css, 'Expected vapor (overriding flatly and darkly) theme color #170229').toMatch(/#170229/);
    expect(css, 'Expected custom rule .my-custom-rule').toMatch(/my-custom-rule/);
  });

  test('subdir qmd + subdir custom.scss with scss:defaults, WITH _metadata.yml', async ({ page }) => {
    // Same as above but custom SCSS uses scss:defaults (like the real test project)
    const serverUrl = getServerUrl();
    await loadProjectFile(page, serverUrl, [
      {
        path: '_quarto.yml',
        content: 'title: "Test"\nformat:\n  html:\n    theme: darkly\n',
        contentType: 'text',
      },
      {
        path: 'chapters/_metadata.yml',
        content: 'author: "Dir Author"\ntheme: flatly\n',
        contentType: 'text',
      },
      {
        path: 'chapters/doc.qmd',
        content: [
          '---',
          'title: Chapter Doc',
          'theme:',
          '  - vapor',
          '  - custom.scss',
          '---',
          '',
          '## Hello',
          'Test content.',
        ].join('\n'),
        contentType: 'text',
      },
      {
        path: 'chapters/custom.scss',
        content: '/*-- scss:defaults --*/\n$body-bg: #274871ff;',
        contentType: 'text',
      },
    ], 'chapters/doc.qmd');

    const css = await getPreviewCss(page);
    expect(css, 'Expected vapor theme color #170229').toMatch(/#170229/);
    expect(css, 'Expected custom body-bg #274871 in CSS').toMatch(/#274871/);
  });
});
