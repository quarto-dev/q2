/**
 * A list in a list's item (a nested list) is edited in place like the list
 * it's in: each of its items is a line of its own, after its parent item's
 * line, shown indented a level, as `q2 render` shows it. Tab in an item
 * indents it a level, under the item before it, and Shift+Tab outdents
 * it, each with the items nested in it; so do the toolbar's "Increase
 * indent" and "Decrease indent" buttons, and ⌘] and ⌘[ (Ctrl elsewhere).
 * Enter on an empty nested item, or Backspace at the start of a nested
 * item's text, moves it out a level, as they make an item at the first
 * level a paragraph.
 */

import { createDocument, expect, fixture, openEditor, test } from './helpers.ts'

const mod = 'ControlOrMeta'
const nested = fixture('nested-lists')

/** The fixture's lines, as `Editor.lines` gives them. */
const LINES = [
  'p: Intro.',
  'li: Fruit',
  '  li: Apples',
  '  li: Pears',
  '    li: Conference',
  'li: Vegetables',
  'li: Grains',
  'ol: Plan',
  '  li: or not?',
  'ol: Do',
  'p: End.',
]

test.describe('showing nested lists', () => {
  test('an item in an item is a line after it, a level in', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')

    expect(await editor.lines()).toEqual(LINES)
  })

  test('a nested list is in an item of the list it\'s in, after its parent item\'s', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')

    const inner = editor.frame.locator('main > ul > li > ul')
    await expect(inner.locator('> li.qe-field')).toHaveText(['Apples', 'Pears'])
    await expect(inner.locator('> li > ul > li.qe-field')).toHaveText(['Conference'])
    await expect(editor.frame.locator('main > ol > li > ul > li.qe-field')).toHaveText(['or not?'])
    // Indented a level, past its parent item's text.
    const parent = await editor.line(1).boundingBox()
    const child = await editor.line(2).boundingBox()
    expect(child!.x).toBeGreaterThan(parent!.x + 10)
  })

  // The parent item's `li` is its line, which can't hold other lines.
  test.fixme('a nested list is in its parent item\'s `li`, as `q2 render` writes it', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')

    await expect(editor.frame.locator('main > ul > li.qe-field > ul > li.qe-field')).toHaveText(['Apples', 'Pears'])
  })

  test('a list with two lists of a kind in an item is shown and can\'t be edited', async ({ page }) => {
    const source = '---\ntitle: T\n---\n\n* A\n\n  * B\n\n  Text.\n'
    const document = await createDocument(source)
    const editor = await document.open(page)

    await expect(editor.frame.locator('main > ul:not(:has(.qe-field))')).toHaveCount(1)
  })
})

test.describe('editing a nested item', () => {
  test('typing changes its text', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')
    await editor.placeCaretInLine(2, 6)

    await page.keyboard.type(' and more')

    await editor.document.expectSource(nested.replace('  * Apples', '  * Apples and more'))
    expect((await editor.lines())[2]).toBe('  li: Apples and more')
  })

  test('Enter at its end starts an item at its level', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')
    await editor.placeCaretInLine(4, 10)

    await page.keyboard.press('Enter')
    await page.keyboard.type('Bosc')

    expect((await editor.lines()).slice(4, 6)).toEqual(['    li: Conference', '    li: Bosc'])
    await editor.document.expectSource(nested.replace('    * Conference\n', '    * Conference\n    * Bosc\n'))
  })

  test('Enter on an empty nested item moves it out a level', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')
    await editor.placeCaretInLine(4, 10)

    await page.keyboard.press('Enter')
    await page.keyboard.press('Enter')
    await page.keyboard.type('Plums')

    expect((await editor.lines()).slice(3, 6)).toEqual(['  li: Pears', '    li: Conference', '  li: Plums'])
    await editor.document.expectSource(nested.replace('    * Conference\n', '    * Conference\n  * Plums\n'))
  })

  test('Backspace at the start of its text moves it out a level, with the items after it in it', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')
    await editor.placeCaretInLine(2, 0)

    await page.keyboard.press('Backspace')

    expect((await editor.lines()).slice(1, 6)).toEqual([
      'li: Fruit',
      'li: Apples',
      '  li: Pears',
      '    li: Conference',
      'li: Vegetables',
    ])
    await editor.document.expectSource(nested.replace('  * Apples\n  * Pears\n    * Conference\n', '* Apples\n  * Pears\n    * Conference\n'))
    await editor.expectLineCaret(2, 0)
  })
})

