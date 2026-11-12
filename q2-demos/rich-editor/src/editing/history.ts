/**
 * An immutable undo history of editor states.
 *
 * Consecutive edits in the same group (see `undoGroup.ts`) coalesce into
 * one undo step until the group changes, `endGroup` is called (the field
 * loses focus), or typing pauses for `COALESCE_MS`.
 */

export const COALESCE_MS = 1000

export interface History<T> {
  past: T[]
  present: T
  future: T[]
  /** The group of the open step, and when it last changed. */
  open: { group: string; at: number } | null
}

export interface RecordOptions<T> {
  /** Edits in the same group coalesce into one step. */
  group?: string
  /** The current time in ms (defaults to `Date.now()`). */
  now?: number
  /**
   * Replaces the present state as it's pushed onto the undo stack, so undo
   * returns to it. Use it to record what changed since the last step
   * without being an edit, such as the caret position.
   */
  before?: T
}

export function createHistory<T>(present: T): History<T> {
  return { past: [], present, future: [], open: null }
}

export function record<T>(h: History<T>, next: T, options: RecordOptions<T> = {}): History<T> {
  const { group, now = Date.now(), before } = options
  const open = group === undefined ? null : { group, at: now }
  const coalesce =
    group !== undefined &&
    h.open !== null &&
    h.open.group === group &&
    now - h.open.at < COALESCE_MS
  if (coalesce) return { ...h, present: next, future: [], open }
  const pushed = before === undefined ? h.present : before
  return { past: [...h.past, pushed], present: next, future: [], open }
}

/** Close the open step, so the next edit starts a new one. */
export function endGroup<T>(h: History<T>): History<T> {
  return h.open === null ? h : { ...h, open: null }
}

export function undo<T>(h: History<T>): History<T> {
  if (h.past.length === 0) return h
  return {
    past: h.past.slice(0, -1),
    present: h.past[h.past.length - 1],
    future: [h.present, ...h.future],
    open: null,
  }
}

export function redo<T>(h: History<T>): History<T> {
  if (h.future.length === 0) return h
  return {
    past: [...h.past, h.present],
    present: h.future[0],
    future: h.future.slice(1),
    open: null,
  }
}
