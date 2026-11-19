/**
 * A numbered list whose items are each one line of plain text, numbered
 * with digits and written `1.` or `1)`, is edited in place like a bullet
 * list (`lists.spec.ts`, whose rules it shares): each item is a line of
 * its own, and the lines of numbered items next to each other are one
 * list. An edit keeps the list's start number and delimiter. An item is
 * made by typing `1.` and a space at the start of a paragraph, or with the
 * toolbar's numbered list button. An example list (`(@)`) can't be
 * edited yet.
 *
 * The fixtures' lists are written as the qmd writer writes a list, which
 * it does whole whenever any item of it changes: renumbered from its start
 * number, with two spaces after a number below 10.
 */

import { createDocument, expect, fixture, openEditor, test } from './helpers.ts'

const mod = 'ControlOrMeta'
const numbered = fixture('numbered')

// The lines of the `numbered` fixture.
const LINES = [
  'p: Intro.',
  'ol: One.',
  'ol: Two, wrapped over two lines.',
  'ol: Three.',
  'p: Middle.',
  'li: A bullet.',
  'ol: Seven.',
  'ol: Eight.',
  'p: End.',
]

const editable = 'main > ol:has(> li.qe-field)'

test.describe('showing numbered lists', () => {
  test('a numbered list of plain items is a line per item', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')

    expect(await editor.lines()).toEqual(LINES)
  })

  test('its items are one list, numbered from its start number, as `q2 render` shows it', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')

    const lists = editor.frame.locator(editable)
    await expect(lists).toHaveCount(2)
    await expect(lists.nth(0).locator('> li')).toHaveText(['One.', 'Two, wrapped over two lines.', 'Three.'])
    await expect(lists.nth(0)).toHaveAttribute('type', '1')
    await expect(lists.nth(0)).not.toHaveAttribute('start')
    await expect(lists.nth(1)).toHaveAttribute('start', '7')
  })

  test('an example list is shown and can\'t be edited', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')

    const readOnly = editor.frame.locator('main > ol:not(:has(.qe-field))')
    await expect(readOnly.locator('> li')).toHaveText(['An example.'])
  })
})

test.describe('editing an item', () => {
  test('typing changes the item\'s text', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(1, 4)

    await page.keyboard.type(' more')

    await editor.document.expectSource(numbered.replace('1.  One.', '1.  One. more'))
    expect((await editor.lines())[1]).toBe('ol: One. more')
  })

  test('typing in a list keeps its start number and its delimiter', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(7, 6)

    await page.keyboard.type('!')

    await editor.document.expectSource(numbered.replace('8)  Eight.', '8)  Eight.!'))
  })

  test('editing a paragraph leaves the list that can\'t be edited as it was', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(8, 4)

    await page.keyboard.type('!')

    await editor.document.expectSource(numbered.replace('End.', 'End.!'))
  })

  // The qmd writer writes a list again as a whole when any of it changes:
  // renumbered, with two spaces after a number below 10.
  test.fixme('editing an item leaves the rest of the list as it is written', async ({ page }) => {
    const source = '---\ntitle: Notes\n---\n\n1. One.\n1. Two.\n'
    const document = await createDocument(source)
    const editor = await document.open(page)
    await editor.placeCaretInLine(0, 4)

    await page.keyboard.type('!')

    // Today the list is written `1.  One.!` and `2.  Two.`.
    await document.expectSource(source.replace('1. One.', '1. One.!'))
  })
})

