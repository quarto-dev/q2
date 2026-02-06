# AST-Level Sync Client API

## Overview

Add AST-level document synchronization to `quarto-sync-client`, enabling consumers to work with parsed Pandoc ASTs instead of raw text. This creates a parallel API alongside the existing text-based `onFileChanged`/`updateFileContent` pair:

- **`onASTChanged(path, ast)`** — callback fired when a QMD file's text changes and the parse succeeds
- **`updateFileAst(path, ast)`** — write an AST back to the synced document (converts to QMD text)

### Design Principles

1. **Dependency injection**: `quarto-sync-client` stays a pure sync layer with no WASM dependency. Parser and writer functions are injected via configuration.
2. **Existing type reuse**: The `@quarto/annotated-qmd` package already defines `RustQmdJson` (the full annotated Pandoc JSON output). This is the AST type.
3. **Incremental write is deferred**: Phase 1 uses the simple QMD writer (full document rewrite). Phase 2 introduces `write_incrementally` in pampa for localized string splicing.

### Architecture Sketch

```
Consumer (hub-client or other app)
    │
    │  provides parser/writer functions at creation time
    │
    ▼
createSyncClient({
  ...,
  astOptions: {
    parseQmd: (content: string) => RustQmdJson | null,
    writeQmd: (ast: RustQmdJson) => string,
  }
})
    │
    │  internally intercepts onFileChanged:
    │    text → parseQmd(text) → if ok → onASTChanged(path, ast)
    │  internally wraps updateFileAst:
    │    ast → writeQmd(ast) → updateFileContent(path, text)
    │
    ▼
Automerge sync layer (unchanged)
```

## Work Items

### Phase 1: Minimal AST sync (dependency-injected parser/writer)

- [x] **1.1** Add new WASM exports to `wasm-quarto-hub-client`
  - `parse_qmd_content(content: string) -> string` — runs pampa QMD reader → JSON writer, returns JSON AST string
  - `ast_to_qmd(ast_json: string) -> string` — runs pampa JSON reader → QMD writer, returns QMD text
  - These are thin wrappers around existing pampa code paths (`readers::qmd::read` + `writers::json::write` and `readers::json::read` + `writers::qmd::write`)

- [x] **1.2** Add TypeScript type declarations for new WASM exports
  - Update `hub-client/src/types/wasm-quarto-hub-client.d.ts`

- [x] **1.3** Extend `SyncClientCallbacks` with optional AST callback
  - Add `onASTChanged?: (path: string, ast: RustQmdJson) => void` to `SyncClientCallbacks`
  - This is optional — existing consumers are unaffected

- [x] **1.4** Add `ASTOptions` configuration type
  - ```typescript
    interface ASTOptions {
      parseQmd: (content: string) => RustQmdJson | null;  // null = parse failure
      writeQmd: (ast: RustQmdJson) => string;
      fileFilter?: (path: string) => boolean;  // default: only .qmd files
    }
    ```
  - Add optional `astOptions?: ASTOptions` to `createSyncClient` parameters

- [x] **1.5** Implement AST interception in `client.ts`
  - When `astOptions` is provided and `onASTChanged` callback exists:
    - After `onFileChanged` fires, call `parseQmd(text)`
    - If parse succeeds (non-null), call `onASTChanged(path, ast)`
    - Cache last successful parse per file (for `updateFileAst` round-trip)
  - When `astOptions` is provided, expose `updateFileAst(path, ast)`:
    - Call `writeQmd(ast)` to get QMD text
    - Call existing `updateFileContent(path, text)` with result

- [x] **1.6** Demo app: `q2-demos/hub-react-todo`
  - Standalone Vite React app that connects to a sync server and renders a todo list from a QMD document's AST
  - Detailed subplan: `claude-notes/plans/2026-02-06-hub-react-todo-demo.md`
  - Note: types factored out into `@quarto/pandoc-types` (zero-dep package); demo depends on that instead of `@quarto/annotated-qmd`

- [ ] **1.7** Tests
  - Unit tests for the interception logic (mock parser/writer)
  - Integration test showing the full flow: text change → AST callback → AST update → text update

### Phase 2: Incremental write (future — design notes only)

- [ ] **2.1** Implement `write_incrementally` in pampa
  - Signature: `pub fn write_incrementally(original_source: &str, original_ast: &Pandoc, new_ast: &Pandoc) -> String`
  - Uses `quarto-ast-reconcile` to compute structural diff between `original_ast` and `new_ast`
  - Uses `SourceInfo` data from `original_ast` to map unchanged subtrees back to original source positions
  - Generates localized string splice operations that only modify changed regions

- [ ] **2.2** Handle indentation blast radius
  - Markdown syntax means some changes have non-local effects (e.g., changing list nesting changes indentation of all child content)
  - Need heuristics or explicit tracking of "indentation context" to determine the minimal region that must be rewritten

- [ ] **2.3** Add WASM export for incremental write
  - `write_qmd_incrementally(original_source: string, original_ast_json: string, new_ast_json: string) -> string`
  - Wire into `ASTOptions` as an optional `writeQmdIncrementally` function

- [ ] **2.4** Update `updateFileAst` to prefer incremental write
  - When `writeQmdIncrementally` is available, use it instead of full `writeQmd`
  - Fall back to full write if incremental write fails

## Key Design Decisions

### Why dependency injection?

- `quarto-sync-client` is a pure TypeScript sync layer with no WASM or parser dependency
- Different consumers may use different parser versions or configurations
- Testing is trivial: inject mock parser/writer functions
- The same pattern hub-client already uses: sync-client provides callbacks, consumer wires up WASM

### Why `RustQmdJson` from `@quarto/annotated-qmd`?

- Already defines the complete annotated Pandoc AST types including source info
- Source info (`astContext.sourceInfoPool`) is critical for Phase 2's incremental write
- Consumers can choose to use plain `PandocDocument` types (also in that package) if they don't need source tracking

### Why optional `onASTChanged`?

- Not all consumers need AST-level events (binary files, non-QMD files)
- Parsing has a cost; consumers opt in explicitly
- `fileFilter` allows consumers to limit which files get parsed (default: `.qmd` only)

### State management for incremental write

For Phase 2, `updateFileAst` needs access to:
- The original source text (available from `getFileContent`)
- The original AST (the last successfully parsed AST — needs caching)

The cache of `(path → { source: string, ast: RustQmdJson })` is maintained internally by the sync client, updated every time `onASTChanged` fires.

## Files Affected

### Phase 1
- `crates/wasm-quarto-hub-client/src/lib.rs` — new WASM exports
- `hub-client/src/types/wasm-quarto-hub-client.d.ts` — type declarations
- `ts-packages/quarto-sync-client/src/types.ts` — `ASTOptions`, updated `SyncClientCallbacks`
- `ts-packages/quarto-sync-client/src/client.ts` — AST interception, `updateFileAst`
- `ts-packages/quarto-sync-client/src/index.ts` — export new types
- `hub-client/src/services/automergeSync.ts` — proof-of-concept wiring

### Phase 2
- `crates/pampa/src/lib.rs` or new module — `write_incrementally` function
- `crates/wasm-quarto-hub-client/src/lib.rs` — new WASM export
- `ts-packages/quarto-sync-client/src/client.ts` — incremental write path
