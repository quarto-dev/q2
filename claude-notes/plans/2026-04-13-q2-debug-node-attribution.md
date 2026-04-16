# Per-Node Attribution in q2-debug AST View

## Context

In the collaborative q2-debug AST view, there is currently no indication of who edited which part of the document. The goal is to show authorship on each AST node: text colored to match the author's cursor color, with a tooltip on hover showing name and timestamp.

The AST already carries source location info (`s` field on each node, referencing `astContext.sourceInfoPool[s]` byte ranges in the original QMD text). Automerge tracks all changes with actor IDs and timestamps. The identities map (`actorId -> { name, color }`) is already threaded to the Editor component.

**Key architectural insight**: The `astJson` string passed to the debug renderer is the full `RustQmdJson` — it already contains `astContext` (with `sourceInfoPool` and `files`) and per-node `s` fields at runtime. The debug renderer's `Ast` component drops this data only because its local `PandocAST` type doesn't declare it. A minimal type extension in the `Ast` component unlocks access without any changes to the upstream data flow (ReactPreview, ReactRenderer). The Automerge attribution map is provided to the Ast component via a React context from Editor, avoiding prop drilling through intermediate components.

## Implementation Checklist

### Phase 0.5: diff() Spike

- [x] Create spike test file (`hub-client/src/services/attribution-spike.test.ts`)
- [x] Validate `diff()` patch shapes for text insertions in this project's Automerge doc model
- [x] Validate `diff()` patch shapes for text deletions
- [x] Validate `diff()` patch shapes for mixed splice (replace)
- [x] Confirm `path` structure (field name + index) matches plan assumptions
- [x] Document any deviations from expected patch format

**Spike caveat**: The spike used the old `am.Text` API which produces `insert`/`del` patches. Real documents use `splice`/`del`/`put` — see Deviation #1 above.

### Phase 0: Tests

- [x] Write `buildAttributionMap` full-build tests (test spec 1)
- [x] Write `updateAttributionMap` incremental-update tests (test spec 1b)
- [x] Write `buildAttributionMap` chunked-processing tests (test spec 1c)
- [x] Write `getNodeAttribution` query tests (test spec 2)
- [x] Write UTF-8 byte offset → JS char index conversion tests (test spec 3)
- [x] Write `useAttribution` hook lifecycle tests (test spec 4)
- [x] Write `Node` component rendering with attribution tests (test spec 5)
- [x] Verify all tests fail (red phase of TDD)

### Phase 1: Attribution Service (`hub-client/src/services/attribution.ts`)

- [x] Define types: `CharAttribution`, `AttributionMap`, `NodeAttribution`, `HistoryCompactedError`
- [x] Implement `buildByteToCharMap(text: string): number[]`
- [x] Implement patch application logic (insert/del, field filtering)
- [x] Implement `buildAttributionMap` — full history processing with chunked idle callbacks + AbortSignal
- [x] Implement `updateAttributionMap` — incremental from `processedHeads` / `processedHistoryIndex`
- [x] Implement `getNodeAttribution` — source info resolution → byte-to-char → attribution lookup → identity
- [x] Verify `ts-packages/annotated-qmd` is importable from hub-client (check workspace deps)
- [x] All Phase 1 tests pass (green) — 24 tests

### Phase 2: React Hook + Context (`hub-client/src/hooks/useAttribution.ts`)

- [x] Implement `useAttribution(filePath, identities)` hook — initial async build on mount
- [x] Implement incremental update on debounced file change
- [x] Implement cancellation on `filePath` change (abort + fresh build)
- [x] Implement cancellation on unmount
- [x] Implement `HistoryCompactedError` catch → async rebuild fallback
- [x] Implement ref stability (`useRef` for map alongside state setter)
- [x] Define and export `AttributionContext`
- [x] All Phase 2 tests pass (green) — 6 tests

### Phase 3: Wire Context + Render

