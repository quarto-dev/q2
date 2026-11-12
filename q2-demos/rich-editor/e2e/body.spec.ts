/**
 * The document's body, under the title block. Paragraphs of plain text are
 * edited in place: Enter splits one, Backspace at its start joins it to
 * the one before. Everything else (headings, formatted text, code) is
 * shown as `q2 render` would show it and can't be edited yet, and no edit
 * changes its source.
 */

import { expect, fixture, openEditor, test } from './helpers.ts'

const mod = 'ControlOrMeta'
const body = fixture('body')

test.describe('showing the body', () => {
  test('the body is under the title block, block by block', async ({ page }) => {
    const editor = await openEditor(page, 'body')

    expect(await editor.blocks()).toEqual([
      'p: First paragraph.',
      'p: Second paragraph.',
      'h2: Results',
      'p: Some emphasised text.',
      'p: Last paragraph.',
    ])
    await expect(editor.frame.locator('main p em')).toHaveText('emphasised')
  })

  test('only paragraphs of plain text can be edited', async ({ page }) => {
    const editor = await openEditor(page, 'body')

    expect(await editor.paragraphs()).toEqual(['First paragraph.', 'Second paragraph.', 'Last paragraph.'])
  })

  test('a paragraph wrapped over lines in the source is one paragraph', async ({ page }) => {
    const editor = await openEditor(page, 'wrapped')

    expect(await editor.paragraphs()).toEqual([
      'A paragraph that the author wrapped over two lines.',
      'Another paragraph.',
    ])
  })

  test('typing on a heading changes nothing', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.frame.locator('main h2').click()
    await page.keyboard.type('x')
    await editor.placeCaretInParagraph(0, 16)
    await page.keyboard.type('!')

    await editor.document.expectSource(body.replace('First paragraph.', 'First paragraph.!'))
    expect(await editor.blocks()).toContain('h2: Results')
  })
})

test.describe('editing a paragraph', () => {
  test('typing changes only the paragraph\'s own line of the source', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 6)

    await page.keyboard.type(' short')

    expect(await editor.paragraphs()).toEqual(['First paragraph.', 'Second short paragraph.', 'Last paragraph.'])
    await editor.document.expectSource(body.replace('Second paragraph.', 'Second short paragraph.'))
  })

  test('markdown characters are typed as text', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 0)

    await page.keyboard.type('*not* emphasis: ')

    await expect.poll(() => editor.paragraphs()).toContain('*not* emphasis: First paragraph.')
    await expect(editor.frame.locator('main p em')).toHaveCount(1)
  })

  test('undo takes back the typing', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 6)
    await page.keyboard.type(' short')

    await page.keyboard.press(`${mod}+z`)

    await editor.document.expectSource(body)
    await editor.expectParagraphCaret(1, 6)
  })

  test('editing a wrapped paragraph leaves the other paragraphs\' source alone', async ({ page }) => {
    const editor = await openEditor(page, 'wrapped')
    await editor.placeCaretInParagraph(1, 7)

    await page.keyboard.type(' short')

    await editor.document.expectSource(fixture('wrapped').replace('Another paragraph.', 'Another short paragraph.'))
  })
})

test.describe('splitting and joining paragraphs', () => {
  test('Enter splits the paragraph at the caret', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 6)

    await page.keyboard.press('Enter')

    await editor.expectParagraphCaret(1, 0)
    await editor.document.expectSource(body.replace('First paragraph.', 'First\n\nparagraph.'))
    expect(await editor.paragraphs()).toEqual(['First', 'paragraph.', 'Second paragraph.', 'Last paragraph.'])
  })

  test('Enter at the end starts a new paragraph', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 16)

    await page.keyboard.press('Enter')
    await page.keyboard.type('New one.')

    await editor.expectParagraphCaret(1, 8)
    await editor.document.expectSource(body.replace('First paragraph.', 'First paragraph.\n\nNew one.'))
  })

  test('Backspace at the start joins the paragraph to the one before', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 0)

    await page.keyboard.press('Backspace')

    await editor.expectParagraphCaret(0, 16)
    expect(await editor.paragraphs()).toEqual(['First paragraph.Second paragraph.', 'Last paragraph.'])
    await editor.document.expectSource(
      body.replace('First paragraph.\n\nSecond paragraph.', 'First paragraph.Second paragraph.'),
    )
  })

  test('Delete at the end joins the next paragraph to this one', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 16)

    await page.keyboard.press('Delete')

    await editor.expectParagraphCaret(0, 16)
    expect(await editor.paragraphs()).toEqual(['First paragraph.Second paragraph.', 'Last paragraph.'])
  })

  test('undo splits a joined paragraph again', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 0)
    await page.keyboard.press('Backspace')

    await page.keyboard.press(`${mod}+z`)

    await editor.document.expectSource(body)
    await editor.expectParagraphCaret(1, 0)
  })

  test('pasted lines become paragraphs', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 16)

    await editor.paste(' One.\nTwo.')

    await editor.document.expectSource(body.replace('First paragraph.', 'First paragraph. One.\n\nTwo.'))
  })
})

