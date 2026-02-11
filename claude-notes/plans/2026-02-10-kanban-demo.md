# Kanban Demo App — Plan

## Overview

A minimal Kanban-style project tool backed by QMD files, demonstrating
bidirectional editing via React components + the incremental writer. Users can
edit the markdown directly or interact through React-provided views; the
document remains human-readable throughout.

This follows the same architecture as `q2-demos/hub-react-todo`: WASM parser,
`@quarto/quarto-sync-client` for Automerge sync, `useSyncedAst` hook for
reactive AST state, and AST helpers for reading/mutating the document structure.

## QMD Document Representation

Each **card** is a level-2 header plus its body content:

```qmd
## Card Title {.type key="value" created="ISO-timestamp"}

Free-form markdown content for the card body.
```

### Card types (header classes)

| Class        | Meaning                                       |
|------------- |-----------------------------------------------|
| `.feature`   | A feature/work item card                      |
| `.milestone` | A milestone that groups/tracks other cards     |
| `.bug`       | A bug report                                  |
| `.task`      | A generic task                                |

(No class = generic card. The app treats unknown classes gracefully.)

### Metadata (key-value attributes on the header)

| Key        | Meaning                                    | Example                     |
|----------- |--------------------------------------------|-----------------------------|
| `created`  | ISO timestamp of card creation             | `2026-02-10T14:30`          |
| `deadline` | ISO date for milestones / due dates        | `2026-03-25`                |
| `status`   | Card status                                | `todo`, `doing`, `done`     |
| `priority` | Priority level                             | `high`, `medium`, `low`     |

### Cross-references

Cards reference each other using standard QMD links:

- **Forward link (in milestone item lists):** `[Card Title](#card-id)` — the
  header text is slugified to produce the anchor ID (e.g., "Project Export" →
  `#project-export`).
- **Dependency link:** `[other-card](#other-card)` in a "Depends on" list.
  (Future: when pampa supports `<#anchor>` shorthand, we can use that instead.)
- **Checkbox items inside milestones:** `- [ ] [Card Title](#card-id)` tracks
  completion of linked cards.

### Example document (already at `q2-demos/kanban/example-documents/example1.qmd`)

```qmd
# Cards

## Work Week {.milestone deadline="2026-03-25" created="2026-02-10T08:56"}

Items:

- [ ] [Project Export](#project-export)

## Project Export {.feature created="2026-02-10T14:30"}

## ACLs for automerge {.feature created="2026-02-10T14:40"}

Automerge doesn't naturally support read-only modes. We will need a websocket
proxy that understands which users are allowed to send patches...
```

## Architecture

Follows the hub-react-todo pattern:

```
  Automerge sync server
         |
  quarto-sync-client  ←→  useSyncedAst hook
         |
  KanbanApp component
         |
  astHelpers.ts  (extract cards, mutate AST)
         |
  View components (BoardView, CardDetail, etc.)
```

### Files to create (under `q2-demos/kanban/`)

```
q2-demos/kanban/
  index.html
  package.json
  vite.config.ts
  vitest.config.ts                # Unit test config (Node env)
  vitest.integration.config.ts    # Component test config (jsdom env)
  vitest.wasm.config.ts           # WASM integration test config
  tsconfig.json
  tsconfig.app.json
  tsconfig.node.json
  src/
    main.tsx                  # React root
    App.tsx                   # Config (server URL, doc ID, file path)
    KanbanApp.tsx             # Main app: connect AST → views
    useSyncedAst.ts           # Reuse from hub-react-todo (copy or share)
    wasm.ts                   # WASM wrapper (copy from hub-react-todo)
    astHelpers.ts             # Card extraction and AST mutation
    types.ts                  # Card, Milestone, CardRef interfaces
    components/
      BoardView.tsx           # Card grid/board view
      CardComponent.tsx       # Single card rendering
      MilestoneView.tsx       # Milestone view with linked items
    __tests__/
      fixtures/               # AST JSON fixtures for unit tests
        example1.json         # Parsed AST of example1.qmd
        edge-cases.json       # Cards with missing attrs, empty bodies, etc.
      astHelpers.test.ts      # Layer 1: pure unit tests for extraction/mutation
      components.integration.test.tsx  # Layer 2: React component render tests
      pipeline.wasm.test.ts   # Layer 3: QMD string → board integration tests
    wasm-js-bridge/
      sass.js                 # Stub
      template.js             # Stub
```

