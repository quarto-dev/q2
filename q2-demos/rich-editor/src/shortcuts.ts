/**
 * The editor's keyboard shortcuts, as in Google Docs, with ⌘ on a Mac and
 * Ctrl elsewhere: which they are, how the toolbar writes them (in its
 * tooltips and its style menu), and whether a key press is one.
 *
 * Undo and redo are handled by the engine (`engine/react.tsx`), which also
 * takes Ctrl+Shift+Z for redo off a Mac; they're here to be written.
 */

export const IS_MAC = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform)

/** A key pressed with ⌘ (Ctrl), and Shift or Alt (Option) or neither. */
export interface Shortcut {
  /**
   * A lowercase letter, read as the character it types, so it follows the
   * keyboard's layout; or a digit, read as the key it's on (`Digit8`), as
   * Shift and Option change the character a digit's key types.
   */
  key: string
  shift?: boolean
  alt?: boolean
}

export const SHORTCUTS = {
  undo: { key: 'z' },
  redo: IS_MAC ? { key: 'z', shift: true } : { key: 'y' },
  bold: { key: 'b' },
  italic: { key: 'i' },
  underline: { key: 'u' },
  strikeout: { key: 'x', shift: true },
  code: { key: 'e' },
  highlight: { key: 'h', shift: true },
  link: { key: 'k' },
  bulletList: { key: '8', shift: true },
  numberedList: { key: '7', shift: true },
  quote: { key: '9', shift: true },
  indent: { key: ']' },
  outdent: { key: '[' },
} satisfies Record<string, Shortcut>

/** The shortcut for a line style: 0 for Paragraph, 1 to 4 for a heading's level. */
export function styleShortcut(level: number): Shortcut {
  return { key: String(level), alt: true }
}

const isDigit = (key: string) => key >= '0' && key <= '9'

/**
 * A shortcut as a menu writes it: on a Mac in glyphs, modifiers in the
 * order a Mac's menus give them (⌥⇧⌘B); elsewhere spelled out and joined
 * by + (Ctrl+Alt+Shift+B).
 */
export function shortcutText(s: Shortcut, mac = IS_MAC): string {
  const key = s.key.toUpperCase()
  if (mac) return `${s.alt ? '⌥' : ''}${s.shift ? '⇧' : ''}⌘${key}`
  return ['Ctrl', s.alt && 'Alt', s.shift && 'Shift', key].filter(Boolean).join('+')
}

/** A shortcut as `aria-keyshortcuts` names it (Meta+Shift+8). */
export function ariaKeyshortcuts(s: Shortcut, mac = IS_MAC): string {
  return [mac ? 'Meta' : 'Control', s.alt && 'Alt', s.shift && 'Shift', s.key.toUpperCase()].filter(Boolean).join('+')
}

type KeyPress = Pick<KeyboardEvent, 'key' | 'code' | 'metaKey' | 'ctrlKey' | 'shiftKey' | 'altKey'>

/** Whether a key press is the shortcut. */
export function matches(e: KeyPress, s: Shortcut, mac = IS_MAC): boolean {
  const primary = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey
  if (!primary || e.shiftKey !== Boolean(s.shift) || e.altKey !== Boolean(s.alt)) return false
  return isDigit(s.key) ? e.code === `Digit${s.key}` : e.key.toLowerCase() === s.key
}
