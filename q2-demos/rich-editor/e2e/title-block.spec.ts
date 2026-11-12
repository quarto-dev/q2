/**
 * Editing the title and subtitle in the visual editor: the caret, Enter,
 * the arrow keys, undo, and adding and removing fields. What the edits do
 * to the source text is in `source.spec.ts`.
 */

import { expect, fixture, openEditor, test } from './helpers.ts'

const mod = 'ControlOrMeta'

test.describe('placing the caret', () => {
  test('clicking a field puts the caret at the click point without moving anything', async ({ page }) => {
    const editor = await openEditor(page)
    const before = await editor.layout()

    await editor.clickAt('title', 60)

    const caret = await editor.caret()
    expect(caret.field).toBe('title')
    expect(caret.start).toBeGreaterThan(0)
    expect(caret.start).toBeLessThan('Quarterly report'.length)
    expect(await editor.layout()).toEqual(before)
  })

  test('a focused field has no focus ring or highlight', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)

    const style = await editor.field('title').evaluate((el) => {
      const s = getComputedStyle(el)
      return { outline: s.outlineStyle, background: s.backgroundColor, boxShadow: s.boxShadow }
    })
    expect(style).toEqual({ outline: 'none', background: 'rgba(0, 0, 0, 0)', boxShadow: 'none' })
  })
})

test.describe('typing', () => {
  test('typing updates the field as you type', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press('End')
    await page.keyboard.type(' (draft)')

    expect(await editor.text('title')).toBe('Quarterly report (draft)')
    expect(await editor.caret()).toMatchObject({ field: 'title', start: 24, end: 24 })
  })

  test('Escape does nothing', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    const caret = await editor.caret()

    await page.keyboard.press('Escape')

    expect(await editor.caret()).toEqual(caret)
  })

  test('Shift+Enter inserts nothing', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    const caret = await editor.caret()

    await page.keyboard.press('Shift+Enter')

    expect(await editor.text('title')).toBe('Quarterly report')
    expect(await editor.caret()).toEqual(caret)
  })

  test('pasting flattens line breaks into spaces', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press('End')
    await editor.paste(' one\ntwo\r\n three')

    expect(await editor.text('title')).toBe('Quarterly report one two three')
  })
})

test.describe('Enter', () => {
  test('Enter in the middle of the title moves to the start of the subtitle without splitting', async ({
    page,
  }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)

    await page.keyboard.press('Enter')

    expect(await editor.text('title')).toBe('Quarterly report')
    expect(await editor.text('subtitle')).toBe('Third quarter')
    expect(await editor.caret()).toMatchObject({ field: 'subtitle', start: 0, end: 0 })
  })

  test('Enter in the last field goes to the body and changes nothing', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('subtitle', 30)

    await page.keyboard.press('Enter')

    expect(await editor.text('subtitle')).toBe('Third quarter')
    await editor.expectParagraphCaret(0, 0)
    await editor.document.expectSource(fixture('basic'))
  })

  test('Enter in the title adds an empty subtitle with a placeholder', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    expect(await editor.text('subtitle')).toBeNull()
    await editor.clickAt('title', 60)

    await page.keyboard.press('Enter')

    expect(await editor.text('subtitle')).toBe('')
    expect(await editor.caret()).toMatchObject({ field: 'subtitle', start: 0, end: 0 })
    const placeholder = await editor
      .field('subtitle')
      .evaluate((el) => getComputedStyle(el, '::before').content)
    expect(placeholder).toBe('"Subtitle"')
  })

  test('a new subtitle left empty goes away', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.clickAt('title', 60)
    await page.keyboard.press('Enter')

    await page.keyboard.press('ArrowUp')

    expect(await editor.text('subtitle')).toBeNull()
    expect((await editor.caret()).field).toBe('title')
  })

  test('typing into a new subtitle keeps it', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.clickAt('title', 60)
    await page.keyboard.press('Enter')
    await page.keyboard.type('Fourth quarter')
    await editor.clickAway()

    expect(await editor.text('subtitle')).toBe('Fourth quarter')
  })
})

