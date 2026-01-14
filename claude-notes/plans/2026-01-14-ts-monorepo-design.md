# TypeScript Monorepo Design for Hub-Client Extraction

**Issue**: kyoto-1ew
**Date**: 2026-01-14
**Status**: Research/Discussion phase

## Motivation

We want to create a more explicit abstraction for the Quarto automerge schema and communication layer. This would:

1. Make the abstraction publicly available as npm packages
2. Enable other tools (with completely different UIs) to be multiplayer collaborators in a Quarto project
3. Keep everything within the Rust workspace (not a separate git repo) because many TS packages will have WASM components

## Current Architecture

### Hub-Client Layers

| Layer | Files | Dependencies | Extractable? |
|-------|-------|--------------|--------------|
| **Automerge Schema/Sync** | `automergeSync.ts`, `types/project.ts` | `@automerge/*` | YES - core target |
| **Presence Protocol** | `presenceService.ts` | Automerge ephemeral messaging | Maybe - UI-specific |
| **WASM Rendering** | `wasmRenderer.ts`, `wasm-js-bridge/` | WASM module, VFS | Later phase |
| **Storage** | `projectStorage.ts`, `storage/*` | `idb` (IndexedDB) | No - app-specific |
| **UI Components** | `components/*`, `hooks/*`, `App.tsx` | React, Monaco | No - app-specific |

### Key Schema Types (from `types/project.ts` and `automergeSync.ts`)

```typescript
// Root document - maps file paths to document IDs
interface IndexDocument {
  files: Record<string, string>; // path → docId
}

// Text file content (e.g., .qmd, .yml)
interface TextDocumentContent {
  text: string; // Automerge Text CRDT
}

// Binary file content (e.g., images)
interface BinaryDocumentContent {
  content: Uint8Array;
  mimeType: string;
  hash: string; // SHA-256 for deduplication
}
```

### Current VFS Coupling Problem

Every file operation in `automergeSync.ts` also updates the WASM VFS:

```typescript
export function updateFileContent(path: string, content: string): void {
  handle.change(doc => { updateText(doc, ['text'], content); });
  vfsAddFile(path, content);  // <-- Coupled to WASM
}
```

This coupling must be made pluggable for extraction.

### Existing Infrastructure

- `ts-packages/` already exists with `@quarto/annotated-qmd`
- Currently independent packages, not a workspace
- Published to npm under `@quarto/` namespace
- `external-sources/quarto/` provides a reference pattern (yarn workspaces + turborepo)

## Proposals

### Proposal A: Minimal Extraction (Recommended Near-Term)

```
kyoto/
├── crates/           # Rust workspace (unchanged)
├── ts-packages/      # Evolve into npm workspace
│   ├── package.json  # NEW: workspace root
│   ├── annotated-qmd/
│   ├── quarto-automerge-schema/   # NEW: types only
│   └── quarto-sync-client/        # NEW: sync logic with callbacks
└── hub-client/       # Remains standalone, consumes ts-packages
```

**Extracted packages:**

1. **`@quarto/quarto-automerge-schema`**
   - Pure types: `IndexDocument`, `TextDocumentContent`, `BinaryDocumentContent`
   - Type guards: `isTextDocument()`, `isBinaryDocument()`
   - File type detection utilities
   - No runtime dependencies except Automerge types

2. **`@quarto/quarto-sync-client`**
   - Sync logic with callback-based VFS hooks
   - `connect()`, `disconnect()`, `createFile()`, `updateFileContent()`, etc.
   - Consumers provide their own VFS implementation

**Callback-based VFS design:**

```typescript
// Discriminated union for type-safe file content
// Principle: "make illegal states unrepresentable"
type TextFilePayload = { type: "text"; text: string };
type BinaryFilePayload = { type: "binary"; data: Uint8Array; mimeType: string };
type FilePayload = TextFilePayload | BinaryFilePayload;

interface SyncClientOptions {
  onFileAdded: (path: string, file: FilePayload) => void;
  onFileChanged: (path: string, text: string, patches: Patch[]) => void;
  onBinaryChanged: (path: string, data: Uint8Array, mimeType: string) => void;
  onFileRemoved: (path: string) => void;
}

const client = createSyncClient(options);
await client.connect(syncServerUrl, indexDocId);
```

