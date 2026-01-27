# Hub-Client Automated Testing Infrastructure

**Date**: 2026-01-27
**Status**: In Progress
**Epic**: kyoto-b4x
**Author**: Claude Code

---

## Session Notes (2026-01-27)

**Progress Made:**
- Phases 1, 2, and partial Phase 4 complete
- Test count: 46 → 102 tests (5 test files)
- All tests passing, type checking passing

**To Resume:**
1. Continue with Phase 4: Add tests for remaining utilities (if desired)
2. Phase 3: E2E Fixture Infrastructure needs `regenerate-fixtures.ts` script
3. Phase 5: Component integration tests (deferred per plan)
4. Phase 6: E2E tests with Playwright

**Key Files Created:**
- `hub-client/src/test-utils/` - Mock utilities and setup
- `hub-client/src/__mocks__/` - Vitest auto-mocks
- `hub-client/e2e/` - Playwright E2E structure
- `hub-client/vitest.integration.config.ts` - jsdom test config
- `hub-client/playwright.config.ts` - E2E config

**Run Tests With:**
```bash
cd hub-client
npm run test        # Unit tests
npm run test:ci     # Unit + integration tests
npm run test:e2e    # E2E tests (requires WASM build)
```

---

## Overview

This plan outlines a comprehensive testing infrastructure for hub-client, addressing the unique challenges of testing a real-time collaborative web application that depends on:
- An Automerge sync server for data synchronization
- WASM modules for QMD rendering and LSP features
- IndexedDB for persistent storage
- Monaco editor for code editing
- Real-time presence features for collaboration

## Current State

**Existing Testing:**
- Vitest configured with `environment: 'node'`
- Two test files:
  - `src/services/sassCache.test.ts` - cache manager tests using `InMemoryCacheStorage`
  - `src/utils/diffToMonacoEdits.test.ts` - utility function tests
- Test scripts: `test`, `test:watch`, `test:coverage`

**Gaps:**
- No integration tests for React components
- No E2E tests with browser automation
- No tests for sync client integration
- No tests for presence features
- No tests for the full rendering pipeline

## Architecture

### Testing Pyramid

```
        /\          E2E Tests (Playwright)
       /  \         - Full user flows
      /----\        - Real browser, real (mock) server
     /      \
    /--------\      Integration Tests (Vitest + jsdom)
   /          \     - Component testing with mocked services
  /------------\    - Service integration with mock sync server
 /              \
/----------------\  Unit Tests (Vitest)
                    - Pure functions
                    - Isolated service logic
```

### Test Environment Options

| Environment | Use Case | Browser APIs | DOM | Speed |
|-------------|----------|--------------|-----|-------|
| `node` | Pure functions, utilities | No | No | Fast |
| `jsdom` | Component rendering, basic DOM | Partial | Yes | Medium |
| `happy-dom` | Similar to jsdom, faster | Partial | Yes | Medium |
| Playwright | Full E2E, visual testing | Full | Yes | Slow |

## Work Items

### Phase 1: Test Infrastructure Setup

- [x] Create test configuration presets for different test types
- [x] Set up jsdom/happy-dom environment for component tests
- [x] Create test utilities directory structure
- [x] Set up Playwright for E2E tests
- [ ] Add `@automerge/automerge-repo-storage-nodefs` dependency for fixture management
- [ ] Add `@automerge/automerge-repo-sync-server` dependency for local E2E sync server
- [x] Update `.github/workflows/test-suite.yml` to run hub-client unit tests
- [x] Create `.github/workflows/hub-client-e2e.yml` for E2E tests (manual trigger initially)

### Phase 2: Mock Infrastructure (Unit/Integration Tests)

- [x] Create mock sync client (`test-utils/mockSyncClient.ts`)
- [x] Create mock WASM renderer (`test-utils/mockWasm.ts`)
- [x] Create mock userSettings (`__mocks__/userSettings.ts`) - needed by presenceService
- [x] Add `fake-indexeddb` dependency for IndexedDB testing
- [ ] Create mock presence service

### Phase 3: E2E Fixture Infrastructure

- [x] Create `e2e/fixtures/` directory structure
- [ ] Write `regenerate-fixtures.ts` script to create test projects
- [ ] Generate initial fixture set with known document IDs
- [ ] Create `fixture-manifest.json` mapping test names to document IDs
- [x] Write `fixtureSetup.ts` helpers (copy to temp, cleanup)
- [x] Write `syncServer.ts` helpers (start/stop local automerge-sync-server)
- [ ] Check in `automerge-data/` as binary assets

### Phase 4: Unit Tests

- [x] Add `_resetForTesting()` functions to `automergeSync.ts` and `presenceService.ts` (module-level state reset)
- [x] Add tests for `automergeSync.ts` service
- [x] Add tests for `presenceService.ts`
- [x] Add tests for `projectStorage.ts`
- [ ] Add tests for remaining utilities

### Phase 5: Component Integration Tests (Deferred)

> **Note**: Monaco editor testing is deferred until UI patterns are more established.
> Focus first on service-level testing (SCSS cache, sync, etc.) before tackling component tests.