- [x] `Editor.tsx` — call `useAttribution`, wrap preview area with `AttributionContext.Provider`
- [x] `ReactAstDebugRenderer.tsx` — extend `PandocAST` type with optional `astContext`
- [x] `ReactAstDebugRenderer.tsx` — create `NodeAttributionContext` with `getNodeAttribution` closure
- [x] `ReactAstDebugRenderer.tsx` — in `Ast`, extract `astContext`, inject `sourceText` into `files[0].content`, construct `SourceInfoReconstructor`, provide `NodeAttributionContext`
- [x] `ReactAstDebugRenderer.tsx` — in `Node`, consume `NodeAttributionContext`, apply color + title tooltip
- [x] All Phase 3 tests pass (green) — 3 tests

### Phase 4: Decouple Attribution Interface from Automerge Internals

Refactor so that the consumer-facing interface carries only source-agnostic `CharAttribution[]` entries, not the full Automerge-specific `AttributionMap` (which includes `processedHeads` and `processedHistoryIndex` bookkeeping). This enables swapping the attribution data source (e.g., to git blame) without changing any consumer code.

- [x] `getNodeAttribution()` takes `entries: CharAttribution[]` instead of `attributionMap: AttributionMap`
- [x] `AttributionContext` shape: `entries: CharAttribution[]` instead of `attributionMap: AttributionMap`
- [x] `UseAttributionResult` type: `entries: CharAttribution[]` instead of `attributionMap: AttributionMap`
- [x] `useAttribution` hook internally retains `AttributionMap` for incremental updates but only surfaces `.entries` to consumers
- [x] `Editor.tsx` context value: `entries: attribution.entries`
- [x] `ReactAstDebugRenderer.tsx`: reads `attributionCtx.entries`
- [x] Tests updated: service tests pass `CharAttribution[]` directly; hook tests assert on `.entries`
- [x] All 453 tests pass, TypeScript type-checks cleanly

**Rationale**: The `processedHeads` and `processedHistoryIndex` fields are Automerge incremental-update bookkeeping — they only matter inside the hook, never to consumers. With this refactor, a future git blame provider only needs to produce `{ entries: CharAttribution[], byteToCharMap: number[] }` (the same hook return shape), and everything downstream works unchanged.

### Verification

- [x] `npm run build:all` passes from hub-client (after annotated-qmd fixes in `c6200953`)
- [x] `npm run test:ci` passes from hub-client — 453 unit + 12 integration + 52 WASM = 517 tests pass
- [x] Manual test: AST nodes colored by author in q2-debug view
- [x] Manual test: hover tooltip shows "{name}, {time}"
- [ ] Manual test: offline/local editing renders without attribution (no regression)

---

## Plan

### Phase 0: Test Specifications (TDD)

Tests are written **before** implementation. Each phase below has corresponding test cases.

**Testing framework**: All hub-client tests use **vitest** (not `node:test`). Follow the established patterns:
- Import `{ describe, it, expect, vi, beforeEach, afterEach }` from `'vitest'`
- Mock modules with `vi.mock()` before imports (see `useReplayMode.test.ts` for pattern)
- Use `@testing-library/react` `renderHook` for hook tests
- Component tests needing DOM: add `@vitest-environment jsdom` pragma at top of file
- The vitest config (`hub-client/vitest.config.ts:20`) includes `'src/**/*.test.ts'` — tests MUST be co-located inside `src/`

**Unit tests for attribution service** (`hub-client/src/services/attribution.test.ts`):

1. **`buildAttributionMap` — full build (cold start)**
   - Given a doc with 3 sequential changes by 2 actors, builds `entries` matching current text length
   - Each character attributed to the correct actor and timestamp
   - `processedHeads` equals the final heads; `processedHistoryIndex` equals `history.length`
   - Empty history → all characters attributed to local actor
   - `handle.history()` returning `undefined` → returns null (no attribution)

