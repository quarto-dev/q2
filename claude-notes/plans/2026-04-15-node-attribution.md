# Node Attribution Feature

## What it does

This feature adds **per-node authorship coloring** to the AST debug view in Quarto Hub (a collaborative document editor). When multiple people edit the same Quarto Markdown document, each AST node in the debug preview is colored to indicate **who last edited** the source text behind that node. Hovering over a node shows a small badge with the author's name and a relative timestamp ("2h ago").

It's gated behind a user preference — toggle **Authorship** in the Settings sidebar while using the `q2-debug` preview format. The toggle only appears when the current format is `q2-debug` and its state is persisted via `usePreference('attributionEnabled')`.

## The problem it solves

In a collaborative editor backed by Automerge (a CRDT), the document tracks the full history of changes by every participant. But the rendered preview shows no indication of who wrote what. This feature surfaces that information visually on the parsed AST, which is useful for debugging and understanding collaborative editing behavior.

## Architecture overview (data flow)

```
Automerge Document (full edit history)
        │
        ▼
  ① Attribution Producer — replays history, builds a run-list map
     (attribution-runs.ts; per-char path in attribution.ts kept as reference)
        │
        ▼
  ② useAttribution Hook — lifecycle (async build, incremental updates),
     wraps the producer in an AttributionSource
        │
        ▼
  ③ AttributionContext — carries { source, identities, sourceText }
        │
        ▼
  ④ AstRenderer — builds SourceInfoReconstructor, binds getNodeAttribution
     against the source, caches per-render
        │
        ▼
  ⑤ Node component — reads `node.s`, colors by author, provides hover data
```

The key boundary is `AttributionSource.queryByteRange(fileId, byteStart, byteEnd)`. Everything downstream of the hook depends only on that query function, not on the storage used to answer it — so the producer can be the run-list implementation (default), a per-char array (reference), a git-blame adapter (in tests), or anything else that can answer byte-ranged "who most recently wrote here?" queries.

## Layer-by-layer breakdown

### 1. Attribution service

**`hub-client/src/services/attribution.ts`** — shared types and the consumer-facing boundary:

