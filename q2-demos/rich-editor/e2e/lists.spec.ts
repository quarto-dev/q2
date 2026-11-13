/**
 * A bullet list whose items are each one line of plain text (a tight
 * list) is edited in place: each item is a line of its own, and the lines
 * of items next to each other are one list. An item is made by typing
 * `-`, `*` or `+` and a space at the start of a paragraph, as in qmd, and
 * Backspace at the start of its text makes it a paragraph again. Enter at
 * the end of an item starts another, and Enter on an empty item ends the
 * list. A list of anything else (formatting, items of more than one
 * paragraph, blank lines between items, lists in items) can't be edited
 * yet.
 */

import { createDocument, expect, fixture, openEditor, test } from './helpers.ts'

const mod = 'ControlOrMeta'
const lists = fixture('lists')

test.describe('showing lists', () => {
  test('a list of plain items is a line per item', async ({ page }) => {
    const editor = await openEditor(page, 'lists')

    expect(await editor.lines()).toEqual([
      'p: Intro.',
      'li: One.',
      'li: Two, wrapped over two lines.',
      'li: Three.',
      'p: Middle.',
      'p: End.',
    ])
  })

  test('its items are one list, with no class, as `q2 render` shows it', async ({ page }) => {
    const editor = await openEditor(page, 'lists')

    const list = editor.frame.locator('main > ul:has(> li.qe-field)')
    await expect(list).toHaveCount(1)
    await expect(list).not.toHaveAttribute('class')
    await expect(list.locator('> li')).toHaveText(['One.', 'Two, wrapped over two lines.', 'Three.'])
  })

  test('a list with formatting, or with blank lines between its items, is shown and can\'t be edited', async ({
    page,
  }) => {
    const editor = await openEditor(page, 'lists')

    const readOnly = editor.frame.locator('main > ul:not(:has(.qe-field))')
    await expect(readOnly).toHaveCount(2)
    await expect(readOnly.nth(0).locator('em')).toHaveText('formatted')
    await expect(readOnly.nth(1).locator('li > p')).toHaveText(['A loose item.', 'And another.'])
  })

  test('an empty item in the source is an empty item the caret stops on', async ({ page }) => {
    const source = '---\ntitle: Notes\n---\n\nIntro.\n\n* One.\n*\n* Three.\n'
    const document = await createDocument(source)
    const editor = await document.open(page)
    expect(await editor.lines()).toEqual(['p: Intro.', 'li: One.', 'li: ', 'li: Three.'])
    await editor.placeCaretInLine(1, 4)

    await page.keyboard.press('ArrowRight')
    await page.keyboard.type('Two.')

    await document.expectSource(source.replace('*\n', '* Two.\n'))
  })
})

test.describe('editing an item', () => {
  test('typing changes the item\'s text', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(1, 4)

    await page.keyboard.type(' more')

    await editor.document.expectSource(lists.replace('* One.', '* One. more'))
    expect((await editor.lines())[1]).toBe('li: One. more')
  })

  test('typing in an item the source wraps keeps its line break', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(2, 12)

    await page.keyboard.type(',')

    await editor.document.expectSource(lists.replace('* Two, wrapped\n', '* Two, wrapped,\n'))
  })

  test('markdown characters are typed as text', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(1, 4)

    await page.keyboard.type(' *now*')

    await editor.document.expectSource(lists.replace('* One.', '* One. \\*now\\*'))
    expect((await editor.lines())[1]).toBe('li: One. *now*')
  })

  test('the caret can\'t go before the item\'s text', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(1, 0)

    await page.keyboard.press('ArrowLeft')
    await page.keyboard.press('ArrowRight')
    await page.keyboard.type('Just ')

    await editor.document.expectSource(lists.replace('* One.', '* Just One.'))
  })

  test('editing a paragraph leaves the lists that can\'t be edited as they were', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(5, 4)

    await page.keyboard.type('!')

    await editor.document.expectSource(lists.replace('End.', 'End.!'))
  })

  // The qmd writer writes a list again as a whole when any of it changes,
  // with `*` for every item.
  test.fixme('editing an item leaves the rest of the list as it is written', async ({ page }) => {
    const source = '---\ntitle: Notes\n---\n\n- One.\n- Two,\n  wrapped.\n'
    const document = await createDocument(source)
    const editor = await document.open(page)
    await editor.placeCaretInLine(0, 4)

    await page.keyboard.type('!')

    // Today the list is written `* One.!` and `* Two,`.
    await document.expectSource(source.replace('- One.', '- One.!'))
  })
})

