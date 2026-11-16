import { describe, expect, it } from 'vitest'
import { ariaKeyshortcuts, matches, shortcutText, type Shortcut } from './shortcuts.ts'

const BOLD: Shortcut = { key: 'b' }
const BULLETS: Shortcut = { key: '8', shift: true }
const HEADING_1: Shortcut = { key: '1', alt: true }

/** A key press, as the browser reports it. */
const press = (key: string, code: string, mods: Partial<Record<'meta' | 'ctrl' | 'shift' | 'alt', boolean>> = {}) => ({
  key,
  code,
  metaKey: mods.meta ?? false,
  ctrlKey: mods.ctrl ?? false,
  shiftKey: mods.shift ?? false,
  altKey: mods.alt ?? false,
})

describe('shortcutText', () => {
  it('writes a Mac shortcut in glyphs, its modifiers in the order of a Mac menu', () => {
    expect(shortcutText(BOLD, true)).toBe('⌘B')
    expect(shortcutText(BULLETS, true)).toBe('⇧⌘8')
    expect(shortcutText(HEADING_1, true)).toBe('⌥⌘1')
    expect(shortcutText({ key: 'h', shift: true, alt: true }, true)).toBe('⌥⇧⌘H')
  })

  it('writes other shortcuts with Ctrl, Alt and Shift joined by +', () => {
    expect(shortcutText(BOLD, false)).toBe('Ctrl+B')
    expect(shortcutText(BULLETS, false)).toBe('Ctrl+Shift+8')
    expect(shortcutText(HEADING_1, false)).toBe('Ctrl+Alt+1')
  })
})

describe('ariaKeyshortcuts', () => {
  it('names the keys as aria-keyshortcuts does, modifiers first', () => {
    expect(ariaKeyshortcuts(BULLETS, true)).toBe('Meta+Shift+8')
    expect(ariaKeyshortcuts(HEADING_1, false)).toBe('Control+Alt+1')
    expect(ariaKeyshortcuts(BOLD, false)).toBe('Control+B')
  })
})

describe('matches', () => {
  it('takes ⌘ on a Mac and Ctrl elsewhere, but not both', () => {
    expect(matches(press('b', 'KeyB', { meta: true }), BOLD, true)).toBe(true)
    expect(matches(press('b', 'KeyB', { ctrl: true }), BOLD, true)).toBe(false)
    expect(matches(press('b', 'KeyB', { ctrl: true }), BOLD, false)).toBe(true)
    expect(matches(press('b', 'KeyB', { meta: true, ctrl: true }), BOLD, true)).toBe(false)
  })

  it('needs exactly its Shift and Alt', () => {
    expect(matches(press('B', 'KeyB', { meta: true, shift: true }), BOLD, true)).toBe(false)
    expect(matches(press('8', 'Digit8', { meta: true }), BULLETS, true)).toBe(false)
    expect(matches(press('1', 'Digit1', { meta: true, shift: true, alt: true }), HEADING_1, true)).toBe(false)
  })

  it('reads a letter by the character it types, whatever the keyboard layout', () => {
    // On a Dvorak layout, the B is where a QWERTY keyboard has its N.
    expect(matches(press('b', 'KeyN', { meta: true }), BOLD, true)).toBe(true)
    expect(matches(press('x', 'KeyB', { meta: true }), BOLD, true)).toBe(false)
  })

  it('reads a digit by its key, as Shift and Option change the character it types', () => {
    expect(matches(press('*', 'Digit8', { meta: true, shift: true }), BULLETS, true)).toBe(true)
    expect(matches(press('¡', 'Digit1', { meta: true, alt: true }), HEADING_1, true)).toBe(true)
    expect(matches(press('8', 'Numpad8', { meta: true, shift: true }), BULLETS, true)).toBe(false)
  })
})
