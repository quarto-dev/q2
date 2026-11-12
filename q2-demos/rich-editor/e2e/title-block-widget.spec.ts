/**
 * Adding and removing title-block fields: the row of "+" items above the
 * title block, one for each field the document doesn't have, and what
 * happens to a field once it's empty.
 *
 * A document needs no title block, and has only the fields its author
 * wants. The row stays out of the way: it shows only while the caret is in
 * the title block or the pointer is over it (or the document is empty),
 * and it never takes space of its own, so showing it moves nothing. An
 * added field stays, showing its placeholder, until Backspace in it while
 * it's empty removes it; the source holds it, empty, all along.
 */

import { createDocument, expect, fixture, lineStart, openEditor, test } from './helpers.ts'

const mod = 'ControlOrMeta'

test.describe('the "+" row', () => {
  test('has an item for each title-block field the document lacks, in reading order', async ({ page }) => {
    const basic = await openEditor(page)
    expect(await basic.widgetItems()).toEqual(['+ Abstract'])

    const noSubtitle = await openEditor(page, 'no-subtitle')
    expect(await noSubtitle.widgetItems()).toEqual(['+ Subtitle', '+ Abstract'])

    const untitled = await openEditor(page, 'untitled')
    expect(await untitled.widgetItems()).toEqual(['+ Title', '+ Subtitle', '+ Abstract'])
  })

  test('shows while the caret is in the title block, and not once it is in the body', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.clickAt('title', 60)
    await editor.pointAway()
    await editor.expectWidgetShown(true)

    await editor.placeCaretInParagraph(0, 0)
    await editor.pointAway()

    await editor.expectWidgetShown(false)
  })

  test('shows while the pointer is over the title block', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.placeCaretInParagraph(0, 0)
    await editor.pointAway()
    await editor.expectWidgetShown(false)

    await editor.hoverTitleBlock()

    await editor.expectWidgetShown(true)
  })

  test('shows in an empty document', async ({ page }) => {
    const editor = await createDocument(fixture('empty')).then((d) => d.open(page, { shows: 'nothing' }))
    await editor.pointAway()

    await editor.expectWidgetShown(true)
    expect(await editor.widgetItems()).toEqual(['+ Title', '+ Subtitle', '+ Abstract'])
  })

  test('showing it moves nothing', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.placeCaretInParagraph(0, 0)
    await editor.pointAway()
    await editor.expectWidgetShown(false)
    const layout = await editor.layout()
    const bodyTop = await editor.bodyTop()

    await editor.clickAt('title', 60)
    await editor.expectWidgetShown(true)

    expect(await editor.layout()).toEqual(layout)
    expect(await editor.bodyTop()).toBe(bodyTop)
  })

  test('sits above the title, clear of its text', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.clickAt('title', 60)

    const row = (await editor.widget().boundingBox())!
    const title = (await editor.field('title').boundingBox())!
    expect(row.y + row.height).toBeLessThanOrEqual(title.y + 1)
  })

  test('is not part of the document: the source never mentions it', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.clickAt('title', 60)
    await page.keyboard.press('ArrowLeft')

    expect(editor.document.source()).toBe(fixture('no-subtitle'))
  })
})

test.describe('adding a field', () => {
  test('its item adds it, empty, with its placeholder and the caret in it', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')

    await editor.addField('subtitle')

    expect(await editor.text('subtitle')).toBe('')
    expect(await editor.placeholderShown('subtitle')).toBe(true)
    await editor.expectCaret('subtitle', 0)
    expect(await editor.widgetItems()).toEqual(['+ Abstract'])
  })

  test('the new field is in the source at once, empty, and then with what is typed', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')

    await editor.addField('subtitle')
    await expect.poll(() => editor.document.source()).toContain('\nsubtitle: ""\n')

    await page.keyboard.type('Draft')
    await expect.poll(() => editor.document.source()).toContain('\nsubtitle: Draft\n')
  })

  test('a document with no title block gets one', async ({ page }) => {
    const editor = await openEditor(page, 'untitled')

    await editor.addField('title')
    await page.keyboard.type('Notes')

    await editor.expectText('title', 'Notes')
    await editor.document.expectSource(`---\ntitle: Notes\n---\n\n${fixture('untitled')}`)
  })

  test('an empty document gets a title', async ({ page }) => {
    const editor = await createDocument(fixture('empty')).then((d) => d.open(page, { shows: 'nothing' }))

    await editor.addField('title')
    await page.keyboard.type('Notes')

    await editor.expectText('title', 'Notes')
    await expect.poll(() => editor.document.source()).toContain('title: Notes\n')
  })

  test('undo takes the new field away again', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.addField('subtitle')
    await expect.poll(() => editor.document.source()).toContain('subtitle')

    await page.keyboard.press(`${mod}+z`)

    await editor.expectText('subtitle', null)
    await editor.document.expectSource(fixture('no-subtitle'))
  })
})