test.describe('Enter in an item', () => {
  test('at the end starts an item', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(1, 4)

    await page.keyboard.press('Enter')
    await page.keyboard.type('New.')

    expect((await editor.lines()).slice(1, 4)).toEqual(['li: One.', 'li: New.', 'li: Two, wrapped over two lines.'])
    await editor.document.expectSource(lists.replace('* One.\n', '* One.\n* New.\n'))
  })

  test('in the middle splits it into two items', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(1, 2)

    await page.keyboard.press('Enter')

    expect((await editor.lines()).slice(1, 3)).toEqual(['li: On', 'li: e.'])
    await editor.document.expectSource(lists.replace('* One.', '* On\n* e.'))
    await editor.expectLineCaret(2, 0)
  })

  test('at the start adds an empty item above, and the caret stays on the item', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(1, 0)

    await page.keyboard.press('Enter')
    await page.keyboard.type('X')

    expect((await editor.lines()).slice(1, 3)).toEqual(['li: ', 'li: XOne.'])
    await editor.document.expectSource(lists.replace('* One.', '*\n* XOne.'))
  })

  test('on an empty item at the end of the list ends it, with a paragraph after it', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(3, 6)

    await page.keyboard.press('Enter')
    await page.keyboard.press('Enter')
    await page.keyboard.type('After.')

    expect((await editor.lines()).slice(3, 6)).toEqual(['li: Three.', 'p: After.', 'p: Middle.'])
    await editor.document.expectSource(lists.replace('* Three.\n', '* Three.\n\nAfter.\n'))
  })

  test('on an empty item in the middle of the list splits it around a paragraph', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(1, 4)

    await page.keyboard.press('Enter')
    await page.keyboard.press('Enter')
    await page.keyboard.type('Between.')

    expect((await editor.lines()).slice(1, 4)).toEqual(['li: One.', 'p: Between.', 'li: Two, wrapped over two lines.'])
    await editor.document.expectSource(lists.replace('* One.\n', '* One.\n\nBetween.\n\n'))
    await expect(editor.frame.locator('main > ul:has(> li.qe-field)')).toHaveCount(2)
  })
})

test.describe('Backspace in an item', () => {
  test('at the start of an item makes it a paragraph, between the items around it', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(2, 0)

    await page.keyboard.press('Backspace')

    expect((await editor.lines()).slice(1, 4)).toEqual(['li: One.', 'p: Two, wrapped over two lines.', 'li: Three.'])
    await editor.document.expectSource(
      lists.replace('* Two, wrapped\n  over two lines.\n', '\nTwo, wrapped\nover two lines.\n\n'),
    )
    await editor.expectLineCaret(2, 0)
  })

  test('at the start of the first item makes it a paragraph before the list', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(1, 0)

    await page.keyboard.press('Backspace')

    expect((await editor.lines()).slice(1, 3)).toEqual(['p: One.', 'li: Two, wrapped over two lines.'])
    await editor.document.expectSource(lists.replace('* One.\n', 'One.\n\n'))
  })

  test('again joins it to the item before', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(2, 0)

    await page.keyboard.press('Backspace')
    await page.keyboard.press('Backspace')

    expect((await editor.lines()).slice(1, 3)).toEqual(['li: One.Two, wrapped over two lines.', 'li: Three.'])
    await editor.document.expectSource(lists.replace('* One.\n* Two,', '* One.Two,'))
    await editor.expectLineCaret(1, 4)
  })

  test('undo turns a paragraph made by Backspace back into the item', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(1, 0)
    await page.keyboard.press('Backspace')
    await editor.document.expectSource(lists.replace('* One.\n', 'One.\n\n'))

    await page.keyboard.press(`${mod}+z`)

    await editor.document.expectSource(lists)
  })

  test('emptying an item keeps it an item, written as a bare *', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(1, 4)
    for (let i = 0; i < 4; i++) await page.keyboard.press('Backspace')

    await editor.placeCaretInLine(0, 0)

    expect((await editor.lines())[1]).toBe('li: ')
    await editor.document.expectSource(lists.replace('* One.', '*'))
  })
})

test.describe('joining lines around a list', () => {
  test('Backspace at the start of a paragraph after a list joins it into the last item', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(4, 0)

    await page.keyboard.press('Backspace')

    expect((await editor.lines())[3]).toBe('li: Three.Middle.')
    await editor.document.expectSource(lists.replace('* Three.\n\nMiddle.', '* Three.Middle.'))
    await editor.expectLineCaret(3, 6)
  })

  test('Delete at the end of a paragraph joins the first item after it into it, as text', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(0, 6)

    await page.keyboard.press('Delete')

    expect((await editor.lines()).slice(0, 2)).toEqual(['p: Intro.One.', 'li: Two, wrapped over two lines.'])
    await editor.document.expectSource(lists.replace('Intro.\n\n* One.\n', 'Intro.One.\n\n'))
  })
})