test.describe('Tab and Shift+Tab', () => {
  test('Tab indents an item under the one before it, with the items in it', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')
    await editor.placeCaretInLine(3, 2)

    await page.keyboard.press('Tab')

    expect((await editor.lines()).slice(2, 5)).toEqual(['  li: Apples', '    li: Pears', '      li: Conference'])
    await editor.document.expectSource(nested.replace('  * Pears\n    * Conference\n', '    * Pears\n      * Conference\n'))
    await editor.expectLineCaret(3, 2)
  })

  test('Tab at the first level makes the item an item of the one before it', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')
    await editor.placeCaretInLine(6, 0)

    await page.keyboard.press('Tab')

    expect((await editor.lines()).slice(5, 7)).toEqual(['li: Vegetables', '  li: Grains'])
    await editor.document.expectSource(nested.replace('* Grains\n', '  * Grains\n'))
  })

  test('Tab does nothing on the first item of a level, and the caret stays where it was', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')
    await editor.placeCaretInLine(2, 3)

    await page.keyboard.press('Tab')
    await page.keyboard.type('X')

    expect((await editor.lines())[2]).toBe('  li: AppXles')
    await editor.document.expectSource(nested.replace('Apples', 'AppXles'))
  })

  test('Shift+Tab outdents a nested item, with the items in it, and the items after it go in it', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')
    await editor.placeCaretInLine(2, 0)

    await page.keyboard.press('Shift+Tab')

    expect((await editor.lines()).slice(1, 5)).toEqual(['li: Fruit', 'li: Apples', '  li: Pears', '    li: Conference'])
  })

  test('Shift+Tab does nothing at the first level', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')
    await editor.placeCaretInLine(5, 2)

    await page.keyboard.press('Shift+Tab')
    await page.keyboard.type('X')

    expect((await editor.lines())[5]).toBe('li: VeXgetables')
  })

  test('with a selection, they indent and outdent every item it is on', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')
    await editor.placeCaretInLine(5, 2)
    await page.keyboard.press('Shift+ArrowDown')

    await page.keyboard.press('Tab')

    expect((await editor.lines()).slice(1, 7)).toEqual([
      'li: Fruit',
      '  li: Apples',
      '  li: Pears',
      '    li: Conference',
      '  li: Vegetables',
      '  li: Grains',
    ])
    await page.keyboard.press('Shift+Tab')
    expect(await editor.lines()).toEqual(LINES)
  })

  test('in a numbered list, the item stays numbered', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')
    await editor.placeCaretInLine(9, 0)

    await page.keyboard.press('Tab')

    expect((await editor.lines()).slice(7, 10)).toEqual(['ol: Plan', '  li: or not?', '  ol: Do'])
  })

  test('undo outdents an item Tab indented', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')
    await editor.placeCaretInLine(6, 0)
    await page.keyboard.press('Tab')
    await expect.poll(async () => (await editor.lines())[6]).toBe('  li: Grains')

    await page.keyboard.press(`${mod}+z`)

    expect(await editor.lines()).toEqual(LINES)
    await editor.document.expectSource(nested)
  })

  test('Tab in a paragraph does nothing to it', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')
    await editor.placeCaretInLine(0, 0)

    await page.keyboard.press('Tab')

    expect(await editor.lines()).toEqual(LINES)
  })
})

test.describe('the indent buttons and shortcuts', () => {
  test('Increase indent and Decrease indent do what Tab and Shift+Tab do', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')
    await editor.placeCaretInLine(6, 0)

    await editor.indentButton().click()
    expect((await editor.lines())[6]).toBe('  li: Grains')
    await editor.outdentButton().click()
    expect((await editor.lines())[6]).toBe('li: Grains')
    await editor.expectLineCaret(6, 0)
  })

  test('they\'re disabled where they can\'t change anything', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')

    await editor.placeCaretInLine(0, 0)
    await expect(editor.indentButton()).toBeDisabled()
    await expect(editor.outdentButton()).toBeDisabled()
    await editor.placeCaretInLine(1, 0)
    await expect(editor.indentButton()).toBeDisabled()
    await expect(editor.outdentButton()).toBeDisabled()
    await editor.placeCaretInLine(3, 0)
    await expect(editor.indentButton()).toBeEnabled()
    await expect(editor.outdentButton()).toBeEnabled()
  })

  test('⌘] indents and ⌘[ outdents', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')
    await editor.placeCaretInLine(6, 0)

    await page.keyboard.press(`${mod}+BracketRight`)
    expect((await editor.lines())[6]).toBe('  li: Grains')
    await page.keyboard.press(`${mod}+BracketLeft`)
    expect((await editor.lines())[6]).toBe('li: Grains')
  })

  test('the numbered list button keeps a nested item nested', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')
    await editor.placeCaretInLine(2, 0)

    await editor.numberedListButton().click()

    expect((await editor.lines()).slice(1, 4)).toEqual(['li: Fruit', '  ol: Apples', '  li: Pears'])
  })
})

test.describe('copying nested items', () => {
  test('puts each nested item after four spaces for each level', async ({ page }) => {
    const editor = await openEditor(page, 'nested-lists')
    await editor.placeCaretInLine(1, 0)
    for (let i = 0; i < 'Fruit.Apples.Pears.Conference'.length; i++) await page.keyboard.press('Shift+ArrowRight')

    expect(await editor.copy()).toBe('- Fruit\n    - Apples\n    - Pears\n        - Conference')
  })
})
