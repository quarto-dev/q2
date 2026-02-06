/**
 * Extract todo items from a Pandoc AST.
 *
 * Expected document structure:
 *
 *   :::{#todo}
 *   - [ ] Unchecked item
 *   - [x] Checked item
 *   :::
 *
 * AST structure:
 *   Div (id="todo")
 *     BulletList
 *       Item: [Plain: [Span([], []), Space, Str("..."), ...]]
 *       Item: [Plain: [Span([], [Str("x")]), Space, Str("..."), ...]]
 */

import type { RustQmdJson, Annotated_Block, Annotated_Inline } from '@quarto/pandoc-types'

export interface TodoItem {
  checked: boolean
  label: string
  itemIndex: number
}

/**
 * Find the Div block with id="todo" in the document.
 */
export function findTodoDiv(ast: RustQmdJson): Annotated_Block | null {
  for (const block of ast.blocks) {
    if (block.t === 'Div') {
      const attr = block.c[0] // [id, classes, kvs]
      if (attr[0] === 'todo') {
        return block
      }
    }
  }
  return null
}

/**
 * Extract todo items from a #todo Div block.
 */
export function extractTodoItems(todoDiv: Annotated_Block): TodoItem[] {
  if (todoDiv.t !== 'Div') return []

  const divContent = todoDiv.c[1] as Annotated_Block[]
  const bulletList = divContent.find(b => b.t === 'BulletList')
  if (!bulletList || bulletList.t !== 'BulletList') return []

  const items: TodoItem[] = []
  const listItems = bulletList.c // Annotated_Block[][] — array of items, each item is Block[]

  for (let i = 0; i < listItems.length; i++) {
    const item = listItems[i]
    if (!item || item.length === 0) continue

    // Each item is Block[] — look for the first Plain or Para
    const block = item[0]
    if (!block || (block.t !== 'Plain' && block.t !== 'Para')) continue

    const inlines = block.c as Annotated_Inline[]
    if (!inlines || inlines.length === 0) continue

    // First inline should be a Span (the checkbox)
    const firstInline = inlines[0]
    if (!firstInline || firstInline.t !== 'Span') continue

    // Span.c = [Attr, Inline[]]
    const spanContent = firstInline.c[1] as Annotated_Inline[]
    const checked = spanContent.length > 0 &&
      spanContent.some(i => i.t === 'Str' && i.c === 'x')

    // Label: remaining inlines after the Span, concatenated
    const label = inlinesToText(inlines.slice(1))

    items.push({ checked, label, itemIndex: i })
  }

  return items
}

/**
 * Concatenate inline nodes into plain text.
 */
function inlinesToText(inlines: Annotated_Inline[]): string {
  const parts: string[] = []
  for (const inline of inlines) {
    switch (inline.t) {
      case 'Str':
        parts.push(inline.c as string)
        break
      case 'Space':
        parts.push(' ')
        break
      case 'SoftBreak':
        parts.push(' ')
        break
      case 'Emph':
      case 'Strong':
      case 'Strikeout':
      case 'Underline':
        parts.push(inlinesToText(inline.c as Annotated_Inline[]))
        break
      case 'Code':
        parts.push((inline.c as [unknown, string])[1])
        break
      default:
        break
    }
  }
  return parts.join('')
}