## Testing Strategy

Three layers, mirroring the hub-client's established Vitest setup. The goal is
to build a catalog of QMD document variations as test fixtures, providing
guardrails for a loosely-specified, permissive syntax.

### Layer 1: Pure AST helper unit tests (Node env, no DOM, no WASM)

**What**: Test all `astHelpers.ts` extraction and mutation functions against
pre-generated AST JSON fixtures. This is the primary guardrail layer.

**How**: Generate fixtures by parsing example QMD documents through the WASM
module once (or via `cargo run -p pampa`), saving the resulting AST JSON. Tests
import these fixtures and assert on the output of `extractCards()`,
`extractCardRefs()`, `buildBoard()`, `setCardStatus()`, etc.

**Test cases to cover**:
- `example1.qmd` — the baseline: milestones, features, cross-refs, dependencies
- Cards with no class (generic card)
- Cards with no metadata attributes
- Cards with empty bodies (header only)
- Cards with rich body content (paragraphs, code blocks, nested lists)
- Milestones with mixed checked/unchecked items
- Cards with multiple cross-references
- Documents with only one card
- Documents with no level-2 headers (no cards)
- Cards with unknown/unexpected classes (graceful handling)
- Mutation tests: `setCardStatus` adds attribute when missing, updates when present
- Mutation tests: `toggleMilestoneItem` toggles the correct checkbox
- Mutation tests: `addCard` appends correctly

**Config**: `vitest.config.ts` — Node environment, `src/**/*.test.ts` pattern.

### Layer 2: Component rendering tests (jsdom + React Testing Library)

**What**: Test that React components render the correct DOM given specific
`KanbanBoard` data, and that interactions fire the right callbacks.

**How**: Use `@testing-library/react` to render components with mock data.
No WASM or sync client needed — components receive pre-built `KanbanBoard`
objects.

**Test cases to cover**:
- `BoardView` renders cards in the correct status columns
- `BoardView` renders an "unset" column for cards without status
- `CardComponent` displays title, type badge, metadata
- `CardComponent` truncates long body previews
- Status change interaction fires `onStatusChange(cardId, newStatus)`
- `MilestoneView` renders linked items with checkboxes
- Milestone checkbox toggle fires the right callback

**Config**: `vitest.integration.config.ts` — jsdom environment, setup file with
browser API mocks (ResizeObserver, etc.), `*.integration.test.tsx` pattern.

### Layer 3: QMD → Board integration tests (WASM env)

**What**: End-to-end pipeline tests that parse actual QMD strings through the
WASM module and verify the resulting `KanbanBoard`. Also tests round-trips:
parse → mutate → incremental write → re-parse → verify.

**How**: Initialize the WASM module in test setup (30s timeout), parse QMD
strings, run through `buildBoard()`, assert on the result. For round-trip tests,
mutate the AST, write back via incremental writer, re-parse, and verify the
mutation took effect while other content is preserved.

**Test cases to cover**:
- Parse `example1.qmd` → `buildBoard()` → verify card count, types, refs
- Round-trip: parse → `setCardStatus` → incremental write → re-parse → verify
  status changed and other content preserved verbatim
- Round-trip: parse → `toggleMilestoneItem` → incremental write → re-parse
- Round-trip: parse → `addCard` → incremental write → re-parse → verify new
  card present and original document unchanged

**Config**: `vitest.wasm.config.ts` — Node environment, 30s timeout,
`*.wasm.test.ts` pattern.

### Generating and maintaining fixtures

AST fixtures are generated from QMD documents using the pampa CLI:

```bash
cargo run -p pampa -- -f qmd -t json < example1.qmd > fixtures/example1.json
```

When the parser changes in ways that affect the AST structure, fixtures can be
regenerated. The fixtures are checked into the repo so tests run without
requiring a Rust build.

## Work Items

### Phase 0: Project scaffolding

- [x] Create `q2-demos/kanban/` directory structure
- [x] Set up `package.json` (same deps as hub-react-todo, plus test deps:
      `vitest`, `@testing-library/react`, `@testing-library/jest-dom`, `jsdom`)
