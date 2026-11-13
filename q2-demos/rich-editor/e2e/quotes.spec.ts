/**
 * A block quote of one paragraph of plain text is edited in place, like a
 * paragraph. A quote is made by typing `>` and a space at the start of a
 * paragraph, as in qmd, and Backspace at the start of its text makes it a
 * paragraph again. A quote of anything else (formatting, more than one
 * paragraph) can't be edited yet.
 */

import { expect, fixture, openEditor, test } from './helpers.ts'

const mod = 'ControlOrMeta'
const quotes = fixture('quotes')

test.describe('showing quotes', () => {
  test('a quote of one paragraph of plain text is an editable line', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')

    expect(await editor.lines()).toEqual([
      'p: Intro.',
      'blockquote: A quote.',
      'p: Middle.',
      'blockquote: Wrapped quote over two lines.',
      'p: End.',
    ])
  })

  test('it is a paragraph in a block quote, as `q2 render` shows it', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')

    await expect(editor.frame.locator('main > blockquote:not([class]) > p.qe-field').first()).toHaveText('A quote.')
  })

  test('a quote with formatting, or of more than one paragraph, is shown and can\'t be edited', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')

    const readOnly = editor.frame.locator('main > blockquote:not(:has(.qe-field))')
    await expect(readOnly).toHaveCount(2)
    await expect(readOnly.nth(0).locator('em')).toHaveText('Formatted')
    await expect(readOnly.nth(1).locator('p')).toHaveText(['One.', 'Two.'])
  })
})

test.describe('editing a quote', () => {
  test('typing changes the quote\'s text', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(1, 7)

    await page.keyboard.type(' here')

    await editor.document.expectSource(quotes.replace('> A quote.', '> A quote here.'))
    expect((await editor.lines())[1]).toBe('blockquote: A quote here.')
  })

  test('typing in a quote the source wraps keeps its line break', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(3, 13)

    await page.keyboard.type(',')

    await editor.document.expectSource(quotes.replace('> Wrapped quote\n', '> Wrapped quote,\n'))
  })

  test('markdown characters are typed as text', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(1, 8)

    await page.keyboard.type(' *now*')

    await editor.document.expectSource(quotes.replace('> A quote.', '> A quote. \\*now\\*'))
    expect((await editor.lines())[1]).toBe('blockquote: A quote. *now*')
  })

  test('the caret can\'t go before the quote\'s text', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(1, 0)

    await page.keyboard.press('ArrowLeft')
    await page.keyboard.press('ArrowRight')
    await page.keyboard.type('Just ')

    await editor.document.expectSource(quotes.replace('> A quote.', '> Just A quote.'))
  })

  test('editing a paragraph leaves the quotes that can\'t be edited as they were', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(4, 4)

    await page.keyboard.type('!')

    await editor.document.expectSource(quotes.replace('End.', 'End.!'))
  })
})

test.describe('Enter in a quote', () => {
  test('at the end starts a paragraph', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(1, 8)

    await page.keyboard.press('Enter')
    await page.keyboard.type('New.')

    await editor.document.expectSource(quotes.replace('> A quote.\n', '> A quote.\n\nNew.\n'))
    expect((await editor.lines()).slice(1, 3)).toEqual(['blockquote: A quote.', 'p: New.'])
  })

  test('in the middle splits it into two quotes', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(1, 5)

    await page.keyboard.press('Enter')

    expect((await editor.lines()).slice(1, 3)).toEqual(['blockquote: A quo', 'blockquote: te.'])
    await editor.document.expectSource(quotes.replace('> A quote.', '> A quo\n\n> te.'))
    await editor.expectLineCaret(2, 0)
  })

  test('at the start adds an empty paragraph above, and the caret stays on the quote', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(1, 0)

    await page.keyboard.press('Enter')
    await page.keyboard.type('X')

    expect((await editor.lines()).slice(1, 3)).toEqual(['p: ', 'blockquote: XA quote.'])
    await editor.document.expectSource(quotes.replace('> A quote.', ' \n\n> XA quote.'))
  })
})