test.describe('Enter and Backspace in an item', () => {
  test('Enter at the end starts an item, and the items after it are numbered on', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(1, 4)

    await page.keyboard.press('Enter')
    await page.keyboard.type('New.')

    expect((await editor.lines()).slice(1, 4)).toEqual(['ol: One.', 'ol: New.', 'ol: Two, wrapped over two lines.'])
    await editor.document.expectSource(
      numbered.replace('2.  Two, wrapped\n    over two lines.\n3.  Three.', '2.  New.\n3.  Two, wrapped\n    over two lines.\n4.  Three.'),
    )
  })

  test('Enter at the start of the first item adds an item before it, and the list keeps its start number', async ({
    page,
  }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(6, 0)

    await page.keyboard.press('Enter')
    await page.keyboard.press('ArrowUp')
    await page.keyboard.type('Six.')

    expect((await editor.lines()).slice(6, 9)).toEqual(['ol: Six.', 'ol: Seven.', 'ol: Eight.'])
    await editor.document.expectSource(numbered.replace('7)  Seven.\n8)  Eight.', '7)  Six.\n8)  Seven.\n9)  Eight.'))
  })

  test('Enter on an empty item at the end of the list ends it, with a paragraph after it', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(3, 6)

    await page.keyboard.press('Enter')
    await page.keyboard.press('Enter')
    await page.keyboard.type('After.')

    expect((await editor.lines()).slice(3, 6)).toEqual(['ol: Three.', 'p: After.', 'p: Middle.'])
    await editor.document.expectSource(numbered.replace('3.  Three.\n', '3.  Three.\n\nAfter.\n'))
  })

  test('Backspace at the start of an item makes it a paragraph, between the lists around it', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(2, 0)

    await page.keyboard.press('Backspace')

    expect((await editor.lines()).slice(1, 4)).toEqual(['ol: One.', 'p: Two, wrapped over two lines.', 'ol: Three.'])
    await editor.document.expectSource(
      numbered.replace('2.  Two, wrapped\n    over two lines.\n3.  Three.', '\nTwo, wrapped\nover two lines.\n\n1.  Three.'),
    )
    await expect(editor.frame.locator(editable)).toHaveCount(3)
  })

  test('emptying an item keeps it an item, written as its bare number', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(1, 4)
    for (let i = 0; i < 4; i++) await page.keyboard.press('Backspace')

    await editor.placeCaretInLine(0, 0)

    expect((await editor.lines())[1]).toBe('ol: ')
    await editor.document.expectSource(numbered.replace('1.  One.', '1.'))
  })
})

test.describe('making a numbered list', () => {
  test('1. and a space at the start of a paragraph make it an item of a list of its own, numbered from 1', async ({
    page,
  }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(8, 0)

    await page.keyboard.type('1. ')

    expect((await editor.lines())[8]).toBe('ol: End.')
    await editor.document.expectSource(numbered.replace('End.', '1.  End.'))
    await editor.expectLineCaret(8, 0)
  })

  test('a paragraph made an item next to a numbered list joins it', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(4, 0)

    await page.keyboard.type('1. ')

    await expect(editor.frame.locator(editable).first().locator('> li')).toHaveCount(4)
    await editor.document.expectSource(numbered.replace('3.  Three.\n\nMiddle.', '3.  Three.\n4.  Middle.'))
  })

  test('a numbered item next to a bullet list is a list of its own, and a bullet next to a numbered list too', async ({
    page,
  }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(4, 0)

    await page.keyboard.type('- ')

    expect((await editor.lines()).slice(3, 6)).toEqual(['ol: Three.', 'li: Middle.', 'li: A bullet.'])
    await expect(editor.frame.locator('main > ul:has(> li.qe-field) > li')).toHaveCount(2)
    await expect(editor.frame.locator(editable).first().locator('> li')).toHaveCount(3)
  })

  test('in a new paragraph, Enter numbers on', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(8, 4)

    await page.keyboard.press('Enter')
    await page.keyboard.type('1. First')
    await page.keyboard.press('Enter')
    await page.keyboard.type('Second')

    expect((await editor.lines()).slice(8)).toEqual(['p: End.', 'ol: First', 'ol: Second'])
    await editor.document.expectSource(numbered.replace('End.\n', 'End.\n\n1.  First\n2.  Second\n'))
  })

  test('Backspace right after gives back the typed 1., as text', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(8, 0)
    await page.keyboard.type('1. ')
    expect((await editor.lines())[8]).toBe('ol: End.')

    await page.keyboard.press('Backspace')

    expect((await editor.lines())[8]).toBe('p: 1. End.')
    await editor.expectLineCaret(8, 3)
  })

  test('another number, or 1. in an item, is text', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(8, 0)
    await page.keyboard.type('2. ')
    await editor.placeCaretInLine(5, 0)
    await page.keyboard.type('1. ')

    expect((await editor.lines())[5]).toBe('li: 1. A bullet.')
    expect((await editor.lines())[8]).toBe('p: 2. End.')
  })
})

test.describe('copying numbered items', () => {
  test('copying items puts a 1. before each on the clipboard', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(0, 0)
    for (let i = 0; i < 'Intro.'.length + 1 + 'One.'.length; i++) await page.keyboard.press('Shift+ArrowRight')

    expect(await editor.copy()).toBe('Intro.\n1. One.')
  })
})

