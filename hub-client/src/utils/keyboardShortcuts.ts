/**
 * Keyboard shortcut map — the single source of truth for hub-client
 * keyboard interactions.
 *
 * Every global handler and every keyboard-operable component pattern is
 * documented here exactly once, and the About tab renders its shortcuts
 * reference from this module — so the docs can't drift from the
 * implementation without editing the implementation's record of itself.
 *
 * When you add a keyboard interaction anywhere in the app, add it here
 * in the same commit. Component-level patterns (tree, menu, dialog) are
 * documented once against the pattern, not per instance.
 *
 * Phase 2 deliverable of the UI/UX modernization plan.
 */

export interface ShortcutEntry {
  /** Human-readable key combo, e.g. "⌘K" or "Shift+F10". */
  keys: string;
  /** What it does, phrased as an action ("Open the focused file"). */
  action: string;
}

export interface ShortcutGroup {
  title: string;
  entries: ShortcutEntry[];
}

export const SHORTCUT_GROUPS: ShortcutGroup[] = [
  {
    title: 'Global',
    entries: [
      {
        keys: '⌘K / Ctrl+K',
        action: 'Focus the project search (Projects home)',
      },
      {
        keys: '⌘S / Ctrl+S',
        action: 'Save — files auto-save; this shows a confirmation',
      },
    ],
  },
  {
    title: 'File tree',
    entries: [
      { keys: '↑ / ↓', action: 'Move between files and folders' },
      { keys: '→', action: 'Expand a folder, or move into it' },
      { keys: '←', action: 'Collapse a folder, or move to its parent' },
      { keys: 'Home / End', action: 'Jump to the first / last visible row' },
      { keys: 'A–Z', action: 'Jump to a file or folder by name' },
      { keys: 'Enter', action: 'Open the file, or toggle the folder' },
      { keys: 'Shift+F10', action: 'Open the actions menu for the row' },
    ],
  },
  {
    title: 'File search',
    entries: [
      { keys: '↓', action: 'Move from the search box into the results' },
      { keys: '↑ / ↓', action: 'Move between results' },
      { keys: 'Enter', action: 'Open the selected result' },
      { keys: 'Escape', action: 'Clear the search' },
    ],
  },
  {
    title: 'Menus',
    entries: [
      { keys: '↑ / ↓', action: 'Move between items' },
      { keys: 'Home / End', action: 'Jump to the first / last item' },
      { keys: 'A–Z', action: 'Jump to an item by name' },
      { keys: '→ / ←', action: 'Open / close a submenu' },
      { keys: 'Enter', action: 'Choose the focused item' },
      { keys: 'Escape', action: 'Close the menu' },
    ],
  },
  {
    title: 'Dialogs',
    entries: [
      { keys: 'Tab / Shift+Tab', action: 'Move between the dialog’s controls' },
      { keys: 'Escape', action: 'Close the dialog' },
    ],
  },
  {
    title: 'Slide preview',
    entries: [{ keys: '← / →', action: 'Previous / next slide' }],
  },
];