**Design note:** The discriminated union `FilePayload` ensures callers cannot pass
mismatched content/type combinations. The small object allocation overhead is
negligible for file sync events (dwarfed by Automerge operations and I/O).
If profiling ever shows this matters, split into `onTextFileAdded`/`onBinaryFileAdded`.

**Pros:**
- Minimal disruption to hub-client
- Other apps can use the same sync protocol
- WASM stays in hub-client where it belongs

**Cons:**
- Two places to maintain npm infrastructure
- hub-client not part of workspace

### Proposal B: Full Monorepo (Future Evolution)

```
kyoto/
├── crates/           # Rust workspace (unchanged)
├── ts/               # Full npm workspace
│   ├── package.json  # Workspace root with turborepo
│   ├── packages/
│   │   ├── annotated-qmd/
│   │   ├── quarto-automerge-schema/
│   │   ├── quarto-sync-client/
│   │   └── quarto-wasm/        # WASM wrapper package
│   └── apps/
│       └── hub-client/         # MOVED here
└── ts-packages/      # Deprecated or symlink
```

**When to evolve to this:**
- When adding another app (VS Code extension, CLI tool, etc.)
- When WASM needs to be shared across multiple apps

**Pros:**
- Clean apps/ vs packages/ separation
- Turborepo for optimized builds
- Matches external-sources/quarto/ pattern

**Cons:**
- Larger refactor
- Need to update WASM symlinks/paths

### Proposal C: WASM In-Crate Package (Alternative)

Include wasm-pack output in workspace:

```
"workspaces": ["packages/*", "../crates/wasm-*/pkg"]
```

**Pros:** WASM lives with Rust source
**Cons:** Complicated paths, wasm-pack regenerates package.json

## Recommended Path

### Phase 1: Extract schema/sync (Proposal A)

1. Create `ts-packages/package.json` as workspace root
2. Extract `@quarto/quarto-automerge-schema` (types only)
3. Extract `@quarto/quarto-sync-client` with callback-based VFS
4. Update hub-client to import from these packages
5. Hub-client provides WASM VFS callbacks

### Phase 2: Full monorepo (Proposal B, when needed)

- Triggered by: new app, shared WASM needs
- Move to `ts/` structure with turborepo
- Extract WASM wrapper if multiple consumers

## Design Decisions

### 1. Presence Service

Keep in hub-client for now. It's UI-specific (cursors, selections). Other apps may want different presence semantics.

### 2. Storage Abstraction

Don't extract. IndexedDB storage is application-specific.

### 3. Package Manager

Use npm workspaces (already established in hub-client). Turborepo can be added later.

### 4. Publishing

**Deferred until API is validated.** Use packages internally (workspace dependencies)
before publishing to npm. This allows us to iterate on the API without committing
to public stability guarantees.

When ready to publish:
- Use `@quarto/` namespace (already established with annotated-qmd)
- Ensure API has been stable through internal use
- Document breaking changes if any occurred during internal phase

## Open Questions (Resolved)

1. **Target consumers**: Posit-internal for now, but will become public eventually.
   This means we can iterate freely during internal phase, but should design with
   eventual public API in mind.

2. **Rendering needs**: Other apps might need WASM rendering, but API shape is unclear.
   Defer WASM extraction until we understand the requirements better.

3. **Sync server**: Assume automerge-repo sync server for now. Can abstract later if needed.

4. **Versioning**: See detailed analysis below.

## Versioning Strategy Analysis

The schema and sync-client packages are tightly coupled: sync-client fundamentally
depends on schema types. This creates a coordination problem when publishing.

### Strategy 1: Lockstep Versioning

All packages share the same version. When any package changes, all bump together.

```
schema v1.0.0, sync-client v1.0.0
schema changes → schema v1.1.0, sync-client v1.1.0 (even if unchanged)
```