- [ ] Set up React Testing Library
- [ ] Add tests for `ProjectSelector` component
- [ ] Add tests for `FileSidebar` component
- [ ] ~~Add tests for `Editor` component (with mocked Monaco)~~ - deferred

### Phase 6: E2E Tests with Playwright

- [ ] Set up Playwright configuration with fixture lifecycle hooks
- [ ] Add tests for project loading (using fixture with known docId)
- [ ] Add tests for project creation flow (fresh documents)
- [ ] Add tests for file editing flow
- [ ] Add tests for SCSS compilation and caching behavior
- [ ] Add tests for preview rendering

---

## Detailed Design

### 1. Directory Structure

```
hub-client/
├── src/
│   ├── __mocks__/                    # Vitest manual mocks
│   │   ├── @quarto/
│   │   │   └── quarto-sync-client.ts # Mock sync client
│   │   └── wasmRenderer.ts           # Mock WASM renderer
│   ├── test-utils/                   # Shared test utilities
│   │   ├── index.ts                  # Re-exports
│   │   ├── mockSyncClient.ts         # Configurable mock sync client
│   │   ├── mockWasm.ts               # Mock WASM functions
│   │   ├── inMemoryStorage.ts        # In-memory IndexedDB replacement
│   │   ├── renderWithProviders.tsx   # React testing helper
│   │   └── testFixtures.ts           # Common test data (content constants)
│   └── services/
│       ├── automergeSync.ts
│       ├── automergeSync.test.ts     # Unit tests
│       └── ...
├── e2e/                              # Playwright E2E tests
│   ├── fixtures/
│   │   ├── automerge-data/           # Checked-in binary fixtures (git LFS optional)
│   │   │   └── ...                   # Sharded document storage
│   │   ├── fixture-manifest.json     # Maps test names to document IDs
│   │   └── testProjects.ts           # Content definitions (for reference/regeneration)
│   ├── helpers/
│   │   ├── fixtureSetup.ts           # Copy fixtures to temp, cleanup
│   │   └── syncServer.ts             # Start/stop local sync server
│   ├── scripts/
│   │   └── regenerate-fixtures.ts    # Recreate fixtures when schema changes
│   ├── project-creation.spec.ts
│   ├── file-editing.spec.ts
│   └── scss-cache.spec.ts            # SCSS cache behavior tests
├── vitest.config.ts                  # Unit test config
├── vitest.integration.config.ts      # Integration test config
└── playwright.config.ts              # E2E test config
```

### 2. Vitest Configuration Updates

**vitest.config.ts** (unit tests - current):
```typescript
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts'],
    exclude: ['src/**/*.integration.test.ts', 'src/**/*.integration.test.tsx'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'html'],
      include: ['src/**/*.ts', 'src/**/*.tsx'],
      exclude: ['src/**/*.test.ts', 'src/**/*.test.tsx', 'src/vite-env.d.ts'],
    },
  },
});
```

**vitest.integration.config.ts** (integration tests):
```typescript
import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  test: {
    environment: 'jsdom',
    include: ['src/**/*.integration.test.ts', 'src/**/*.integration.test.tsx'],
    setupFiles: ['./src/test-utils/setup.ts'],
    deps: {
      inline: ['@monaco-editor/react'],
    },
  },
});
```

### 3. Mock Sync Client Design

The mock sync client should support:
- Programmatic file operations for test setup
- Callback invocation for testing event handlers
- Connection state simulation
- Error injection for error handling tests