1b. **`updateAttributionMap` — incremental update (warm path)**
   - Given an existing map and 1 new insertion by a different actor: only the new characters are attributed to the new actor; pre-existing entries are unchanged
   - Given an existing map and 1 deletion: entries at the deleted range are removed; surrounding entries are preserved
   - Given an existing map and a mixed splice (replace): deletion removes old entries, insertion adds new ones at the same position
   - `processedHeads` and `processedHistoryIndex` are advanced to reflect the new state
   - If `processedHistoryIndex > history.length` (history compacted): throws `HistoryCompactedError`

1c. **`buildAttributionMap` — chunked processing**
   - Given a history of 120 entries and `CHUNK_SIZE=50`: resolves after 3 idle callbacks (50 + 50 + 20), result is identical to processing all 120 in one pass
   - Given `signal.aborted = true` before first chunk: resolves to `null` immediately
   - Given signal aborted between chunks: resolves to `null`, partial work is discarded

2. **`getNodeAttribution` query**
   - Given source info ID, resolves through `SourceInfoReconstructor.getSourceLocation()` to file-level byte range
   - Converts byte range to char range, finds most-recent attribution in span
   - Returns `{ actor, time, color, name }` from identities map
   - Returns `null` for invalid source info ID, empty identity map, or null attribution map

3. **UTF-8 byte offset → JS char index conversion**
   - ASCII text: byte offset === char index
   - 2-3 byte UTF-8 (e.g., CJK `\u4e16` = 3 bytes → 1 JS char): byte offset > char index
   - 4-byte UTF-8 (e.g., emoji `\u{1F600}` = 4 bytes → 2 JS chars via surrogate pair): mapping accounts for surrogate pairs
   - Empty text: returns empty mapping
   - Mapping length === byte length of UTF-8 encoded text + 1 (for end-of-string boundary)

**Integration tests for React hook** (`hub-client/src/hooks/useAttribution.test.ts`):

4. **Hook lifecycle**
   - Returns `null` when `getFileHandle()` returns null (offline)
   - Starts async `buildAttributionMap` on mount, returns `null` until Promise resolves, then returns non-null
   - On debounced file change with existing map: calls `updateAttributionMap` (incremental, sync), not `buildAttributionMap`
   - On `HistoryCompactedError` from `updateAttributionMap`: starts a new async `buildAttributionMap`, returns `null` in the interim
   - On `filePath` change: aborts in-flight build (signal), resets to `null`, starts fresh build for new file
   - On unmount: aborts in-flight build

**Component tests** (`hub-client/src/components/render/ReactAstDebugRenderer.test.tsx`):

5. **Node rendering with attribution**
   - Node with attribution context → text colored with actor's color, title tooltip set
   - Node without attribution context (null) → renders identically to current behavior (regression guard)
   - Node with `s` field missing → renders without attribution styling

### Phase 1: Attribution Service

**New file**: `hub-client/src/services/attribution.ts`

Core types and logic for building per-character attribution:

```typescript
interface CharAttribution { actor: string; time: number; }

interface AttributionMap {
  entries: CharAttribution[];          // One entry per JS char (UTF-16 code unit)
  /** The heads we've processed up to. Used for incremental updates. */
  processedHeads: Heads;
  /** Index into handle.history() — the next unprocessed entry. */
  processedHistoryIndex: number;
}

interface NodeAttribution { actor: string; time: number; color: string; name: string; }
```

#### Full build (cold start — document load, no prior map)

1. Get `DocHandle` via `getFileHandle(path)` (already exposed by automergeSync.ts:260)
2. Call `handle.history()` for ordered `UrlHeads[]`
3. For each consecutive pair of heads, decode with `decodeHeads()` (from `@automerge/automerge-repo`, as in `replay.ts:63`) and call `diff(doc, prevHeads, currHeads)` (import `diff` from `@automerge/automerge`) to get `Patch[]`
4. Extract actor/time: `handle.history()` returns `UrlHeads[]` where each entry may be an array — extract the change hash via `Array.isArray(heads) ? heads[0] : heads` (see `replay.ts:79`), then call `handle.metadata(changeHash)` to get `{ actor, time }`
5. **Filter and apply patches** (see "Patch application" below)
6. Result: `AttributionMap` with `entries` of length matching current text, `processedHeads` set to the final heads, `processedHistoryIndex` set to `history.length`

