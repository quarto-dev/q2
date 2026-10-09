---
title: 'Hub-Client Color Scheme Refactor'
date: 2026-04-02
---

## Overview

Replace the current binary light/dark toggle in ProjectSelector with a proper three-way color scheme preference ("follow user", "force dark", "force light") that:

1. Respects the browser's `prefers-color-scheme` media query by default
2. Persists the user's choice via the existing preferences service
3. Applies consistently across the entire hub-client UI (project selector, editor, Monaco)
4. Does **not** affect the rendered Quarto preview documents (out of scope)

## Current State

- **Theme toggle**: Boolean `useState(false)` in `ProjectSelector.tsx:96`, not persisted
- **Theme CSS**: CSS variables in `ProjectSelector.css` — dark default, `.light-theme` class overrides
- **Monaco**: Hardcoded `theme="vs-dark"` in `Editor.tsx:997`
- **Editor CSS**: All hardcoded dark colors (`Editor.css`, `MarkdownSummary.css`)
- **Toast CSS**: Uses `@media (prefers-color-scheme: light)` independently
- **index.css**: Has a `@media (prefers-color-scheme: light)` block for root styles
- **ProjectSetSetup.css**: Uses dark CSS variables with fallbacks, no light variant
- **Preferences system**: Existing `services/preferences/` with Zod schema, localStorage persistence, and `usePreference` hook — ready to use
- **ViewModeContext**: Good pattern to follow for a ThemeContext (React context + persistence)

## Design Decisions

### Color scheme type

```typescript
type ColorScheme = 'auto' | 'dark' | 'light';
```

- `auto` (default): Follow `prefers-color-scheme` via `matchMedia`
- `dark`: Force dark theme
- `light`: Force light theme

### Architecture: React Context + CSS class on `<html>`

Create a `ThemeContext` (following the `ViewModeContext` pattern) that:

1. Reads the preference from the preferences service
2. Listens to `matchMedia('(prefers-color-scheme: dark)')` for `auto` mode
3. Resolves the effective theme (`'dark' | 'light'`) and exposes it to consumers
4. Sets class `dark` or `light` on `document.documentElement`

CSS uses `:root.light` / `:root.dark` selectors. This is the standard idiomatic pattern (used by Tailwind and most CSS frameworks). Class-based is preferred over `data-theme` because we don't need multiple coexisting themes on the same page — the only component that might differ (Quarto preview) lives in an iframe.

### Monaco integration

Monaco's `theme` prop is controlled by the resolved effective theme:
- `'dark'` → `"vs-dark"`  
- `'light'` → `"light"` (Monaco's built-in light theme)

### CSS migration

Move theme CSS variables from `.project-selector` / `.project-selector.light-theme` scope to `:root.dark` / `:root.light`. This allows all components (editor, toasts, etc.) to use the same variables.

Hardcoded colors in `Editor.css`, `MarkdownSummary.css`, `ProjectSetSetup.css` get replaced with theme CSS variables.

### Toggle UI

Icon-cycle button showing the **current** selection (not the next one):

- ☀️ when in light mode ("I'm currently light")
- 🌙 when in dark mode ("I'm currently dark")  
- 💻 (or similar system/monitor icon) when in auto mode ("I'm following your OS")

Clicking cycles: `auto → dark → light → auto`

Tooltip shows the current mode name (e.g., "Theme: Follow system").

## Work Items

### Phase 1: Infrastructure

- [x] Add `colorScheme` field to preferences schema (`'auto' | 'dark' | 'light'`, default `'auto'`)
- [x] Create `ThemeContext.tsx` with `ThemeProvider` and `useTheme` hook
  - Exposes `{ colorScheme, setColorScheme, cycleColorScheme, effectiveTheme }` 
  - Uses `getPreference`/`setPreference` for persistence
  - Listens to `matchMedia` change events for `auto` mode
  - Sets class `dark` or `light` on `<html>` element
- [x] Wire `ThemeProvider` into the app's component tree (wraps ProjectSelector, Editor, and Toast)

### Phase 2: CSS migration

- [x] Move CSS variables from `.project-selector` / `.project-selector.light-theme` to `:root.dark` / `:root.light` (created `theme.css`, imported in `main.tsx`)
- [x] Update `ProjectSelector.css` to remove the old scoped theme variables, use the global ones
- [x] Add light-theme CSS variables for Editor components (`Editor.css`, `MarkdownSummary.css`)
- [x] Update `ProjectSetSetup.css` to use theme variables
- [x] Update `Toast.css` to use theme variables (replaced `@media prefers-color-scheme` with CSS variables)
- [x] Update `index.css` to use CSS variables (replaced `@media prefers-color-scheme` block)

### Phase 3: Component updates

- [x] Update `ProjectSelector.tsx`: replace `useState(false)` toggle with `useTheme()` context; change toggle UI to icon-cycle showing current mode (auto=💻, dark=🌙, light=☀️), cycling `auto → dark → light → auto`
- [x] Update `Editor.tsx`: use `useTheme()` to set Monaco `theme` prop (`vs-dark` / `light`)
- [x] Update `Editor.css`: replace hardcoded dark colors with CSS variables (editor header, margins, status indicators, etc.)

### Phase 4: Cleanup & verification

- [x] Remove any dead code (old `lightTheme` state, unused CSS `.light-theme` selectors converted to `:root.light`)
- [x] Verify `npm run build:all` passes
- [ ] Manual testing: verify all three modes work across project selector and editor
- [ ] Verify preview pane is unaffected
