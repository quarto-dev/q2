/**
 * More than one person in the same document: two visual editors, each in
 * a browser of its own, and a collaborator editing the source text.
 *
 * What someone else does shows up without disturbing what I'm doing: my
 * caret and selection stay on the text they were on, what I've typed
 * stays, and undo undoes only what I did.
 */

import { createDocument, expect, fixture, openEditor, test } from './helpers.ts'

const mod = 'ControlOrMeta'
const basic = fixture('basic')

test.describe('seeing each other\'s edits', () => {
  test('what one types, the other sees, and the source has', async ({ page }) => {
    const ana = await openEditor(page)
    const ben = await ana.join()

    await ana.placeCaret('title', 16)
    await ana.page.keyboard.type(' 2026')

    await ben.expectText('title', 'Quarterly report 2026')
    await ana.document.expectSource(basic.replace('Quarterly report', 'Quarterly report 2026'))
  })

  test('someone who opens the document later sees the current text', async ({ page }) => {
    const ana = await openEditor(page)
    await ana.placeCaret('title', 16)
    await ana.page.keyboard.type(' 2026')

    const ben = await ana.join()

    expect(await ben.text('title')).toBe('Quarterly report 2026')
  })

  test('an edit to the source text shows in the visual editor', async ({ page }) => {
    const ana = await openEditor(page)

    ana.document.replace('Third quarter', 'Fourth quarter')

    await ana.expectText('subtitle', 'Fourth quarter')
  })

  test('both type in the same field at once, and both edits stay', async ({ page }) => {
    const ana = await openEditor(page)
    const ben = await ana.join()
    await ana.placeCaret('title', 0)
    await ben.placeCaret('title', 16)

    await Promise.all([ana.page.keyboard.type('The '), ben.page.keyboard.type(' 2026')])

    await ana.expectText('title', 'The Quarterly report 2026')
    await ben.expectText('title', 'The Quarterly report 2026')
    await ana.document.expectSource(basic.replace('Quarterly report', 'The Quarterly report 2026'))
  })

  test('each edits a different field at once, and both edits stay', async ({ page }) => {
    const ana = await openEditor(page)
    const ben = await ana.join()
    await ana.placeCaret('title', 16)
    await ben.placeCaret('subtitle', 13)

    await Promise.all([ana.page.keyboard.type(' 2026'), ben.page.keyboard.type(' results')])

    await ana.document.expectSource(
      basic.replace('Quarterly report', 'Quarterly report 2026').replace('Third quarter', 'Third quarter results'),
    )
    await ana.expectText('subtitle', 'Third quarter results')
    await ben.expectText('title', 'Quarterly report 2026')
  })
})

test.describe('my caret, while someone else edits', () => {
  test('stays on the same text when they type before it in my field', async ({ page }) => {
    const ana = await openEditor(page)
    const ben = await ana.join()
    await ana.placeCaret('title', 10) // Quarterly |report
    await ben.placeCaret('title', 0)

    await ben.page.keyboard.type('The ')

    await ana.expectCaret('title', 14)
    // And my typing goes where my caret was.
    await ana.page.keyboard.type('sales ')
    await ben.expectText('title', 'The Quarterly sales report')
  })

  test('does not move when they type after it', async ({ page }) => {
    const ana = await openEditor(page)
    const ben = await ana.join()
    await ana.placeCaret('title', 10)
    await ben.placeCaret('title', 16)

    await ben.page.keyboard.type(' 2026')

    await ana.expectText('title', 'Quarterly report 2026')
    await ana.expectCaret('title', 10)
  })

  test('does not move when they edit another field, or the source of something else', async ({ page }) => {
    const ana = await openEditor(page)
    const ben = await ana.join()
    await ana.placeCaret('title', 10)
    await ben.placeCaret('subtitle', 13)

    await ben.page.keyboard.type(' results')
    ana.document.replace('Body text.', 'Body text, now longer.')
    ana.document.replace('author: Ana Silva', 'author: Ben Okafor')

    await ana.expectText('subtitle', 'Third quarter results')
    await expect(ana.frame.locator('.quarto-title-meta')).toContainText('Ben Okafor')
    await ana.expectCaret('title', 10)
  })

  test('stays before text they type exactly where it is', async ({ page }) => {
    const ana = await openEditor(page)
    const ben = await ana.join()
    await ana.placeCaret('title', 9)
    await ben.placeCaret('title', 9)

    await ben.page.keyboard.type(' sales')

    await ana.expectText('title', 'Quarterly sales report')
    await ana.expectCaret('title', 9)
  })

  test('goes to where the text was when they delete the text around it', async ({ page }) => {
    const ana = await openEditor(page)
    const ben = await ana.join()
    await ana.placeCaret('title', 13) // Quarterly rep|ort
    await ben.select('title', 9, 16)

    await ben.page.keyboard.press('Backspace')

    await ana.expectText('title', 'Quarterly')
    await ana.expectCaret('title', 9)
  })

  test('keeps the focus: an editor nobody is typing in stays that way', async ({ page }) => {
    const ana = await openEditor(page)
    const ben = await ana.join()
    await ana.clickAt('title', 60)
    await ana.clickAway()
    await ben.placeCaret('title', 16)

    await ben.page.keyboard.type(' 2026')

    await ana.expectText('title', 'Quarterly report 2026')
    expect((await ana.caret()).field).toBeNull()
  })
})