#### Chunked idle processing (prevents jank on large histories)

The full build can process thousands of history entries. To avoid blocking the main thread, `buildAttributionMap` is structured as a **chunked async operation** driven by `requestIdleCallback`:

```typescript
const CHUNK_SIZE = 50; // history entries per idle callback

export async function buildAttributionMap(
  handle: DocHandle,
  textFieldName: string,
  signal?: AbortSignal,       // allows cancellation if file changes mid-build
): Promise<AttributionMap | null>;
```

Internally:
1. Snapshot the full `history` array and allocate the `entries` array once
2. Process up to `CHUNK_SIZE` history entries per iteration
3. After each chunk, yield to the event loop via `requestIdleCallback` (wrapped in a Promise)
4. Check `signal?.aborted` before starting each chunk — if aborted, return `null`
5. After processing all chunks, return the completed `AttributionMap`

The `CHUNK_SIZE` of 50 keeps each chunk under ~5ms on typical hardware (`diff()` + patch application for one history entry is ~0.1ms). Tune based on the Phase 0.5 spike measurements.

**Cancellation**: When the user switches files or a new edit arrives before the initial build finishes, the hook aborts the in-flight build via the `AbortSignal` and either starts a fresh build (file switch) or queues an incremental update once the build completes (edit during build — see hook lifecycle below).

**Note**: `updateAttributionMap` (incremental path) remains **synchronous** — it processes only 1-2 history entries and doesn't need chunking.

#### Incremental update (warm path — on each debounced edit)

When a previous `AttributionMap` exists:

1. Call `handle.history()` — this returns the full ordered list including new entries
2. Skip to `processedHistoryIndex` — all entries before this have already been applied
3. For each new entry from `processedHistoryIndex` to end:
   - `diff(doc, prevHeads, currHeads)` using the previous entry's heads (or `map.processedHeads` for the first new entry) as `prevHeads`
   - Extract actor/time via `handle.metadata(changeHash)`
   - Apply patches to existing `entries` array (splice in / splice out — see below)
4. Update `processedHeads` and `processedHistoryIndex`
5. Return the mutated `AttributionMap`

This reduces work from O(full_history) to O(new_changes) on every edit — typically 1 change.

**Fallback to full rebuild**: If `processedHistoryIndex > history.length` (history was compacted/truncated), or if `diff()` against the stored `processedHeads` throws, discard the existing map and do a full rebuild. This handles edge cases like Automerge compaction or sync resets.

#### Patch application (shared by both paths)

**UPDATED** — actual patch shapes differ from the original plan's spike findings. See Deviation #1.

For text attribution, process three patch types:

- **`splice`**: `{ action: 'splice', path: ['text', index], value: string }` — **Insert** `value.length` new `CharAttribution` entries at `index`:
  ```typescript
  const idx = patch.path[1] as number;
  const newEntries = new Array(patch.value.length).fill({ actor, time });
  entries.splice(idx, 0, ...newEntries);
  ```
- **`del`**: `{ action: 'del', path: ['text', index], length: number }` — **Remove** `length` entries at `index`:
  ```typescript
  const idx = patch.path[1] as number;
  entries.splice(idx, patch.length ?? 1);
  ```
- **`put`**: `{ action: 'put', path: ['text'], value: string }` — **Replace all** entries (field-level initialization or full replacement):
  ```typescript
  entries.length = 0;
  for (let i = 0; i < patch.value.length; i++) entries.push({ actor, time });
  ```
- Skip patches where `path[0]` doesn't match the text field name (e.g., `"text"`)

**Ordering**: Patches within a single diff must be applied in order.