test.describe('arrow keys between fields', () => {
  test('Down on the last line of the title goes to the subtitle at the same x, and Up comes back', async ({
    page,
  }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 100)
    const inTitle = await editor.caret()

    await page.keyboard.press('ArrowDown')
    const inSubtitle = await editor.caret()
    expect(inSubtitle.field).toBe('subtitle')
    expect(Math.abs(inSubtitle.x! - inTitle.x!)).toBeLessThan(15)

    await page.keyboard.press('ArrowUp')
    const back = await editor.caret()
    expect(back.field).toBe('title')
    expect(Math.abs(back.x! - inSubtitle.x!)).toBeLessThan(15)
  })

  test('Down on an earlier line of a wrapped title stays in the title', async ({ page }) => {
    await page.setViewportSize({ width: 700, height: 700 })
    const editor = await openEditor(page, 'long-title')
    expect(await editor.lineCount('title')).toBeGreaterThan(1)
    await editor.clickAt('title', 40)

    await page.keyboard.press('ArrowDown')

    const caret = await editor.caret()
    expect(caret.field).toBe('title')
    expect(caret.start).toBeGreaterThan(0)
  })

  test('Right at the end of the title goes to the start of the subtitle', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press('End')

    await page.keyboard.press('ArrowRight')

    expect(await editor.caret()).toMatchObject({ field: 'subtitle', start: 0, end: 0 })
  })

  test('Left at the start of the subtitle goes to the end of the title', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('subtitle', 30)
    await page.keyboard.press('Home')

    await page.keyboard.press('ArrowLeft')

    expect(await editor.caret()).toMatchObject({ field: 'title', start: 16, end: 16 })
  })

  test('Right at the end of the document stays put', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaretInParagraph(0, 10)

    await page.keyboard.press('ArrowRight')

    await editor.expectParagraphCaret(0, 10)
  })
})

test.describe('undo', () => {
  test('typing and then deleting are separate steps', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press('End')
    await page.keyboard.type(' mmmm')
    for (let i = 0; i < 5; i++) await page.keyboard.press('Backspace')
    expect(await editor.text('title')).toBe('Quarterly report')

    await page.keyboard.press(`${mod}+z`)
    expect(await editor.text('title')).toBe('Quarterly report mmmm')

    await page.keyboard.press(`${mod}+z`)
    expect(await editor.text('title')).toBe('Quarterly report')
  })

  test('redo reapplies an undone step', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press('End')
    await page.keyboard.type('!')
    await page.keyboard.press(`${mod}+z`)

    await page.keyboard.press(`${mod}+Shift+z`)

    expect(await editor.text('title')).toBe('Quarterly report!')
    expect(await editor.caret()).toMatchObject({ field: 'title', start: 17 })
  })

  test('typing in two places is two steps', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press('End')
    await page.keyboard.type('X')
    await page.keyboard.press('Home')
    await page.keyboard.type('Y')

    await page.keyboard.press(`${mod}+z`)

    expect(await editor.text('title')).toBe('Quarterly reportX')
  })

  test('a paste is its own step', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press('End')
    await page.keyboard.type(' a')
    await editor.paste('b')
    expect(await editor.text('title')).toBe('Quarterly report ab')

    await page.keyboard.press(`${mod}+z`)

    expect(await editor.text('title')).toBe('Quarterly report a')
  })

  test('undo with no field focused restores the text and puts the caret back', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press('End')
    await page.keyboard.type(' (draft)')
    await editor.clickAway()
    expect((await editor.caret()).field).toBeNull()

    await page.keyboard.press(`${mod}+z`)

    expect(await editor.text('title')).toBe('Quarterly report')
    expect(await editor.caret()).toMatchObject({ field: 'title', start: 16, end: 16 })
  })

  test('undo in one field can undo an edit in another', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press('End')
    await page.keyboard.type('!')
    await editor.clickAt('subtitle', 30)

    await page.keyboard.press(`${mod}+z`)

    expect(await editor.text('title')).toBe('Quarterly report')
    expect((await editor.caret()).field).toBe('title')
  })
})

test.describe('removing fields', () => {
  test('a field emptied and left goes away, and undo brings its text back', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('subtitle', 30)
    await page.keyboard.press(`${mod}+a`)
    await page.keyboard.press('Backspace')
    expect(await editor.text('subtitle')).toBe('')

    await editor.clickAt('title', 60)
    expect(await editor.text('subtitle')).toBeNull()

    await page.keyboard.press(`${mod}+z`)
    expect(await editor.text('subtitle')).toBe('Third quarter')
  })

  test('an emptied field keeps its line while it has the caret', async ({ page }) => {
    const editor = await openEditor(page)
    const height = (await editor.field('title').boundingBox())!.height
    await editor.clickAt('title', 60)
    await page.keyboard.press(`${mod}+a`)
    await page.keyboard.press('Backspace')

    expect((await editor.field('title').boundingBox())!.height).toBe(height)
  })
})