test.describe('around what can\'t be edited', () => {
  test('Backspace at the start of a paragraph after one deletes nothing', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(2, 0)

    await page.keyboard.press('Backspace')
    await page.keyboard.type('The ')

    await editor.document.expectSource(body.replace('Last paragraph.', 'The Last paragraph.'))
  })

  test('deleting a selection across them trims the paragraphs and keeps them', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 6)
    await editor.shiftClickParagraph(2, 4)
    expect(await editor.selectedText()).toContain('Results')

    await page.keyboard.press('Backspace')

    await editor.document.expectSource(
      body.replace('Second paragraph.', 'Second').replace('Last paragraph.', 'paragraph.'),
    )
    expect(await editor.blocks()).toEqual([
      'p: First paragraph.',
      'p: Second',
      'h2: Results',
      'p: Some emphasised text.',
      'p: paragraph.',
    ])
  })
})

test.describe('between the title block and the body', () => {
  test('Enter in the last field of the title block goes to the body', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('subtitle', 13)

    await page.keyboard.press('Enter')

    await editor.expectParagraphCaret(0, 0)
  })

  test('Backspace at the start of the body goes to the title block without deleting', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaretInParagraph(0, 0)

    await page.keyboard.press('Backspace')

    await editor.expectCaret('subtitle', 13)
    await editor.document.expectSource(fixture('basic'))
  })

  test('the arrow keys move between them', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('subtitle', 13)

    await page.keyboard.press('ArrowRight')
    await editor.expectParagraphCaret(0, 0)

    await page.keyboard.press('ArrowLeft')
    await editor.expectCaret('subtitle', 13)
  })
})

test.describe('with collaborators', () => {
  test('a paragraph added to the source above the caret leaves the caret on its text', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 6)

    editor.document.replace('First paragraph.', 'First paragraph.\n\nAn addition.')

    await expect.poll(() => editor.paragraphs()).toContain('An addition.')
    await editor.expectParagraphCaret(2, 6)
    await page.keyboard.type(' short')
    await editor.document.expectSource(
      body
        .replace('First paragraph.', 'First paragraph.\n\nAn addition.')
        .replace('Second paragraph.', 'Second short paragraph.'),
    )
  })

  test('a paragraph that gets formatting in the source stops being editable', async ({ page }) => {
    const editor = await openEditor(page, 'body')

    editor.document.replace('Second paragraph.', 'Second **bold** paragraph.')

    await expect.poll(() => editor.paragraphs()).toEqual(['First paragraph.', 'Last paragraph.'])
    await expect(editor.frame.locator('main p strong')).toHaveText('bold')
  })

  test('two people type in different paragraphs at once, and both edits stay', async ({ page }) => {
    const ana = await openEditor(page, 'body')
    const ben = await ana.join()
    await ana.placeCaretInParagraph(0, 16)
    await ben.placeCaretInParagraph(2, 15)

    await Promise.all([ana.page.keyboard.type(' Ana.'), ben.page.keyboard.type(' Ben.')])

    await ana.document.expectSource(
      body.replace('First paragraph.', 'First paragraph. Ana.').replace('Last paragraph.', 'Last paragraph. Ben.'),
    )
    await expect.poll(() => ana.paragraphs()).toEqual(['First paragraph. Ana.', 'Second paragraph.', 'Last paragraph. Ben.'])
  })

  test('someone splits the paragraph I am typing in, and my caret stays on its text', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 17)

    editor.document.replace('Second paragraph.', 'Second\n\nparagraph.')

    await editor.expectParagraphCaret(2, 10)
  })
})

test.describe('moving through the document', () => {
  test('the keys for the document\'s start and end go to its first and last text', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 3)

    await page.keyboard.press(`${mod}+ArrowDown`)
    await editor.expectParagraphCaret(2, 15)

    await page.keyboard.press(`${mod}+ArrowUp`)
    await editor.expectCaret('title', 0)
  })

  test('Down and Up step over what can\'t be edited', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 3)

    // Onto the nearest line that can be edited, at about the same place
    // across it.
    await page.keyboard.press('ArrowDown')
    await page.keyboard.type('x')
    await expect.poll(async () => (await editor.paragraphs())[2]).toContain('x')
    expect((await editor.paragraphs()).slice(0, 2)).toEqual(['First paragraph.', 'Second paragraph.'])
    expect((await editor.blocks())[2]).toBe('h2: Results')

    await page.keyboard.press('ArrowUp')
    await page.keyboard.type('y')
    await expect.poll(async () => (await editor.paragraphs())[1]).toContain('y')
  })
})