**Exported API surface** for `attribution.ts`:
```typescript
// Cold start — process full history in idle chunks, return new map
// Returns null if history is unavailable or signal is aborted
export function buildAttributionMap(
  handle: DocHandle, textFieldName: string, signal?: AbortSignal,
): Promise<AttributionMap | null>;

// Warm path (synchronous) — process only changes since map.processedHeads
// Returns updated map, or a fresh full-rebuild Promise if history was compacted
export function updateAttributionMap(
  map: AttributionMap, handle: DocHandle, textFieldName: string,
): AttributionMap;

// Query — resolve a source info ID to its node attribution
export function getNodeAttribution(
  sourceInfoId: number,
  reconstructor: SourceInfoReconstructor,
  entries: CharAttribution[],       // source-agnostic (Phase 4 refactor)
  byteToCharMap: number[],
  identities: Record<string, ActorIdentity>,
): NodeAttribution | null;
```

**Byte offset → JS char index conversion**: Source info uses **Rust UTF-8 byte offsets**. The attribution map uses **JS string indices (UTF-16 code units)**, which is also what Automerge uses for text positions. Build a `byteOffset → charIndex` mapping array from the source text in one pass using `TextEncoder().encode(text)`:
- For each UTF-8 byte offset, store the corresponding JS string index
- ASCII (1 byte = 1 code unit): offset === charIndex
- 2-3 byte UTF-8 (e.g., CJK `\u4e16` = 3 bytes): byte offset > charIndex, but 1 UTF-16 code unit
- 4-byte UTF-8 (e.g., emoji `\u{1F600}` = 4 bytes): maps to **2** UTF-16 code units (surrogate pair)
- The mapping must account for surrogate pairs: a single emoji is 4 UTF-8 bytes but 2 JS `.length` units

**Source info resolution**: Use the existing `SourceInfoReconstructor.getSourceLocation(id)` from `ts-packages/annotated-qmd/src/source-map.ts` which handles the full chain (Original → direct, Substring → parent offset translation, Concat → multi-piece assembly) with caching.

To construct the `SourceInfoReconstructor`, convert `astContext.files` (`RustFileInfo[]`) to `SourceContext` format — the pattern is established in `ts-packages/annotated-qmd/test/document-converter.test.ts:38-45`:
```typescript
const sourceContext: SourceContext = {
  files: json.astContext.files.map((f, idx) => ({
    id: idx,
    path: f.name,       // RustFileInfo uses "name", SourceContext uses "path"
    content: f.content || ''  // content is optional in RustFileInfo, required here
  }))
};
```

**Note**: `RustFileInfo` has fields `{ name, line_breaks?, total_length?, content? }` — NOT `{ id, path, content }`. The `id` is the array index; `path` maps from `name`; `content` is optional and must be populated by the consumer before constructing the reconstructor.

**IMPORTANT — `content` is NOT in the WASM output**: The Rust JSON writer (`crates/pampa/src/writers/json.rs:66-72`) serializes `FileEntryJson` with only `{ line_breaks?, name, total_length? }` — no `content` field. The `SourceInfoReconstructor` constructor **throws** if `file.content` is null/undefined (`ts-packages/annotated-qmd/src/source-map.ts:63-67`). Before constructing the reconstructor, the QMD source text must be injected into `astContext.files[0].content`. This text is the same Automerge document content that was passed to the WASM parser. The `Ast` component must receive this text (via `AttributionContext` or as a prop) to populate the field.

**Query function**: `getNodeAttribution(sourceInfoId, reconstructor, entries, byteToCharMap, identities) -> NodeAttribution | null` — calls `reconstructor.getSourceLocation(sourceInfoId)` to get `{ fileId, start, end }` in top-level byte coordinates, converts to char range via `byteToCharMap`, finds the most recent `{ actor, time }` in the `CharAttribution[]` entries for that range, resolves identity.

### Phase 2: React Hook + Context

**New file**: `hub-client/src/hooks/useAttribution.ts`