```typescript
// src/test-utils/mockSyncClient.ts

import type { SyncClient, SyncClientCallbacks, FileEntry } from '@quarto/quarto-sync-client';

export interface MockSyncClientOptions {
  initialFiles?: Map<string, { type: 'text'; text: string } | { type: 'binary'; data: Uint8Array; mimeType: string }>;
  connectionDelay?: number;
  failConnection?: boolean;
}

export function createMockSyncClient(
  callbacks: SyncClientCallbacks,
  options: MockSyncClientOptions = {}
): SyncClient & { _simulateRemoteChange: (path: string, content: string) => void } {
  const files = new Map(options.initialFiles || []);
  let connected = false;
  const fileHandles = new Map<string, { documentId: string }>();

  const client: SyncClient & { _simulateRemoteChange: (path: string, content: string) => void } = {
    async connect(syncServerUrl: string, indexDocId: string): Promise<FileEntry[]> {
      if (options.failConnection) {
        const error = new Error('Connection failed');
        callbacks.onError?.(error);
        throw error;
      }

      if (options.connectionDelay) {
        await new Promise(r => setTimeout(r, options.connectionDelay));
      }

      connected = true;
      callbacks.onConnectionChange?.(true);

      const entries: FileEntry[] = [];
      for (const [path, content] of files) {
        const docId = `automerge:test-${path}`;
        fileHandles.set(path, { documentId: docId });
        entries.push({ path, docId });

        if (content.type === 'text') {
          callbacks.onFileAdded(path, content);
        } else {
          callbacks.onFileAdded(path, content);
        }
      }

      callbacks.onFilesChange?.(entries);
      return entries;
    },

    async disconnect(): Promise<void> {
      for (const path of files.keys()) {
        callbacks.onFileRemoved(path);
      }
      connected = false;
      callbacks.onConnectionChange?.(false);
    },

    isConnected: () => connected,

    getFileContent(path: string): string | null {
      const file = files.get(path);
      if (!file || file.type !== 'text') return null;
      return file.text;
    },

    getBinaryFileContent(path: string): { content: Uint8Array; mimeType: string } | null {
      const file = files.get(path);
      if (!file || file.type !== 'binary') return null;
      return { content: file.data, mimeType: file.mimeType };
    },

    updateFileContent(path: string, content: string): void {
      files.set(path, { type: 'text', text: content });
      callbacks.onFileChanged(path, content, []);
    },

    async createFile(path: string, content: string = ''): Promise<void> {
      files.set(path, { type: 'text', text: content });
      const docId = `automerge:test-${path}`;
      fileHandles.set(path, { documentId: docId });
      callbacks.onFileAdded(path, { type: 'text', text: content });
    },

    async createBinaryFile(path: string, content: Uint8Array, mimeType: string) {
      files.set(path, { type: 'binary', data: content, mimeType });
      const docId = `automerge:test-${path}`;
      fileHandles.set(path, { documentId: docId });
      callbacks.onFileAdded(path, { type: 'binary', data: content, mimeType });
      return { docId, path, deduplicated: false };
    },

    deleteFile(path: string): void {
      files.delete(path);
      fileHandles.delete(path);
      callbacks.onFileRemoved(path);
    },

    renameFile(oldPath: string, newPath: string): void {
      const file = files.get(oldPath);
      if (!file) throw new Error(`File not found: ${oldPath}`);
      files.delete(oldPath);
      files.set(newPath, file);
      callbacks.onFileRemoved(oldPath);
      if (file.type === 'text') {
        callbacks.onFileAdded(newPath, file);
      } else {
        callbacks.onFileAdded(newPath, file);
      }
    },

    isFileBinary: (path: string) => files.get(path)?.type === 'binary',
    getFileHandle: (path: string) => fileHandles.get(path) as any,
    getFilePaths: () => Array.from(files.keys()),

    async createNewProject(options) {
      // Implementation for testing project creation
      return { indexDocId: 'test-index', files: [] };
    },

    // Test helper: simulate a remote change
    _simulateRemoteChange(path: string, content: string): void {
      files.set(path, { type: 'text', text: content });
      callbacks.onFileChanged(path, content, []);
    },
  };

  return client;
}
```

### 4. Mock WASM Renderer Design

The mock must match the actual `VfsResponse` return type used by `wasmRenderer.ts`:

```typescript
// src/test-utils/mockWasm.ts

/**
 * VfsResponse matches the actual return type from wasmRenderer.ts
 */
interface VfsResponse {
  success: boolean;
  error?: string;
  files?: string[];
  content?: string;
}

export interface MockWasmOptions {
  renderResult?: string;
  renderError?: Error;
  vfsFiles?: Map<string, string | Uint8Array>;
}

export function createMockWasmRenderer(options: MockWasmOptions = {}) {
  const vfs = new Map<string, string | Uint8Array>(options.vfsFiles || []);
  let initialized = false;

  return {
    async initWasm(): Promise<void> {
      initialized = true;
    },

    isWasmReady: () => initialized,

    // VFS operations return VfsResponse to match actual API
    vfsAddFile(path: string, content: string): VfsResponse {
      vfs.set(path, content);
      return { success: true };
    },

    vfsAddBinaryFile(path: string, content: Uint8Array): VfsResponse {
      vfs.set(path, content);
      return { success: true };
    },

    vfsRemoveFile(path: string): VfsResponse {
      const existed = vfs.has(path);
      vfs.delete(path);
      return { success: existed };
    },

    vfsListFiles(): VfsResponse {
      return { success: true, files: Array.from(vfs.keys()) };
    },

    vfsClear(): VfsResponse {
      vfs.clear();
      return { success: true };
    },

    vfsReadFile(path: string): VfsResponse {
      const content = vfs.get(path);
      if (typeof content === 'string') {
        return { success: true, content };
      }
      return { success: false, error: `File not found: ${path}` };
    },

    // Rendering operations
    async renderQmd(path: string): Promise<{ success: boolean; html?: string; error?: string }> {
      if (options.renderError) {
        return { success: false, error: options.renderError.message };
      }
      return { success: true, html: options.renderResult || `<div>Rendered: ${path}</div>` };
    },

    async renderToHtml(content: string, renderOptions?: any): Promise<{ html: string; success: boolean; error?: string }> {
      if (options.renderError) {
        return { html: '', success: false, error: options.renderError.message };
      }
      return { html: options.renderResult || `<div>Rendered content</div>`, success: true };
    },

    // SASS compilation
    async compileScss(scss: string): Promise<string> {
      return `/* compiled CSS */`;
    },

    async sassAvailable(): Promise<boolean> {
      return true;
    },

    // For test verification
    _getVfs: () => vfs,
  };
}
```

### 4b. Mock UserSettings Design

