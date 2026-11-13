/**
 * The document's body, under the title block. Paragraphs of plain text are
 * edited in place: Enter splits one, Backspace at its start joins it to
 * the one before. (So are headings of plain text: see `headings.spec.ts`.)
 * Everything else (formatted text, code) is shown as `q2 render` would
 * show it and can't be edited yet, and no edit changes its source.
 */

import { createDocument, expect, fixture, lineEnd, openEditor, test, type Editor } from './helpers.ts'

const mod = 'ControlOrMeta'
const body = fixture('body')
const basic = fixture('basic')

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

  test('typing on a heading with formatting changes nothing', async ({ page }) => {
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
    await editor.document.expectSource(body.replace('First paragraph.', '\\*not\\* emphasis: First paragraph.'))
  })

  test('spaces the source does not keep show while the caret is in the paragraph', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaretInParagraph(0, 10)

    // At the end of the paragraph: written with the next character.
    await page.keyboard.type('  ')
    expect(await editor.paragraphs()).toEqual(['Body text.  '])
    expect(editor.document.source()).toBe(basic)

    // More than one in a row: one in the source.
    await page.keyboard.type('x')
    expect(await editor.paragraphs()).toEqual(['Body text.  x'])
    await editor.document.expectSource(basic.replace('Body text.', 'Body text. x'))

    await page.keyboard.press('ArrowUp')
    await expect.poll(() => editor.paragraphs()).toEqual(['Body text. x'])
  })

  test('Shift+Enter inserts nothing', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 6)

    await page.keyboard.press('Shift+Enter')

    expect(await editor.paragraphs()).toEqual(['First paragraph.', 'Second paragraph.', 'Last paragraph.'])
    await editor.expectParagraphCaret(0, 6)
    expect(editor.document.source()).toBe(body)
  })

  test('a typed apostrophe is shown curly, as the document reads it, once the caret leaves the paragraph', async ({
    page,
  }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 16)
    await page.keyboard.type(" It's \"so\".")
    expect((await editor.paragraphs())[0]).toBe('First paragraph. It\'s "so".')

    await editor.placeCaretInParagraph(1, 0)

    // The source has what was typed; a typed double quote stays as typed.
    await expect.poll(async () => (await editor.paragraphs())[0]).toBe('First paragraph. It’s "so".')
    await editor.document.expectSource(body.replace('First paragraph.', 'First paragraph. It\'s \\"so\\".'))
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

  test('typing in a wrapped paragraph keeps its line breaks', async ({ page }) => {
    const editor = await openEditor(page, 'wrapped')
    await editor.placeCaretInParagraph(0, 11)

    await page.keyboard.type(' long')

    await editor.document.expectSource(fixture('wrapped').replace('A paragraph', 'A paragraph long'))
  })

  test('splitting a wrapped paragraph keeps each half\'s line breaks', async ({ page }) => {
    const editor = await openEditor(page, 'wrapped')
    await editor.placeCaretInParagraph(0, 2)

    await page.keyboard.press('Enter')

    await editor.document.expectSource(fixture('wrapped').replace('A paragraph', 'A\n\nparagraph'))
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

  test('Enter at the start puts an empty paragraph above, and the caret stays with the text', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 0)

    await page.keyboard.press('Enter')

    await editor.expectParagraphCaret(2, 0)
    expect(await editor.paragraphs()).toEqual(['First paragraph.', '', 'Second paragraph.', 'Last paragraph.'])
    await editor.document.expectSource(body.replace('Second paragraph.', '\u00a0\n\nSecond paragraph.'))
  })

  test('Enter is an undo step of its own', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 16)
    await page.keyboard.type('ab')
    await page.keyboard.press('Enter')
    await page.keyboard.type('cd')
    const firstTwo = async () => (await editor.paragraphs()).slice(0, 2)
    expect(await firstTwo()).toEqual(['First paragraph.ab', 'cd'])

    await page.keyboard.press(`${mod}+z`)
    expect(await firstTwo()).toEqual(['First paragraph.ab', ''])
    await page.keyboard.press(`${mod}+z`)
    expect(await firstTwo()).toEqual(['First paragraph.ab', 'Second paragraph.'])
    await page.keyboard.press(`${mod}+z`)
    expect(await firstTwo()).toEqual(['First paragraph.', 'Second paragraph.'])
    await editor.document.expectSource(body)
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

  test('deleting a selection across paragraphs joins them', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 6)
    await editor.shiftClickParagraph(1, 6)

    await page.keyboard.press('Backspace')

    await editor.expectParagraphCaret(0, 6)
    expect((await editor.paragraphs()).slice(1)).toEqual(['Last paragraph.'])
    // ("First " and " paragraph.": the two spaces are one in the source.)
    await editor.document.expectSource(
      body.replace('First paragraph.\n\nSecond paragraph.', 'First paragraph.'),
    )
  })

  test('undo splits a joined paragraph again', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 0)
    await page.keyboard.press('Backspace')

    await page.keyboard.press(`${mod}+z`)

    await editor.document.expectSource(body)
    await editor.expectParagraphCaret(1, 0)
  })

  test('Enter on an empty line adds another, and each is an empty paragraph', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 16)

    await page.keyboard.press('Enter')
    await page.keyboard.press('Enter')
    await page.keyboard.press('Enter')
    await page.keyboard.type('New one.')

    await editor.expectParagraphCaret(3, 8)
    expect((await editor.paragraphs()).slice(0, 4)).toEqual(['First paragraph.', '', '', 'New one.'])
    // An empty paragraph is a non-breaking space on a line of its own.
    await editor.document.expectSource(
      body.replace('First paragraph.', 'First paragraph.\n\n\u00a0\n\n\u00a0\n\nNew one.'),
    )
  })

  test('an empty paragraph stays, as tall as a line, when the caret leaves it', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 16)
    await page.keyboard.press('Enter')

    await editor.placeCaretInParagraph(2, 0)

    expect(await editor.paragraphs()).toEqual(['First paragraph.', '', 'Second paragraph.', 'Last paragraph.'])
    const [first, emptyLine] = await editor.frame.evaluate(() =>
      [...document.querySelectorAll('#quarto-document-content > p.qe-field')]
        .slice(0, 2)
        .map((el) => el.getBoundingClientRect().height),
    )
    expect(emptyLine).toBe(first)
  })

  test('Backspace on an empty paragraph removes it', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 16)
    await page.keyboard.press('Enter')
    await editor.document.expectSource(body.replace('First paragraph.', 'First paragraph.\n\n\u00a0'))

    await page.keyboard.press('Backspace')

    await editor.expectParagraphCaret(0, 16)
    await editor.document.expectSource(body)
  })

  test('an empty paragraph shows nothing: it has no placeholder', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 16)
    await page.keyboard.press('Enter')

    const empty = editor.paragraph(1)
    expect(await empty.evaluate((el) => (el as HTMLElement).innerText.trim())).toBe('')
    expect(await empty.evaluate((el) => getComputedStyle(el, '::before').content)).toBe('none')
  })

  test('a paragraph emptied of its text stays, as an empty paragraph, until Backspace removes it', async ({
    page,
  }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 0)
    await page.keyboard.press(`Shift+${lineEnd}`)

    await page.keyboard.press('Backspace')
    expect(await editor.paragraphs()).toEqual(['First paragraph.', '', 'Last paragraph.'])
    await editor.document.expectSource(body.replace('Second paragraph.', '\u00a0'))

    await page.keyboard.press('Backspace')
    await editor.expectParagraphCaret(0, 16)
    await editor.document.expectSource(body.replace('Second paragraph.\n\n', ''))
  })

  test('the only paragraph, emptied, is gone from the source, and from the page when the caret leaves it', async ({
    page,
  }) => {
    const editor = await openEditor(page)
    await editor.placeCaretInParagraph(0, 4)
    await page.keyboard.press(`${mod}+a`)

    await page.keyboard.press('Backspace')
    expect(await editor.blocks()).toEqual(['p: '])
    await editor.document.expectSource(basic.replace('Body text.\n', ''))

    await page.keyboard.press('ArrowUp')
    expect((await editor.caret()).field).toBe('subtitle')
    expect(await editor.blocks()).toEqual([])
  })

  test('an empty paragraph in the source is an empty line', async ({ page }) => {
    const editor = await openEditor(page, 'body')

    editor.document.replace('First paragraph.', 'First paragraph.\n\n&nbsp;')

    await expect
      .poll(() => editor.paragraphs())
      .toEqual(['First paragraph.', '', 'Second paragraph.', 'Last paragraph.'])
  })

  test('pasted lines become paragraphs', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 16)

    await editor.paste(' One.\nTwo.')

    await editor.document.expectSource(body.replace('First paragraph.', 'First paragraph. One.\n\nTwo.'))
  })

  test('a blank line in pasted text is an empty paragraph, and its tabs and runs of spaces are single spaces', async ({
    page,
  }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 16)

    await editor.paste(' One.\n\nTwo.\r\n\tThree  four.')

    await editor.document.expectSource(
      body.replace('First paragraph.', 'First paragraph. One.\n\n\u00a0\n\nTwo.\n\nThree four.'),
    )
    await editor.expectParagraphCaret(3, 13)
  })

  test('the character that stands for an object in text is never put in', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 16)

    await editor.paste(' a\uFFFCb')

    await editor.document.expectSource(body.replace('First paragraph.', 'First paragraph. ab'))
    expect(await editor.blocks()).toHaveLength(5)
  })
})