- **`AttributionSource`** — the query interface every producer implements. Single method: `queryByteRange(fileId, byteStart, byteEnd) → { actor, time } | null`.
- **`getNodeAttribution(sourceInfoId, reconstructor, source, identities)`** — resolves an AST source-info ID through the `SourceInfoReconstructor` to a byte range, queries the source, and attaches display-name/color from the identities map (falling back to `actor.slice(0, 8)` and `#888888`).
- **`buildByteToCharMap(text)`** — UTF-8 byte offset → JS char index. Correctly handles 2-byte accented chars, 3-byte CJK, and 4-byte emoji (which become surrogate pairs in JS strings). Used by producers to translate byte ranges (from the Rust parser's source info) into char indices for any per-char storage.
- **`makeCharArraySource(entries, byteToCharMap)`** — reference producer: wraps a flat `CharAttribution[]` as an `AttributionSource`. Kept for tests and as a reference implementation.
- **`buildAttributionMap` / `updateAttributionMap`** — per-char Automerge producer. Retained for tests but no longer used in production. `applyPatch` chunks large splices into 10K-element calls to avoid V8's argument-spread overflow (see Performance results below).

**`hub-client/src/services/attribution-runs.ts`** — run-length-encoded Automerge producer, default since `3265de11`:

- **`buildRunListAttribution(handle, textFieldName, signal?)`** — replays Automerge history in chunks of 50 via `requestIdleCallback` (`{ timeout: 100 }` so the build can't be starved while React is mounting), applying each patch in-place to a sorted `AttributionRun[]`. The first chunk runs without yielding so cold start begins immediately.
- **`updateRunListAttribution(state, handle, textFieldName)`** — synchronous warm-path update from the last processed heads. Throws `HistoryCompactedError` if Automerge garbage-collected older history, signalling a full rebuild is required.
- **`applyPatchToRuns`** — splice/del/put on the run list. Splice: binary-search for the insertion point, split a straddled run, shift all runs past the cut in place, then merge with like-attributed neighbours. Delete: trim or remove overlapping runs, shift the tail. The in-place approach (vs. full-array rebuild) is load-bearing for RLE's parity on realistic workloads.
- **`makeRunListSource(runs, byteToCharMap)`** — binary-search the first run whose end exceeds the query start, then scan forward while runs overlap the query range and track the maximum time.

**`hub-client/src/services/attribution-gitblame.ts`** — alternate producer for `git blame --porcelain` output. Not wired into production yet but available:

- **`parseBlamePorcelain(output)`** — porcelain text → `BlameLine[]` (caches commit metadata across lines from the same commit).
- **`buildBlameRuns(blame, text)`** — line records → `BlameRun[]` with byte offsets computed by `TextEncoder` (handles multi-byte UTF-8 correctly).
- **`makeGitBlameSource(runs)`** — `AttributionSource` that binary-searches the run list.
- **`blameSourceFromPorcelain(porcelain, sourceText)`** — one-call convenience that wires all three together.

The module is pure JS — no git shell-out, no node-only APIs. Consumers supply the porcelain text from whatever source they choose (backend RPC, preloaded blob, server-rendered HTML dataset). Each source represents one blame'd file, so `queryByteRange` only honours `fileId === 0`.

### 2. React hook (`hub-client/src/hooks/useAttribution.ts`)

Manages lifecycle:

- **On mount / file path change**: starts an async `buildRunListAttribution`. Returns `null` until resolved.
- **On source text change** (debounced 500 ms): calls the synchronous `updateRunListAttribution` if a map already exists.
- **On `HistoryCompactedError`**: triggers a fresh full rebuild.
- **On unmount / path change**: aborts any in-flight build via `AbortController`.

Uses `useRef` for the current `RunListAttribution` state to avoid stale closures in the debounced callback. The public return type is `{ source: AttributionSource } | null` — Automerge-specific bookkeeping (`processedHeads`, `processedHistoryIndex`) is kept private to the ref. Also exports `AttributionContext`, which carries `{ source, identities, sourceText }` down the component tree.

### 3. Editor integration (`hub-client/src/components/Editor.tsx`)

1. Reads the user preference via `usePreference('attributionEnabled')` and gates on `currentFormat === 'q2-debug'`. (An earlier iteration used a `attribution: true` YAML metadata flag; that was replaced in `3ab292ab` to make the setting per-user, not per-document.)
2. Calls `useAttribution(filePath, displayContent)` — passing `null` for the path when disabled (making the hook return `null`).
3. Builds `attributionContextValue = { source, identities, sourceText }` and wraps `<PreviewRouter>` with `<AttributionContext.Provider>`.
4. Passes `attributionEnabled` and `setAttributionPref` to `SettingsTab`, which renders the "Authorship" checkbox only when `currentFormat === 'q2-debug'`.

### 4. AST debug renderer (`hub-client/src/components/render/ReactAstDebugRenderer.tsx`)

The `AstRenderer` component (registered as the `"Ast"` entry in the component registry):

1. **Reads `AttributionContext`** from the provider above.
2. **Constructs a `SourceInfoReconstructor`** from the AST's `astContext` (source info pool + file metadata), injecting the current Automerge document text as `files[0].content` because the WASM JSON output doesn't serialize file content.
3. **Creates a cached `getNodeAttribution` closure** — wraps the service function with a `Map<number, NodeAttribution | null>`. The cache is invalidated automatically when the `useMemo` recomputes (new AST or new attribution context).
4. **Provides via `NodeAttributionContext`** — a narrower context exposing only the `getNodeAttribution` function.
5. **Event-delegated hover handling** — a single `onMouseOver`/`onMouseOut` on the container walks up to the nearest `.q2-attr-wrap[data-sid]`, reads the source info ID, calls `getNodeAttribution`, and sets a `hoveredAttr` state with `getBoundingClientRect()`. One floating `AttributionBadge` renders on that state — no hidden per-node badges (optimization landed in `e41cccc6`).

The **`Node` component** reads `sourceInfoId` from `node.s`, calls `getNodeAttribution(sourceInfoId)`, and if attributed wraps the node in a `<div>` or `<span>` with `className="q2-attr-wrap"`, `data-sid={sourceInfoId}`, and `style={{ color: attr.color }}`.

The **`AttributionBadge`** is a small styled tooltip showing a colored dot, the author's name, and a relative timestamp. It uses CSS custom properties (`--attr-color`) for theming and is `position: fixed` so the container can place it just below the hovered node. Replaced the native `title` tooltip in `a49e4cb9`. Unknown actors fall back to the first 8 chars of the actor ID (`0f08bcef`).

### 5. Annotated QMD compatibility fixes (`ts-packages/annotated-qmd/`)

Minor tweaks to fix build issues under hub-client's stricter TypeScript config: explicit field declarations instead of `private` constructor shorthand, unused import cleanup, `type`-only imports where appropriate, underscore-prefixed unused variable. Not functional changes.

## Key design decisions

1. **Opt-in via Settings toggle** — Attribution is expensive (replays full history), so it's off by default. Users enable it via the "Authorship" toggle in the Settings sidebar, only shown for `q2-debug` previews, persisted via `usePreference('attributionEnabled')` (`3ab292ab`). Earlier iteration used a YAML metadata flag; replaced so the setting is per-user, not per-document.

2. **Two-tier update strategy** — Full async build on cold start (chunked via `requestIdleCallback` to avoid UI jank), then cheap synchronous incremental updates on each edit. Fallback to full rebuild if history is compacted.

3. **"Most recent writer wins"** — For nodes spanning multiple characters with different authors, the character with the most recent timestamp determines the node's attribution. "Last touch" heuristic.

4. **Event delegation** — One mouse handler on the container instead of N handlers on each node. Uses `closest('.q2-attr-wrap[data-sid]')` for target resolution.

5. **Context-based data flow** — No prop drilling through `PreviewRouter` → `ReactPreview` → `ReactRenderer`. The `AttributionContext` and `NodeAttributionContext` skip straight from `Editor` to the leaf renderer.

6. **Caching at multiple levels** — `byteToCharMap` computed once per source text inside `useAttribution` and shared via `AttributionContext`; `getNodeAttribution` results cached per render cycle; Automerge history processed incrementally; badge DOM allocated lazily on hover instead of emitted for every node (`e41cccc6`).

7. **`AttributionSource` query-interface boundary** (`f44f76c0`) — Consumers depend only on `queryByteRange`, not on the producer's storage shape. Originally introduced `CharAttribution[]` at the boundary (`1a544289`); the query-interface follow-up replaced that with a query function so producers can freely choose RLE, segment trees, typed arrays, or external sources. The `attribution-gitblame.ts` adapter is a concrete second producer — it consumes `git blame --porcelain` output end-to-end without touching any consumer code.

8. **Run-length encoding as the default producer** (`3265de11`) — Automerge history is replayed into a sorted `AttributionRun[]` rather than a per-char `CharAttribution[]`. Realistic batched workloads see 4× faster updates, 5× faster queries, 20× smaller storage, and arbitrary-size bulk inserts don't need the splice-chunking workaround. Numbers below.

9. **Cold-start latency tuning** (`d880d643`) — Two small, unambiguous changes to `buildRunListAttribution` / `waitForIdle`: (a) skip the `await waitForIdle()` before the *first* chunk so attribution work begins immediately, and (b) pass `{ timeout: 100 }` to `requestIdleCallback` so the build isn't starved when React is still mounting. A progressive-rendering variant was tried and reverted — partial runs describe an intermediate text state whose byte positions don't align with the current AST, so intermediate paints could briefly show attribution against the wrong bytes.

## Performance results

Bench harness: `cd hub-client && npm run bench`. Not wired into CI.

**`applyPatch` single-patch size limit**: ~118K chars before `3c704749` (V8 argument-stack overflow on splice spread), ~1M+ after (chunked into 10K-element splices).

**RLE vs per-char, realistic batched workloads (100K-char doc):**

| Measure | per-char | RLE | ratio |
|---|---|---|---|
| Build | 123 ms | 126 ms | parity |
| Update (one mid-doc patch) | 318 µs | 72 µs | **4.4×** faster |
| Query (range=1000) | 1.03 µs | 206 ns | **5×** faster |
| Query (range=100) | 199 ns | 86 ns | 2.3× faster |
| Storage | 100K entries | 5K runs | **20×** fewer |

**Bulk inserts (single big patch):**

| N | per-char | RLE |
|---|---|---|
| 100K | 2.27 ms | 1.17 ms |
| 1M | (chunked, slow) | 1.15 ms |

RLE build time is size-independent (one run regardless of N); bulk inserts no longer need the splice-chunking workaround.

**RLE pessimal case**: pathological 1-char-per-history-entry prepend at N=100K runs 1.94× slower than per-char (10.68 s vs 5.51 s). Doesn't occur in production — Automerge batches keystrokes per change.

**Correctness**: `attribution-runs.test.ts` includes 23 unit tests with cross-validation that `makeRunListSource` and `makeCharArraySource` return identical `queryByteRange` results on append/prepend/random synthetic histories.

### Deferred indefinitely

| Item | Why skipped |
|---|---|
| Block-max / segment tree for queries | RLE queries already 86–206 ns at 100K; further work is noise. |
| Typed arrays / actor-ID interning | RLE's 20× storage reduction suffices at current/projected sizes. |
| Incremental `byteToCharMap` updates | Not a measured pain point; rebuild is O(bytes), debounced 500 ms. |

Reopen if users report slow cold-start, we ship multi-MB documents, or a new profile flags something.

## Commit history (this thread)

| Commit | What |
|---|---|
| `1a544289` | Decouple `CharAttribution[]` from `AttributionMap` internals |
| `f44f76c0` | Replace `CharAttribution[]` at the consumer boundary with `AttributionSource` |
| `3c704749` | Fix `applyPatch` stack overflow on large splice inserts |
| `3265de11` | Switch attribution producer to run-length encoding (add bench harness) |
| `2992f4b4` | Extract git-blame `AttributionSource` adapter into production module |
| `d880d643` | Reduce attribution cold-start latency (skip first idle-wait, add `rIC` timeout) |