Hook `useAttribution(filePath, identities)` that:
1. **Initial build** (cold start): On mount, calls `buildAttributionMap(handle, field, signal)` which internally chunks work across idle callbacks. Returns `null` until the Promise resolves. Stores an `AbortController` in a ref so the build can be cancelled.
2. **Incremental update** (warm path): On debounced file change (~500ms), if an `AttributionMap` already exists, calls `updateAttributionMap(existingMap, handle)` synchronously — diffs only from `processedHeads` forward, typically 1 change, O(1) patches. If no map exists yet (edit arrived before initial build completed), the edits will be picked up when the build finishes (it processes the full history including recent changes).
3. **Cancellation on file switch**: When `filePath` changes, the hook aborts the in-flight build (if any) via `abortController.abort()`, resets state to `null`, and starts a fresh build for the new file.
4. **Cancellation on unmount**: Cleanup function aborts any in-flight build.
5. **Fallback to full rebuild**: If `updateAttributionMap` detects history compaction (stored index exceeds history length) or `diff()` fails, it throws a `HistoryCompactedError`. The hook catches this and starts a new async `buildAttributionMap` (returning `null` in the interim until it completes).
6. Returns `{ entries, byteToCharMap }` or `null` (entries is `CharAttribution[]`, extracted from the internal `AttributionMap`)

**Ref stability**: The hook stores the `AttributionMap` in a `useRef` alongside the React state setter, so the debounced callback always operates on the latest map without needing the state value in its closure (avoids stale-closure bugs with rapid edits).

**Context definition** (in the same file or a shared types file):
```typescript
export const AttributionContext = createContext<{
  entries: CharAttribution[];   // source-agnostic per-character attribution
  byteToCharMap: number[];
  identities: Record<string, ActorIdentity>;
  sourceText: string;  // QMD text from Automerge doc, needed to populate astContext.files[].content
} | null>(null);
```

**Design note (Phase 4 refactor)**: The context deliberately exposes `entries: CharAttribution[]` instead of the full `AttributionMap`. The Automerge-specific bookkeeping (`processedHeads`, `processedHistoryIndex`) stays internal to the hook. This allows a future git blame provider to produce the same context shape without any consumer changes.

### Phase 3: Wire Context in Editor, Extract + Render in Ast

**Attribution data flows via context**, avoiding prop drilling through PreviewRouter, ReactPreview, and ReactRenderer:

- [ ] `Editor.tsx` — call `useAttribution(currentFile?.path, identities)`, wrap preview area with `AttributionContext.Provider` passing `{ entries, byteToCharMap, identities, sourceText }` where `sourceText` is the current Automerge document content (already available as the text passed to the preview pipeline)

The `astJson` string flowing through `ReactPreview → ReactRenderer → Ast` is already the full `RustQmdJson` — it contains `astContext` and per-node `s` fields at runtime. No upstream component needs to extract or thread this data.

**File**: `hub-client/src/components/render/ReactAstDebugRenderer.tsx`

**Step 1 — Extend `PandocAST` type** (line 11-15):
```typescript
interface PandocAST {
  'pandoc-api-version': [number, number, number];
  meta: Record<string, unknown>;
  blocks: BlockNode[];
  astContext?: {                          // NEW — optional, present in RustQmdJson output
    sourceInfoPool: SerializableSourceInfo[];
    files: RustFileInfo[];
  };
}
```

**Step 2 — Create local `NodeAttributionContext`** in the same file:
```typescript
const NodeAttributionContext = createContext<{
  getNodeAttribution: (sourceInfoId: number) => NodeAttribution | null;
} | null>(null);
```

**Step 3 — Extract astContext in `Ast` component** (line 85-116):
After `JSON.parse(astJson)`, extract `ast.astContext`. Consume `AttributionContext` via `useContext()`. If both `astContext` and attribution data are present:
1. **Populate `files[].content`** — the WASM output does NOT include file content (the Rust serializer omits it). Inject the QMD source text from `AttributionContext.sourceText` into `astContext.files[0].content` before constructing the reconstructor.
2. Construct a `SourceInfoReconstructor` and `byteToCharMap`.
3. Provide `getNodeAttribution` via `NodeAttributionContext.Provider` wrapping the existing `RegistryContext.Provider`.

