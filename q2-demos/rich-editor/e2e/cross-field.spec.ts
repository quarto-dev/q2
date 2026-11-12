/**
 * The title block is one editing surface: selections can span fields, and
 * edits over them trim each field without ever joining two. Undo is one
 * history for the whole block.
 */

import { expect, test } from '@playwright/test'
import { openEditor, type Editor } from './helpers.ts'

const mod = 'ControlOrMeta'

/** Select from "Quarterly |report" to "Third| quarter" with the keyboard. */
async function selectAcross(editor: Editor) {
  await editor.clickAt('title', 60)
  await editor.page.keyboard.press('Home')
  for (let i = 0; i < 10; i++) await editor.page.keyboard.press('ArrowRight')
  // 6 to the end of the title, 1 across to the subtitle, 5 into it.
  for (let i = 0; i < 12; i++) await editor.page.keyboard.press('Shift+ArrowRight')
  expect(await editor.selection()).toEqual({
    anchor: { field: 'title', offset: 10 },
    focus: { field: 'subtitle', offset: 5 },
  })
}

test.describe('selecting across fields', () => {
  test('typing over the selection goes into the first field and trims the second', async ({ page }) => {
    const editor = await openEditor(page)
    await selectAcross(editor)

    await page.keyboard.type('X')

    expect(await editor.text('title')).toBe('Quarterly X')
    expect(await editor.text('subtitle')).toBe(' quarter')
    expect(await editor.caret()).toMatchObject({ field: 'title', start: 11, end: 11 })
  })

  test('Backspace trims both fields without joining them', async ({ page }) => {
    const editor = await openEditor(page)
    await selectAcross(editor)

    await page.keyboard.press('Backspace')

    expect(await editor.text('title')).toBe('Quarterly ')
    expect(await editor.text('subtitle')).toBe(' quarter')
    expect(await editor.caret()).toMatchObject({ field: 'title', start: 10, end: 10 })
  })

  test('a mouse drag selects across fields', async ({ page }) => {
    const editor = await openEditor(page)
    const title = (await editor.field('title').boundingBox())!
    const subtitle = (await editor.field('subtitle').boundingBox())!
    await page.mouse.move(title.x + 60, title.y + title.height / 2)
    await page.mouse.down()
    await page.mouse.move(subtitle.x + 30, subtitle.y + subtitle.height / 2, { steps: 5 })
    await page.mouse.up()

    const selection = (await editor.selection())!
    expect(selection.anchor.field).toBe('title')
    expect(selection.focus.field).toBe('subtitle')

    await page.keyboard.press('Delete')
    expect((await editor.text('title'))!.length).toBe(selection.anchor.offset)
    expect(await editor.text('subtitle')).toBe('Third quarter'.slice(selection.focus.offset))
  })

  test('undo restores both fields and the selection', async ({ page }) => {
    const editor = await openEditor(page)
    await selectAcross(editor)
    await page.keyboard.press('Backspace')

    await page.keyboard.press(`${mod}+z`)

    expect(await editor.text('title')).toBe('Quarterly report')
    expect(await editor.text('subtitle')).toBe('Third quarter')
    expect(await editor.selection()).toEqual({
      anchor: { field: 'title', offset: 10 },
      focus: { field: 'subtitle', offset: 5 },
    })
  })

  test('select all selects the field, then the whole title block', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('subtitle', 30)

    await page.keyboard.press(`${mod}+a`)
    expect(await editor.selection()).toEqual({
      anchor: { field: 'subtitle', offset: 0 },
      focus: { field: 'subtitle', offset: 13 },
    })

    await page.keyboard.press(`${mod}+a`)
    expect(await editor.selection()).toEqual({
      anchor: { field: 'title', offset: 0 },
      focus: { field: 'subtitle', offset: 13 },
    })
  })
})

test.describe('Backspace at a field edge', () => {
  test('at the start of the subtitle moves to the end of the title without deleting', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('subtitle', 30)
    await page.keyboard.press('Home')

    await page.keyboard.press('Backspace')

    expect(await editor.text('title')).toBe('Quarterly report')
    expect(await editor.text('subtitle')).toBe('Third quarter')
    expect(await editor.caret()).toMatchObject({ field: 'title', start: 16, end: 16 })
  })

  test('in an empty subtitle removes it', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.clickAt('title', 60)
    await page.keyboard.press('Enter')

    await page.keyboard.press('Backspace')

    expect(await editor.text('subtitle')).toBeNull()
    expect(await editor.caret()).toMatchObject({ field: 'title', start: 16, end: 16 })
  })
})

test.describe('one history', () => {
  test('undo undoes the latest edit wherever the caret is', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press('End')
    await page.keyboard.type('!')
    await editor.clickAt('subtitle', 30)
    await page.keyboard.press('End')
    await page.keyboard.type('?')
    await editor.clickAt('title', 60)

    await page.keyboard.press(`${mod}+z`)
    expect(await editor.text('title')).toBe('Quarterly report!')
    expect(await editor.text('subtitle')).toBe('Third quarter')
    expect(await editor.caret()).toMatchObject({ field: 'subtitle', start: 13 })

    await page.keyboard.press(`${mod}+z`)
    expect(await editor.text('title')).toBe('Quarterly report')
  })
})