- [x] Set up `vite.config.ts`, `tsconfig*.json`, `index.html`
- [x] Set up `vitest.config.ts`, `vitest.integration.config.ts`,
      `vitest.wasm.config.ts`
- [x] Copy WASM bridge files (`wasm.ts`, `wasm-js-bridge/`, `useSyncedAst.ts`)
- [x] Copy pre-built `wasm-quarto-hub-client/` directory (symlink)
- [x] Add test scripts to `package.json` (`test`, `test:integration`,
      `test:wasm`, `test:ci`)
- [x] Verify typecheck and vitest run cleanly

### Phase 1: Types and AST helpers — card extraction (test-first)

- [x] Define TypeScript types (`types.ts`):
  - `KanbanCard`: id, title, type, status, created, deadline, priority,
    bodyBlocks, headerBlockIndex
  - `CardRef`: sourceCardId, targetCardId, label, isCheckbox, checked,
    itemIndex, bulletListBodyIndex
  - `KanbanBoard`: cards, refs (extracted from the full document)
- [x] Generate AST fixture from `example1.qmd`
- [x] Write Layer 1 tests for `extractCards()` — expected card count, ids,
      types, metadata values
- [x] Implement `extractCards(ast)` — walk top-level blocks, find Header level-2
      nodes, extract attributes (classes, key-value pairs), collect body blocks
      until the next Header level-2
- [x] Write Layer 1 tests for `extractCardRefs()` — expected refs from
      milestone items and dependency lists
- [x] Implement `extractCardRefs(card)` — find bullet list items containing
      links to other cards (`[text](#id)` form)
- [x] Write Layer 1 tests for `buildBoard()` — full board extraction
- [x] Implement `buildBoard(ast)` — orchestrate extraction into a `KanbanBoard`
- [x] Write and run Layer 1 edge-case tests: no class, no metadata, empty body,
      single card, no cards, unknown classes (19 tests, all passing)

### Phase 2: AST helpers — card mutations (test-first)

- [x] Write Layer 1 tests for `setCardStatus()` — adds attribute when missing,
      updates when present, preserves other attributes, no mutation of original
- [x] Implement `setCardStatus(ast, cardId, newStatus)` — find the card's Header
      block, clone AST, update or add the `status` key-value attribute, return
      new AST
- [x] Write Layer 1 tests for `toggleMilestoneItem()` — toggles correct
      checkbox, round-trips, no mutation of original
- [x] Implement `toggleMilestoneItem(ast, milestoneId, itemIndex)` — same
      pattern as todo app's `toggleCheckbox`, but scoped to a specific card
- [x] Write Layer 1 tests for `addCard()` — new card appended with correct
      attributes, slug uniqueness, optional type
- [x] Implement `addCard(ast, title, type)` — append a new Header level-2 +
      empty content at the end of the document, with `created` timestamp
- [x] All 35 tests passing, typecheck clean

### Phase 3: Minimal UI — board view (with component tests)

- [x] `App.tsx` — hardcoded config (sync server, doc ID, file path)
- [x] `KanbanApp.tsx` — connect `useSyncedAst` → `buildBoard(ast)` → `BoardView`
      with `onStatusChange` wired to `setCardStatus` → `updateAst`
- [x] Write Layer 2 tests for `CardComponent` — renders title, type badge,
      deadline, body preview, status selector, onStatusChange callback (7 tests)
- [x] `CardComponent.tsx` — render a single card: title, type badge, metadata,
      truncated body preview, status dropdown
- [x] Write Layer 2 tests for `BoardView` — cards in correct columns, unset
      column, empty columns, onStatusChange passthrough (4 tests)
- [x] `BoardView.tsx` — render cards in a 4-column grid (todo/doing/done/unset)
- [x] Wire up bidirectional editing: status changes → `setCardStatus` →
      `updateAst`
- [x] All 46 tests passing (35 unit + 11 integration), typecheck clean

### Phase 4: Project selector with IndexedDB persistence

Replace the hardcoded sync server / document ID / file path in `App.tsx` with a
connection selector that persists recent connections in IndexedDB. Follows the
same pattern as hub-client's `ProjectSelector` + `projectStorage`, simplified
for a demo app (no create, no share URLs, no user identity, no import/export).

**New files:**

