/**
 * Links in the body's text. A link is shown as `q2 render` shows it, and
 * its text is edited like any other; typing at its end isn't in it.
 *
 * The caret in a link, or the mouse resting on one, shows a popover under
 * it, as in Google Docs: the link's address, Edit and Remove. Tab in the
 * text moves into the popover of the caret's link (the address, where
 * Enter opens the link, then Edit and Remove), and Escape or Tab past
 * either end returns to the text. Escape in the text puts the popover away
 * until the caret comes back to the link.
 *
 * ⌘K (Ctrl elsewhere) and the toolbar's link button open a small form
 * with the link's text and address: for the caret's link, or to make the
 * selected text a link. Enter makes it, Escape goes back to the text
 * leaving everything as it was.
 */

import { createDocument, expect, fixture, openEditor, test } from './helpers.ts'

const mod = 'ControlOrMeta'
const inlines = fixture('inlines')
const FIRST = 'Some bold text, italic text, underlined, struck, highlighted, code and a link.'
const LINK = FIRST.indexOf('a link')
// Placing the caret by arrow presses crosses the code before the link,
// which takes one press more at each of its edges.
const inLink = (offset: number) => LINK + offset + 2

/** The text of the element with the focus in the document's frame. */
const focused = (editor: { frame: import('@playwright/test').Frame }) =>
  editor.frame.evaluate(() => document.activeElement?.textContent ?? null)

test.describe('the popover', () => {
  test('the caret in a link shows its address, with Edit and Remove', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')

    await editor.placeCaretInLine(0, inLink(2))

    await expect(editor.linkPopover()).toBeVisible()
    await expect(editor.linkPopover().getByRole('link')).toHaveText('https://quarto.org')
    await expect(editor.linkPopover().getByRole('button')).toHaveText(['Edit', 'Remove'])
    // Under the link.
    const link = (await editor.line(0).locator('a').boundingBox())!
    const popover = (await editor.linkPopover().boundingBox())!
    expect(popover.y).toBeGreaterThan(link.y + link.height - 1)
  })

  test('the caret anywhere else shows none', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(0, inLink(2))
    await expect(editor.linkPopover()).toBeVisible()

    await editor.placeCaretInLine(2, 2)

    await expect(editor.linkPopover()).toHaveCount(0)
  })

  test('the mouse resting on a link shows its popover, and leaving it puts it away', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(2, 2)

    await editor.line(0).locator('a').hover()
    await expect(editor.linkPopover()).toBeVisible()

    await editor.pointAway()
    await expect(editor.linkPopover()).toHaveCount(0)
  })

  test('Remove takes the link off its text', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(0, inLink(2))

    await editor.linkPopover().getByRole('button', { name: 'Remove' }).click()

    await editor.document.expectSource(inlines.replace('[a link](https://quarto.org)', 'a link'))
    await expect(editor.line(0).locator('a')).toHaveCount(0)
    expect((await editor.lines())[0]).toBe(`p: ${FIRST}`)
  })

  test('Edit opens the form, where Enter changes the address', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(0, inLink(2))

    await editor.linkPopover().getByRole('button', { name: 'Edit' }).click()
    await expect(editor.linkForm()).toBeVisible()
    await expect(editor.linkForm().locator('input[name=text]')).toHaveValue('a link')
    await expect(editor.linkForm().locator('input[name=url]')).toHaveValue('https://quarto.org')
    await expect(editor.linkForm().locator('input[name=url]')).toBeFocused()
    await page.keyboard.press(`${mod}+a`)
    await page.keyboard.type('https://quarto.org/docs')
    await page.keyboard.press('Enter')

    await editor.document.expectSource(inlines.replace('(https://quarto.org)', '(https://quarto.org/docs)'))
    await expect(editor.linkForm()).toHaveCount(0)
  })

  test('Tab moves into it: the address, Edit, Remove, and back to the text', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(0, inLink(2))
    await expect(editor.linkPopover()).toBeVisible()

    await page.keyboard.press('Tab')
    await expect.poll(() => focused(editor)).toBe('https://quarto.org')
    await page.keyboard.press('Tab')
    await expect.poll(() => focused(editor)).toBe('Edit')
    await page.keyboard.press('Tab')
    await expect.poll(() => focused(editor)).toBe('Remove')
    await page.keyboard.press('Tab')

    await editor.expectLineCaret(0, LINK + 2)
    await page.keyboard.type('x')
    await editor.document.expectSource(inlines.replace('[a link]', '[a xlink]'))
  })

  test('Escape in it returns to the text', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(0, inLink(2))
    await page.keyboard.press('Tab')
    await expect.poll(() => focused(editor)).toBe('https://quarto.org')

    await page.keyboard.press('Escape')

    await editor.expectLineCaret(0, LINK + 2)
  })

  test('Enter on the address opens the link', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await page.context().route('https://quarto.org/**', (route) => route.fulfill({ body: 'Quarto', contentType: 'text/plain' }))
    await editor.placeCaretInLine(0, inLink(2))
    await page.keyboard.press('Tab')
    await expect.poll(() => focused(editor)).toBe('https://quarto.org')

    const [opened] = await Promise.all([page.context().waitForEvent('page'), page.keyboard.press('Enter')])

    expect(opened.url()).toBe('https://quarto.org/')
  })

  test('Escape in the text puts it away until the caret comes back to the link', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(0, inLink(2))
    await expect(editor.linkPopover()).toBeVisible()

    await page.keyboard.press('Escape')
    await expect(editor.linkPopover()).toHaveCount(0)
    await page.keyboard.press('ArrowRight')
    await expect(editor.linkPopover()).toHaveCount(0)

    // Out of the link, past its end, and back in.
    for (let i = 0; i < 5; i++) await page.keyboard.press('ArrowRight')
    await expect(editor.linkPopover()).toHaveCount(0)
    await page.keyboard.press('ArrowLeft')
    await page.keyboard.press('ArrowLeft')
    await expect(editor.linkPopover()).toBeVisible()
  })
})

