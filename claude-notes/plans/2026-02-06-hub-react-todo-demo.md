# Demo: hub-react-todo

**Parent plan:** `claude-notes/plans/2026-02-06-ast-sync-client-api.md` (item 1.6)
**Beads issue:** `bd-3lsb`

## Overview

A standalone Vite React app that demonstrates AST-level sync. A user edits a QMD document in hub-client's Monaco editor; a second user (or browser tab) running this demo sees a live-updating React todo list rendered from the document's Pandoc AST.

### Demo Scenario

1. User A opens a project in hub-client containing a QMD file with a `#todo` div
2. User B opens `hub-react-todo` pointed at the same project/file
3. User B sees a rendered todo list (checkboxes + labels)
4. User A edits the markdown (adds/removes/checks items) — User B's view updates in real time
5. (Future) User B clicks a checkbox — the markdown document updates, and User A sees the change

### QMD Document Format

```markdown
:::{#todo}

- [ ] Buy groceries
- [x] Write code
- [ ] Review PR

:::
```

### Corresponding AST Structure

```
Div (id="todo")
  └── BulletList
        ├── Item 0: [Plain: [Span([],[]), Space, Str("Buy"), Space, Str("groceries")]]
        ├── Item 1: [Plain: [Span([],[Str("x")]), Space, Str("Write"), Space, Str("code")]]
        └── Item 2: [Plain: [Span([],[]), Space, Str("Review"), Space, Str("PR")]]
```

- **Unchecked:** `Span` with empty content (`c[1]` is `[]`)
- **Checked:** `Span` whose content is `[{t: "Str", c: "x"}]`
- **Label:** remaining inlines after the Span in the Plain block

## Work Items

### Setup

- [x] **S.1** Create `q2-demos/` directory and add to npm workspace
  - Update root `package.json` workspaces to include `q2-demos/*`

- [x] **S.2** Scaffold `q2-demos/hub-react-todo` as a Vite React app
  - `package.json` with deps: `react`, `react-dom`, `@quarto/quarto-sync-client`, `@quarto/pandoc-types`
  - `vite.config.ts` with WASM support and `resolve.conditions: ['source']`
  - `tsconfig.json`, `tsconfig.app.json`
  - `index.html` entry point
  - Symlink to WASM package: `q2-demos/hub-react-todo/wasm-quarto-hub-client -> ../../crates/wasm-quarto-hub-client/pkg`
  - Note: originally planned `@quarto/annotated-qmd` dep; changed to `@quarto/pandoc-types` (new zero-dep types package)

### Connection UI

- [x] **C.1** Create `src/App.tsx`
  - Hardcoded to `wss://sync.automerge.org` / `FqXQmLvicAYfARgVMdSjrsMiS54` / `todo.qmd`
  - Auto-connects on mount; shows connection params in a header bar

### WASM Integration

- [x] **W.1** Create `src/wasm.ts` — thin wrapper for WASM initialization and parse function
  - `initWasm()`, `parseQmdContent()`, `writeQmdFromAst()`

### Sync + AST Hook

- [x] **H.1** Create `src/useSyncedAst.ts` — React hook for synced AST
  - Takes: `{ syncServer, indexDocId, filePath }`
  - Returns: `{ ast, connected, error, connecting }`

### AST Extraction

- [x] **E.1** Create `src/astHelpers.ts` — extract todo items from AST
  - `findTodoDiv(ast)` and `extractTodoItems(div)` implemented
  - `TodoItem = { checked, label, itemIndex }`

### React Components

- [x] **R.1** Create `src/TodoList.tsx` — renders todo items
- [x] **R.2** Create `src/TodoApp.tsx` — connects AST to TodoList

### Integration & Testing

- [ ] **T.1** Manual integration test
  - Run `npm run dev` in `q2-demos/hub-react-todo`
  - Open in browser; should auto-connect and show todo list from `todo.qmd`
  - Edit the QMD in hub-client and observe the todo list updating

## File Structure

```
q2-demos/
├── README.md
└── hub-react-todo/
    ├── package.json
    ├── tsconfig.json
    ├── tsconfig.app.json
    ├── tsconfig.node.json
    ├── vite.config.ts
    ├── index.html
    ├── wasm-quarto-hub-client -> ../../crates/wasm-quarto-hub-client/pkg
    ├── README.md
    └── src/
        ├── main.tsx              — React entry point
        ├── App.tsx               — Connection form + routing
        ├── wasm.ts               — WASM init + parseQmdContent wrapper
        ├── useSyncedAst.ts       — React hook: sync client + AST
        ├── astHelpers.ts         — Extract TodoItem[] from Pandoc AST
        ├── TodoApp.tsx           — Connects AST to TodoList
        ├── TodoList.tsx          — Renders checkboxes + labels
        └── styles.css            — Minimal styling
```

## Dependencies

### Requires from parent plan (must be done first)

- **1.1** WASM export `parse_qmd_content` (for `wasm.ts`)
- **1.3–1.5** Sync client `onASTChanged` / `ASTOptions` API (for `useSyncedAst.ts`)

### Package dependencies

```json
{
  "dependencies": {
    "@quarto/quarto-sync-client": "*",
    "@quarto/annotated-qmd": "*",
    "react": "^19.2.0",
    "react-dom": "^19.2.0"
  },
  "devDependencies": {
    "typescript": "~5.9.3",
    "vite": "^7.2.4",
    "@vitejs/plugin-react": "^5.1.1",
    "vite-plugin-wasm": "^3.5.0",
    "@types/react": "^19.2.5",
    "@types/react-dom": "^19.2.3"
  }
}
```

## Design Notes

### Error handling strategy

- **Parse errors**: `console.warn()` the error, keep showing the last successfully parsed AST. The `parseQmdContent` wrapper returns `null` on failure. The sync client only fires `onASTChanged` on successful parses.
- **Missing #todo div**: `TodoApp` shows a message like "No #todo div found in document"
- **Malformed bullet list**: `extractTodoItems` returns empty array; `TodoApp` shows "No todo items found"
- **Connection errors**: shown in the connection UI

### Future: checkbox toggle (Phase 2 integration)

When clicked, `onToggle(itemIndex)` would:
1. Clone the current AST
2. Find the bullet list item at `itemIndex`
3. Toggle the Span content (empty ↔ `[{t: "Str", c: "x"}]`)
4. Call `updateFileAst(path, modifiedAst)`
5. This triggers `writeQmd(ast)` → `updateFileContent(path, text)` → syncs to hub-client

This requires Phase 1.5's `updateFileAst` and ideally Phase 2's incremental write (otherwise toggling a checkbox reformats the entire document).