- `src/connectionStorage.ts` — IndexedDB CRUD via `idb` library. Single object
  store `connections` with `ConnectionEntry` records:

  ```typescript
  interface ConnectionEntry {
    id: string           // crypto.randomUUID()
    syncServer: string   // e.g. "wss://sync.automerge.org"
    indexDocId: string    // Automerge document ID
    filePath: string     // e.g. "kanban.qmd"
    description: string  // user-provided label
    createdAt: string    // ISO timestamp
    lastAccessed: string // ISO timestamp
  }
  ```

  Functions: `listConnections`, `addConnection`, `getConnection`,
  `touchConnection`, `deleteConnection`. No migrations, no import/export, no
  schema versioning — single DB version.

- `src/components/ConnectionSelector.tsx` — form + recent connections list
  (inline styles, no CSS file). Shows recent connections sorted by last accessed,
  "Connect to Project" form (server URL, document ID, file path, description),
  click to reconnect, delete button, default server URL pre-filled.

**Modified files:**

- `src/App.tsx` — state machine: no active connection → `ConnectionSelector`,
  active connection → `KanbanApp` with a "back" button/header. No URL routing
  (simplicity over deep linking).

- `package.json` — add `idb` dependency.

**Files unchanged:** `KanbanApp.tsx`, `astHelpers.ts`, `types.ts`,
`BoardView.tsx`, `CardComponent.tsx`, all tests, all configs.

**Work items:**

- [x] Add `idb` dependency to `package.json`
- [x] Create `connectionStorage.ts` with IndexedDB CRUD
- [x] Create `ConnectionSelector.tsx` component
- [x] Update `App.tsx` to use the selector (state machine)
- [x] Run existing tests to verify nothing is broken (35 unit + 11 integration)
- [x] Typecheck clean
- [ ] Manual verification: open app → empty selector → fill form → connect →
      kanban board → back → connection in list → click to reconnect

### Phase 5: WASM integration tests

- [ ] Write Layer 3 test: parse `example1.qmd` → `buildBoard()` → verify
- [ ] Write Layer 3 round-trip test: `setCardStatus` → incremental write →
      re-parse → verify
- [ ] Write Layer 3 round-trip test: `toggleMilestoneItem` → incremental write
      → re-parse → verify
- [ ] Write Layer 3 round-trip test: `addCard` → incremental write → re-parse
      → verify

### Phase 6: Milestone view (stretch)

- [ ] Write Layer 2 tests for `MilestoneView` — linked items, checkboxes,
      progress display
- [ ] `MilestoneView.tsx` — show milestones with their linked items, checkboxes
- [ ] Wire checkbox toggle to `toggleMilestoneItem`
- [ ] Show deadline and progress (checked / total)

## Design Decisions

### Why level-2 headers for cards

- Natural markdown structure: each card is a section
- Headers are easy to find in the AST (walk top-level blocks for `Header` with level 2)
- Card body = everything between this Header and the next level-2 Header
- Attributes on headers map cleanly to card metadata via Pandoc's Attr type
- The document reads well as plain markdown

### Minimal bidirectional edits for v1

For the first version, we support:
1. **Status toggling** — change a card's `status` attribute (adds/modifies a KV pair on the header)
2. **Milestone checkbox toggling** — same pattern as the todo demo
3. **Adding new cards** — append a new header at the end

These are all AST-level mutations that work well with the incremental writer
(they change a small part of the AST while preserving the rest of the source).

### Testing philosophy

The syntax is intentionally loose and permissive — users can write anything in a
card body. This makes strong test coverage essential. The three-layer testing
strategy ensures:

- **Layer 1** (fixtures) catches regressions in the extraction/mutation logic
  and documents the expected behavior for each QMD variation. New QMD patterns
  become new fixture files.
- **Layer 2** (components) catches UI regressions — the right data appears in
  the right places.
- **Layer 3** (WASM) catches parser/representation mismatches and ensures
  round-trip fidelity with the incremental writer.

Fixtures are the backbone: when the syntax evolves (e.g., adding `<#anchor>`
support), we add new fixtures and the tests document what's expected.

### What we intentionally defer

- Drag-and-drop reordering of cards (requires moving AST blocks, more complex)
- Card deletion (destructive, needs confirmation UX)
- Inline text editing of card bodies from the React UI
- Multiple QMD files / multi-file projects
- Calendar view, dependency graph view
- Card filtering / search
