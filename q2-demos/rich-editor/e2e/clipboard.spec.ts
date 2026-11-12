/**
 * The clipboard, and drag and drop. Copy takes the text of what's
 * selected, cut takes it and deletes it as any deletion would, and paste
 * puts plain text in. Nothing can be dragged from one place to another.
 */

import { expect, fixture, openEditor, test, type Editor } from './helpers.ts'

const mod = 'ControlOrMeta'
const basic = fixture('basic')
const body = fixture('body')

/** Select from "Quarterly |report" to "Third| quarter". */
async function selectAcrossFields(editor: Editor) {
  await editor.placeCaret('title', 10)
  for (let i = 0; i < 12; i++) await editor.page.keyboard.press('Shift+ArrowRight')
}

/** Select from "First |paragraph." to "Last| paragraph.", across the heading. */
async function selectAcrossHeading(editor: Editor) {
  await editor.placeCaretInParagraph(0, 6)
  await editor.shiftClickParagraph(2, 4)
}

test.describe('copy', () => {
  test('takes the selected text, and changes nothing', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.select('title', 0, 9)

    expect(await editor.copy()).toBe('Quarterly')

    expect(await editor.text('title')).toBe('Quarterly report')
    expect(await editor.selectedText()).toBe('Quarterly')
    expect(editor.document.source()).toBe(basic)
  })

  test('across fields, takes each field\'s text on a line of its own', async ({ page }) => {
    const editor = await openEditor(page)
    await selectAcrossFields(editor)

    expect(await editor.copy()).toBe('report\nThird')
  })

  test('across paragraphs, takes a line per paragraph', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 6)
    await editor.shiftClickParagraph(1, 6)

    expect(await editor.copy()).toBe('paragraph.\nSecond')
  })

  test('across what can\'t be edited, takes the text the page shows', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await selectAcrossHeading(editor)

    // The blank lines between blocks are the browser's.
    const lines = (await editor.copy()).split('\n').filter((line) => line !== '')
    expect(lines).toEqual(['paragraph.', 'Second paragraph.', 'Results', 'Some emphasised text.', 'Last'])
  })

  test('in the authors, takes the text the page shows', async ({ page }) => {
    const editor = await openEditor(page, 'abstract')
    const box = (await editor.frame.getByText('Example University').boundingBox())!
    await page.mouse.move(box.x + 2, box.y + box.height / 2)
    await page.mouse.down()
    await page.mouse.move(box.x + box.width - 2, box.y + box.height / 2, { steps: 5 })
    await page.mouse.up()
    await page.waitForTimeout(100)

    expect(await editor.copy()).toBe('Example University')
  })
})

test.describe('cut', () => {
  test('takes the selected text and deletes it, as one undo step', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.select('title', 9, 16)

    expect(await editor.copy('cut')).toBe(' report')

    expect(await editor.text('title')).toBe('Quarterly')
    await editor.expectCaret('title', 9)
    await editor.document.expectSource(basic.replace('Quarterly report', 'Quarterly'))

    await page.keyboard.press(`${mod}+z`)
    expect(await editor.text('title')).toBe('Quarterly report')
    await editor.document.expectSource(basic)
  })

  test('across fields trims each field and joins none', async ({ page }) => {
    const editor = await openEditor(page)
    await selectAcrossFields(editor)

    expect(await editor.copy('cut')).toBe('report\nThird')

    // (The subtitle shows what the source keeps of " quarter".)
    expect(await editor.text('title')).toBe('Quarterly ')
    expect(await editor.text('subtitle')).toBe('quarter')
    await editor.expectCaret('title', 10)
    await editor.document.expectSource(
      basic.replace('Quarterly report', 'Quarterly').replace('Third quarter', 'quarter'),
    )
  })

  test('across what can\'t be edited keeps it', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await selectAcrossHeading(editor)

    expect(await editor.copy('cut')).toContain('Results')

    expect(await editor.blocks()).toEqual([
      'p: First ',
      'h2: Results',
      'p: Some emphasised text.',
      'p: paragraph.',
    ])
    await editor.document.expectSource(
      body.replace('First paragraph.\n\nSecond paragraph.', 'First').replace('Last paragraph.', 'paragraph.'),
    )
  })
})

test.describe('paste', () => {
  test('replaces the selection, and leaves the caret after the pasted text', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.select('title', 0, 9)

    await editor.paste('Annual')

    expect(await editor.text('title')).toBe('Annual report')
    await editor.expectCaret('title', 6)
    await editor.document.expectSource(basic.replace('Quarterly report', 'Annual report'))
  })

  test('takes the plain text of formatted content', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 16)

    await editor.paste(' in bold', ' in <b>bold</b>')

    expect(await editor.text('title')).toBe('Quarterly report in bold')
    expect(await editor.field('title').innerHTML()).toBe('Quarterly report in bold')
    await editor.document.expectSource(basic.replace('Quarterly report', 'Quarterly report in bold'))
  })
})

test.describe('drag and drop', () => {
  test('dragging selected text somewhere else moves nothing', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.select('title', 0, 9)
    await editor.frame.evaluate(() => {
      const w = window as unknown as { __dragged: string[] }
      w.__dragged = []
      for (const type of ['dragstart', 'drop']) document.addEventListener(type, () => w.__dragged.push(type), true)
    })

    // Hold the selected text, then drag it into the subtitle.
    const title = (await editor.field('title').boundingBox())!
    const subtitle = (await editor.field('subtitle').boundingBox())!
    await page.mouse.move(title.x + 30, title.y + title.height / 2)
    await page.mouse.down()
    await page.waitForTimeout(700)
    await page.mouse.move(title.x + 36, title.y + title.height / 2 + 3, { steps: 3 })
    await page.mouse.move(subtitle.x + 40, subtitle.y + subtitle.height / 2, { steps: 8 })
    await page.waitForTimeout(100)
    await page.mouse.up()
    await page.waitForTimeout(300)

    // The text was dragged and dropped, and nothing came of it.
    expect(await editor.frame.evaluate(() => (window as unknown as { __dragged: string[] }).__dragged)).toEqual([
      'dragstart',
      'drop',
    ])
    expect(await editor.text('title')).toBe('Quarterly report')
    expect(await editor.text('subtitle')).toBe('Third quarter')
    expect(editor.document.source()).toBe(basic)
  })
})