test.describe('select all', () => {
  test('in a paragraph selects the whole body, then the whole document', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 3)

    await page.keyboard.press(`${mod}+a`)
    const lines = (await editor.selectedText()).split('\n').filter((line) => line !== '')
    expect(lines).toEqual(['First paragraph.', 'Second paragraph.', 'Results', 'Some emphasised text.', 'Last paragraph.'])

    await page.keyboard.press(`${mod}+a`)
    expect((await editor.selectedText()).startsWith('Quarterly report')).toBe(true)
    expect((await editor.selectedText()).endsWith('Last paragraph.')).toBe(true)
  })

  test('then Backspace deletes every paragraph and keeps what can\'t be edited, and undo brings them back', async ({
    page,
  }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 3)
    await page.keyboard.press(`${mod}+a`)

    await page.keyboard.press('Backspace')
    expect(await editor.blocks()).toEqual(['h2: Results', 'p: Some emphasised text.'])
    expect(await editor.text('title')).toBe('Quarterly report')
    await editor.document.expectSource(
      body
        .replace('First paragraph.\n\nSecond paragraph.\n\n', '')
        .replace('\n\nLast paragraph.', ''),
    )

    await page.keyboard.press(`${mod}+z`)
    await editor.document.expectSource(body)
    expect(await editor.blocks()).toHaveLength(5)
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

test.describe('around what can\'t be edited, with the mouse and the keys', () => {
  const caretColor = (editor: Editor) =>
    editor.frame.evaluate(() => {
      const node = document.getSelection()!.focusNode!
      const el = node.nodeType === Node.ELEMENT_NODE ? (node as Element) : node.parentElement!
      return getComputedStyle(el).caretColor
    })

  test('clicking a heading shows no caret', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.frame.locator('main h2').click()
    await page.waitForTimeout(100)

    expect(await caretColor(editor)).toBe('rgba(0, 0, 0, 0)')
  })

  test('an arrow key after clicking a heading goes to the nearest paragraph that way', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    const heading = editor.frame.locator('main h2')

    // Forward, past the formatted paragraph too.
    await heading.click()
    await page.keyboard.press('ArrowDown')
    await editor.expectParagraphCaret(2, 0)
    await heading.click()
    await page.keyboard.press('ArrowRight')
    await editor.expectParagraphCaret(2, 0)

    await heading.click()
    await page.keyboard.press('ArrowUp')
    await editor.expectParagraphCaret(1, 17)
    await heading.click()
    await page.keyboard.press('ArrowLeft')
    await editor.expectParagraphCaret(1, 17)
  })

  test('Delete at the end of the paragraph before one deletes nothing', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 17)

    await page.keyboard.press('Delete')
    await page.keyboard.type('!')

    await editor.document.expectSource(body.replace('Second paragraph.', 'Second paragraph.!'))
  })

  test('Enter at the end of the paragraph before one starts a paragraph between them', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 17)

    await page.keyboard.press('Enter')
    await page.keyboard.type('New')

    expect(await editor.blocks()).toEqual([
      'p: First paragraph.',
      'p: Second paragraph.',
      'p: New',
      'h2: Results',
      'p: Some emphasised text.',
      'p: Last paragraph.',
    ])
    await editor.document.expectSource(body.replace('Second paragraph.', 'Second paragraph.\n\nNew'))
  })

  test('Right and Left step over them, to the nearest paragraph that way', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 17)

    await page.keyboard.press('ArrowRight')
    await editor.expectParagraphCaret(2, 0)

    await page.keyboard.press('ArrowLeft')
    await editor.expectParagraphCaret(1, 17)
  })

  // One way it could work: a click on a block that can't be edited, then
  // Enter, starts a paragraph after it.
  test.fixme('a paragraph can be started between two of them', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.frame.locator('main h2').click()

    // Today there is nowhere between the heading and the formatted
    // paragraph for the caret to be.
    await page.keyboard.press('Enter')
    await page.keyboard.type('New')

    await editor.document.expectSource(body.replace('## Results', '## Results\n\nNew'))
  })

  test.fixme('a paragraph can be started after one at the end of the document', async ({ page }) => {
    const source = '---\ntitle: Notes\n---\n\nSome text.\n\n## The end\n'
    const document = await createDocument(source)
    const editor = await document.open(page)
    await editor.frame.locator('main h2').click()

    await page.keyboard.press('Enter')
    await page.keyboard.type('New')

    await document.expectSource(`${source}\nNew\n`)
  })

  test('typing over a selection across them puts the text in the first paragraph', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 6)
    await editor.shiftClickParagraph(2, 4)

    await page.keyboard.type('X')

    expect(await editor.blocks()).toEqual([
      'p: First paragraph.',
      'p: SecondX',
      'h2: Results',
      'p: Some emphasised text.',
      'p: paragraph.',
    ])
    await editor.document.expectSource(
      body.replace('Second paragraph.', 'SecondX').replace('Last paragraph.', 'paragraph.'),
    )
  })
})