/** Record, in the page, whether every selection change kept the caret in a field. */
async function watchSelection(editor: Editor) {
  await editor.frame.evaluate(() => {
    const w = window as unknown as { leftFields: boolean }
    w.leftFields = false
    document.addEventListener('selectionchange', () => {
      const n = document.getSelection()?.focusNode
      const el = n?.nodeType === Node.ELEMENT_NODE ? (n as Element) : n?.parentElement
      if (!el?.closest('.qe-field')) w.leftFields = true
    })
  })
  return async () => {
    await editor.page.waitForTimeout(50)
    return editor.frame.evaluate(() => (window as unknown as { leftFields: boolean }).leftFields)
  }
}

/** The page's selected text and whether its focus is inside a field. */
const pageSelection = (editor: Editor) =>
  editor.frame.evaluate(() => {
    const s = document.getSelection()!
    const inField = (n: Node | null) =>
      !!(n?.nodeType === Node.ELEMENT_NODE ? (n as Element) : n?.parentElement)?.closest('.qe-field')
    return { text: s.toString(), rangeCount: s.rangeCount, focusInField: inField(s.focusNode) }
  })

test.describe('arrow keys past the author grid', () => {
  test('Right at the end of the subtitle goes straight to the abstract', async ({ page }) => {
    const editor = await openEditor(page, 'abstract')
    await editor.clickAt('subtitle', 30)
    await page.keyboard.press('End')
    const leftFields = await watchSelection(editor)

    await page.keyboard.press('ArrowRight')

    expect(await editor.caret()).toMatchObject({ field: 'abstract', start: 0, end: 0 })
    expect(await leftFields()).toBe(false)
  })

  test('Down from the subtitle goes straight to the abstract, and Up comes back', async ({ page }) => {
    const editor = await openEditor(page, 'abstract')
    await editor.clickAt('subtitle', 30)
    const leftFields = await watchSelection(editor)

    await page.keyboard.press('ArrowDown')
    expect((await editor.caret()).field).toBe('abstract')
    await page.keyboard.press('ArrowUp')
    expect((await editor.caret()).field).toBe('subtitle')
    expect(await leftFields()).toBe(false)
  })

  test('Right and Down at the end of the last field never move the caret', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('subtitle', 30)
    await page.keyboard.press('End')
    const leftFields = await watchSelection(editor)

    await page.keyboard.press('ArrowRight')
    await page.keyboard.press('ArrowDown')

    expect(await editor.caret()).toMatchObject({ field: 'subtitle', start: 13, end: 13 })
    expect(await leftFields()).toBe(false)
  })

  test('Shift+Down extends the selection into the abstract', async ({ page }) => {
    const editor = await openEditor(page, 'abstract')
    await editor.clickAt('subtitle', 30)
    await page.keyboard.press('Shift+ArrowDown')

    const selection = (await editor.selection())!
    expect(selection.anchor.field).toBe('subtitle')
    expect(selection.focus.field).toBe('abstract')
  })
})

test.describe('the abstract', () => {
  test('is editable, and Enter starts a new line in it', async ({ page }) => {
    const editor = await openEditor(page, 'abstract')
    await editor.frame.getByText('We summarise').click()
    await page.waitForTimeout(100)
    expect((await editor.caret()).field).toBe('abstract')

    await page.keyboard.press(`${'ControlOrMeta'}+ArrowDown`)
    await page.keyboard.press('Enter')
    await page.keyboard.type('More.')

    expect(await editor.text('abstract')).toBe(
      "We summarise the third quarter’s results and what they mean for the next one.\nMore.",
    )
    expect(await editor.caret()).toMatchObject({ field: 'abstract' })
  })

  test('Enter at the end moves the caret onto the new, empty line at once', async ({ page }) => {
    const editor = await openEditor(page, 'abstract')
    await editor.frame.getByText('We summarise').click()
    await page.keyboard.press(`${'ControlOrMeta'}+ArrowDown`)
    // A collapsed range at an element position (before the new line's
    // <br>) has no rectangle; measure the node after it instead.
    const caretTop = () =>
      editor.frame.evaluate(() => {
        const s = document.getSelection()!
        const rects = s.getRangeAt(0).getClientRects()
        if (rects.length > 0) return rects[0].top
        const next = s.focusNode!.childNodes[s.focusOffset] as Element
        return next.getBoundingClientRect().top
      })
    const field = (await editor.field('abstract').boundingBox())!
    const before = await caretTop()

    await page.keyboard.press('Enter')

    expect(await caretTop()).toBeGreaterThan(before + 10)
    expect((await editor.field('abstract').boundingBox())!.height).toBeGreaterThan(field.height + 10)
    expect(await editor.caret()).toMatchObject({ field: 'abstract' })
  })

  test('Enter at the end of the subtitle moves to the abstract', async ({ page }) => {
    const editor = await openEditor(page, 'abstract')
    await editor.clickAt('subtitle', 30)

    await page.keyboard.press('Enter')

    expect(await editor.caret()).toMatchObject({ field: 'abstract', start: 0, end: 0 })
  })
})