**Examples:** Babel (@babel/*), Angular (@angular/*)

| Pros | Cons |
|------|------|
| Simple mental model: "use v1.2.3 of everything" | Spurious version bumps |
| No compatibility matrix | Harder to see what actually changed |
| Consumers can't have version mismatches | Changelog has "no changes" entries |
| Easy to communicate upgrades | |

**Tools:** Lerna (fixed mode), changesets (linked packages)

### Strategy 2: Independent Versioning

Each package has its own version, bumped only when that package changes.

```
schema v1.0.0, sync-client v1.0.0
schema changes → schema v1.1.0, sync-client stays v1.0.0
```

But sync-client depends on schema. If schema v1.1.0 is compatible, fine.
If schema v2.0.0 has breaking changes, sync-client needs updating too.

| Pros | Cons |
|------|------|
| Accurate: version reflects actual changes | Compatibility matrix: "sync-client 1.2.x works with schema 1.0.x-1.3.x" |
| Smaller updates for single-package users | Consumers can create mismatched installations |
| Clear per-package changelogs | More cognitive load |

**Tools:** Lerna (independent mode), changesets, Nx

### Strategy 3: Peer Dependencies

sync-client declares schema as peer dependency; consumer coordinates.

```json
// sync-client package.json
{ "peerDependencies": { "@quarto/quarto-automerge-schema": "^1.0.0" } }
```

| Pros | Cons |
|------|------|
| Consumer controls schema version | Must install both packages explicitly |
| Avoids duplicate installations | npm warns about mismatches |
| Clear about the relationship | Still need compatibility docs |

Best for plugin architectures where consumers might have their own schema version.

### Strategy 4: Single Package

Don't split. One package exports both types and sync client.

| Pros | Cons |
|------|------|
| No coordination problem | Larger bundle for types-only consumers |
| Simpler for consumers | Defeats separation goal |

### Strategy 5: Re-export Pattern

Separate packages internally, but sync-client re-exports schema types:

```typescript
// @quarto/quarto-sync-client
export type { IndexDocument, TextDocumentContent, BinaryDocumentContent }
  from "@quarto/quarto-automerge-schema";
export { createSyncClient } from "./client";
```

Consumers only need one import:
```typescript
import { createSyncClient, IndexDocument } from "@quarto/quarto-sync-client";
```

Can combine with either lockstep or independent versioning.

| Pros | Cons |
|------|------|
| Consumers need only one package | Types duplicated in both package APIs |
| Clean internal separation preserved | Slightly more complex publishing |
| Reduces mismatch risk | |

### Recommendation

**During internal phase:** Version numbers are irrelevant. Use `"workspace:*"`
or `"*"` as version specifiers. Focus on API design.

**For public phase:** Start with **lockstep versioning + re-export pattern**.

Rationale:
1. Packages are fundamentally coupled—schema change almost always affects sync-client
2. Simpler for consumers: "use v1.2.3 of @quarto/quarto-sync-client"
3. Re-exports mean most consumers only import from sync-client
4. Can switch to independent versioning later if packages diverge significantly

**Implementation:**
- Use changesets with `linked` configuration, or
- Lerna in fixed mode, or
- Simple script that bumps all versions together

**Decision:** TBD after internal validation phase. Revisit when preparing for npm publish.

## Files of Interest

Key files to review when implementing:

- `hub-client/src/services/automergeSync.ts` - Main extraction target
- `hub-client/src/types/project.ts` - Schema types
- `hub-client/src/services/presenceService.ts` - Presence protocol
- `hub-client/src/services/wasmRenderer.ts` - VFS integration
- `ts-packages/annotated-qmd/package.json` - Existing package pattern
- `external-sources/quarto/package.json` - Monorepo reference

## Next Steps

### Phase 1a: Internal extraction (COMPLETED 2026-01-14)

1. [x] Create `ts-packages/package.json` with workspace config
2. [x] Create `ts-packages/quarto-automerge-schema/` with types
3. [x] Create `ts-packages/quarto-sync-client/` with callback design
4. [x] Refactor hub-client to use new packages (workspace dependency)
5. [ ] Validate API through internal use (ongoing)

### Phase 1b: Publish (deferred)

Only after API has stabilized through internal use:

6. [ ] Review API for breaking changes since extraction
7. [ ] Add package documentation and examples
8. [ ] Publish to npm under @quarto/ namespace