test.describe('the keyboard', () => {
  test('Left at the start of the first field goes to the last item of the row', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.placeCaret('title', 0)

    await page.keyboard.press('ArrowLeft')

    await editor.expectFocusedWidgetItem('+ Abstract')
    await editor.expectWidgetShown(true)
  })

  test('Up on the first line of the first field goes to the row', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.clickAt('title', 5)

    await page.keyboard.press('ArrowUp')

    await editor.expectFocusedWidgetItem('+ Subtitle')
  })

  test('Up from the body goes to the row when there is no title block', async ({ page }) => {
    const editor = await openEditor(page, 'untitled')
    await editor.placeCaretInParagraph(0, 3)

    await page.keyboard.press('ArrowUp')

    await editor.expectFocusedWidgetItem('+ Title')
    await editor.expectWidgetShown(true)
  })

  test('Left and Right move along the row, and Left stops at its first item', async ({ page }) => {
    const editor = await openEditor(page, 'untitled')
    await editor.placeCaretInParagraph(0, 0)
    await page.keyboard.press('ArrowLeft')
    await editor.expectFocusedWidgetItem('+ Abstract')

    await page.keyboard.press('ArrowLeft')
    await editor.expectFocusedWidgetItem('+ Subtitle')
    await page.keyboard.press('ArrowLeft')
    await editor.expectFocusedWidgetItem('+ Title')
    await page.keyboard.press('ArrowLeft')
    await editor.expectFocusedWidgetItem('+ Title')

    await page.keyboard.press('ArrowRight')
    await editor.expectFocusedWidgetItem('+ Subtitle')
  })

  test('Right past the last item goes to the start of the first field', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.placeCaret('title', 0)
    await page.keyboard.press('ArrowLeft')

    await page.keyboard.press('ArrowRight')

    await editor.expectCaret('title', 0)
  })

  test('Down and Escape go back to the start of the first field', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.placeCaret('title', 0)

    await page.keyboard.press('ArrowLeft')
    await page.keyboard.press('ArrowDown')
    await editor.expectCaret('title', 0)

    await page.keyboard.press('ArrowLeft')
    await page.keyboard.press('Escape')
    await editor.expectCaret('title', 0)
  })

  test('Enter on an item adds its field', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.placeCaret('title', 0)
    await page.keyboard.press('ArrowLeft')
    await page.keyboard.press('ArrowLeft')
    await editor.expectFocusedWidgetItem('+ Subtitle')

    await page.keyboard.press('Enter')

    expect(await editor.text('subtitle')).toBe('')
    await editor.expectCaret('subtitle', 0)
  })

  test('Enter in a field goes on to the next field, never into the row or to a new field', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.placeCaret('title', 16)

    await page.keyboard.press('Enter')

    await editor.expectParagraphCaret(0, 0)
    expect(await editor.text('subtitle')).toBeNull()
    expect(editor.document.source()).toBe(fixture('no-subtitle'))
  })
})

