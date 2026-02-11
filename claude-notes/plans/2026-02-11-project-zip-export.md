# Project ZIP Export

## Overview

Add a "Export ZIP" feature that allows users to download all project files as a `.zip` archive. The implementation is split into two layers:

1. **quarto-sync-client**: A new `exportProjectAsZip()` async function that walks all project files and returns a `Uint8Array` of ZIP bytes. This keeps the functionality accessible to any consumer of the library.
2. **hub-client**: An "Export ZIP" button in the PROJECT accordion tab that calls the library function and triggers a browser download.

### Motivation

This allows users working in hub-client to export their projects and render/publish them using Quarto 1 while the Quarto 2 port is in progress.

## Design Decisions

### ZIP library choice

We need a JavaScript ZIP library that works in the browser. Options:

- **fflate** (~29KB gzip) — Fast, modern, tree-shakeable, zero-dependency. Supports streaming compression. Well-maintained.
- **JSZip** (~40KB gzip) — Widely used, Promise-based API. Heavier, older architecture.
- **client-zip** (~5KB) — Minimal, uses Streams API. Limited features.

**Recommendation: fflate.** It's the fastest, has good tree-shaking (we only import what we need), zero dependencies, and works in both browser and Node.js contexts (good for quarto-sync-client being environment-agnostic).

### API placement

The ZIP export function will be added to quarto-sync-client as a standalone utility function (not a method on SyncClient). This keeps it a pure function:

```typescript
export async function exportProjectAsZip(client: SyncClient): Promise<Uint8Array>
```

The function takes a connected SyncClient and returns ZIP bytes. This is clean — the caller decides what to do with the bytes (download, upload, pipe to another tool, etc.).

### File handling

- Text files: encoded as UTF-8 in the ZIP
- Binary files: included as raw bytes
- Empty files: included (zero-byte entries)
- File paths: preserved as-is from the project (relative paths, no leading slash)

### Progress reporting (future)

For now, the function is fire-and-forget. If projects grow large enough to need progress reporting, we can add an optional callback parameter later.

## Work Items

### Phase 1: Library layer (quarto-sync-client)

- [x] Add `fflate` dependency to `ts-packages/quarto-sync-client/package.json`
- [x] Create `ts-packages/quarto-sync-client/src/export-zip.ts` with `exportProjectAsZip()` function
- [x] Export `exportProjectAsZip` from `ts-packages/quarto-sync-client/src/index.ts`
- [x] Write unit tests for the export function (mock SyncClient with known files, verify ZIP contents)
- [x] Add vitest devDependency and test script to quarto-sync-client

### Phase 2: UI layer (hub-client)

- [x] Add "Export ZIP" button to `hub-client/src/components/tabs/ProjectTab.tsx`
- [x] Style the button in `ProjectTab.css`
- [x] Wire up the button to call `exportProjectAsZip()` and trigger browser download
- [x] Handle loading/error states (button shows spinner during export, error toast on failure)
- [x] Add `exportProjectAsZip()` wrapper to `automergeSync.ts` service layer
- [x] Pass `onExportZip` prop from Editor through to ProjectTab

### Phase 3: Verification

- [x] Run `npm run typecheck` in quarto-sync-client
- [x] Run hub-client typecheck
- [x] Run hub-client tests (`npm run test` — 272 tests pass)
- [x] Run quarto-sync-client tests (8 tests pass)
- [x] Run hub-client full build (`npm run build:all`)
- [ ] Manual smoke test: connect to a project, click export, verify ZIP contains correct files

## Implementation Details

### `exportProjectAsZip()` implementation sketch

```typescript
import { zipSync, strToU8 } from 'fflate';
import type { SyncClient } from './client.js';

export async function exportProjectAsZip(client: SyncClient): Promise<Uint8Array> {
  const paths = client.getFilePaths();
  const files: Record<string, Uint8Array> = {};

  for (const path of paths) {
    if (client.isFileBinary(path)) {
      const binary = client.getBinaryFileContent(path);
      if (binary) {
        files[path] = binary.content;
      }
    } else {
      const text = client.getFileContent(path);
      if (text !== null) {
        files[path] = strToU8(text);
      }
    }
  }

  return zipSync(files, { level: 6 });
}
```

### Browser download trigger (hub-client)

```typescript
function downloadBlob(data: Uint8Array, filename: string) {
  const blob = new Blob([data], { type: 'application/zip' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}
```

### ProjectTab button

The "Export ZIP" button goes in the `project-tab-actions` div, above the "Choose New Project" button. It needs access to the SyncClient instance, which will be threaded through as a prop.