test.describe('making a list', () => {
  test('- and a space at the start of a paragraph make it an item of a list of its own', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(5, 0)

    await page.keyboard.type('- ')

    expect((await editor.lines())[5]).toBe('li: End.')
    await editor.document.expectSource(lists.replace('End.', '* End.'))
    await editor.expectLineCaret(5, 0)
  })

  test('so do * and +', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(5, 0)
    await page.keyboard.type('* ')
    await editor.placeCaretInLine(0, 0)
    await page.keyboard.type('+ ')

    expect((await editor.lines())[0]).toBe('li: Intro.')
    expect((await editor.lines())[5]).toBe('li: End.')
  })

  test('a paragraph made an item next to a list joins it', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(4, 0)

    await page.keyboard.type('- ')

    expect((await editor.lines())[4]).toBe('li: Middle.')
    await expect(editor.frame.locator('main > ul:has(> li.qe-field) > li')).toHaveCount(4)
    await editor.document.expectSource(lists.replace('* Three.\n\nMiddle.', '* Three.\n* Middle.'))
  })

  test('in a new paragraph', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(5, 4)

    await page.keyboard.press('Enter')
    await page.keyboard.type('- First')
    await page.keyboard.press('Enter')
    await page.keyboard.type('Second')

    expect((await editor.lines()).slice(5)).toEqual(['p: End.', 'li: First', 'li: Second'])
    await editor.document.expectSource(lists.replace('End.\n', 'End.\n\n* First\n* Second\n'))
  })

  test('undo gives back the typed -', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(5, 0)
    await page.keyboard.type('- ')
    expect((await editor.lines())[5]).toBe('li: End.')

    await page.keyboard.press(`${mod}+z`)

    expect((await editor.lines())[5]).toBe('p: - End.')
    await editor.expectLineCaret(5, 2)
  })

  test('Backspace right after gives back the typed -, as text', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(5, 0)
    await page.keyboard.type('- ')
    expect((await editor.lines())[5]).toBe('li: End.')

    await page.keyboard.press('Backspace')

    expect((await editor.lines())[5]).toBe('p: - End.')
    await editor.expectLineCaret(5, 2)
  })

  test('- that isn\'t at the start is text', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(5, 4)

    await page.keyboard.type(' - x')

    expect((await editor.lines())[5]).toBe('p: End. - x')
  })

  test('- and a space in an item, a heading or a quote are text', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(1, 0)
    await page.keyboard.type('- ')
    await editor.placeCaretInLine(4, 0)
    await page.keyboard.type('## - ')
    await editor.placeCaretInLine(5, 0)
    await page.keyboard.type('> - ')

    expect((await editor.lines())[1]).toBe('li: - One.')
    expect((await editor.lines()).slice(4)).toEqual(['h2: - Middle.', 'blockquote: - End.'])
  })

  test('#s and a space, or > and a space, in an item are text', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(1, 0)
    await page.keyboard.type('# ')
    await editor.placeCaretInLine(3, 0)
    await page.keyboard.type('> ')

    expect((await editor.lines())[1]).toBe('li: # One.')
    expect((await editor.lines())[3]).toBe('li: > Three.')
  })

  test('- and a space in the title are text', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaret('title', 0)

    await page.keyboard.type('- ')

    await editor.expectText('title', '- Field notes')
  })
})

test.describe('copying and pasting lists', () => {
  test('copying items puts a - before each on the clipboard', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(0, 0)
    for (let i = 0; i < 'Intro.'.length + 1 + 'One.'.length + 1 + 'Two,'.length; i++) {
      await page.keyboard.press('Shift+ArrowRight')
    }

    expect(await editor.copy()).toBe('Intro.\n- One.\n- Two,')
  })

  test('a pasted - is text', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(5, 4)

    await editor.paste('\n- Pasted')

    expect((await editor.lines()).slice(5)).toEqual(['p: End.', 'p: - Pasted'])
  })
})

test.describe('lists and collaborators', () => {
  test('a paragraph made an item in the source becomes one, and the caret stays on its text', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(5, 2)

    editor.document.replace('End.', '* End.')

    await expect.poll(async () => (await editor.lines())[5]).toBe('li: End.')
    await editor.expectLineCaret(5, 2)
    await page.keyboard.type('x')
    await editor.document.expectSource(lists.replace('End.', '* Enxd.'))
  })

  test('an item added in the source shows up, and the caret stays where it was', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    await editor.placeCaretInLine(3, 3)

    editor.document.replace('* Three.\n', '* Three.\n* Four.\n')

    await expect.poll(async () => (await editor.lines())[4]).toBe('li: Four.')
    await editor.expectLineCaret(3, 3)
  })
})

test.describe('moving through a list', () => {
  test('Up and Down move onto and off items', async ({ page }) => {
    const editor = await openEditor(page, 'lists')
    // (An item's text is indented, so the caret starts past the indent.)
    await editor.placeCaretInLine(4, 6)

    await page.keyboard.press('ArrowUp')
    await page.keyboard.type('x')
    await expect.poll(async () => (await editor.lines())[3]).toMatch(/^li: T.*x/)

    await page.keyboard.press('ArrowDown')
    await page.keyboard.type('y')
    await expect.poll(async () => (await editor.lines())[4]).toMatch(/^p: M.*y/)
  })
})