test.describe('my selection, while someone else edits', () => {
  test('stays on the same text', async ({ page }) => {
    const ana = await openEditor(page)
    const ben = await ana.join()
    await ana.select('title', 10, 16) // "report"
    await ben.placeCaret('title', 0)

    await ben.page.keyboard.type('The ')

    await ana.expectText('title', 'The Quarterly report')
    await expect.poll(() => ana.selectedText()).toBe('report')
    // Typing over it replaces what I selected.
    await ana.page.keyboard.type('review')
    await ben.expectText('title', 'The Quarterly review')
  })

  test('does not take in text they type at its edges', async ({ page }) => {
    const ana = await openEditor(page)
    const ben = await ana.join()
    await ana.select('title', 10, 16) // "report"
    await ben.placeCaret('title', 10)

    await ben.page.keyboard.type('sales ')

    await ana.expectText('title', 'Quarterly sales report')
    await expect.poll(() => ana.selectedText()).toBe('report')
  })

  test('across fields stays on the same text', async ({ page }) => {
    const ana = await openEditor(page)
    const ben = await ana.join()
    await ana.placeCaret('title', 10)
    for (let i = 0; i < 12; i++) await ana.page.keyboard.press('Shift+ArrowRight')
    await ben.placeCaret('title', 0)

    await ben.page.keyboard.type('The ')

    await ana.expectText('title', 'The Quarterly report')
    await expect.poll(() => ana.selection()).toEqual({
      anchor: { field: 'title', offset: 14 },
      focus: { field: 'subtitle', offset: 5 },
    })
  })
})

test.describe('what I have typed, while someone else edits', () => {
  test('a space I just typed stays when their edit arrives', async ({ page }) => {
    const ana = await openEditor(page)
    const ben = await ana.join()
    await ana.placeCaret('title', 16)
    await ben.placeCaret('title', 0)

    // The source doesn't hold a space at the end of the title, so this one
    // is only in my editor until I type the next character.
    await ana.page.keyboard.type(' ')
    await ben.page.keyboard.type('The ')
    await ana.expectText('title', 'The Quarterly report ')
    await ana.page.keyboard.type('2026')

    await ben.expectText('title', 'The Quarterly report 2026')
  })

  test('a new, still empty subtitle stays, with my caret in it', async ({ page }) => {
    const ana = await openEditor(page, 'no-subtitle')
    const ben = await ana.join()
    await ana.addField('subtitle')
    await ben.placeCaret('title', 0)

    await ben.page.keyboard.type('The ')

    await ana.expectText('title', 'The Quarterly report')
    expect(await ana.text('subtitle')).toBe('')
    await ana.expectCaret('subtitle', 0)
    // They see it too: it's in the source, empty.
    await ben.expectText('subtitle', '')
  })
})

