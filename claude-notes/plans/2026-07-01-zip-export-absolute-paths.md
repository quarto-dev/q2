---
title: 'Fix: downloaded project ZIP uses absolute paths'
date: 2026-07-01
description: 'Fixes the hub-client project ZIP export so entries are relative and nested under one project-named folder, by stripping leading slashes in `export-zip.ts` and sharing one sanitized folder-name helper with the download filename.'
braid:
  strand: bd-esnxtcoy
  priority: P2
  labels: [quarto-hub]
---

**GitHub issue:** [quarto-dev/q2#147](https://github.com/quarto-dev/q2/issues/147)
**Related:** bd-1oxt (original "Project ZIP export from hub-client" feature — this is a follow-up bug)

## Overview

The hub-client "Export ZIP" button downloads a project archive whose entries
are **absolute paths**, e.g.:

```
% unzip -l Demo-Playground.zip
      941  04-30-2026 14:46   /cscheid/columns.qmd
      327  04-30-2026 14:46   /cscheid/crossrefs.qmd
```

`unzip` refuses to honor absolute paths and prints `warning: stripped
absolute path spec from /cscheid/columns.qmd` for every entry.

They should instead be **relative and nested under a single top-level folder
named after the project** — matching the download filename stem — so the
archive is well-formed and extracts into one tidy directory:

```
      941  04-30-2026 14:46   Demo-Playground/cscheid/columns.qmd
      327  04-30-2026 14:46   Demo-Playground/cscheid/crossrefs.qmd
```

## Root-cause assessment

The feature is **pure TypeScript** (no Rust involved). It uses `fflate`\'s
`zipSync`. The archive entry keys are exactly whatever `client.getFilePaths()`
returns, used verbatim.

- `ts-packages/quarto-sync-client/src/export-zip.ts` — `exportProjectAsZip(client)`
  builds `files[path] = …` for each `path` (lines 33, 38) and calls
  `zipSync(files, { level: 6 })` (line 43). **No path normalization.**
- `getFilePaths()` (`client.ts:1344`) returns
  `Array.from(state.fileHandles.keys())`. Those keys are the raw index paths,
  stored **absolute with a leading slash** (e.g. `/cscheid/columns.qmd`).
- `fflate` preserves keys verbatim, so a leading-`/` key becomes an absolute
  ZIP entry.

So the bug is fully contained in `export-zip.ts`: it (a) never strips the
leading `/`, and (b) never adds the project-name top-level folder. The
download filename that *does* carry the project name is computed separately in
the UI and never reaches the library:

- `hub-client/src/components/tabs/ProjectTab.tsx:45`:
  `a.download = ${(project.description || 'project').replace(/ /g, '-')}.zip;`

### Why fix at the ZIP boundary, not the storage convention

One could argue the *real* bug is that `state.fileHandles` keys carry a leading
slash. That convention, however, is load-bearing across the sync client and the
Automerge index; changing how paths are stored is a broad, risky change with
many consumers and is out of scope for this issue. The archive is a
serialization boundary, and normalizing there (strip leading slash + add the
project folder) is the correct, localized fix. We do **not** change stored
paths.

### The import side already expects this shape (round-trip)

`ts-packages/quarto-sync-client/src/import-zip.ts` (`parseProjectZip`) is the
inverse and already:

- strips a single **common leading directory** (the wrapper folder a GitHub
  "Download ZIP" adds — and, after this fix, our own project-name folder), via
  `commonLeadingDirectory()`; and
- **rejects absolute paths** (`isSafePath` returns false for `path.startsWith('/')`).

So today's buggy export only round-trips by accident: `commonLeadingDirectory`
sees `/` as the shared prefix and strips it. After this fix the shared prefix
becomes `Demo-Playground/`, which import strips cleanly — a real, intentional
round-trip. A regression test will lock this in.

## Design

### 1. Thread a `rootDir` (project folder name) into the library

Change the pure library function to accept the top-level folder name:

```ts
// export-zip.ts
export function exportProjectAsZip(client: SyncClient, rootDir?: string): Uint8Array
```

For each project path, compute the archive key as:

1. **Strip leading slashes** — `path.replace(/^\/+/, '')` (idempotent; a
   path that is already relative is unchanged).
2. **Prefix `rootDir/`** when a non-empty `rootDir` is provided; otherwise use
   the relative path as-is.

`rootDir` itself is sanitized (see below) and its own leading/trailing slashes
trimmed so we never emit `Demo-Playground//cscheid/...` or an absolute prefix.

Making `rootDir` optional keeps the function usable by any generic caller while
letting hub-client supply the project name. Absolute-path stripping happens
**unconditionally**, so even the no-`rootDir` path is now well-formed.

### 2. Share one slug between the folder name and the download filename

The issue explicitly wants the internal folder to equal "the project name we
put in the zip filename". Extract the current inline slug logic into a single
helper **exported from `quarto-sync-client`** (hub-client already depends on it,
so no new dependency edge is needed). If a third consumer ever appears, a code
exploration can lift it into a shared utilities library then — we do not
speculatively create one now.

The helper both (a) preserves the existing space→hyphen behavior and (b)
tightens sanitization to guard characters that are hostile as a single path
segment — notably the Windows-reserved set ``< > : " / \ | ? *`` and control
characters — plus trailing dots/spaces (also illegal on Windows). It falls back
to `project` when the name is empty, matching the existing filename fallback:

```ts
// exported from quarto-sync-client
export function projectFolderName(description: string | undefined): string {
  const cleaned = (description || '')
    .replace(/ /g, '-')            // existing behavior: spaces -> hyphens
    .replace(/[<>:"/\\|?*\x00-\x1f]/g, '-') // Windows-hostile + control chars
    .replace(/[. ]+$/, '');        // no trailing dot/space (Windows)
  return cleaned || 'project';     // fallback matches download-filename fallback
}
```

The exact regex is to be finalized against the unit tests below; the intent is:
keep space→hyphen, collapse reserved/hostile **and control** characters to `-`,
drop trailing dots/spaces, and never return empty.

- `ProjectTab.tsx` uses `${projectFolderName(project.description)}.zip` for the
  download filename **and** passes the same value through as `rootDir`, so the
  folder and filename can never drift.

### 3. Wire the name through the call chain

`project.description` lives in `ProjectTab`; the library lives two hops away:

```
ProjectTab.tsx (has project.description)
  → onExportZip(rootDir)                         prop, type update
    → Editor.tsx  onExportZip={exportProjectAsZip}
      → preview-runtime/automergeSync.ts  exportProjectAsZip(rootDir?)   wrapper
        → quarto-sync-client  exportProjectAsZip(client, rootDir?)       library
```

Signature changes required:
- `ts-packages/quarto-sync-client/src/export-zip.ts` — add `rootDir?`.
- `ts-packages/preview-runtime/src/automergeSync.ts:309` — `exportProjectAsZip(rootDir?)`
  forwarding to `exportZip(ensureClient(), rootDir)`.
- `hub-client/src/components/tabs/ProjectTab.tsx` — `onExportZip` prop type
  becomes `(rootDir: string) => Uint8Array`; call site passes the slug; reuse
  the same slug for `a.download`.
- `hub-client/src/components/Editor.tsx:1008` — prop still passes the wrapper;
  verify the type lines up (the wrapper param is optional, so this stays
  compatible).

## Test plan (TDD — write/adjust tests first)

All tests are TypeScript (`vitest`). Per project policy, write/adjust the
failing tests before the implementation.

### `ts-packages/quarto-sync-client/src/export-zip.test.ts`

Existing tests assert bare keys like `entries['index.qmd']`; these must be
updated for the new prefixing behavior. Add:

- [ ] **Strips leading slash**: input path `/cscheid/columns.qmd`, `rootDir`
  `Demo-Playground` → entry key exactly `Demo-Playground/cscheid/columns.qmd`.
- [ ] **No absolute entries**: for a set of `/`-prefixed inputs, assert
  `Object.keys(entries).every(k => !k.startsWith('/'))`.
- [ ] **All entries share the project prefix** when `rootDir` is given.
- [ ] **No `rootDir`**: leading slash still stripped; no folder added
  (entries relative, no `/` prefix).
- [ ] **`rootDir` normalization**: a `rootDir` passed with stray slashes/spaces
  still yields a single clean top-level segment (no `//`, no absolute).
- [ ] **Existing content/binary/empty-project tests** updated to the new key
  shape (content assertions otherwise unchanged).

### `projectFolderName` helper unit tests (new)

- [ ] `"Demo Playground"` → `"Demo-Playground"`.
- [ ] `undefined` / `""` → `"project"`.
- [ ] name containing `/` or `\` collapses to a safe single segment.
- [ ] Windows-hostile characters (`: " | ? * < >`) are replaced, not preserved.
- [ ] Trailing dot/space is stripped (`"My Project."` → no trailing `.`).
- [ ] A name made **entirely** of hostile characters still yields a non-empty
  result (falls back to `project` if it would otherwise be empty).

### Round-trip test (export → import)

- [ ] In `export-zip.test.ts` (or an integration test), run
  `parseProjectZip(exportProjectAsZip(client, 'Demo-Playground'))` and
  assert the returned files have the **original project-relative** paths
  (prefix stripped by `commonLeadingDirectory`), proving export/import
  agree on the convention.

### hub-client

- [ ] Update/extend any `ProjectTab` test to confirm the download filename and
  the `rootDir` passed to `onExportZip` come from the same slug.

## Verification (end-to-end)

Per `CLAUDE.md`, tests passing is necessary but not sufficient.

- [x] `npm run typecheck` in `ts-packages/quarto-sync-client` and `preview-runtime` — clean.
- [x] hub-client `tsc -b` + `npm run build` (`tsc -b && vite build`, the stricter
  production build) — clean. *(Not `build:all`: my changes are pure TS; the
  WASM leg rebuilds Rust unchanged. Offer to run full `build:all` if desired.)*
- [x] Tests: quarto-sync-client **117**, preview-runtime **74**, hub-client
  unit **675** + integration **76** + wasm **121** — all pass.
- [x] **End-to-end with the real function + system `unzip`** (equivalent to the
  bug report's exact command). Ran real `exportProjectAsZip(client,
  'Demo Playground')` with absolute stored paths, wrote the bytes, then:

  ```
  $ unzip -l Demo-Playground.zip
      10  ...  Demo-Playground/cscheid/columns.qmd
      12  ...  Demo-Playground/cscheid/crossrefs.qmd
       7  ...  Demo-Playground/index.qmd
  $ unzip Demo-Playground.zip     # extract, watch for warnings
   inflating: Demo-Playground/cscheid/columns.qmd
   inflating: Demo-Playground/cscheid/crossrefs.qmd
   inflating: Demo-Playground/index.qmd
  ```

  **No** `/`-prefixed entries, **no** "stripped absolute path spec" warnings,
  one tidy `Demo-Playground/` root. The space in "Demo Playground" was
  sanitized to `Demo-Playground`, matching the download filename. Output
  inspected directly.
- [x] Round-trip export→import verified by the `parseProjectZip` unit test
  (recovers original project-relative paths).
- [ ] **(Optional follow-up)** Live-browser hub session: click "Export ZIP",
  confirm the browser download names the file and writes the bytes. The
  button→slug wiring is covered by `ProjectTab.test.tsx` (jsdom); a live
  browser would additionally exercise the real anchor download. Not run this
  session (needs a running hub + auth + project).
- [ ] **(Optional follow-up)** Playwright export e2e mirroring
  `e2e/import-zip.spec.ts` (there is currently no export e2e).

## Work items

### Phase 0: Tests first
- [x] Add/adjust `export-zip.test.ts` cases above (verify they fail). *(4 new
  cases added; existing tests kept as no-`rootDir` coverage. Confirmed
  failing before implementation.)*
- [x] Add `projectFolderName` unit tests. *(`project-folder-name.test.ts`, 8 cases.)*
- [x] Add export→import round-trip test.

### Phase 1: Library
- [x] Add `projectFolderName` helper (exported from `quarto-sync-client`).
  *(`src/project-folder-name.ts`; uses a char-code hostility check — no
  regex control-char literals — and trims leading/trailing separators.)*
- [x] `exportProjectAsZip(client, rootDir?)`: strip leading slashes + prefix
  sanitized `rootDir`. *(All 20 target tests + 117 package tests pass;
  `tsc --noEmit` clean.)*

### Phase 2: Wiring
- [x] Update `automergeSync.ts` wrapper signature. *(`exportProjectAsZip(rootDir?)`
  forwards to `exportZip(ensureClient(), rootDir)`.)*
- [x] Update `ProjectTab.tsx` (prop type, slug reuse for filename + rootDir).
  *(One `folderName = projectFolderName(project.description)` drives both
  `onExportZip(folderName)` and `a.download`. Added `ProjectTab.test.tsx`,
  3 jsdom wiring tests — pass.)*
- [x] Confirm `Editor.tsx` prop wiring/types. *(`(rootDir?: string)` wrapper is
  assignable to the `(rootDir: string)` prop; `tsc -b` clean.)*
- [x] Export `projectFolderName` from `quarto-sync-client` index (hub-client
  imports it from there).

### Phase 3: Verification
- [x] typecheck + production build + tests (all packages). See the
  "Verification (end-to-end)" section for counts and the `unzip -l` evidence.
- [x] `unzip -l` evidence recorded (via the real function + system `unzip`).
- [x] Round-trip re-import check (unit test).
- [ ] Optional live-browser + Playwright export e2e (follow-ups; see above).

## Resolved decisions (2026-07-01 review)

1. **Helper location** — export `projectFolderName` from `quarto-sync-client`
   (hub-client already depends on it). If a third consumer ever needs it, a
   code exploration can lift it into a shared utilities library then; we do not
   create one speculatively now.
2. **Sanitization strictness** — tighten beyond the current space-only handling:
   collapse Windows-hostile characters (`< > : " / \ | ? *`) and control
   characters, and drop trailing dots/spaces. This changes the download filename
   too (same helper), which is the intended improvement.
3. **`rootDir` default when name missing** — fall back to `project`, matching the
   existing `|| 'project'` filename fallback, so folder and filename stay
   consistent.
