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

test.describe('the rest of the title block', () => {
  test('clicking the author puts the caret in the nearest field, and the author never changes', async ({
    page,
  }) => {
    const editor = await openEditor(page)
    const author = editor.frame.getByText('Ana Silva')
    await author.click()

    expect(await editor.caret()).toMatchObject({ field: 'subtitle', start: 13, end: 13 })
    await page.keyboard.type('x')
    expect(await author.textContent()).toBe('Ana Silva')
  })
})