test.describe('editing a link', () => {
  test("typing at its end isn't in it", async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(0, inLink('a link'.length))

    await page.keyboard.type('s')

    await editor.document.expectSource(inlines.replace('[a link](https://quarto.org).', '[a link](https://quarto.org)s.'))
  })

  test('typing inside it is in it', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(0, inLink(1))

    await page.keyboard.type(' new')

    await editor.document.expectSource(inlines.replace('[a link]', '[a new link]'))
  })
})

test.describe('the link form', () => {
  test('⌘K on a selection makes it a link', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(2, 0)
    for (let i = 0; i < 5; i++) await page.keyboard.press('Shift+ArrowRight')

    await page.keyboard.press(`${mod}+k`)
    await expect(editor.linkForm().locator('input[name=text]')).toHaveValue('Plain')
    await expect(editor.linkForm().locator('input[name=url]')).toBeFocused()
    await page.keyboard.type('https://example.com')
    await page.keyboard.press('Enter')

    await editor.document.expectSource(inlines.replace('Plain words.', '[Plain](https://example.com) words.'))
    // The caret is back in the text, after the link.
    await editor.expectLineCaret(2, 5)
  })

  test('⌘K with nothing selected puts in a new link, its address as its text when it has none', async ({ page }) => {
    const document = await createDocument('---\ntitle: T\n---\n\nSee .\n')
    const editor = await document.open(page)
    await editor.placeCaretInLine(0, 4)

    await page.keyboard.press(`${mod}+k`)
    await expect(editor.linkForm().locator('input[name=text]')).toBeFocused()
    await page.keyboard.press('Tab')
    await page.keyboard.type('https://example.com')
    await page.keyboard.press('Enter')

    await document.expectSource('---\ntitle: T\n---\n\nSee [https://example.com](https://example.com).\n')
  })

  test('the toolbar\'s link button is pressed in a link, and opens the form', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(2, 2)
    await expect(editor.linkButton()).toHaveAttribute('aria-pressed', 'false')
    await editor.placeCaretInLine(0, inLink(2))
    await expect(editor.linkButton()).toHaveAttribute('aria-pressed', 'true')

    await editor.linkButton().click()

    await expect(editor.linkForm().locator('input[name=url]')).toHaveValue('https://quarto.org')
  })

  test('Escape goes back to the text, changing nothing', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(2, 0)
    for (let i = 0; i < 5; i++) await page.keyboard.press('Shift+ArrowRight')
    await page.keyboard.press(`${mod}+k`)
    await expect(editor.linkForm()).toBeVisible()

    await page.keyboard.press('Escape')

    await expect(editor.linkForm()).toHaveCount(0)
    expect(await editor.selectedText()).toBe('Plain')
    expect(editor.document.source()).toBe(inlines)
  })
})
