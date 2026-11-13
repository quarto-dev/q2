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

  test('a heading has its id and the classes written on it', async ({ page }) => {
    const editor = await openEditor(page, 'headings')

    await expect(editor.frame.locator('main > h2.qe-field')).toHaveClass(/\bbig\b/)
    await expect(editor.frame.locator('main > h2.qe-field')).toHaveId('res')
    // An id the source doesn't write is the one `q2 render` makes from the text.
    await expect(editor.frame.locator('main > h3.qe-field')).toHaveId('details')
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
    await editor.document.expectSource(headings.replace('Middle.', '\\#\\# Middle.'))
  })

  test('Backspace right after gives back the typed #s, as text', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(2, 0)
    await page.keyboard.type('## ')
    expect((await editor.lines())[2]).toBe('h2: Middle.')

    await page.keyboard.press('Backspace')

    expect((await editor.lines())[2]).toBe('p: ## Middle.')
    await editor.expectLineCaret(2, 3)
    await editor.document.expectSource(headings.replace('Middle.', '\\#\\# Middle.'))
  })

  test('Backspace after the caret has moved makes the heading a paragraph as usual', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(2, 0)
    await page.keyboard.type('## ')
    await page.keyboard.press('ArrowRight')
    await page.keyboard.press('ArrowLeft')

    await page.keyboard.press('Backspace')

    expect((await editor.lines())[2]).toBe('p: Middle.')
    await editor.document.expectSource(headings)
  })

  test('Backspace after typing in the new heading deletes what was typed', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(4, 4)
    await page.keyboard.press('Enter')
    await page.keyboard.type('## X')

    await page.keyboard.press('Backspace')

    expect((await editor.lines())[5]).toBe('h2: ')
    await page.keyboard.type('Y')
    await editor.document.expectSource(headings.replace('End.\n', 'End.\n\n## Y\n'))
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

test.describe('joining lines around a heading', () => {
  test('Backspace at the start of a paragraph after a heading joins it into the heading', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(2, 0)

    await page.keyboard.press('Backspace')

    expect((await editor.lines())[1]).toBe('h2: ResultsMiddle.')
    await editor.document.expectSource(headings.replace('## Results {#res .big}\n\nMiddle.', '## ResultsMiddle. {#res .big}'))
    await editor.expectLineCaret(1, 7)
  })

  test('Delete on an empty paragraph before a heading leaves the heading a heading', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(2, 7)
    await page.keyboard.press('Enter')
    expect((await editor.lines()).slice(2, 5)).toEqual(['p: Middle.', 'p: ', 'h3: Details'])

    await page.keyboard.press('Delete')

    expect((await editor.lines()).slice(2, 4)).toEqual(['p: Middle.', 'h3: Details'])
    await editor.document.expectSource(headings)
    await editor.expectLineCaret(3, 0)
  })

  test('deleting a selection from a heading into a paragraph keeps the heading', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(1, 3)
    // "ults", the line break, and "Mid".
    for (let i = 0; i < 8; i++) await page.keyboard.press('Shift+ArrowRight')

    await page.keyboard.press('Backspace')

    expect((await editor.lines()).slice(1, 3)).toEqual(['h2: Resdle.', 'h3: Details'])
    await editor.document.expectSource(headings.replace('## Results {#res .big}\n\nMiddle.', '## Resdle. {#res .big}'))
  })

  test('deleting a selection from a paragraph into a heading keeps the paragraph', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(2, 3)
    // "dle.", the line break, and "Det".
    for (let i = 0; i < 8; i++) await page.keyboard.press('Shift+ArrowRight')

    await page.keyboard.press('Backspace')

    expect((await editor.lines()).slice(2, 4)).toEqual(['p: Midails', 'p: End.'])
    await editor.document.expectSource(headings.replace('Middle.\n\n### Details', 'Midails'))
  })
})

test.describe('a heading\'s attributes', () => {
  test('a heading made a paragraph loses its attributes', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(1, 0)

    await page.keyboard.press('Backspace')

    expect((await editor.lines())[1]).toBe('p: Results')
    await editor.document.expectSource(headings.replace('## Results {#res .big}', 'Results'))
  })

  test('splitting a heading leaves its attributes on the first half', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(1, 3)

    await page.keyboard.press('Enter')

    await editor.document.expectSource(headings.replace('## Results {#res .big}', '## Res {#res .big}\n\n## ults'))
    await expect(editor.frame.locator('main > h2.qe-field').nth(0)).toHaveClass(/\bbig\b/)
    await expect(editor.frame.locator('main > h2.qe-field').nth(1)).not.toHaveClass(/\bbig\b/)
  })
})

test.describe('an empty heading', () => {
  test('an emptied heading stays when the caret leaves it, and is written as its #s', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(3, 7)
    for (let i = 0; i < 7; i++) await page.keyboard.press('Backspace')

    await editor.placeCaretInLine(0, 0)

    // (The qmd writer leaves a space after the #s.)
    await editor.document.expectSource(headings.replace('### Details', '### '))
    expect((await editor.lines())[3]).toBe('h3: ')
  })

  test('an empty heading in the source is an empty heading line the caret stops on', async ({ page }) => {
    const editor = await openEditor(page, 'empty-heading')
    expect(await editor.lines()).toEqual(['p: Intro.', 'h2: ', 'p: End.'])
    await editor.placeCaretInLine(0, 6)

    await page.keyboard.press('ArrowRight')
    await page.keyboard.type('Title')

    expect((await editor.lines())[1]).toBe('h2: Title')
    await editor.document.expectSource(fixture('empty-heading').replace('##', '## Title'))
  })

  test('Enter on an empty heading adds an empty paragraph above it', async ({ page }) => {
    const editor = await openEditor(page, 'empty-heading')
    await editor.placeCaretInLine(1, 0)

    await page.keyboard.press('Enter')

    expect(await editor.lines()).toEqual(['p: Intro.', 'p: ', 'h2: ', 'p: End.'])
    await editor.expectLineCaret(2, 0)
  })
})