When either `astContext` or attribution data is absent, provide `null` — no attribution, no regression.

**Step 4 — Consume in `Node` component** (line 498-535):
```typescript
const attributionCtx = useContext(NodeAttributionContext);
const sourceInfoId = (node as { s?: number }).s;
// If either is missing, render exactly as today
if (sourceInfoId != null && attributionCtx) {
  const attr = attributionCtx.getNodeAttribution(sourceInfoId);
  // Apply color + title tooltip
}
```

Per-node `s` is accessed via type assertion `(node as { s?: number }).s` — the field is present at runtime on all annotated nodes but absent from the simplified local types. This avoids modifying every `BlockNode`/`InlineNode` union member.

**Visual styling**:
- Text color: actor's cursor color (from `attr.color`)
- On hover: native `title` attribute — "{name}, {relative_time}"
- When attribution is null: render exactly as today (no regression)

### Graceful Degradation

- Offline / no sync: `getFileHandle()` returns null → hook returns null → no colors (existing behavior)
- New document / no history: all attributed to local actor
- Non-QMD files: attribution hook is only active for the q2-debug format

## Key Architecture Details

### AST Source Locations
- WASM `parse_qmd_to_ast()` outputs `AstResponse { success, ast?, error?, diagnostics? }` where `ast` is a JSON-serialized `RustQmdJson`
- Each `Annotated_Block`/`Annotated_Inline` node has `s: number` (source info ID)
- `astContext.sourceInfoPool[s]` → `SerializableSourceInfo { r: [startByte, endByte], t: typeCode, d: data }`
  - `t=0` (Original): `d` = file_id (number), `r` = byte range in file
  - `t=1` (Substring): `d` = parent_id (number), `r` = byte range relative to parent — requires chain resolution
  - `t=2` (Concat): `d` = `[[source_info_id, offset, length], ...]` — requires multi-piece assembly
  - `t=3` (FilterProvenance): not relevant for attribution
- `astContext.files[]` → `RustFileInfo { name: string, line_breaks?: number[], total_length?: number, content?: string }`
  - Note: field is `name` not `path`, `content` is optional, no `id` field (use array index)
  - **`content` is NOT serialized by the Rust JSON writer** — must be populated from the Automerge document text before use
  - Must convert to `SourceContext { files: { id, path, content }[] }` for `SourceInfoReconstructor`
- Use `SourceInfoReconstructor.getSourceLocation(id)` (from `ts-packages/annotated-qmd/src/source-map.ts`) to resolve any source info ID to top-level `{ fileId, start, end }` — handles chain resolution with caching
- Types defined in `ts-packages/pandoc-types/src/types.ts`

### Automerge APIs Available (v2.2.9 / repo v2.5.1)
- `handle.history()` → `UrlHeads[]` (ordered, each entry may be an array of tagged hash strings)
  - **Incremental usage**: history is append-only under normal operation — new entries appear at the end. `processedHistoryIndex` marks the boundary between already-applied and new entries. If Automerge compacts history (rare), the list may shrink, which is detected by `processedHistoryIndex > history.length`.
- `handle.metadata(changeHash)` → `DecodedChange | undefined` (contains `{ time: number, actor: string, ... }`)
  - **Heads → hash extraction**: `const changeHash = Array.isArray(heads) ? heads[0] : heads` (see `replay.ts:79`)