test.describe('numbered lists and collaborators', () => {
  test('an item added in the source shows up, and the caret stays where it was', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(7, 3)

    editor.document.replace('8)  Eight.\n', '8)  Eight.\n9)  Nine.\n')

    await expect.poll(async () => (await editor.lines())[8]).toBe('ol: Nine.')
    await editor.expectLineCaret(7, 3)
  })
})

test.describe('the numbered list button', () => {
  test('makes the caret\'s paragraph an item, with the caret where it was', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(8, 2)

    await editor.numberedListButton().click()

    expect((await editor.lines())[8]).toBe('ol: End.')
    await editor.document.expectSource(numbered.replace('End.', '1.  End.'))
    await expect(editor.numberedListButton()).toHaveAttribute('aria-pressed', 'true')
    await expect(editor.listButton()).toHaveAttribute('aria-pressed', 'false')
    await page.keyboard.type('!')
    await editor.document.expectSource(numbered.replace('End.', '1.  En!d.'))
  })

  test('is pressed on a numbered item, and makes it a paragraph again', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(3, 2)
    await expect(editor.numberedListButton()).toHaveAttribute('aria-pressed', 'true')

    await editor.numberedListButton().click()

    expect((await editor.lines())[3]).toBe('p: Three.')
    await editor.document.expectSource(numbered.replace('3.  Three.', '\nThree.'))
  })

  test('makes a bullet item numbered, joining the numbered list after it, and the bulleted list button makes it a bullet again', async ({
    page,
  }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(5, 2)

    await editor.numberedListButton().click()

    expect((await editor.lines())[5]).toBe('ol: A bullet.')
    await editor.document.expectSource(
      numbered.replace('* A bullet.\n\n7)  Seven.\n8)  Eight.', '7)  A bullet.\n8)  Seven.\n9)  Eight.'),
    )
    await expect(editor.listButton()).toHaveAttribute('aria-pressed', 'false')

    await editor.listButton().click()

    expect((await editor.lines())[5]).toBe('li: A bullet.')
    await editor.document.expectSource(numbered)
  })

  test('a selection over a paragraph and a bullet makes each a numbered item, joining the lists around them into one', async ({
    page,
  }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(4, 2)
    // To the end of the line's text, then on to the next line.
    await page.keyboard.press('Shift+ArrowDown')
    await page.keyboard.press('Shift+ArrowDown')

    await editor.numberedListButton().click()

    expect((await editor.lines()).slice(1, 6)).toEqual([
      'ol: One.',
      'ol: Two, wrapped over two lines.',
      'ol: Three.',
      'ol: Middle.',
      'ol: A bullet.',
    ])
    await expect(editor.frame.locator(editable).first().locator('> li')).toHaveCount(7)
    // Numbered as the first list was.
    await editor.document.expectSource(
      numbered.replace(
        '3.  Three.\n\nMiddle.\n\n* A bullet.\n\n7)  Seven.\n8)  Eight.',
        '3.  Three.\n4.  Middle.\n5.  A bullet.\n6.  Seven.\n7.  Eight.',
      ),
    )
  })

  test('undo gives the line back its style', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(8, 2)
    await editor.numberedListButton().click()
    await editor.document.expectSource(numbered.replace('End.', '1.  End.'))

    await page.keyboard.press(`${mod}+z`)

    await editor.document.expectSource(numbered)
  })

  test('on a numbered item, no heading can be chosen and the quote button is disabled', async ({ page }) => {
    const editor = await openEditor(page, 'numbered')
    await editor.placeCaretInLine(1, 2)

    await expect(editor.quoteButton()).toBeDisabled()
    await editor.styleBox().click()
    await expect(editor.styleMenu().getByRole('menuitemradio', { name: 'Heading 1' })).toBeDisabled()
  })

  test('is disabled on a heading, in a quote and in the title block', async ({ page }) => {
    const toolbar = await openEditor(page, 'toolbar')
    await toolbar.placeCaretInLine(1, 2)
    await expect(toolbar.numberedListButton()).toBeDisabled()
    await toolbar.placeCaretInLine(2, 2)
    await expect(toolbar.numberedListButton()).toBeDisabled()

    await toolbar.placeCaret('title', 2)
    await expect(toolbar.numberedListButton()).toBeDisabled()
  })
})