test.describe('Backspace in a quote', () => {
  test('at the start of the quote makes it a paragraph', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(1, 0)

    await page.keyboard.press('Backspace')

    await editor.document.expectSource(quotes.replace('> A quote.', 'A quote.'))
    expect((await editor.lines())[1]).toBe('p: A quote.')
    await editor.expectLineCaret(1, 0)
  })

  test('again joins it to the paragraph before', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(1, 0)

    await page.keyboard.press('Backspace')
    await page.keyboard.press('Backspace')

    await editor.document.expectSource(quotes.replace('Intro.\n\n> A quote.', 'Intro.A quote.'))
  })

  test('undo turns a paragraph made by Backspace back into the quote', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(1, 0)
    await page.keyboard.press('Backspace')
    await editor.document.expectSource(quotes.replace('> A quote.', 'A quote.'))

    await page.keyboard.press(`${mod}+z`)

    await editor.document.expectSource(quotes)
  })

  test('emptying a quote keeps it a quote', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(1, 8)

    for (let i = 0; i < 8; i++) await page.keyboard.press('Backspace')
    await page.keyboard.type('Notes')

    await editor.document.expectSource(quotes.replace('> A quote.', '> Notes'))
  })
})

test.describe('making a quote', () => {
  test('> and a space at the start of a paragraph make it a quote', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(2, 0)

    await page.keyboard.type('> ')

    expect((await editor.lines())[2]).toBe('blockquote: Middle.')
    await editor.document.expectSource(quotes.replace('Middle.', '> Middle.'))
    await editor.expectLineCaret(2, 0)
  })

  test('in a new paragraph', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(4, 4)

    await page.keyboard.press('Enter')
    await page.keyboard.type('> Last word')

    await editor.document.expectSource(quotes.replace('End.\n', 'End.\n\n> Last word\n'))
    expect((await editor.lines())[5]).toBe('blockquote: Last word')
  })

  test('undo gives back the typed >', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(2, 0)
    await page.keyboard.type('> ')
    expect((await editor.lines())[2]).toBe('blockquote: Middle.')

    await page.keyboard.press(`${mod}+z`)

    expect((await editor.lines())[2]).toBe('p: > Middle.')
    await editor.expectLineCaret(2, 2)
    await editor.document.expectSource(quotes.replace('Middle.', '\\> Middle.'))
  })

  test('Backspace right after gives back the typed >, as text', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(2, 0)
    await page.keyboard.type('> ')
    expect((await editor.lines())[2]).toBe('blockquote: Middle.')

    await page.keyboard.press('Backspace')

    expect((await editor.lines())[2]).toBe('p: > Middle.')
    await editor.expectLineCaret(2, 2)
    await editor.document.expectSource(quotes.replace('Middle.', '\\> Middle.'))
  })

  test('> that isn\'t at the start is text', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(4, 4)

    await page.keyboard.type(' > x')

    expect((await editor.lines())[4]).toBe('p: End. > x')
  })

  test('> and a space in a quote, or in a heading, are text', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(1, 0)
    await page.keyboard.type('> ')
    await editor.placeCaretInLine(2, 0)
    await page.keyboard.type('## > ')

    expect((await editor.lines()).slice(1, 3)).toEqual(['blockquote: > A quote.', 'h2: > Middle.'])
  })

  test('#s and a space in a quote are text', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(1, 0)

    await page.keyboard.type('# ')

    expect((await editor.lines())[1]).toBe('blockquote: # A quote.')
  })

  test('> and a space in the title are text', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaret('title', 0)

    await page.keyboard.type('> ')

    await editor.expectText('title', '> Field notes')
  })
})