The `presenceService.ts` imports `getUserIdentity` from `userSettings.ts`. Tests need this mocked:

```typescript
// src/__mocks__/userSettings.ts
import { vi } from 'vitest';
import type { UserSettings } from '../services/storage/types';

const defaultMockIdentity: UserSettings = {
  userId: 'test-user-id',
  userName: 'Test User',
  userColor: '#3498db',
};

let mockIdentity: UserSettings | null = defaultMockIdentity;

export const getUserIdentity = vi.fn(async (): Promise<UserSettings> => {
  if (!mockIdentity) {
    // Return default if not set
    return defaultMockIdentity;
  }
  return mockIdentity;
});

// Test helper to configure the mock
export function _setMockIdentity(identity: UserSettings | null): void {
  mockIdentity = identity;
}

export function _resetMock(): void {
  mockIdentity = defaultMockIdentity;
  getUserIdentity.mockClear();
}
```

### 5. Playwright E2E Test Setup

**playwright.config.ts**:

```typescript
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  // Parallel tests are OK - they use different documents, single sync server handles concurrency
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 4 : undefined, // Parallel in CI, auto in dev
  reporter: 'html',

  // Global setup/teardown for sync server lifecycle
  globalSetup: './e2e/helpers/globalSetup.ts',
  globalTeardown: './e2e/helpers/globalTeardown.ts',

  use: {
    baseURL: 'http://localhost:5173',
    trace: 'on-first-retry',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
  webServer: {
    command: 'npm run dev',
    url: 'http://localhost:5173',
    reuseExistingServer: !process.env.CI,
  },
});
```

**e2e/helpers/globalSetup.ts**:

```typescript
import { setupTestFixtures } from './fixtureSetup';
import { startSyncServer } from './syncServer';

export default async function globalSetup() {
  // Copy fixtures to temp directory
  const tempDir = setupTestFixtures();
  process.env.E2E_FIXTURE_DIR = tempDir;

  // Start single sync server for all tests
  const server = await startSyncServer({
    port: 3030,
    storageDir: `${tempDir}/automerge-data`,
  });
  process.env.E2E_SYNC_SERVER_URL = 'ws://localhost:3030';

  // Store server reference for teardown
  (globalThis as any).__E2E_SYNC_SERVER__ = server;
  (globalThis as any).__E2E_FIXTURE_DIR__ = tempDir;
}
```

**e2e/helpers/globalTeardown.ts**:

```typescript
import { cleanupTestFixtures } from './fixtureSetup';

export default async function globalTeardown() {
  // Stop sync server
  const server = (globalThis as any).__E2E_SYNC_SERVER__;
  if (server) {
    await server.close();
  }

  // Clean up temp fixtures
  const tempDir = (globalThis as any).__E2E_FIXTURE_DIR__;
  if (tempDir) {
    cleanupTestFixtures(tempDir);
  }
}
```

**e2e/fixtures/testProjects.ts** - Content-based fixtures:

```typescript
/**
 * Test project content definitions.
 *
 * These define the CONTENT of test projects, not their document IDs.
 * Each test run creates fresh documents with new UUIDs, but the
 * content is predictable and can be used for assertions.
 */

export const BASIC_QMD_PROJECT = {
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

export const SCSS_TEST_PROJECT = {
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
`,
    },
    {
      path: 'styles.scss',
      content: `$primary-color: #3498db;

.custom-class {
  color: $primary-color;
  font-weight: bold;
}
`,
    },
  ],
};
```

**e2e/project-creation.spec.ts** - Content-based verification:

```typescript
import { test, expect } from '@playwright/test';
import { BASIC_QMD_PROJECT } from './fixtures/testProjects';

test.describe('Project Creation', () => {
  test('should create a new project with specified files', async ({ page }) => {
    await page.goto('/');

    // Click "New Project" button
    await page.getByRole('button', { name: /new project/i }).click();

    // Fill in project details
    await page.getByLabel('Project Name').fill(BASIC_QMD_PROJECT.description);
    await page.getByRole('button', { name: /create/i }).click();

    // Verify project is created - check for file names in sidebar
    await expect(page.getByText('index.qmd')).toBeVisible();
    await expect(page.getByText('_quarto.yml')).toBeVisible();

    // Click on index.qmd to open it
    await page.getByText('index.qmd').click();

    // Verify content is loaded in editor (content-based assertion)
    // Note: We check for content, not document IDs
    await expect(page.locator('.monaco-editor')).toContainText('Hello World');
  });

  test('should render preview with correct content', async ({ page }) => {
    // ... create project first ...

    // Wait for preview to render
    const previewFrame = page.frameLocator('iframe.preview');

    // Content-based assertion on rendered output
    await expect(previewFrame.locator('h1')).toContainText('Hello World');
  });
});
```

**e2e/scss-cache.spec.ts** - Testing SCSS compilation behavior:

```typescript
import { test, expect } from '@playwright/test';
import { SCSS_TEST_PROJECT } from './fixtures/testProjects';

