import { describe, expect, it } from 'vitest'
import { insertText, deleteBackward, select } from './commands.ts'
import { apply, createEditor, fieldChanges, redo, settle, undo, type Editor } from './editor.ts'
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

describe('fieldChanges', () => {
  it('lists the fields whose text differs, with emptied ones as removed', () => {
    expect(fieldChanges(schema, { title: 'A', subtitle: 'B' }, { title: 'A2', subtitle: '' })).toEqual({
      title: 'A2',
      subtitle: undefined,
    })
  })
  it('counts an absent field and an empty one as the same', () => {
    expect(fieldChanges(schema, { title: 'A' }, { title: 'A', subtitle: '' })).toEqual({})
  })
})

describe('fields that stay when empty (keepEmpty)', () => {
  const kept: Schema = { order: ['title', 'subtitle'], keepEmpty: ['title', 'subtitle'] }

  it('an emptied one is written empty, and a removed one as removed', () => {
    expect(fieldChanges(kept, { title: 'A', subtitle: 'B' }, { title: '', subtitle: undefined })).toEqual({
      title: '',
      subtitle: undefined,
    })
  })

  it('adding or removing one, empty, is a change to write', () => {
    expect(fieldChanges(kept, { title: 'A' }, { title: 'A', subtitle: '' })).toEqual({ subtitle: '' })
    expect(fieldChanges(kept, { title: 'A', subtitle: '' }, { title: 'A' })).toEqual({ subtitle: undefined })
  })

  it('adding one, empty, is an undo step', () => {
    let e = createEditor({ title: 'Report' })
    e = apply(e, { fields: { title: 'Report', subtitle: '' }, selection: caret('subtitle', 0) }, { schema: kept })
    expect(e.past.length).toBe(1)
    expect(undo(e).present.fields).toEqual({ title: 'Report' })
  })

  it('removing one is a step of its own, even right after deleting its text', () => {
    let e = apply(createEditor({ title: 'Report', subtitle: 'Q3' }), select(kept, createEditor({ title: 'Report', subtitle: 'Q3' }).present, caret('subtitle', 2)), { schema: kept })
    for (let i = 0; i < 3; i++) {
      const next = deleteBackward(kept, e.present)
      e = apply(e, next, { inputType: 'deleteContentBackward', before: e.present.selection, now: i * 50, schema: kept })
    }
    expect(e.present.fields).toEqual({ title: 'Report' })
    e = undo(e)
    expect(e.present.fields).toEqual({ title: 'Report', subtitle: '' })
    e = undo(e)
    expect(e.present.fields).toEqual({ title: 'Report', subtitle: 'Q3' })
  })

  it('settles to the document: one it holds empty stays, one it removed goes', () => {
    const editor = createEditor({ title: 'A', subtitle: '' })
    expect(settle(kept, editor, { title: 'A', subtitle: '' })).toBe(editor)
    expect(settle(kept, editor, { title: 'A' }).present.fields).toEqual({ title: 'A' })
  })
})

describe('settle', () => {
  it('shows the document\'s text in the fields the selection is not in', () => {
    const editor = createEditor({ title: 'A ', subtitle: 'B ' })
    const held = apply(editor, { fields: editor.present.fields, selection: caret('title', 2) })
    const settled = settle(schema, held, { title: 'A', subtitle: 'B' })
    expect(settled.present.fields).toEqual({ title: 'A ', subtitle: 'B' })
  })
  it('shows the document\'s text in the lines of a multi-line field the selection is not on', () => {
    const lines: Schema = { order: ['body'], multiline: ['body'] }
    // A trailing space on the first line, the caret on an empty third
    // line, and an empty last line: the document keeps none of them.
    const editor = createEditor({ body: 'One \nTwo\n\nThree\n' })
    const held = apply(editor, { fields: editor.present.fields, selection: caret('body', 9) })
    const settled = settle(lines, held, { body: 'One\nTwo\nThree' })
    expect(settled.present.fields.body).toBe('One\nTwo\n\nThree')
    expect(settled.present.selection).toEqual(caret('body', 8))
  })
  it('keeps the empty last line the caret is on', () => {
    const lines: Schema = { order: ['body'], multiline: ['body'] }
    const editor = createEditor({ body: 'One\n' })
    const held = apply(editor, { fields: editor.present.fields, selection: caret('body', 4) })
    expect(settle(lines, held, { body: 'One' })).toBe(held)
  })
  it('keeps a space typed at the end of the line the caret is on', () => {
    const lines: Schema = { order: ['body'], multiline: ['body'] }
    const editor = createEditor({ body: 'One \nTwo' })
    const held = apply(editor, { fields: editor.present.fields, selection: caret('body', 4) })
    expect(settle(lines, held, { body: 'One\nTwo' })).toBe(held)
  })
  it('returns the same editor when nothing differs', () => {
    const editor = createEditor({ title: 'A' })
    expect(settle(schema, editor, { title: 'A' })).toBe(editor)
  })
})