test.describe('an empty quote', () => {
  test('an emptied quote stays when the caret leaves it, and is written as a non-breaking space', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(1, 8)
    for (let i = 0; i < 8; i++) await page.keyboard.press('Backspace')

    await editor.placeCaretInLine(0, 0)

    // (qmd has no empty quote: `>` alone is one with no paragraph in it.)
    await editor.document.expectSource(quotes.replace('> A quote.', '>  '))
    expect((await editor.lines())[1]).toBe('blockquote: ')
  })

  test('a quote of a non-breaking space in the source is an empty quote the caret stops on', async ({ page }) => {
    const editor = await openEditor(page, 'empty-quote')
    expect(await editor.lines()).toEqual(['p: Intro.', 'blockquote: ', 'p: End.'])
    await editor.placeCaretInLine(0, 6)

    await page.keyboard.press('ArrowRight')
    await page.keyboard.type('Quote')

    expect((await editor.lines())[1]).toBe('blockquote: Quote')
    await editor.document.expectSource(fixture('empty-quote').replace('> &nbsp;', '> Quote'))
  })
})

test.describe('joining lines around a quote', () => {
  test('Backspace at the start of a paragraph after a quote joins it into the quote', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(2, 0)

    await page.keyboard.press('Backspace')

    expect((await editor.lines())[1]).toBe('blockquote: A quote.Middle.')
    await editor.document.expectSource(quotes.replace('> A quote.\n\nMiddle.', '> A quote.Middle.'))
    await editor.expectLineCaret(1, 8)
  })

  test('Delete at the end of a paragraph joins the quote after into it, as text', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(0, 6)

    await page.keyboard.press('Delete')

    expect((await editor.lines())[0]).toBe('p: Intro.A quote.')
    await editor.document.expectSource(quotes.replace('Intro.\n\n> A quote.', 'Intro.A quote.'))
  })
})

test.describe('copying and pasting quotes', () => {
  test('copying a quote puts its > on the clipboard', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(0, 0)
    for (let i = 0; i < 'Intro.'.length + 1 + 'A quote.'.length; i++) await page.keyboard.press('Shift+ArrowRight')

    expect(await editor.copy()).toBe('Intro.\n> A quote.')
  })

  test('a pasted > is text', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(4, 4)

    await editor.paste('\n> Pasted')

    expect((await editor.lines()).slice(4)).toEqual(['p: End.', 'p: > Pasted'])
  })
})

test.describe('quotes and collaborators', () => {
  test('a paragraph made a quote in the source becomes one, and the caret stays on its text', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(2, 3)

    editor.document.replace('Middle.', '> Middle.')

    await expect.poll(async () => (await editor.lines())[2]).toBe('blockquote: Middle.')
    await editor.expectLineCaret(2, 3)
    await page.keyboard.type('x')
    await editor.document.expectSource(quotes.replace('Middle.', '> Midxdle.'))
  })

  test('a quote made a paragraph in the source becomes one, and the caret stays on its text', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    await editor.placeCaretInLine(1, 3)

    editor.document.replace('> A quote.', 'A quote.')

    await expect.poll(async () => (await editor.lines())[1]).toBe('p: A quote.')
    await editor.expectLineCaret(1, 3)
  })
})

test.describe('moving through quotes', () => {
  test('Up and Down move onto and off quotes', async ({ page }) => {
    const editor = await openEditor(page, 'quotes')
    // (A quote's text is indented, so the caret starts past the indent.)
    await editor.placeCaretInLine(2, 6)

    await page.keyboard.press('ArrowUp')
    await page.keyboard.type('x')
    await expect.poll(async () => (await editor.lines())[1]).toMatch(/^blockquote: A.+x/)

    await page.keyboard.press('ArrowDown')
    await page.keyboard.press('ArrowDown')
    await page.keyboard.type('y')
    await expect.poll(async () => (await editor.lines())[3]).toMatch(/^blockquote: W.+y/)
    expect((await editor.lines())[2]).toBe('p: Middle.')
  })
})