test.describe('SCSS Compilation', () => {
  test('should compile SCSS and apply styles to preview', async ({ page }) => {
    await page.goto('/');

    // Create project with SCSS file
    await page.getByRole('button', { name: /new project/i }).click();
    await page.getByLabel('Project Name').fill(SCSS_TEST_PROJECT.description);
    // ... fill in files ...

    // Wait for preview to render
    const previewFrame = page.frameLocator('iframe.preview');

    // Verify compiled CSS is applied
    const styledElement = previewFrame.locator('.custom-class');
    await expect(styledElement).toHaveCSS('color', 'rgb(52, 152, 219)'); // #3498db
  });

  test('should update preview when SCSS is modified', async ({ page }) => {
    // ... setup project ...

    // Modify SCSS content
    await page.getByText('styles.scss').click();

    // Change the color variable
    // (This tests the re-compilation and cache invalidation)
    await page.locator('.monaco-editor').fill(`
      $primary-color: #e74c3c;
      .custom-class { color: $primary-color; }
    `);

    // Verify preview updates with new color
    const previewFrame = page.frameLocator('iframe.preview');
    const styledElement = previewFrame.locator('.custom-class');
    await expect(styledElement).toHaveCSS('color', 'rgb(231, 76, 60)'); // #e74c3c
  });
});
```

### 6. Package.json Script Updates

```json
{
  "scripts": {
    // Unit tests - no WASM needed (services are mocked)
    "test": "vitest run",
    "test:watch": "vitest",
    "test:coverage": "vitest run --coverage",

    // Integration tests - no WASM needed (jsdom + mocks)
    "test:integration": "vitest run --config vitest.integration.config.ts",
    "test:integration:watch": "vitest --config vitest.integration.config.ts",

    // E2E tests - WASM required (real browser)
    "test:e2e": "npm run build:wasm && playwright test",
    "test:e2e:ui": "npm run build:wasm && playwright test --ui",

    // CI scripts
    "test:ci": "npm run test && npm run test:integration",  // Fast, no WASM
    "test:all": "npm run build:wasm && npm run test && npm run test:integration && npm run test:e2e",

    // Fixture management
    "e2e:regenerate-fixtures": "npx tsx e2e/scripts/regenerate-fixtures.ts"
  },
  "devDependencies": {
    "@playwright/test": "^1.45.0",
    "@testing-library/react": "^16.0.0",
    "@testing-library/jest-dom": "^6.4.0",
    "fake-indexeddb": "^6.0.0",
    "jsdom": "^24.0.0"
  }
}
```

**Note:** The `test:ci` script is designed for the main test-suite.yml workflow - it runs unit and integration tests without requiring the WASM build, keeping CI fast.
```

### 7. Test Setup File

**src/test-utils/setup.ts**:

```typescript
import '@testing-library/jest-dom';
import { vi } from 'vitest';

// Provide IndexedDB in Node.js environment
// fake-indexeddb is a drop-in replacement that works with the 'idb' library
import 'fake-indexeddb/auto';

// Mock crypto.randomUUID for presence service (if needed - modern Node has this)
if (!global.crypto?.randomUUID) {
  global.crypto = {
    ...global.crypto,
    randomUUID: () => 'test-uuid-' + Math.random().toString(36).substr(2, 9),
  } as Crypto;
}

// Mock ResizeObserver (jsdom may provide this, but mock to be safe)
if (!global.ResizeObserver) {
  global.ResizeObserver = vi.fn().mockImplementation(() => ({
    observe: vi.fn(),
    unobserve: vi.fn(),
    disconnect: vi.fn(),
  }));
}

// Mock IntersectionObserver (jsdom may provide this, but mock to be safe)
if (!global.IntersectionObserver) {
  global.IntersectionObserver = vi.fn().mockImplementation(() => ({
    observe: vi.fn(),
    unobserve: vi.fn(),
    disconnect: vi.fn(),
  }));
}
```

---

## Design Decisions (Resolved)

### Decision 1: No Visual Regression Testing

Visual snapshot tests are explicitly excluded from this infrastructure because:
- High sensitivity to browser version changes creates maintenance burden
- Hub-client UI is under active development with frequent visual changes
- Focus should be on behavioral tests (e.g., SCSS compilation cache behavior)

### Decision 2: No Explicit Coverage Goals

Starting from zero coverage, the goal is "more is better." Quantitative targets will be set once the infrastructure is proven and we have baseline measurements.

### Decision 3: No Git LFS for Fixtures (Initially)

The `e2e/fixtures/automerge-data/` directory will be checked in as regular binary files. Git handles small binary files well, and we can migrate to Git LFS later if the fixture size becomes problematic.

### Decision 4: Fixture Versioning Strategy

Fixtures will be versioned to match the Automerge project schema version:

- **Current state**: The `IndexDocument` interface in `@quarto/quarto-automerge-schema` has no version field
- **Fixture version**: Use `0.0.1` (pre-alpha) until schema versioning is added
- **Directory structure**: `e2e/fixtures/automerge-data/` (no version suffix initially)
- **Future**: When schema versioning is added to `IndexDocument`, fixture version should match

