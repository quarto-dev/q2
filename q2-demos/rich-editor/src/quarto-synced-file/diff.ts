import type { EditorContentChange } from '@quarto/quarto-sync-client'

/** The single replaced range that turns `before` into `after`, or null if equal. */
export function diffToChange(before: string, after: string): EditorContentChange | null {
  if (before === after) return null
  let start = 0
  const minLen = Math.min(before.length, after.length)
  while (start < minLen && before[start] === after[start]) start++
  let end = 0
  while (
    end < minLen - start &&
    before[before.length - 1 - end] === after[after.length - 1 - end]
  ) end++
  return {
    rangeOffset: start,
    rangeLength: before.length - start - end,
    text: after.slice(start, after.length - end),
  }
}
