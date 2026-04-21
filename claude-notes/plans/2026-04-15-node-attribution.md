# Node Attribution Feature

## What it does

This feature adds **per-node authorship coloring** to the AST debug view in Quarto Hub (a collaborative document editor). When multiple people edit the same Quarto Markdown document, each AST node in the debug preview is colored to indicate **who last edited** the source text behind that node. Hovering over a node shows a small badge with the author's name and a relative timestamp ("2h ago").

It's gated behind a YAML metadata flag — you must add `attribution: true` to the document frontmatter and use the `q2-debug` preview format.

## The problem it solves

In a collaborative editor backed by Automerge (a CRDT), the document tracks the full history of changes by every participant. But the rendered preview shows no indication of who wrote what. This feature surfaces that information visually on the parsed AST, which is useful for debugging and understanding collaborative editing behavior.

## Architecture overview (data flow)

```
Automerge Document (full edit history)
        │
        ▼
  ① Attribution Service — replays history, builds per-character map
        │
        ▼
  ② useAttribution Hook — manages lifecycle (async build, incremental updates)
        │
        ▼
  ③ AttributionContext (React Context) — carries map + identities into component tree
        │
        ▼
  ④ AstRenderer — builds SourceInfoReconstructor, maps AST nodes → attributions
        │
        ▼
  ⑤ Node component — colors each AST node by author, provides data for hover badge
```

## Layer-by-layer breakdown

### 1. Attribution Service (`hub-client/src/services/attribution.ts`)

This is the core engine. It has three main functions:

**`buildAttributionMap`** — Cold-start full build. It replays the entire Automerge history to build an array where `entries[i]` tells you who last wrote character `i` and when. It processes history in chunks of 50 entries, yielding to the browser's idle callback between chunks so the UI doesn't freeze. It supports an `AbortSignal` for cancellation.

For each history entry, it calls Automerge's `diff()` between consecutive document states to get patches (splice = insert text, del = delete text, put = replace field). Each patch is applied to the entries array, attributing the affected characters to the current actor.

**`updateAttributionMap`** — Warm-path incremental update. After the initial build, subsequent edits only need to process new history entries (typically 1-2). This is synchronous and fast. If the history has been compacted (Automerge garbage-collected old entries), it throws `HistoryCompactedError` to signal a full rebuild is needed.

**`buildByteToCharMap`** — The Rust parser produces source locations as UTF-8 byte offsets, but JavaScript strings use UTF-16 code units. This function builds a mapping array so byte offset N can be converted to the correct JS character index. It correctly handles multi-byte UTF-8 sequences (2-byte for accented chars, 3-byte for CJK, 4-byte for emoji which become surrogate pairs in JS).

**`getNodeAttribution`** — Given an AST node's source info ID, resolves it to a byte range in the source file (via `SourceInfoReconstructor`), converts to a char range (via the byte-to-char map), scans that range of the attribution array, and returns the **most recent** attribution (highest timestamp). It also looks up the actor's identity (display name and color).

### 2. React Hook (`hub-client/src/hooks/useAttribution.ts`)

Manages the attribution lifecycle:

- **On mount** (or file path change): starts an async `buildAttributionMap`. Returns `null` until it resolves.
- **On source text change** (debounced 500ms): calls the synchronous `updateAttributionMap` if a map already exists.
- **On `HistoryCompactedError`**: triggers a fresh full rebuild.
- **On unmount or path change**: aborts any in-flight build via `AbortController`.

Uses `useRef` for the map to avoid stale closures in the debounced callback.

Also exports `AttributionContext` — a React context that carries the attribution map, byte-to-char map, actor identities, and source text down the component tree.

### 3. Editor Integration (`hub-client/src/components/Editor.tsx`)

The Editor component:
1. Checks if attribution is enabled: format must be `q2-debug` AND the parsed AST metadata must contain `attribution: true` (supports both `MetaBool` and `MetaString` forms).
2. Calls `useAttribution(filePath, displayContent)` — passing `null` for the path when disabled (which makes the hook return `null`).
3. Wraps the `<PreviewRouter>` with `<AttributionContext.Provider>` so the debug renderer can consume it.

### 4. AST Debug Renderer (`hub-client/src/components/render/ReactAstDebugRenderer.tsx`)

The `AstRenderer` component (registered as the "Ast" entry in the component registry):

1. **Reads `AttributionContext`** from the provider above.
2. **Constructs a `SourceInfoReconstructor`** using the AST's `astContext` (source info pool and file metadata). It injects the current Automerge document text as `files[0].content` because the WASM JSON output doesn't serialize file content.
3. **Creates a cached `getNodeAttribution` closure** — wraps the service function with a `Map<number, NodeAttribution | null>` cache. The cache is invalidated automatically when the `useMemo` recomputes (new AST or new attribution context).
4. **Provides via `NodeAttributionContext`** — a second, narrower context that only exposes the `getNodeAttribution` function.
5. **Event-delegated hover handling** — instead of attaching mouse handlers to every node, a single `onMouseOver`/`onMouseOut` on the container element walks up to the nearest `.q2-attr-wrap[data-sid]` element, reads the source info ID from `data-sid`, calls `getNodeAttribution`, and positions a floating `AttributionBadge`.

The **`Node` component** (renders every block/inline in the tree):
1. Reads `sourceInfoId` from `node.s` (the source info field present at runtime in the Pandoc AST, though not in the simplified TypeScript types).
2. Calls `getNodeAttribution(sourceInfoId)` from context.
3. If attributed, wraps the node in a `<div>` or `<span>` with `className="q2-attr-wrap"`, `data-sid={sourceInfoId}`, and `style={{ color: attr.color }}`.

The **`AttributionBadge`** is a small styled tooltip showing a colored dot, the author's name, and a relative timestamp. It uses CSS custom properties for theming.

### 5. Annotated QMD changes (`ts-packages/annotated-qmd/`)

Minor cleanup changes to fix build issues under the hub-client's stricter TypeScript config:
- Changed `private` constructor parameter shorthand to explicit field declarations (fixes a TypeScript `isolatedDeclarations` issue).
- Removed unused imports (`BlockConverter`, `InlineConverter`, `asMappedString`).
- Changed `MappedString` import to a `type` import where it's only used as a type.
- Renamed an unused variable from `offset` to `_offset`.

These are compatibility fixes, not functional changes.

## Key design decisions

1. **Opt-in via YAML metadata** — Attribution is expensive (replays full history), so it's off by default. Users enable it with `attribution: true` in frontmatter.

2. **Two-tier update strategy** — Full async build on cold start (chunked to avoid UI jank), then cheap synchronous incremental updates on each edit. Fallback to full rebuild if history is compacted.

3. **"Most recent writer wins"** — For nodes spanning multiple characters with different authors, the character with the most recent timestamp determines the node's attribution. This gives a reasonable "last touch" heuristic.

4. **Event delegation** — One mouse handler on the container instead of N handlers on each node. The handler uses DOM traversal (`closest('.q2-attr-wrap[data-sid]')`) to find the relevant node.

5. **Context-based data flow** — No prop drilling through intermediate components (`PreviewRouter` → `ReactPreview` → `ReactRenderer`). The `AttributionContext` and `NodeAttributionContext` skip straight from Editor to the leaf renderer.

6. **Caching at multiple levels** — `byteToCharMap` computed once per source text, `getNodeAttribution` results cached per render cycle, Automerge history processed incrementally.