test.describe('an empty field', () => {
  test('stays, with its placeholder, when the caret leaves it, and the source holds it empty', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.select('subtitle', 0, 13)
    await page.keyboard.press('Backspace')

    await editor.clickAt('title', 60)

    expect(await editor.text('subtitle')).toBe('')
    expect(await editor.placeholderShown('subtitle')).toBe(true)
    await editor.document.expectSource(fixture('basic').replace('subtitle: Third quarter', 'subtitle: ""'))
  })

  test('Backspace in it removes it, from the page and the source, and goes to the end of the field before', async ({
    page,
  }) => {
    const editor = await openEditor(page)
    await editor.select('subtitle', 0, 13)
    await page.keyboard.press('Backspace')

    await page.keyboard.press('Backspace')

    await editor.expectText('subtitle', null)
    await editor.expectCaret('title', 16)
    await editor.document.expectSource(fixture('basic').replace('subtitle: Third quarter\n', ''))
    expect(await editor.widgetItems()).toEqual(['+ Subtitle', '+ Abstract'])
  })

  test('Backspace in the first field, empty, removes it and goes to its item in the row', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.select('title', 0, 16)
    await page.keyboard.press('Backspace')

    await page.keyboard.press('Backspace')

    await editor.expectText('title', null)
    await editor.expectFocusedWidgetItem('+ Title')
    await editor.expectWidgetShown(true)

    // So Enter puts it straight back.
    await page.keyboard.press('Enter')
    await editor.expectCaret('title', 0)
  })

  test('holding Backspace empties a field and stops there', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaretAtEnd('subtitle')

    // A key held down repeats: every press after the first is a repeat.
    for (let i = 0; i < 20; i++) await page.keyboard.down('Backspace')
    await page.keyboard.up('Backspace')

    expect(await editor.text('subtitle')).toBe('')
    expect(await editor.text('title')).toBe('Quarterly report')
    await editor.expectCaret('subtitle', 0)
  })

  test('undo after removing it brings it back, empty', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.select('subtitle', 0, 13)
    await page.keyboard.press('Backspace')
    await page.keyboard.press('Backspace')
    await editor.expectText('subtitle', null)

    await editor.document.expectSource(fixture('basic').replace('subtitle: Third quarter\n', ''))

    await page.keyboard.press(`${mod}+z`)

    await editor.expectText('subtitle', '')
    // Back in the source, empty; after the last key, like any added key
    // (see the fixme in `source.spec.ts`).
    await expect.poll(() => editor.document.source()).toContain('\nsubtitle: ""\n')
    expect(editor.document.source()).not.toContain('Third quarter')
  })

  test('Up and Down stop on each empty field, never skipping one', async ({ page }) => {
    const editor = await openEditor(page, 'empty-fields')
    await editor.placeCaretInParagraph(0, 3)

    await page.keyboard.press('ArrowUp')
    await editor.expectCaret('subtitle', 0)
    await page.keyboard.press('ArrowUp')
    await editor.expectCaret('title', 0)

    await page.keyboard.press('ArrowDown')
    await editor.expectCaret('subtitle', 0)
    // Back where it came from: Up and Down keep the caret's column.
    await page.keyboard.press('ArrowDown')
    await editor.expectParagraphCaret(0, 3)
  })

  test('Up and Down stop on a field just added, still empty', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.addField('subtitle')
    await page.keyboard.press('ArrowUp')
    await editor.expectCaret('title', 0)

    await page.keyboard.press('ArrowDown')
    await editor.expectCaret('subtitle', 0)
    await page.keyboard.press('ArrowDown')
    await editor.expectParagraphCaret(0, 0)

    await page.keyboard.press('ArrowUp')
    await editor.expectCaret('subtitle', 0)
  })

  test('a field removed and added again shows its caret', async ({ page }) => {
    // The title block keeps the author when the title goes, so it's there,
    // with nothing to edit in it, until the title comes back.
    const editor = await openEditor(page, 'no-subtitle')
    await editor.select('title', 0, 16)
    await page.keyboard.press('Backspace')
    await page.keyboard.press('Backspace')
    await editor.expectFocusedWidgetItem('+ Title')

    await page.keyboard.press('Enter')
    await editor.expectCaret('title', 0)

    expect(await editor.caretVisible('title')).toBe(true)
    await page.keyboard.type('Notes')
    await editor.expectText('title', 'Notes')
    expect(await editor.caretVisible('title')).toBe(true)
  })

  test('typing into it again puts the text back on its line', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.select('title', 0, 16)
    await page.keyboard.press('Backspace')
    await page.keyboard.type('Annual report')

    await editor.document.expectSource(fixture('basic').replace('Quarterly report', 'Annual report'))
  })
})

test.describe('with collaborators', () => {
  test('a field someone adds shows to everyone, empty, with its placeholder', async ({ page }) => {
    const ana = await openEditor(page, 'no-subtitle')
    const ben = await ana.join()
    await ben.placeCaretInParagraph(0, 0)

    await ana.addField('subtitle')

    await ben.expectText('subtitle', '')
    expect(await ben.placeholderShown('subtitle')).toBe(true)
    expect(await ben.caret()).toMatchObject({ field: null })
    await ben.expectParagraphCaret(0, 0)
  })

  test('a field someone removes goes from everyone', async ({ page }) => {
    const ana = await openEditor(page)
    const ben = await ana.join()
    await ben.placeCaret('title', 3)
    await ana.select('subtitle', 0, 13)
    await ana.page.keyboard.press('Backspace')
    await ben.expectText('subtitle', '')

    await ana.page.keyboard.press('Backspace')

    await ben.expectText('subtitle', null)
    await ben.expectCaret('title', 3)
  })

  test('an empty field someone adds in the source shows, with its placeholder', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.placeCaret('title', 3)

    editor.document.replace('title: Quarterly report\n', 'title: Quarterly report\nsubtitle: ""\n')

    await editor.expectText('subtitle', '')
    await editor.expectCaret('title', 3)
  })
})

test('Home in the first field keeps the caret in it', async ({ page }) => {
  // The row is outside the fields: the keys that move the caret along a
  // line never take it there.
  const editor = await openEditor(page, 'no-subtitle')
  await editor.placeCaret('title', 5)

  await page.keyboard.press(lineStart)

  await editor.expectCaret('title', 0)
  expect(await editor.focusedWidgetItem()).toBeNull()
})
