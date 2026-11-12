/**
 * When nothing can be edited: while the source doesn't parse, and in a
 * document that has no text the editor can edit. The document stays
 * readable (the caret moves, text can be selected and copied), and
 * nothing that would change it does anything.
 *
 * (The parts of an editable document that can't be edited are in
 * `body.spec.ts` and `cross-field.spec.ts`.)
 */

import { createDocument, expect, fixture, openEditor, test, type Editor } from './helpers.ts'

const mod = 'ControlOrMeta'
const basic = fixture('basic')
const broken = basic.replace('title: Quarterly report', 'title: "Quarterly report')

/** A collaborator leaves the title's quote open in the source. */
async function breakSource(editor: Editor) {
  editor.document.replace('title: Quarterly report', 'title: "Quarterly report')
  await expect(editor.page.locator('.visual-editor .status')).toContainText('Read-only')
}

test.describe('while the source does not parse', () => {
  test('the status line under the editor says why nothing can be edited', async ({ page }) => {
    const editor = await openEditor(page)
    await expect(page.locator('.visual-editor .status')).not.toContainText('Read-only')

    await breakSource(editor)

    await expect(page.locator('.visual-editor .status')).toContainText("Read-only: the source doesn't parse.")
  })

  test('the caret still moves, and text can be selected and copied', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 16)
    await breakSource(editor)

    await page.keyboard.press('ArrowLeft')
    await editor.expectCaret('title', 15)
    await page.keyboard.press('ArrowRight')
    for (let i = 0; i < 6; i++) await page.keyboard.press('Shift+ArrowLeft')

    expect(await editor.selectedText()).toBe('report')
    expect(await editor.copy()).toBe('report')
  })

  test('cut takes the text and deletes nothing', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.select('title', 0, 9)
    await breakSource(editor)

    expect(await editor.copy('cut')).toBe('Quarterly')

    expect(await editor.text('title')).toBe('Quarterly report')
    expect(editor.document.source()).toBe(broken)
  })

  test('pasting, Enter and Delete change nothing, and the caret stays', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 16)
    await breakSource(editor)

    await editor.paste(' 2026')
    await page.keyboard.press('Enter')
    await page.keyboard.press('Delete')

    await editor.expectCaret('title', 16)
    expect(await editor.text('title')).toBe('Quarterly report')
    expect(await editor.text('subtitle')).toBe('Third quarter')
    expect(editor.document.source()).toBe(broken)
  })

  test('undo and redo do nothing', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('subtitle', 13)
    await page.keyboard.type('!')
    await editor.document.expectSource(basic.replace('Third quarter', 'Third quarter!'))
    await breakSource(editor)

    await page.keyboard.press(`${mod}+z`)
    await page.keyboard.press(`${mod}+Shift+z`)

    expect(await editor.text('subtitle')).toBe('Third quarter!')
    expect(editor.document.source()).toBe(broken.replace('Third quarter', 'Third quarter!'))
  })

  test('what was typed meanwhile is not added once the source parses again', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 16)
    await breakSource(editor)
    await page.keyboard.type(' 2026')

    editor.document.replace('title: "Quarterly report', 'title: "Quarterly report"')
    await expect(page.locator('.visual-editor .status')).not.toContainText('Read-only')

    expect(await editor.text('title')).toBe('Quarterly report')
    expect(editor.document.source()).toBe(basic.replace('title: Quarterly report', 'title: "Quarterly report"'))
  })
})

test.describe('a document with no text that can be edited', () => {
  const source = '## Results\n\nSome *emphasised* text.\n'

  test('is shown, with no title block', async ({ page }) => {
    const editor = await (await createDocument(source)).open(page)

    expect(await editor.blocks()).toEqual(['h2: Results', 'p: Some emphasised text.'])
    await expect(editor.frame.locator('#title-block-header')).toHaveCount(0)
  })

  test('takes no edits', async ({ page }) => {
    const document = await createDocument(source)
    const editor = await document.open(page)

    await editor.frame.locator('main h2').click()
    await page.keyboard.type('x')
    await page.keyboard.press('Enter')
    await editor.frame.locator('main p').click()
    await page.keyboard.type('x')

    expect(await editor.blocks()).toEqual(['h2: Results', 'p: Some emphasised text.'])
    expect(document.source()).toBe(source)
  })
})