- `diff(doc, before: Heads, after: Heads)` → `Patch[]` — import from `@automerge/automerge` (**must be a direct dependency of hub-client** — see Deviation #2)
  - `decodeHeads(urlHeads)` from `@automerge/automerge-repo` converts `UrlHeads` → `Heads` (see `replay.ts:63`)
  - For text: `splice` (insert), `del` (delete), `put` (field init) — see Deviation #1 for actual shapes
  - Position is `patch.path[1]` for splice/del; `put` has no index (field-level)
  - **Incremental usage**: `diff(doc, map.processedHeads, newHeads)` gives only the patches since last update
  - **Timestamps**: `handle.metadata(hash).time` is in **seconds** (not ms) — see Deviation #4
- `view(doc, heads)` → reconstruct document at any point (used by `replay.ts`)
- `getFileHandle(path)` exposed via `automergeSync.ts:260`
- `getActorId()` exposed via `automergeSync.ts:252`
- **No built-in per-character attribution API** — must be reconstructed from history

### Identities
- `Record<string, ActorIdentity>` where `ActorIdentity = { name: string, color: string }`
- Already passed as prop to `Editor` → available for context provider
- 16-color palette in `hub-client/src/services/storage/utils.ts`
- `generateColorFromId(userId)` for fallback color derivation

### Existing Patterns to Reuse
- `replay.ts` — history traversal pattern (`handle.history()` + `handle.metadata()`), including `UrlHeads` → hash extraction (`line 79`) and `decodeHeads()` usage (`line 63`)
- `SourceInfoReconstructor` in `ts-packages/annotated-qmd/src/source-map.ts` — resolves source info ID chains to top-level file coordinates; use `getSourceLocation(id)` for `{ fileId, start, end }`
- `createSourceContext()` pattern in `ts-packages/annotated-qmd/test/document-converter.test.ts:38-45` — converts `RustFileInfo[]` to `SourceContext` format
- `RegistryContext` in `ReactAstDebugRenderer.tsx` — React context pattern for deeply nested components
- `ThemeContext` in `hub-client/src/components/ThemeContext.tsx` — app-level context pattern
- `presenceService.ts` — per-user color state management
- `useReplayMode.test.ts` — vitest mocking pattern for `automergeSync` and `@quarto/quarto-sync-client` (`vi.mock`, `vi.mocked`, `renderHook` from `@testing-library/react`)

## Key Files

| File | Role |
|------|------|
| `hub-client/src/services/attribution.ts` (new) | Core attribution model, history builder, query function |
| `hub-client/src/hooks/useAttribution.ts` (new) | React hook + AttributionContext definition |
| `hub-client/src/components/Editor.tsx` | Wire useAttribution hook, wrap preview with `AttributionContext.Provider` |
| `hub-client/src/components/render/ReactAstDebugRenderer.tsx` | **Primary change site**: extend PandocAST type, extract astContext, add NodeAttributionContext, consume in Node |
| `ts-packages/pandoc-types/src/types.ts` | Reference: RustQmdJson, SerializableSourceInfo, RustFileInfo, SourceContext types |
| `ts-packages/annotated-qmd/src/source-map.ts` | Reuse: SourceInfoReconstructor — resolves source info chains to file coordinates |
| `ts-packages/annotated-qmd/test/document-converter.test.ts` | Reference: createSourceContext() conversion pattern (lines 38-45) |
| `ts-packages/quarto-sync-client/src/replay.ts` | Reference: existing history traversal pattern |
| `hub-client/src/services/storage/utils.ts` | Reference: color palette + generateColorFromId |
| `hub-client/package.json` | Added `@automerge/automerge` direct dependency (Deviation #2) |
| `ts-packages/annotated-qmd/src/*.ts` | Fixed strict-tsconfig compatibility (Deviation #3) |

## Verification

1. **Build**: `npm run build:all` from hub-client (includes WASM + TS build)
2. **Test**: `npm run test:ci` from hub-client
3. **Manual**: Open hub-client in browser with `format: q2-debug`, edit text as two different users, verify:
   - Each AST node's text is colored with the editor's cursor color
   - Hovering shows "{name}, {time}" tooltip
   - Nodes edited by different users show different colors
   - Offline/local editing renders without attribution (no regression)
