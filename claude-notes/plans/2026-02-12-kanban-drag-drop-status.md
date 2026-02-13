# Kanban: Drag-and-Drop Status Changes

**Issue:** bd-3okv
**Date:** 2026-02-12

## Overview

In the BoardView, card status is currently displayed redundantly: each card has a `<select>` dropdown showing its status, AND the card is grouped under the corresponding status section (Todo / Doing / Done / Unset). The position already communicates the status, making the dropdown noise.

**Goals:**
1. Remove the status `<select>` dropdown from `CardComponent` when rendered inside `BoardView` (keep it in `CardDetailView` where there's no positional context)
2. Allow cards to be dragged between status sections, updating the `status` attribute in the AST on drop

## Design Decisions

### Drag-and-drop approach: @dnd-kit
- Use `@dnd-kit/core` and `@dnd-kit/sortable` for drag-and-drop
- Provides touch/pointer support (works on tablets), keyboard accessibility, smooth drop animations, and customizable drag overlays
- ~15KB gzipped for core + sortable — reasonable for a real application
- Clean React hooks API (`useDraggable`, `useDroppable`, `DndContext`)

### Conditional status dropdown hiding
- `CardComponent` gets a new optional prop `showStatusDropdown?: boolean` (default `true`)
- `BoardView` passes `showStatusDropdown={false}` since the section position implies status
- `CardDetailView` keeps the dropdown as-is (it's a standalone modal with no positional context)

### Keep row-based layout
- The current row-based layout (status sections stacked vertically, cards in a 2-column grid per section) is space-efficient and works well
- Cards are dragged vertically between sections rather than horizontally between columns
- Each status section acts as a droppable zone; visual feedback on the target section during drag

## Work Items

### Phase 1: Tests
- [x] Add unit tests for `CardComponent` with `showStatusDropdown={false}` — verify no `<select>` is rendered
- [x] Add integration tests for drag-and-drop: test `makeDragEndHandler` directly (jsdom can't fully simulate @dnd-kit sensor events)
- [x] Update existing `BoardView` tests that reference `combobox` elements (they will no longer be present in board context)

### Phase 2: Add @dnd-kit dependency
- [x] Add `@dnd-kit/core` and `@dnd-kit/sortable` to package.json
- [x] Run `npm install` from repo root

### Phase 3: Hide status dropdown in BoardView
- [x] Add `showStatusDropdown?: boolean` prop to `CardComponent` (default `true`)
- [x] Conditionally render the `<select>` based on that prop
- [x] Pass `showStatusDropdown={false}` from `BoardView`
- [x] Verify `CardDetailView` still shows the dropdown (no changes needed there)

### Phase 4: Implement drag-and-drop
- [x] Wrap `BoardView` content in `<DndContext>` with `onDragEnd` handler
- [x] Make each card draggable using `useDraggable` (card id as draggable id)
- [x] Make each status section a drop target using `useDroppable` (status value as droppable id)
- [x] On drag end: if card landed in a different status section, call `onStatusChange(cardId, targetStatus)`
- [x] Add visual feedback: highlight the target section during drag-over (background color + border color change)
- [x] Add a `DragOverlay` for a polished drag preview
- [x] Handle edge case: dropping a card on its own section (handler fires, but AST `setCardStatus` already handles same-status no-ops)

### Phase 5: Polish
- [x] Add `cursor: grab` / `cursor: grabbing` on draggable cards (cursor: grab set in DraggableCard)
- [x] Verify touch/pointer drag works (PointerSensor handles pointer + touch events)
- [x] Verify keyboard accessibility (KeyboardSensor configured)
- [x] Verify the calendar view is unaffected (CalendarView is independent, no shared imports)
- [x] Run full test suite — 35 unit + 20 integration tests all pass

## Files to Modify

| File | Change |
|------|--------|
| `package.json` | Add `@dnd-kit/core`, `@dnd-kit/sortable` |
| `src/components/CardComponent.tsx` | Add `showStatusDropdown` prop, conditionally render `<select>` |
| `src/components/BoardView.tsx` | DndContext, droppable sections, draggable cards, pass `showStatusDropdown={false}` |
| `src/__tests__/components.integration.test.tsx` | Update existing tests, add new drag-and-drop and dropdown-hiding tests |

## Files Unchanged

| File | Reason |
|------|--------|
| `src/components/CardDetailView.tsx` | Keeps status dropdown as-is (no positional context in modal) |
| `src/astHelpers.ts` | `setCardStatus()` already handles status changes — no new mutations needed |
| `src/types.ts` | No type changes needed |
| `src/KanbanApp.tsx` | Already wires `onStatusChange` through; no changes needed |