test.describe('a body that starts with a heading', () => {
  test('Enter at the end of the title block goes to the heading\'s text', async ({ page }) => {
    const editor = await openEditor(page, 'heading-first')
    await editor.placeCaret('subtitle', 13)

    await page.keyboard.press('Enter')
    await page.keyboard.type('Key ')

    await editor.expectLineCaret(0, 4)
    await editor.document.expectSource(fixture('heading-first').replace('## Results', '## Key Results'))
  })

  test('Enter at the start of the heading adds a paragraph above, which Up goes to', async ({ page }) => {
    const editor = await openEditor(page, 'heading-first')
    await editor.placeCaretInLine(0, 0)

    await page.keyboard.press('Enter')
    await page.keyboard.press('ArrowUp')
    await page.keyboard.type('Intro.')

    expect(await editor.lines()).toEqual(['p: Intro.', 'h2: Results', 'p: Text.'])
    await editor.document.expectSource(fixture('heading-first').replace('## Results', 'Intro.\n\n## Results'))
  })

  test('Delete on an empty paragraph before the heading leaves the heading a heading', async ({ page }) => {
    const editor = await openEditor(page, 'heading-first')
    await editor.placeCaretInLine(0, 0)
    await page.keyboard.press('Enter')
    await page.keyboard.press('ArrowUp')

    await page.keyboard.press('Delete')

    expect(await editor.lines()).toEqual(['h2: Results', 'p: Text.'])
    await editor.document.expectSource(fixture('heading-first'))
    await editor.expectLineCaret(0, 0)
  })

  test('typing over the whole body selected keeps the first line a heading', async ({ page }) => {
    const editor = await openEditor(page, 'heading-first')
    await editor.placeCaretInLine(1, 2)

    await page.keyboard.press(`${mod}+a`)
    await page.keyboard.type('Summary')

    expect(await editor.lines()).toEqual(['h2: Summary'])
    await editor.document.expectSource(fixture('heading-first').replace('## Results\n\nText.', '## Summary'))
  })
})

test.describe('where #s make a heading', () => {
  test('#s and a space in the abstract are text', async ({ page }) => {
    const editor = await openEditor(page, 'abstract')
    await editor.placeCaret('abstract', 0)

    await page.keyboard.type('# ')
    // Read back from the source once the caret has left.
    await editor.placeCaret('title', 0)

    await expect.poll(() => editor.text('abstract')).toMatch(/^# We summarise/)
    await expect.poll(() => editor.document.source()).toContain('\\# We summarise')
  })
})

test.describe('copying from a heading', () => {
  test('a selection from the start of a heading\'s text copies its #s', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(1, 0)
    for (let i = 0; i < 3; i++) await page.keyboard.press('Shift+ArrowRight')

    expect(await editor.copy()).toBe('## Res')
  })

  test('a selection inside a heading\'s text copies only the text', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(1, 1)
    for (let i = 0; i < 3; i++) await page.keyboard.press('Shift+ArrowRight')

    expect(await editor.copy()).toBe('esu')
  })
})

test.describe('headings and collaborators', () => {
  test('a heading\'s level changed in the source changes it here, and the caret stays on its text', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(1, 3)

    editor.document.replace('## Results', '### Results')

    await expect.poll(async () => (await editor.lines())[1]).toBe('h3: Results')
    await expect(editor.frame.locator('main > h3.qe-field').first()).toHaveClass(/\bbig\b/)
    await editor.expectLineCaret(1, 3)
  })

  test('a class added to a heading in the source shows here', async ({ page }) => {
    const editor = await openEditor(page, 'headings')

    editor.document.replace('### Details', '### Details {.extra}')

    await expect(editor.frame.locator('main > h3.qe-field')).toHaveClass(/\bextra\b/)
    expect((await editor.lines())[3]).toBe('h3: Details')
  })

  test('someone edits the heading I am typing in, and both edits stay', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(1, 7)
    await page.keyboard.type(' now')
    await editor.document.expectSource(headings.replace('## Results', '## Results now'))

    editor.document.replace('## Results', '## Key Results')
    await expect.poll(async () => (await editor.lines())[1]).toBe('h2: Key Results now')
    await page.keyboard.type('!')

    await editor.document.expectSource(headings.replace('## Results', '## Key Results now!'))
  })
})

test.describe('moving through headings', () => {
  test('Up and Down move onto and off headings', async ({ page }) => {
    const editor = await openEditor(page, 'headings')
    await editor.placeCaretInLine(2, 3)

    await page.keyboard.press('ArrowUp')
    await page.keyboard.type('x')
    await expect.poll(async () => (await editor.lines())[1]).toMatch(/^h2: R\w*x/)

    await page.keyboard.press('ArrowDown')
    await page.keyboard.press('ArrowDown')
    await page.keyboard.type('y')
    await expect.poll(async () => (await editor.lines())[3]).toMatch(/^h3: D\w*y/)
    expect((await editor.lines())[2]).toBe('p: Middle.')
  })
})
