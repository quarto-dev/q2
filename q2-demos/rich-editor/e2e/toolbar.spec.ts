/**
 * The toolbar above the document, as in a word processor. It follows the
 * caret: its style box shows the style of the caret's line (Paragraph,
 * Heading 1, …) and its menu changes it, and its quote button is pressed
 * while the caret is in a quote and quotes or unquotes the line. Using the
 * toolbar leaves the caret where it was, on the same text. It changes the
 * lines of the body only: in the title block, it's disabled.
 */

import { expect, fixture, openEditor, test } from './helpers.ts'

const mod = 'ControlOrMeta'
const toolbar = fixture('toolbar')

test.describe('following the caret', () => {
  test('the style box shows the style of the caret\'s line', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')

    await editor.placeCaretInLine(0, 2)
    await expect(editor.styleBox()).toHaveText('Paragraph')
    await editor.placeCaretInLine(1, 2)
    await expect(editor.styleBox()).toHaveText('Heading 2')
    await editor.placeCaretInLine(2, 2)
    await expect(editor.styleBox()).toHaveText('Paragraph')
  })

  test('the quote button is pressed while the caret is in a quote', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')

    await editor.placeCaretInLine(0, 2)
    await expect(editor.quoteButton()).toHaveAttribute('aria-pressed', 'false')
    await editor.placeCaretInLine(2, 2)
    await expect(editor.quoteButton()).toHaveAttribute('aria-pressed', 'true')
  })

  test('in the title block, the toolbar is disabled', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')

    await editor.placeCaret('title', 2)

    await expect(editor.styleBox()).toBeDisabled()
    await expect(editor.quoteButton()).toBeDisabled()
  })
})

test.describe('the style box', () => {
  test('its menu checks the caret\'s line\'s style', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(1, 2)

    await editor.styleBox().click()

    await expect(editor.styleMenu().getByRole('menuitemradio', { checked: true })).toHaveText('Heading 2')
  })

  test('its check mark is the same size whichever style is checked', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(1, 2)

    await editor.styleBox().click()

    const checked = editor.styleMenu().getByRole('menuitemradio', { checked: true })
    const paragraph = editor.styleMenu().getByRole('menuitemradio', { name: 'Paragraph' })
    const markSize = await checked.evaluate((el) => getComputedStyle(el, '::before').fontSize)
    const itemSize = await paragraph.evaluate((el) => getComputedStyle(el).fontSize)
    expect(markSize).toBe(itemSize)
  })

  test('opening its menu changes nothing in the document', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    const before = await editor.frame.locator('#quarto-document-content').innerHTML()

    await editor.styleBox().click()
    await expect(editor.styleMenu()).toBeVisible()

    expect(await editor.frame.locator('#quarto-document-content').innerHTML()).toBe(before)
  })

  test('choosing a heading makes the paragraph a heading, with the caret where it was', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 5)

    await editor.chooseStyle('Heading 1')

    await expect(editor.styleMenu()).toBeHidden()
    expect((await editor.lines())[0]).toBe('h1: Intro.')
    await editor.document.expectSource(toolbar.replace('Intro.', '# Intro.'))
    await page.keyboard.type('!')
    await editor.document.expectSource(toolbar.replace('Intro.', '# Intro!.'))
  })

  test('choosing Paragraph makes a heading a paragraph', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(1, 2)

    await editor.chooseStyle('Paragraph')

    expect((await editor.lines())[1]).toBe('p: Results')
    await editor.document.expectSource(toolbar.replace('## Results', 'Results'))
  })

  test('choosing another level changes the heading\'s level', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(1, 2)

    await editor.chooseStyle('Heading 3')

    await editor.document.expectSource(toolbar.replace('## Results', '### Results'))
  })

  test('in a quote, a heading can\'t be chosen', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(2, 2)

    await editor.styleBox().click()

    await expect(editor.styleMenu().getByRole('menuitemradio', { name: 'Heading 1' })).toBeDisabled()
    await expect(editor.styleMenu().getByRole('menuitemradio', { name: 'Paragraph' })).toBeEnabled()
  })

  test('a selection over several lines changes each of them', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    await page.keyboard.press('Shift+ArrowDown')

    await editor.chooseStyle('Heading 3')

    await editor.document.expectSource(toolbar.replace('Intro.', '### Intro.').replace('## Results', '### Results'))
  })

  test('undo gives the line back its style', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    await editor.chooseStyle('Heading 1')
    await editor.document.expectSource(toolbar.replace('Intro.', '# Intro.'))

    await page.keyboard.press(`${mod}+z`)

    await editor.document.expectSource(toolbar)
  })

  test('Escape closes its menu and changes nothing', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    await editor.styleBox().click()

    await page.keyboard.press('Escape')

    await expect(editor.styleMenu()).toBeHidden()
    await page.keyboard.type('!')
    await editor.document.expectSource(toolbar.replace('Intro.', 'In!tro.'))
  })

  test('clicking the style box again closes its menu', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    await editor.styleBox().click()

    await editor.styleBox().click()

    await expect(editor.styleMenu()).toBeHidden()
  })

  test('clicking the document closes its menu', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    await editor.styleBox().click()

    await editor.placeCaretInLine(3, 1)

    await expect(editor.styleMenu()).toBeHidden()
  })
})

test.describe('the quote button', () => {
  test('quotes the caret\'s paragraph, with the caret where it was', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(3, 2)

    await editor.quoteButton().click()

    expect((await editor.lines())[3]).toBe('blockquote: End.')
    await editor.document.expectSource(toolbar.replace('End.', '> End.'))
    await expect(editor.quoteButton()).toHaveAttribute('aria-pressed', 'true')
    await page.keyboard.type('!')
    await editor.document.expectSource(toolbar.replace('End.', '> En!d.'))
  })

  test('in a quote, makes it a paragraph again', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(2, 2)

    await editor.quoteButton().click()

    expect((await editor.lines())[2]).toBe('p: A quote.')
    await editor.document.expectSource(toolbar.replace('> A quote.', 'A quote.'))
  })

  test('is disabled on a heading', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')

    await editor.placeCaretInLine(1, 2)

    await expect(editor.quoteButton()).toBeDisabled()
  })
})
