/**
 * A figure is edited as in Google Docs: its image is one unit in the
 * text, and its caption is a line of text under it. The caption is typed
 * in place, and a selection takes the image whole and as much of the
 * caption as it covers. Deleting the image (with Backspace at the start
 * of its caption, or a selection that takes it in) leaves the caption's
 * text as a paragraph, joined to the line the selection started in, and
 * undo brings the figure back. Enter at the end of a caption starts a
 * paragraph after the figure, and in its middle moves the rest of the
 * caption to one. An emptied caption leaves the figure, with a
 * placeholder, written as an image alone on its line (`![](tray.png)`).
 */

import { createDocument, expect, fixture, lineEnd, lineStart, openEditor, test } from './helpers.ts'

const mod = 'ControlOrMeta'
const figures = fixture('figures')

const wide = '![A *wide* view of the site](site.png)'
const tray = '![The sample tray](tray.png){#tray .bordered}'
const sketch = '![A sketch](sketch.png){fig-alt="A pencil sketch of the trench"}'

test.describe('captions', () => {
  test('a caption is a line of text under its image', async ({ page }) => {
    const editor = await openEditor(page, 'figures')

    expect(await editor.lines()).toEqual([
      'p: Before the figures.',
      'figcaption: A wide view of the site',
      'figcaption: The sample tray',
      'figcaption: A sketch',
      'p: After the figures.',
    ])
    await expect(editor.frame.locator('figure > img')).toHaveCount(3)
  })

  test('typing in a caption changes it, and the image keeps its address and attributes', async ({ page }) => {
    const document = await createDocument(figures)
    const editor = await document.open(page)
    await editor.placeCaretInLine(2, 'The sample tray'.length)

    await page.keyboard.type(' and its lid')

    await document.expectSource(figures.replace(tray, '![The sample tray and its lid](tray.png){#tray .bordered}'))
    // Its alt text is its caption's.
    await expect(editor.frame.locator('figure > img').nth(1)).toHaveAttribute('alt', 'The sample tray and its lid')
  })

  test('a caption can be formatted', async ({ page }) => {
    const document = await createDocument(figures)
    const editor = await document.open(page)
    await editor.placeCaretInLine(2, 4)
    await page.keyboard.press(`Shift+${lineEnd}`)

    await page.keyboard.press(`${mod}+b`)

    await document.expectSource(figures.replace(tray, '![The **sample tray**](tray.png){#tray .bordered}'))
  })

  test('an image with alt text of its own keeps it when the caption changes', async ({ page }) => {
    const document = await createDocument(figures)
    const editor = await document.open(page)
    await editor.placeCaretInLine(3, 'A sketch'.length)

    await page.keyboard.type(' of the trench')

    await document.expectSource(figures.replace(sketch, '![A sketch of the trench](sketch.png){fig-alt="A pencil sketch of the trench"}'))
  })

  test('an emptied caption leaves the figure, with a placeholder, and typing gives it a caption again', async ({ page }) => {
    const document = await createDocument(figures)
    const editor = await document.open(page)
    await editor.placeCaretInLine(2, 'The sample tray'.length)
    await page.keyboard.press(`Shift+${lineStart}`)

    await page.keyboard.press('Backspace')

    await document.expectSource(figures.replace(tray, '![](tray.png){#tray .bordered}'))
    await expect(editor.frame.locator('figure > img')).toHaveCount(3)
    await expect(editor.frame.locator('figcaption.qe-field').nth(1)).toHaveAttribute('data-placeholder', 'Add a caption')

    await page.keyboard.type('Trays')

    await document.expectSource(figures.replace(tray, '![Trays](tray.png){#tray .bordered}'))
  })

  test("a caption's line can't be given a style", async ({ page }) => {
    const editor = await openEditor(page, 'figures')
    await editor.placeCaretInLine(2, 4)

    await expect(editor.styleBox()).toBeDisabled()
    await expect(editor.listButton()).toBeDisabled()
    await expect(editor.numberedListButton()).toBeDisabled()
    await expect(editor.quoteButton()).toBeDisabled()
  })
})

// The qmd writer writes every attribute's value quoted when it writes a
// figure again, whatever changed in it.
test.fixme("editing a caption leaves the image's attributes as they are written", async ({ page }) => {
  const source = '---\ntitle: Notes\n---\n\n![A tray](tray.png){width=50%}\n'
  const document = await createDocument(source)
  const editor = await document.open(page)
  await editor.placeCaretInLine(0, 'A tray'.length)

  await page.keyboard.type('s')

  // Today it's written `width="50%"`.
  await document.expectSource(source.replace('A tray', 'A trays'))
})

test.describe('Enter in a caption', () => {
  test('at its end starts a paragraph after the figure', async ({ page }) => {
    const document = await createDocument(figures)
    const editor = await document.open(page)
    await editor.placeCaretInLine(2, 'The sample tray'.length)

    await page.keyboard.press('Enter')
    await page.keyboard.type('Then.')

    await document.expectSource(figures.replace(tray, `${tray}\n\nThen.`))
  })

  test('in its middle moves the rest of the caption to a paragraph after the figure', async ({ page }) => {
    const document = await createDocument(figures)
    const editor = await document.open(page)
    await editor.placeCaretInLine(2, 'The sample'.length)

    await page.keyboard.press('Enter')

    await document.expectSource(figures.replace(tray, '![The sample](tray.png){#tray .bordered}\n\ntray'))
    await editor.expectLineCaret(3, 0)
  })
})

test.describe('deleting an image', () => {
  test('Backspace at the start of a caption removes the image, and the caption is a paragraph', async ({ page }) => {
    const document = await createDocument(figures)
    const editor = await document.open(page)
    await editor.placeCaretInLine(2, 0)

    await page.keyboard.press('Backspace')

    await document.expectSource(figures.replace(tray, 'The sample tray'))
    expect((await editor.lines())[2]).toBe('p: The sample tray')
    await editor.expectLineCaret(2, 0)

    await page.keyboard.press(`${mod}+z`)

    await document.expectSource(figures)
    await expect(editor.frame.locator('figure > img')).toHaveCount(3)
  })

  test('deleting a selection from a paragraph into a caption removes the image, and the rest of the caption joins the paragraph', async ({ page }) => {
    const document = await createDocument(figures)
    const editor = await document.open(page)
    await editor.placeCaretInLine(0, 'Before '.length)
    // To the end of the paragraph, over the line break, and over "A ".
    for (let i = 0; i < 'the figures.'.length + 1 + 'A '.length; i++) await page.keyboard.press('Shift+ArrowRight')

    await page.keyboard.press('Backspace')

    await document.expectSource(figures.replace(`Before the figures.\n\n${wide}`, 'Before *wide* view of the site'))
    await expect(editor.frame.locator('figure > img')).toHaveCount(2)

    await page.keyboard.press(`${mod}+z`)

    await document.expectSource(figures)
    await expect(editor.frame.locator('figure > img')).toHaveCount(3)
  })

  test('copying a selection from a paragraph into a caption copies the text, without the image', async ({ page }) => {
    const editor = await openEditor(page, 'figures')
    await editor.placeCaretInLine(0, 'Before '.length)
    for (let i = 0; i < 'the figures.'.length + 1 + 'A '.length; i++) await page.keyboard.press('Shift+ArrowRight')

    expect(await editor.copy()).toBe('the figures.\nA ')
  })
})
