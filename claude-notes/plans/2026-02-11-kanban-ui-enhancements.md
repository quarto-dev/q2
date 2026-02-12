# Kanban Demo UI Enhancements

## Overview

A set of UI improvements to the kanban demo app (`q2-demos/kanban/`). Each feature should be committed separately for easy review and rollback.

**Key files:**
- `src/components/BoardView.tsx` - Board layout (columns → rows)
- `src/components/CardComponent.tsx` - Card rendering, status dropdown
- `src/KanbanApp.tsx` - Main app logic, bridges AST to views
- `src/astHelpers.ts` - AST extraction and mutation functions
- `src/types.ts` - Type definitions

## Feature 1: Move status dropdown left of title (single line)

Currently the status dropdown is at the bottom of each card. Move it inline with the title, to the left.

- [x] Modify `CardComponent.tsx`: move `<select>` from bottom to first element in the title row
- [x] Adjust flex layout so status + title are on one line
- [x] Commit: "Move status dropdown to left of title in kanban cards"

## Feature 2: Horizontal rows instead of columns

Change the board layout from vertical columns to horizontal rows. Each status group becomes a full-width horizontal row with cards flowing left-to-right.

- [x] Modify `BoardView.tsx`: change from grid columns to stacked rows
- [x] Each row: status header on the left or top, cards flowing horizontally
- [x] Commit: "Rearrange kanban board to horizontal rows"

## Feature 3: Card detail view

Clicking a card title opens a detail view showing all card information (title, type, status, dates, body, cross-references).

- [x] Create `CardDetailView.tsx` component (modal/overlay)
- [x] Show all card fields: title, type, status, created, deadline, priority, full body
- [x] Add click handler on card title in `CardComponent.tsx`
- [x] Wire through `KanbanApp.tsx` (selected card state)
- [x] Commit: "Add card detail view on title click"

## Feature 4: Calendar view

A new view that shows cards with deadlines organized by date in a calendar grid.

- [x] Create `CalendarView.tsx` component
- [x] Show a month grid with cards placed on their deadline dates
- [x] Add month navigation (prev/next)
- [x] Add view switcher in `KanbanApp.tsx` (Board / Calendar tabs)
- [x] Commit: "Add calendar view for cards with deadlines"

## Feature 5: New card creation

A button to create a new card with form fields.

- [x] Add "New Card" button to the board UI
- [x] Create `NewCardForm.tsx` component (modal/overlay)
- [x] Fields: title (required), type (toggle buttons from existing types), deadline (optional, with date picker), status (optional)
- [x] Default creation date to today
- [x] Extend `addCard()` in astHelpers.ts to accept options object with deadline and status
- [x] Wire through `KanbanApp.tsx` via `updateAst`
- [x] Update existing tests to match new `addCard()` signature
- [x] Commit: "Add new card creation with type and deadline support"

## Feature 6: Consolidate header / toolbar

The top of the connected view currently has three layers of redundant info:
1. `App.tsx` h1: "Quarto Hub - Kanban"
2. `App.tsx` connection bar: description, filePath, syncServer, [Disconnect]
3. `KanbanApp.tsx` toolbar: "Live from filePath — N cards", [+ New Card], [Board], [Calendar]

The file path appears 3 times and the sync server is shown but rarely needed.

**Target layout**: A single compact toolbar row with:
- Left side: "Kanban — kanban.qmd — N cards" + clickable index doc ID (copies to clipboard)
- Right side: [Disconnect] [+ New Card] [Board|Calendar] (joined toggle group)

**Implementation approach**: Move `onDisconnect` and connection metadata into `KanbanApp` as props,
so it owns the single unified toolbar. Remove the h1 and connection bar from `App.tsx`.

- [x] Update `KanbanApp` props to accept `onDisconnect` and `indexDocId`
- [x] Build unified toolbar in `KanbanBoard`: title, file path, card count, clickable doc ID
- [x] Implement Board/Calendar as a joined button group (shared border, no gap)
- [x] Put all action buttons (Disconnect, New Card, toggle group) in one row on the right
- [x] Remove h1 and connection bar from `App.tsx`; just render `KanbanApp` directly
- [x] Commit: "Consolidate header into single toolbar row"