test.describe('the authors', () => {
  test('clicking an author leaves the caret there, and typing changes nothing', async ({ page }) => {
    const editor = await openEditor(page, 'abstract')
    const author = editor.frame.getByText('Ana Silva')
    await author.click()
    await page.waitForTimeout(100)

    expect((await editor.caret()).field).toBeNull()
    expect((await pageSelection(editor)).focusInField).toBe(false)
    await page.keyboard.type('x')
    expect(await author.textContent()).toBe('Ana Silva')
    expect(await editor.text('subtitle')).toBe('Third quarter')
  })

  // (Not by double-click: Chrome doesn't select words in non-editable
  // content inside an editable region.)
  test('clicking an author shows no caret', async ({ page }) => {
    const editor = await openEditor(page, 'abstract')
    await editor.frame.getByText('Ana Silva').click()
    await page.waitForTimeout(100)

    const caretColor = await editor.frame.evaluate(() => {
      const n = document.getSelection()!.focusNode!
      const el = n.nodeType === Node.ELEMENT_NODE ? (n as Element) : n.parentElement!
      return getComputedStyle(el).caretColor
    })
    expect(caretColor).toBe('rgba(0, 0, 0, 0)')
  })

  test('an arrow key after clicking an author goes straight to the nearest field that way', async ({ page }) => {
    const editor = await openEditor(page, 'abstract')
    const author = editor.frame.getByText('Ana Silva')
    await author.click()
    const leftFields = await watchSelection(editor)

    await page.keyboard.press('ArrowRight')
    expect(await editor.caret()).toMatchObject({ field: 'abstract', start: 0, end: 0 })

    await author.click()
    await page.keyboard.press('ArrowUp')
    expect(await editor.caret()).toMatchObject({ field: 'subtitle', start: 13, end: 13 })
    expect(await leftFields()).toBe(false)
  })

  test("an author's text can be selected by dragging", async ({ page }) => {
    const editor = await openEditor(page, 'abstract')
    const box = (await editor.frame.getByText('Example University').boundingBox())!
    await page.mouse.move(box.x + 2, box.y + box.height / 2)
    await page.mouse.down()
    await page.mouse.move(box.x + box.width - 2, box.y + box.height / 2, { steps: 5 })
    await page.mouse.up()
    await page.waitForTimeout(100)

    expect((await pageSelection(editor)).text).toBe('Example University')
  })

  test('dragging from the title through the authors keeps the selection, and typing edits only the fields', async ({
    page,
  }) => {
    const editor = await openEditor(page, 'abstract')
    const title = (await editor.field('title').boundingBox())!
    const author = (await editor.frame.getByText('Ana Silva').boundingBox())!
    await page.mouse.move(title.x + 60, title.y + title.height / 2)
    await page.mouse.down()
    for (let i = 1; i <= 5; i++) {
      await page.mouse.move(author.x + 20, title.y + ((author.y + author.height / 2 - title.y) * i) / 5)
      await page.waitForTimeout(50)
    }
    // Still dragging: the browser's selection is left alone.
    const during = await pageSelection(editor)
    expect(during.rangeCount).toBe(1)
    expect(during.focusInField).toBe(false)
    expect(during.text).toContain('Third quarter')

    await page.mouse.up()
    await page.waitForTimeout(100)
    expect(await pageSelection(editor)).toEqual(during)

    const { start } = await editor.caret()
    await page.keyboard.type('X')
    expect(await editor.text('title')).toBe('Quarterly report'.slice(0, start) + 'X')
    expect(await editor.text('subtitle')).toBeNull()
    expect(await editor.frame.getByText('Ana Silva').textContent()).toBe('Ana Silva')
    expect(await editor.text('abstract')).toMatch(/^We summarise/)
  })
})

test.describe('on load', () => {
  test('the caret starts at the start of the title', async ({ page }) => {
    const editor = await openEditor(page)
    await expect.poll(() => editor.caret()).toMatchObject({ field: 'title', start: 0, end: 0 })
  })
})

test.describe('macOS shortcuts', () => {
  test.skip(process.platform !== 'darwin', 'macOS key bindings')

  test('Ctrl+A goes to the start of the line, and Ctrl+E to the end', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)

    await page.keyboard.press('Control+a')
    expect(await editor.caret()).toMatchObject({ field: 'title', start: 0, end: 0 })
    await page.keyboard.press('Control+e')
    expect(await editor.caret()).toMatchObject({ field: 'title', start: 16, end: 16 })
  })
})
