/**
 * Test Project Content Definitions
 *
 * These define the CONTENT of test projects, not their document IDs.
 * Each test run can create fresh documents with new UUIDs, but the
 * content is predictable and can be used for assertions.
 *
 * These definitions are used both for:
 * 1. Generating pre-baked fixtures (regenerate-fixtures.ts)
 * 2. Creating fresh projects during tests that need unique documents
 */

export interface TestFile {
  path: string;
  content: string;
}

export interface TestProject {
  description: string;
  files: TestFile[];
}

/**
 * Basic QMD project for simple E2E tests
 */
export const BASIC_QMD_PROJECT: TestProject = {
  description: 'E2E Test Project',
  files: [
    {
      path: 'index.qmd',
      content: `---
title: "E2E Test Document"
format: html
---

# Hello World

This is a test document for E2E testing.

## Section One

Some content in section one.

## Section Two

Some content in section two.
`,
    },
    {
      path: '_quarto.yml',
      content: `project:
  type: default
`,
    },
  ],
};

/**
 * Project with SCSS for testing SCSS compilation and caching
 */
export const SCSS_TEST_PROJECT: TestProject = {
  description: 'SCSS Cache Test Project',
  files: [
    {
      path: 'index.qmd',
      content: `---
title: "SCSS Test"
format:
  html:
    css: styles.scss
---

# Styled Content

This document tests SCSS compilation.

::: {.custom-class}
This should be styled with the custom class.
:::
`,
    },
    {
      path: 'styles.scss',
      content: `$primary-color: #3498db;

.custom-class {
  color: $primary-color;
  font-weight: bold;
  padding: 1rem;
  border-left: 3px solid $primary-color;
}
`,
    },
    {
      path: '_quarto.yml',
      content: `project:
  type: default
`,
    },
  ],
};

/**
 * Multi-file project for testing file navigation and sidebar
 */
export const MULTI_FILE_PROJECT: TestProject = {
  description: 'Multi-File Test Project',
  files: [
    {
      path: 'index.qmd',
      content: `---
title: "Multi-File Project"
format: html
---

# Home

This is the home page. See also:

- [Chapter 1](chapter1.qmd)
- [Chapter 2](chapter2.qmd)
`,
    },
    {
      path: 'chapter1.qmd',
      content: `---
title: "Chapter 1"
format: html
---

# Chapter 1

Content for chapter 1.
`,
    },
    {
      path: 'chapter2.qmd',
      content: `---
title: "Chapter 2"
format: html
---

# Chapter 2

Content for chapter 2.
`,
    },
    {
      path: '_quarto.yml',
      content: `project:
  type: website
`,
    },
  ],
};

/**
 * Empty project template for testing project creation
 */
export const EMPTY_PROJECT: TestProject = {
  description: 'Empty Test Project',
  files: [
    {
      path: '_quarto.yml',
      content: `project:
  type: default
`,
    },
  ],
};

/**
 * All test projects, keyed for easy lookup
 */
export const TEST_PROJECTS = {
  basic: BASIC_QMD_PROJECT,
  scss: SCSS_TEST_PROJECT,
  multiFile: MULTI_FILE_PROJECT,
  empty: EMPTY_PROJECT,
} as const;

export type TestProjectKey = keyof typeof TEST_PROJECTS;