test.describe('undo, with someone else editing', () => {
  test('undoes what I did and keeps what they did', async ({ page }) => {
    const ana = await openEditor(page)
    const ben = await ana.join()
    await ana.placeCaret('title', 16)
    await ana.page.keyboard.type(' 2026')
    await ben.expectText('title', 'Quarterly report 2026')
    await ben.placeCaret('title', 0)
    await ben.page.keyboard.type('The ')
    await ana.expectText('title', 'The Quarterly report 2026')

    await ana.page.keyboard.press(`${mod}+z`)

    await ana.expectText('title', 'The Quarterly report')
    await ben.expectText('title', 'The Quarterly report')
    await ana.expectCaret('title', 20)

    await ana.page.keyboard.press(`${mod}+Shift+z`)
    await ben.expectText('title', 'The Quarterly report 2026')
  })

  test('does not undo their edits', async ({ page }) => {
    const ana = await openEditor(page)
    const ben = await ana.join()
    await ana.clickAt('title', 60)
    await ben.placeCaret('title', 0)
    await ben.page.keyboard.type('The ')
    await ana.expectText('title', 'The Quarterly report')

    await ana.page.keyboard.press(`${mod}+z`)

    expect(await ana.text('title')).toBe('The Quarterly report')
    await ana.document.expectSource(basic.replace('Quarterly report', 'The Quarterly report'))
  })

  test('still works after edits to other parts of the document', async ({ page }) => {
    const ana = await openEditor(page)
    await ana.placeCaret('title', 16)
    await ana.page.keyboard.type(' 2026')
    await ana.document.expectSource(basic.replace('Quarterly report', 'Quarterly report 2026'))

    ana.document.replace('Body text.', 'Body text, now longer.')
    ana.document.replace('author: Ana Silva', 'author: Ben Okafor')
    await expect(ana.frame.locator('.quarto-title-meta')).toContainText('Ben Okafor')

    await ana.page.keyboard.press(`${mod}+z`)

    await ana.document.expectSource(
      basic.replace('Body text.', 'Body text, now longer.').replace('author: Ana Silva', 'author: Ben Okafor'),
    )
  })
})

test.describe('fields that come and go', () => {
  test('a subtitle they add appears without moving my caret', async ({ page }) => {
    const ana = await openEditor(page, 'no-subtitle')
    await ana.placeCaret('title', 10)

    ana.document.replace('title: Quarterly report\n', 'title: Quarterly report\nsubtitle: Draft\n')

    await ana.expectText('subtitle', 'Draft')
    await ana.expectCaret('title', 10)
  })

  test('a field they remove while my caret is in it empties, and I can type it back', async ({ page }) => {
    const ana = await openEditor(page)
    await ana.placeCaret('subtitle', 5)

    ana.document.replace('subtitle: Third quarter\n', '')

    await ana.expectText('subtitle', '')
    await ana.expectCaret('subtitle', 0)
    await ana.page.keyboard.type('Draft')
    await expect.poll(() => ana.document.source()).toContain('\nsubtitle: Draft\n')
  })

  test('a field they remove while my caret is elsewhere goes away', async ({ page }) => {
    const ana = await openEditor(page)
    await ana.placeCaret('title', 10)

    ana.document.replace('subtitle: Third quarter\n', '')

    await ana.expectText('subtitle', null)
    await ana.expectCaret('title', 10)
  })
})

test.describe('a source that does not parse', () => {
  const broken = 'title: "Quarterly report'

  test('the visual editor keeps showing the last text it could read, and takes no edits', async ({ page }) => {
    const ana = await openEditor(page)
    await ana.placeCaret('title', 16)

    // A collaborator is halfway through quoting the title in the source.
    ana.document.replace('title: Quarterly report', broken)
    await expect(ana.page.locator('.visual-editor .status')).toContainText('Read-only')

    await ana.page.keyboard.type(' 2026')
    await ana.page.keyboard.press('Backspace')
    await ana.page.keyboard.press(`${mod}+z`)

    expect(await ana.text('title')).toBe('Quarterly report')
    expect(ana.document.source()).toBe(basic.replace('title: Quarterly report', broken))
  })

  test('editing works again once the source parses', async ({ page }) => {
    const ana = await openEditor(page)
    await ana.placeCaret('title', 16)
    ana.document.replace('title: Quarterly report', broken)
    await expect(ana.page.locator('.visual-editor .status')).toContainText('Read-only')

    ana.document.replace(broken, 'title: "Quarterly review"')
    await ana.expectText('title', 'Quarterly review')
    await expect(ana.page.locator('.visual-editor .status')).not.toContainText('Read-only')

    await ana.page.keyboard.type(' 2026')
    await expect.poll(() => ana.document.source()).toContain('title: Quarterly review 2026\n')
  })
})

test('a document can be made from any source text', async ({ page }) => {
  const document = await createDocument('---\ntitle: Notes\n---\n')
  const editor = await document.open(page)
  expect(await editor.text('title')).toBe('Notes')
})
