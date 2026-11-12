/**
 * Headings of plain text in the body are edited in place, like
 * paragraphs. A heading is made by typing `#`s and a space at the start of
 * a paragraph, as in qmd, and Backspace at the start of its text makes it
 * a paragraph again. A heading with formatting can't be edited yet.
 */

import { expect, fixture, openEditor, test } from './helpers.ts'

const mod = 'ControlOrMeta'
const headings = fixture('headings')

test.describe('showing headings', () => {
  test('a heading of plain text is an editable line of its level', async ({ page }) => {
    const editor = await openEditor(page, 'headings')

    expect(await editor.lines()).toEqual(['p: Intro.', 'h2: Results', 'p: Middle.', 'h3: Details', 'p: End.'])
    expect(await editor.blocks()).toContain('h2: Formatted heading')
  })

  test('a heading shows the classes written on it', async ({ page }) => {
    const editor = await openEditor(page, 'headings')

    await expect(editor.frame.locator('main > h2.qe-field')).toHaveClass(/\bbig\b/)
  })
})

test.describe('editing a heading', () => {
  test('typing changes only the heading\'s text, keeping its attributes', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(1, 7)

    await page.keyboard.type(' so far')

    await editor.document.expectSource(headings.replace('## Results {#res', '## Results so far {#res'))
    expect((await editor.lines())[1]).toBe('h2: Results so far')
  })

  test('typing in a heading without attributes writes no id', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(3, 7)

    await page.keyboard.type('!')

    await editor.document.expectSource(headings.replace('### Details', '### Details!'))
  })

  test('markdown characters are typed as text', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(3, 7)

    await page.keyboard.type(' *now*')

    await editor.document.expectSource(headings.replace('### Details', '### Details \\*now\\*'))
    expect((await editor.lines())[3]).toBe('h3: Details *now*')
  })

  test('the caret can\'t go before the heading\'s text', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(1, 0)

    await page.keyboard.press('ArrowLeft')
    await page.keyboard.press('ArrowRight')
    await page.keyboard.type('Our ')

    await editor.document.expectSource(headings.replace('## Results', '## Our Results'))
  })
})

test.describe('Enter in a heading', () => {
  test('at the end starts a paragraph', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(3, 7)

    await page.keyboard.press('Enter')
    await page.keyboard.type('New.')

    await editor.document.expectSource(headings.replace('### Details\n', '### Details\n\nNew.\n'))
    expect((await editor.lines()).slice(3, 5)).toEqual(['h3: Details', 'p: New.'])
  })

  test('in the middle splits it into two headings', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(3, 3)

    await page.keyboard.press('Enter')

    await editor.document.expectSource(headings.replace('### Details', '### Det\n\n### ails'))
    await editor.expectLineCaret(4, 0)
  })

  test('at the start adds an empty paragraph above, and the caret stays on the heading', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(3, 0)

    await page.keyboard.press('Enter')
    await page.keyboard.type('X')

    expect((await editor.lines()).slice(3, 5)).toEqual(['p: ', 'h3: XDetails'])
    // An empty paragraph is a non-breaking space on a line of its own.
    await editor.document.expectSource(headings.replace('### Details', '\u00a0\n\n### XDetails'))
  })
})

