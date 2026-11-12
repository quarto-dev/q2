import { describe, expect, it } from 'vitest'
import { insertText, deleteBackward, select } from './commands.ts'
import { apply, createEditor, redo, undo, type Editor } from './editor.ts'
import { caret, type Schema } from './model.ts'

const schema: Schema = { order: ['title', 'subtitle'] }

/** Type `text` one character at a time, `ms` apart, starting at `now`. */
function type(e: Editor, text: string, now: number, ms = 50): Editor {
  for (const [i, ch] of [...text].entries()) {
    const next = insertText(schema, e.present, ch)
    e = apply(e, next, { inputType: 'insertText', before: e.present.selection, now: now + i * ms })
  }
  return e
}

function backspace(e: Editor, times: number, now: number): Editor {
  for (let i = 0; i < times; i++) {
    const next = deleteBackward(schema, e.present)
    e = apply(e, next, { inputType: 'deleteContentBackward', before: e.present.selection, now: now + i * 50 })
  }
  return e
}

/** "Report" / "Q3", with the caret at the end of the title. */
function start(): Editor {
  const e = createEditor({ title: 'Report', subtitle: 'Q3' })
  return apply(e, select(schema, e.present, caret('title', 6)), {})
}

describe('editor history', () => {
  it('a run of typing is one step, and undo restores the text and caret', () => {
    let e = type(start(), ' one', 0)
    expect(e.present.fields.title).toBe('Report one')
    e = undo(e)
    expect(e.present.fields.title).toBe('Report')
    expect(e.present.selection).toEqual(caret('title', 6))
    e = redo(e)
    expect(e.present.fields.title).toBe('Report one')
    expect(e.present.selection).toEqual(caret('title', 10))
  })

  it('typing then deleting are separate steps', () => {
    let e = type(start(), 'mm', 0)
    e = backspace(e, 2, 200)
    expect(undo(e).present.fields.title).toBe('Reportmm')
  })

  it('a caret jump starts a new step', () => {
    let e = type(start(), 'X', 0)
    e = apply(e, select(schema, e.present, caret('title', 0)), {})
    e = type(e, 'Y', 100)
    expect(undo(e).present.fields.title).toBe('ReportX')
  })

  it('a caret jump the selection events missed still starts a new step', () => {
    let e = type(start(), 'X', 0)
    const moved = select(schema, e.present, caret('title', 0))
    e = apply(e, insertText(schema, moved, 'Y'), { inputType: 'insertText', before: moved.selection, now: 100 })
    expect(undo(e).present.fields.title).toBe('ReportX')
  })

  it('undo is global: it undoes the latest edit wherever the caret is', () => {
    let e = type(start(), '!', 0)
    e = apply(e, select(schema, e.present, caret('subtitle', 2)), {})
    e = type(e, '?', 100)
    e = apply(e, select(schema, e.present, caret('title', 0)), {})
    e = undo(e)
    expect(e.present.fields).toEqual({ title: 'Report!', subtitle: 'Q3' })
    expect(e.present.selection).toEqual(caret('subtitle', 2))
    e = undo(e)
    expect(e.present.fields).toEqual({ title: 'Report', subtitle: 'Q3' })
  })

  it('moving the caret adds no undo step, even with Backspace', () => {
    let e = start()
    e = apply(e, select(schema, e.present, caret('subtitle', 0)), {})
    e = backspace(e, 1, 0)
    expect(e.present.selection).toEqual(caret('title', 6))
    expect(e.past.length).toBe(0)
  })

  it('an emptied field removed on leaving comes back with undo', () => {
    let e = apply(start(), select(schema, start().present, caret('subtitle', 2)), {})
    e = backspace(e, 2, 0)
    expect(e.present.fields.subtitle).toBe('')
    e = apply(e, select(schema, e.present, caret('title', 0)), {})
    expect(e.present.fields.subtitle).toBeUndefined()
    e = undo(e)
    expect(e.present.fields.subtitle).toBe('Q3')
  })
})