The `fixture-manifest.json` will include the schema version:
```json
{
  "schemaVersion": "0.0.1",
  "generatedAt": "2026-01-27T...",
  "projects": {
    "basicProject": { "indexDocId": "automerge:..." },
    "scssProject": { "indexDocId": "automerge:..." }
  }
}
```

When schema changes require fixture regeneration, regenerate in place and update the version.

### Decision 5: Single Sync Server for All Tests

One sync server instance serves the entire test suite execution:

**Rationale:**
- Automerge is designed for concurrent edits; this is ecologically valid testing
- Parallel tests can run against the same server (different documents)
- Enables future "diamond tests" for concurrency paths (a→b→c→d vs a→c→b→d)
- Simpler infrastructure than per-test or per-file servers

**Implementation:**
- Start sync server in `globalSetup` (Playwright)
- Stop sync server in `globalTeardown`
- All tests share the same server instance
- Tests that modify documents should use separate document IDs

### Decision 6: Use Official Automerge Sync Server Package

Use `@automerge/automerge-repo-sync-server` for local E2E testing:
- Official package, well-maintained
- Compatible with `@automerge/automerge-repo` client
- Supports `NodeFSStorageAdapter` for fixture-based testing
- No need for custom implementation

---

## Implementation Considerations

### Challenge: Automerge Sync Server for E2E Tests

#### Analysis: Automerge Storage is Git-Friendly

Investigation of the `NodeFSStorageAdapter` from `@automerge/automerge-repo-storage-nodefs` reveals that automerge storage **can be checked into git** as binary assets:

**Storage Structure:**
```
baseDirectory/
  {docId[0:2]}/           # First 2 chars of document ID (sharding)
    {docId[2:]}/          # Rest of document ID
      snapshot/
        {headsHash}       # Binary file (CBOR-encoded)
      incremental/
        {changeHash}      # Binary file
      syncState/
        {peerId}          # Binary file
```

**Filesystem Operations Used** (from `external-sources/automerge-repo/packages/automerge-repo-storage-nodefs/src/index.ts`):
- `fs.promises.writeFile()` - standard file writes
- `fs.promises.readFile()` - standard file reads
- `fs.promises.mkdir({ recursive: true })` - standard directories
- `rimraf` - recursive delete

**No Filesystem Tricks:**
- No hard links
- No symlinks
- No file locking mechanisms
- Just regular directories and binary files

**Git Compatibility:**
- Files are content-addressable (git handles this natively)
- Binary diffs won't be human-readable, but git tracks them correctly
- Additions/deletions work as expected
- Similar to checking in images, fonts, or other binary assets

#### Proposed Approach: Fixture-Based E2E Testing

Given that automerge storage is git-friendly, we can use **checked-in fixtures** for reproducible E2E tests:

**1. Create Test Fixture Storage Once**

Create a test fixture with known documents (manually or via setup script):
```bash
# scripts/create-e2e-fixtures.ts
# Creates projects with known content and document IDs
```

**2. Check In Fixture Directory**

```
hub-client/
├── e2e/
│   ├── fixtures/
│   │   ├── automerge-data/          # Checked into git as binary assets
│   │   │   ├── 4a/                  # Sharded by docId prefix
│   │   │   │   └── bc123.../
│   │   │   │       └── snapshot/
│   │   │   └── ...
│   │   ├── fixture-manifest.json    # Maps test names to document IDs
│   │   └── testProjects.ts          # Content definitions (for reference)
```

**3. Test Setup: Copy Fixture to Temp Location**

```typescript
// e2e/helpers/fixtureSetup.ts
import { copySync } from 'fs-extra';
import { mkdtempSync } from 'fs';
import { join } from 'path';
import { tmpdir } from 'os';

export function setupTestFixtures(): string {
  const tempDir = mkdtempSync(join(tmpdir(), 'hub-client-e2e-'));
  copySync(
    join(__dirname, '../fixtures/automerge-data'),
    join(tempDir, 'automerge-data')
  );
  return tempDir;
}

export function cleanupTestFixtures(tempDir: string): void {
  rmSync(tempDir, { recursive: true, force: true });
}
```

**4. Run Tests Against Copied Storage**

```typescript
// e2e/project-editing.spec.ts
import { test, expect } from '@playwright/test';
import { setupTestFixtures, cleanupTestFixtures } from './helpers/fixtureSetup';
import fixtureManifest from './fixtures/fixture-manifest.json';

let tempDir: string;

test.beforeAll(async () => {
  tempDir = setupTestFixtures();
  // Start local sync server with this storage
  // Or configure hub-client to use NodeFSStorageAdapter pointing to tempDir
});

test.afterAll(async () => {
  cleanupTestFixtures(tempDir);
});

test('should load existing project with known document ID', async ({ page }) => {
  const { indexDocId } = fixtureManifest.basicProject;

  await page.goto(`/?project=${indexDocId}`);

  // Now we CAN assert on known document IDs!
  await expect(page.getByText('index.qmd')).toBeVisible();
  await expect(page.locator('.monaco-editor')).toContainText('Hello World');
});
```

**5. After Tests: Discard Temp Copy**

The original fixture in git remains unchanged. Tests are reproducible.