test.describe('between the title block and the body', () => {
  test('Enter in the last field of the title block goes to the body', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('subtitle', 13)

    await page.keyboard.press('Enter')

    await editor.expectParagraphCaret(0, 0)
  })

  test('Backspace at the start of the body stays there and deletes nothing', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaretInParagraph(0, 0)

    await page.keyboard.press('Backspace')

    await editor.expectParagraphCaret(0, 0)
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

test.describe('between the title block and a body that starts with what can\'t be edited', () => {
  // (A heading of plain text can be edited; these have formatting.)
  const source = '---\ntitle: Notes\nsubtitle: Rough\n---\n\n## *Results*\n\nSome text.\n'

  test('Enter in the last field of the title block goes to the first paragraph', async ({ page }) => {
    const editor = await (await createDocument(source)).open(page)
    await editor.placeCaret('subtitle', 5)

    await page.keyboard.press('Enter')

    await editor.expectParagraphCaret(0, 0)
  })

  test('Left at the start of the first paragraph goes to the title block, and Backspace there does nothing', async ({
    page,
  }) => {
    const document = await createDocument(source)
    const editor = await document.open(page)
    await editor.placeCaretInParagraph(0, 0)

    await page.keyboard.press('Backspace')
    await editor.expectParagraphCaret(0, 0)
    expect(document.source()).toBe(source)

    await page.keyboard.press('ArrowLeft')
    await editor.expectCaret('subtitle', 5)
  })

  test('Enter stays where it is when the body has no paragraph to go to', async ({ page }) => {
    const headingOnly = '---\ntitle: Notes\nsubtitle: Rough\n---\n\n## *Results*\n'
    const document = await createDocument(headingOnly)
    const editor = await document.open(page)
    await editor.placeCaret('subtitle', 5)

    await page.keyboard.press('Enter')
    await page.keyboard.type('!')

    await editor.expectCaret('subtitle', 6)
    await document.expectSource(headingOnly.replace('Rough', 'Rough!'))
    expect(await editor.blocks()).toEqual(['h2: Results'])
  })
})

test.describe('a document without a subtitle', () => {
  test('Enter in the title goes to the body, and adds no subtitle', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaret('title', 16)

    await page.keyboard.press('Enter')

    await editor.expectParagraphCaret(0, 0)
    expect(await editor.text('subtitle')).toBeNull()
    expect(editor.document.source()).toBe(body)
  })
})

test.describe('a document without a title block', () => {
  const source = 'Just text.\n\nMore.\n'

  test('is its body, with the caret at the start of the first paragraph', async ({ page }) => {
    const editor = await (await createDocument(source)).open(page)

    await expect(editor.frame.locator('#title-block-header')).toHaveCount(0)
    expect(await editor.blocks()).toEqual(['p: Just text.', 'p: More.'])
    await editor.expectParagraphCaret(0, 0)
  })

  test('Backspace at its start stays put; Left and Up go to the "+" row, and back', async ({ page }) => {
    // Adding a title from the row is in `title-block-widget.spec.ts`.
    const document = await createDocument(source)
    const editor = await document.open(page)
    await editor.expectParagraphCaret(0, 0)

    await page.keyboard.press('Backspace')
    await editor.expectParagraphCaret(0, 0)
    await page.keyboard.press('ArrowLeft')
    await editor.expectFocusedWidgetItem('+ Abstract')
    await page.keyboard.press('ArrowRight')
    await editor.expectParagraphCaret(0, 0)
    await page.keyboard.press('ArrowUp')
    await editor.expectFocusedWidgetItem('+ Title')
    await page.keyboard.press('ArrowDown')
    await editor.expectParagraphCaret(0, 0)

    await page.keyboard.type('Only ')
    await document.expectSource(`Only ${source}`)
    await expect(editor.frame.locator('#title-block-header')).toHaveCount(0)
  })
})

test.describe('a document without a body', () => {
  const source = '---\ntitle: Notes\nsubtitle: Rough\n---\n'

  test('Enter in the last field of the title block starts the body, with the caret in it', async ({ page }) => {
    const document = await createDocument(source)
    const editor = await document.open(page)
    expect(await editor.blocks()).toEqual([])
    await editor.placeCaret('subtitle', 5)

    await page.keyboard.press('Enter')
    expect(await editor.blocks()).toEqual(['p: '])
    expect(document.source()).toBe(source)

    await page.keyboard.type('Hello')
    await editor.expectParagraphCaret(0, 5)
    // On the line after the title block, or with a blank line between.
    await expect.poll(() => document.source()).toMatch(/^---\ntitle: Notes\nsubtitle: Rough\n---\n\n?Hello\n$/)
  })

  test('a body started and left empty goes away', async ({ page }) => {
    const document = await createDocument(source)
    const editor = await document.open(page)
    await editor.placeCaret('subtitle', 5)
    await page.keyboard.press('Enter')
    expect(await editor.blocks()).toEqual(['p: '])

    await page.keyboard.press('ArrowUp')

    expect((await editor.caret()).field).toBe('subtitle')
    expect(await editor.blocks()).toEqual([])
    expect(document.source()).toBe(source)
  })

  test('Backspace in a body just started goes back to the title block, and the body goes away', async ({ page }) => {
    const document = await createDocument(source)
    const editor = await document.open(page)
    await editor.placeCaret('subtitle', 5)
    await page.keyboard.press('Enter')
    expect(await editor.blocks()).toEqual(['p: '])

    await page.keyboard.press('Backspace')

    await editor.expectCaret('subtitle', 5)
    expect(await editor.blocks()).toEqual([])
    expect(document.source()).toBe(source)
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

test.describe('with collaborators, when the paragraph my caret is in goes', () => {
  test('someone deletes it, and my caret goes to the end of the paragraph before', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 6)

    editor.document.replace('Second paragraph.\n\n', '')

    await expect.poll(() => editor.paragraphs()).toEqual(['First paragraph.', 'Last paragraph.'])
    await editor.expectParagraphCaret(0, 16)
  })

  test('someone gives it formatting, and my caret goes to the end of the paragraph before', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 6)

    editor.document.replace('Second paragraph.', 'Second **bold** paragraph.')

    await expect.poll(() => editor.paragraphs()).toEqual(['First paragraph.', 'Last paragraph.'])
    await editor.expectParagraphCaret(0, 16)
    await page.keyboard.type('!')
    await editor.document.expectSource(
      body.replace('First paragraph.', 'First paragraph.!').replace('Second paragraph.', 'Second **bold** paragraph.'),
    )
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

  test('with Shift, the same keys select to there', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 3)

    await page.keyboard.press(`Shift+${mod}+ArrowDown`)
    const toEnd = await editor.selectedText()
    expect(toEnd.startsWith('ond paragraph.')).toBe(true)
    expect(toEnd.endsWith('Last paragraph.')).toBe(true)

    // From where the selection started, not from where it ended.
    await page.keyboard.press(`Shift+${mod}+ArrowUp`)
    const toStart = await editor.selectedText()
    expect(toStart.startsWith('Quarterly report')).toBe(true)
    expect(toStart.endsWith('Sec')).toBe(true)
  })

  test('the arrow keys stop on each empty paragraph', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    editor.document.replace('First paragraph.', 'First paragraph.\n\n&nbsp;\n\n&nbsp;')
    await expect.poll(async () => (await editor.paragraphs()).slice(0, 3)).toEqual(['First paragraph.', '', ''])
    await editor.placeCaretInParagraph(3, 0)

    await page.keyboard.press('ArrowUp')
    await editor.expectParagraphCaret(2, 0)
    await page.keyboard.press('ArrowUp')
    await editor.expectParagraphCaret(1, 0)
    await page.keyboard.press('ArrowUp')
    await editor.expectParagraphCaret(0, 0)

    await page.keyboard.press('ArrowDown')
    await editor.expectParagraphCaret(1, 0)
    await page.keyboard.press('ArrowDown')
    await editor.expectParagraphCaret(2, 0)
    await page.keyboard.press('ArrowRight')
    await editor.expectParagraphCaret(3, 0)
    await page.keyboard.press('ArrowLeft')
    await editor.expectParagraphCaret(2, 0)
    await page.keyboard.press('ArrowLeft')
    await editor.expectParagraphCaret(1, 0)
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

test.describe('the mouse in the body', () => {
  test('a double click in a paragraph selects a word', async ({ page }) => {
    const editor = await openEditor(page, 'body')

    await editor.paragraph(0).click({ clickCount: 2, position: { x: 10, y: 8 } })

    expect(await editor.selectedText()).toBe('First')
  })

  test('a triple click selects the paragraph, and typing replaces it and nothing else', async ({ page, browserName }) => {
    test.fixme(
      browserName !== 'firefox',
      'The selection takes in the break after the paragraph, and typing joins the next paragraph to it',
    )
    const editor = await openEditor(page, 'body')

    await editor.paragraph(0).click({ clickCount: 3, position: { x: 10, y: 8 } })
    expect((await editor.selectedText()).trim()).toBe('First paragraph.')
    await page.keyboard.type('Z')

    expect((await editor.paragraphs()).slice(0, 2)).toEqual(['Z', 'Second paragraph.'])
    await editor.document.expectSource(body.replace('First paragraph.', 'Z'))
  })

  test('typing after a click below the document adds to its last paragraph', async ({ page, browserName }) => {
    test.fixme(browserName === 'firefox', 'What is typed after the click goes nowhere')
    const editor = await openEditor(page, 'body')
    await editor.placeCaret('title', 4)

    await editor.clickAway()
    await page.keyboard.type('!')

    await expect.poll(async () => (await editor.paragraphs()).at(-1)).toBe('Last paragraph.!')
    expect(await editor.text('title')).toBe('Quarterly report')
    await editor.document.expectSource(body.replace('Last paragraph.', 'Last paragraph.!'))
  })
})

test.describe('scrolling', () => {
  const long = `---\ntitle: Notes\n---\n\n${Array.from({ length: 80 }, (_, i) => `Paragraph ${i}.`).join('\n\n')}\n`

  /** Whether the caret is inside the visual editor's window. */
  const caretInView = (editor: Editor) =>
    editor.frame.evaluate(() => {
      const selection = document.getSelection()!
      if (selection.rangeCount === 0) return false
      const range = selection.getRangeAt(0)
      const rect = range.getClientRects()[0] ?? (range.startContainer as Element).getBoundingClientRect?.()
      return !!rect && rect.top >= 0 && rect.bottom <= window.innerHeight
    })

  test('a document opens at its start', async ({ page }) => {
    const editor = await (await createDocument(long)).open(page)

    expect(await editor.frame.evaluate(() => window.scrollY)).toBe(0)
    expect(await caretInView(editor)).toBe(true)
  })

  // Today the page stays where it was, with the caret out of view.
  test.fixme('a key that moves the caret out of view scrolls to it', async ({ page }) => {
    const editor = await (await createDocument(long)).open(page)

    await page.keyboard.press(`${mod}+ArrowDown`)

    await expect.poll(() => caretInView(editor)).toBe(true)
  })

  test('typing with the caret out of view scrolls back to it', async ({ page, browserName }) => {
    test.fixme(browserName !== 'webkit', 'The page stays where it was')
    const editor = await (await createDocument(long)).open(page)
    await editor.placeCaretInParagraph(79, 0)
    await editor.frame.evaluate(() => window.scrollTo(0, 0))

    await page.keyboard.type('x')

    await expect.poll(() => caretInView(editor)).toBe(true)
  })
})

test.describe('characters that look like spaces', () => {
  // Today it's written, and shown, as a plain space.
  test.fixme('a non-breaking space typed in a paragraph stays one', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 5)

    await page.keyboard.insertText(' x')
    await page.keyboard.press('ArrowDown')

    await expect.poll(async () => (await editor.paragraphs())[0]).toBe('First x paragraph.')
    await editor.document.expectSource(body.replace('First paragraph.', 'First x paragraph.'))
  })
})
