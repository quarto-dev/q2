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
- [ ] Add unit tests for `CardComponent` with `showStatusDropdown={false}` — verify no `<select>` is rendered
- [ ] Add integration tests for drag-and-drop: simulate drag from one section to another, verify `onStatusChange` fires with correct arguments
- [ ] Update existing `BoardView` tests that reference `combobox` elements (they will no longer be present in board context)

### Phase 2: Add @dnd-kit dependency
- [ ] Add `@dnd-kit/core` and `@dnd-kit/sortable` to package.json
- [ ] Run `npm install` from repo root

### Phase 3: Hide status dropdown in BoardView
- [ ] Add `showStatusDropdown?: boolean` prop to `CardComponent` (default `true`)
- [ ] Conditionally render the `<select>` based on that prop
- [ ] Pass `showStatusDropdown={false}` from `BoardView`
- [ ] Verify `CardDetailView` still shows the dropdown (no changes needed there)

### Phase 4: Implement drag-and-drop
- [ ] Wrap `BoardView` content in `<DndContext>` with `onDragEnd` handler
- [ ] Make each card draggable using `useDraggable` (card id as draggable id)
- [ ] Make each status section a drop target using `useDroppable` (status value as droppable id)
- [ ] On drag end: if card landed in a different status section, call `onStatusChange(cardId, targetStatus)`
- [ ] Add visual feedback: highlight the target section during drag-over (e.g. background color change)
- [ ] Add a `DragOverlay` for a polished drag preview
- [ ] Handle edge case: dropping a card on its own section (no-op, no AST mutation)

### Phase 5: Polish
- [ ] Add `cursor: grab` / `cursor: grabbing` on draggable cards
- [ ] Verify touch/pointer drag works (tablet-friendly)
- [ ] Verify keyboard accessibility (tab to card, space to pick up, arrows to move)
- [ ] Verify the calendar view is unaffected
- [ ] Run full test suite (`npm run test:ci` from kanban dir)

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