#### Benefits of Fixture-Based Approach

| Benefit | Description |
|---------|-------------|
| **Reproducible** | Same document IDs every run |
| **Known State** | Can assert on specific documents |
| **No Network** | No dependency on external sync servers |
| **Fast** | Local filesystem, no network latency |
| **Isolated** | Each test run uses fresh copy |
| **Versioned** | Fixtures evolve with schema changes |

#### Fixture Regeneration

When the Automerge schema changes or test data needs updating:

```bash
# Regenerate fixtures
npm run e2e:regenerate-fixtures

# This script:
# 1. Starts a temporary sync server with fresh storage
# 2. Creates test projects programmatically
# 3. Copies the resulting storage to e2e/fixtures/automerge-data/
# 4. Updates fixture-manifest.json with document IDs
```

#### Alternative: Hybrid Approach

For some tests, fresh document creation may still be appropriate:
- Tests for "create new project" flow (needs to create, not load)
- Tests for edge cases not covered by fixtures
- Tests that intentionally corrupt or modify documents

These can use content-based assertions as a fallback.

#### Local Sync Server for E2E

Run a local automerge-sync-server with the fixture storage:

```bash
# Start local sync server with fixture data
npx automerge-sync-server --port 3030 --storage ./e2e-temp/automerge-data

# Configure tests to use local server
SYNC_SERVER_URL=ws://localhost:3030 npm run test:e2e
```

Benefits:
- No network dependency on external services
- Clean slate each CI run (copy fixture first)
- Faster than remote server
- Known document IDs for assertions

### Challenge: WASM Module in Tests

**For Unit/Integration Tests:**
- Mock the WASM module entirely using the mock described above
- This allows testing the TypeScript layer without WASM complexity

**For E2E Tests:**
- Use the real WASM module since Playwright runs in a real browser
- The WASM build must complete before E2E tests run
- Tests can verify real SCSS compilation, rendering behavior

### Challenge: Module-Level Singleton State

Both `presenceService.ts` and `automergeSync.ts` use module-level singleton state:

```typescript
// presenceService.ts:109
const state: PresenceServiceState = { peerId: crypto.randomUUID(), ... };

// automergeSync.ts:39
let client: SyncClient | null = null;
```

**Problem**: Node.js caches modules, so state persists between tests. Test A's state leaks into Test B.

**Solution**: Add `_resetForTesting()` exports to each service:

```typescript
// In presenceService.ts
export function _resetForTesting(): void {
  state.peerId = crypto.randomUUID();
  state.identity = null;
  state.currentFilePath = null;
  state.currentHandle = null;
  state.remotePresences.clear();
  state.localCursor = null;
  state.localSelection = null;
  // ... reset all state
}

// In automergeSync.ts
export function _resetForTesting(): void {
  client = null;
  onFilesChange = null;
  onFileContent = null;
  onBinaryContent = null;
  onConnectionChange = null;
  onError = null;
}
```

Tests call these in `beforeEach()` to ensure clean state.

### Challenge: Monaco Editor in Component Tests (Deferred)

Monaco is complex to mock. Options:
1. **Full Mock**: Mock the `@monaco-editor/react` module entirely
2. **Minimal Mock**: Create a simple textarea that captures the same events
3. **Skip in Component Tests**: Only test Monaco integration in E2E tests

**Decision**: Monaco component testing is deferred until UI patterns are more established. Focus first on service-level testing (SCSS cache, sync, storage) before tackling Editor component tests. E2E tests will exercise Monaco in a real browser environment.

### Challenge: Test Data Management

**Primary Approach: Checked-In Automerge Fixtures**

The fixture-based approach (described above) is the primary strategy:
- Pre-generated automerge storage checked into git as binary assets
- Known document IDs enable direct assertions
- Copied to temp directory before tests, discarded after
- Reproducible across runs and CI environments

**Secondary Approach: Programmatic Generation**

For tests that need to create fresh documents (e.g., "create project" flow):
- Content is defined in TypeScript fixtures
- Document IDs are generated fresh each run
- Assertions use content-based verification

```typescript
// e2e/fixtures/testProjects.ts
export const BASIC_QMD_PROJECT = {
  files: [
    { path: 'index.qmd', content: '---\ntitle: Test\n---\n\n# Hello World\n' },
    { path: '_quarto.yml', content: 'project:\n  type: default\n' },
  ],
};

// In test (for fresh document creation):
const { indexDocId } = await syncClient.createNewProject({
  syncServer: SYNC_SERVER_URL,
  files: BASIC_QMD_PROJECT.files,
});
// indexDocId is unique each run, use content-based assertions
```

**Tertiary Approach: IndexedDB Fixtures**

For testing project metadata, settings, and the project list UI:
- Use `projectStorage.exportData()` / `importData()`
- JSON format is human-readable and version-controllable
- Useful for testing the project selector component

---

## Example: Testing SCSS Compilation Cache

One key use case is testing the SCSS compilation cache behavior. Here's how the infrastructure supports this:

```typescript
// src/services/sassCache.integration.test.ts
import { describe, it, expect, beforeEach } from 'vitest';
import { createSassCacheManager } from './sassCache';
import { InMemoryCacheStorage } from '../test-utils/inMemoryStorage';

describe('SCSS Compilation Cache - Integration', () => {
  let cache: ReturnType<typeof createSassCacheManager>;
  let storage: InMemoryCacheStorage;

  beforeEach(() => {
    storage = new InMemoryCacheStorage();
    cache = createSassCacheManager({ storage, maxEntries: 100 });
  });

  it('should cache compiled CSS and return on subsequent requests', async () => {
    const scss = '$color: red; .test { color: $color; }';

    // First compilation - cache miss
    const result1 = await cache.compile(scss);
    expect(result1.cached).toBe(false);

    // Second compilation - cache hit
    const result2 = await cache.compile(scss);
    expect(result2.cached).toBe(true);
    expect(result2.css).toBe(result1.css);
  });

  it('should evict oldest entries when cache is full', async () => {
    // Fill cache to capacity
    for (let i = 0; i < 100; i++) {
      await cache.compile(`.class-${i} { color: blue; }`);
    }

    // Add one more - should evict oldest
    await cache.compile('.new-class { color: green; }');

    // Verify oldest was evicted
    const stats = await cache.getStats();
    expect(stats.entryCount).toBe(100);
  });
});
```

---

## Open Questions

All design questions have been resolved (see Design Decisions above).

**Implementation note**: After initial fixture generation, assess total size and document if it approaches problematic levels for regular git operations.

---

## CI Configuration

### Unit/Integration Tests in test-suite.yml

Add hub-client unit tests to the existing workflow. These run quickly and don't need WASM:

```yaml
# Add after "Build TypeScript packages" step in .github/workflows/test-suite.yml

- name: Run hub-client tests
  shell: bash
  run: |
    cd hub-client
    npm run test:ci
```

This runs both unit and integration tests without requiring the WASM build.

### E2E Tests Workflow (Separate)

Create a new workflow for E2E tests. Initially manual-trigger only due to heavier requirements:

```yaml
# .github/workflows/hub-client-e2e.yml
name: Hub-Client E2E Tests
on:
  workflow_dispatch:  # Manual trigger initially
  # Later: enable on push when stable
  # push:
  #   branches: [main, kyoto]
  #   paths:
  #     - 'hub-client/**'
  #     - 'crates/wasm-qmd-parser/**'

jobs:
  e2e-tests:
    runs-on: ubuntu-latest
    name: Hub-Client E2E Tests

    steps:
      - uses: actions/checkout@v4

      # Rust toolchain for WASM build
      - name: Set up Rust nightly
        uses: dtolnay/rust-toolchain@nightly
        with:
          targets: wasm32-unknown-unknown

      - name: Set up Clang
        uses: egor-tensin/setup-clang@v1
        with:
          version: latest

      - name: Install wasm-pack
        run: cargo install wasm-pack

      # tree-sitter for grammar builds
      - name: Set up tree-sitter CLI
        run: |
          curl -LO https://github.com/tree-sitter/tree-sitter/releases/download/v0.25.8/tree-sitter-linux-x86.gz
          gunzip tree-sitter-linux-x86.gz
          chmod +x tree-sitter-linux-x86
          sudo mv tree-sitter-linux-x86 /usr/local/bin/tree-sitter

      # Node.js and dependencies
      - name: Set up Node.js
        uses: actions/setup-node@v4
        with:
          node-version: '24'
          cache: 'npm'

      - name: Install npm dependencies
        run: npm ci

      - name: Build TypeScript packages
        run: npm run build

      # Build WASM module
      - name: Build WASM
        run: |
          cd hub-client
          npm run build:wasm

      # Install Playwright browsers
      - name: Install Playwright
        run: |
          cd hub-client
          npx playwright install --with-deps chromium

      # Run E2E tests
      - name: Run E2E tests
        run: |
          cd hub-client
          npm run test:e2e

      # Upload test artifacts on failure
      - name: Upload Playwright report
        uses: actions/upload-artifact@v4
        if: failure()
        with:
          name: playwright-report
          path: hub-client/playwright-report/
          retention-days: 7
```

### CI Strategy

| Test Type | Workflow | Trigger | Dependencies |
|-----------|----------|---------|--------------|
| Unit tests | test-suite.yml | Push/PR | None (mocked) |
| Integration tests | test-suite.yml | Push/PR | fake-indexeddb, jsdom |
| E2E tests | hub-client-e2e.yml | Manual (initially) | WASM, Playwright, sync-server |

**Rationale:**
- Unit/integration tests are fast and don't block PRs
- E2E tests are slower and require more infrastructure
- E2E can be promoted to automatic trigger once stable
- Keeps main test-suite.yml fast for quick PR feedback

---

## Related Issues

- kyoto-b4x: Hub-client automated testing infrastructure (this epic)

## References

- [Vitest Documentation](https://vitest.dev/)
- [Playwright Documentation](https://playwright.dev/)
- [React Testing Library](https://testing-library.com/docs/react-testing-library/intro/)
- [Automerge Repo](https://automerge.org/docs/quickstart/)