test.describe('Backspace and Delete', () => {
  test('Backspace at the start of a heading makes it a paragraph', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(3, 0)

    await page.keyboard.press('Backspace')

    await editor.document.expectSource(headings.replace('### Details', 'Details'))
    expect((await editor.lines())[3]).toBe('p: Details')
    await editor.expectLineCaret(3, 0)
  })

  test('Backspace again joins it to the paragraph before', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(3, 0)

    await page.keyboard.press('Backspace')
    await page.keyboard.press('Backspace')

    await editor.document.expectSource(headings.replace('Middle.\n\n### Details', 'Middle.Details'))
  })

  test('Delete at the end of a paragraph joins the heading after into it, as text', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(2, 7)

    await page.keyboard.press('Delete')

    await editor.document.expectSource(headings.replace('Middle.\n\n### Details', 'Middle.Details'))
    expect((await editor.lines())[2]).toBe('p: Middle.Details')
  })

  test('emptying a heading keeps it a heading', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(3, 7)

    for (let i = 0; i < 7; i++) await page.keyboard.press('Backspace')
    await page.keyboard.type('Notes')

    await editor.document.expectSource(headings.replace('### Details', '### Notes'))
  })

  test('undo turns a paragraph made by Backspace back into the heading', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(3, 0)
    await page.keyboard.press('Backspace')
    await editor.document.expectSource(headings.replace('### Details', 'Details'))

    await page.keyboard.press(`${mod}+z`)

    await editor.document.expectSource(headings)
  })
})

test.describe('making a heading', () => {
  test('# and a space at the start of a paragraph make it a heading', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(2, 0)

    await page.keyboard.type('#### ')

    expect((await editor.lines())[2]).toBe('h4: Middle.')
    await editor.document.expectSource(headings.replace('Middle.', '#### Middle.'))
    await editor.expectLineCaret(2, 0)
  })

  test('in a new paragraph', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(4, 4)

    await page.keyboard.press('Enter')
    await page.keyboard.type('# Conclusion')

    await editor.document.expectSource(headings.replace('End.\n', 'End.\n\n# Conclusion\n'))
    expect((await editor.lines())[5]).toBe('h1: Conclusion')
  })

  test('undo gives back the typed #s', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(2, 0)
    await page.keyboard.type('## ')
    expect((await editor.lines())[2]).toBe('h2: Middle.')

    await page.keyboard.press(`${mod}+z`)

    expect((await editor.lines())[2]).toBe('p: ## Middle.')
    await editor.expectLineCaret(2, 3)
  })

  test('seven #s, or #s that aren\'t at the start, are text', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(2, 0)
    await page.keyboard.type('####### ')
    await editor.placeCaretInLine(4, 4)
    await page.keyboard.type(' # ')

    expect((await editor.lines())[2]).toBe('p: ####### Middle.')
    // (The space typed last shows until the caret leaves the line.)
    expect((await editor.lines())[4]).toBe('p: End. # ')
  })

  test('#s and a space in a heading are text', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(3, 0)

    await page.keyboard.type('# ')

    expect((await editor.lines())[3]).toBe('h3: # Details')
  })

  test('#s in the title are text', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaret('title', 0)

    await page.keyboard.type('# ')

    await editor.expectText('title', '# Quarterly report')
  })
})

test.describe('the clipboard', () => {
  test('copying a heading puts its #s on the clipboard', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(2, 0)
    for (let i = 0; i < 'Middle.'.length + 1 + 'Details'.length; i++) await page.keyboard.press('Shift+ArrowRight')

    expect(await editor.copy()).toBe('Middle.\n### Details')
  })

  test('pasted #s are text', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(4, 4)

    await editor.paste('\n## Pasted')

    expect((await editor.lines()).slice(4)).toEqual(['p: End.', 'p: ## Pasted'])
  })
})

test.describe('with collaborators', () => {
  test('a paragraph made a heading in the source becomes one, and the caret stays on its text', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(2, 3)

    editor.document.replace('Middle.', '## Middle.')

    await expect.poll(async () => (await editor.lines())[2]).toBe('h2: Middle.')
    await editor.expectLineCaret(2, 3)
    await page.keyboard.type('x')
    await editor.document.expectSource(headings.replace('Middle.', '## Midxdle.'))
  })

  test('a heading made a paragraph in the source becomes one, and the caret stays on its text', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(3, 3)

    editor.document.replace('### Details', 'Details')

    await expect.poll(async () => (await editor.lines())[3]).toBe('p: Details')
    await editor.expectLineCaret(3, 3)
  })
})
