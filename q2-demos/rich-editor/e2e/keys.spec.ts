/**
 * Moving and deleting by more than a character: a word at a time and, on
 * a Mac, to the start of the line. What Left and Right do with a
 * selection, what Page Up and Page Down do, and the other macOS key
 * bindings for editing text.
 *
 * The word modifier is Option on a Mac and Ctrl elsewhere (`wordKey`).
 */

import { expect, fixture, openEditor, test, wordKey, lineEnd } from './helpers.ts'

const mod = 'ControlOrMeta'
const basic = fixture('basic')
const body = fixture('body')

test.describe('moving by word', () => {
  test('the word keys move the caret a word at a time', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 0)

    await page.keyboard.press(`${wordKey}+ArrowRight`)
    await editor.expectCaret('title', 9)
    await page.keyboard.press(`${wordKey}+ArrowRight`)
    await editor.expectCaret('title', 16)

    await page.keyboard.press(`${wordKey}+ArrowLeft`)
    await editor.expectCaret('title', 10)
    await page.keyboard.press(`${wordKey}+ArrowLeft`)
    await editor.expectCaret('title', 0)
  })

  // Where in the next field depends on the browser: Chromium stops at its
  // edge, Firefox and WebKit after its first word.
  test('past the end of a field they go into the next field, and back', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 16)

    await page.keyboard.press(`${wordKey}+ArrowRight`)
    expect((await editor.caret()).field).toBe('subtitle')

    await editor.placeCaret('subtitle', 0)
    await page.keyboard.press(`${wordKey}+ArrowLeft`)
    expect((await editor.caret()).field).toBe('title')
  })

  test('with Shift they select a word at a time', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 16)

    await page.keyboard.press(`Shift+${wordKey}+ArrowLeft`)
    expect(await editor.selectedText()).toBe('report')

    await page.keyboard.type('review')
    expect(await editor.text('title')).toBe('Quarterly review')
  })
})

test.describe('deleting by word', () => {
  test('word-Backspace deletes the word before the caret', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 16)

    await page.keyboard.press(`${wordKey}+Backspace`)

    expect(await editor.text('title')).toBe('Quarterly ')
    await editor.expectCaret('title', 10)
    await editor.document.expectSource(basic.replace('Quarterly report', 'Quarterly'))
  })

  test('word-Delete deletes the word after the caret', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 10)

    await page.keyboard.press(`${wordKey}+Delete`)

    expect(await editor.text('title')).toBe('Quarterly ')
    await editor.expectCaret('title', 10)
  })

  test('at the start of a field, word-Backspace goes to the field before without deleting', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('subtitle', 0)

    await page.keyboard.press(`${wordKey}+Backspace`)

    await editor.expectCaret('title', 16)
    expect(await editor.text('title')).toBe('Quarterly report')
    expect(await editor.text('subtitle')).toBe('Third quarter')
    expect(editor.document.source()).toBe(basic)
  })

  test('at the start of a paragraph, word-Backspace joins it to the one before, as Backspace does', async ({
    page,
  }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 0)

    await page.keyboard.press(`${wordKey}+Backspace`)

    expect(await editor.paragraphs()).toEqual(['First paragraph.Second paragraph.', 'Last paragraph.'])
    await editor.document.expectSource(
      body.replace('First paragraph.\n\nSecond paragraph.', 'First paragraph.Second paragraph.'),
    )
  })

  test('at the end of a paragraph before what can\'t be edited, word-Delete deletes nothing', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 17)

    await page.keyboard.press(`${wordKey}+Delete`)

    expect(editor.document.source()).toBe(body)
  })

  test('a run of word deletions is one undo step', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 16)
    await page.keyboard.press(`${wordKey}+Backspace`)
    await page.keyboard.press(`${wordKey}+Backspace`)
    expect(await editor.text('title')).toBe('')

    await page.keyboard.press(`${mod}+z`)

    expect(await editor.text('title')).toBe('Quarterly report')
  })
})

test.describe('deleting to the start of the line', () => {
  test.skip(process.platform !== 'darwin', 'Cmd+Backspace is a macOS key binding')

  test('Cmd+Backspace deletes from the start of the line to the caret', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('subtitle', 6)

    await page.keyboard.press('Meta+Backspace')

    expect(await editor.text('subtitle')).toBe('quarter')
    await editor.expectCaret('subtitle', 0)
    await editor.document.expectSource(basic.replace('Third quarter', 'quarter'))
  })

  test('in a paragraph it stops at the start of the paragraph', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 7)

    await page.keyboard.press('Meta+Backspace')

    expect(await editor.paragraphs()).toEqual(['First paragraph.', 'paragraph.', 'Last paragraph.'])
  })
})

test.describe('Left and Right with a selection', () => {
  test('Right puts the caret at the end of the selection, and Left at its start', async ({ page }) => {
    const editor = await openEditor(page)

    await editor.select('title', 3, 8)
    await page.keyboard.press('ArrowRight')
    await editor.expectCaret('title', 8)

    await editor.select('title', 3, 8)
    await page.keyboard.press('ArrowLeft')
    await editor.expectCaret('title', 3)
  })

  test('whichever way the selection was made', async ({ page }) => {
    const editor = await openEditor(page)
    const selectBackward = async () => {
      await editor.placeCaret('title', 8)
      for (let i = 0; i < 5; i++) await page.keyboard.press('Shift+ArrowLeft')
    }

    await selectBackward()
    await page.keyboard.press('ArrowRight')
    await editor.expectCaret('title', 8)

    await selectBackward()
    await page.keyboard.press('ArrowLeft')
    await editor.expectCaret('title', 3)
  })

  test('across fields too', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 10)
    for (let i = 0; i < 12; i++) await page.keyboard.press('Shift+ArrowRight')

    await page.keyboard.press('ArrowLeft')
    await editor.expectCaret('title', 10)

    await page.keyboard.press(lineEnd)
    await editor.placeCaret('title', 10)
    for (let i = 0; i < 12; i++) await page.keyboard.press('Shift+ArrowRight')
    await page.keyboard.press('ArrowRight')
    await editor.expectCaret('subtitle', 5)
  })
})

test.describe('Page Up and Page Down', () => {
  test.skip(process.platform !== 'darwin', 'On a Mac they scroll; elsewhere they move the caret a page')

  test('scroll, and leave the caret where it is', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 4)

    await page.keyboard.press('PageDown')
    await editor.expectCaret('title', 4)
    await page.keyboard.press('PageUp')
    await editor.expectCaret('title', 4)

    await page.keyboard.type('x')
    expect(await editor.text('title')).toBe('Quarxterly report')
  })
})

test.describe('macOS key bindings', () => {
  test.skip(process.platform !== 'darwin', 'macOS key bindings')

  test('Option+Up and Option+Down go to the start and the end of the field', async ({ page, browserName }) => {
    test.skip(browserName === 'firefox', "Playwright's Firefox ignores macOS key bindings")
    const editor = await openEditor(page)
    await editor.placeCaret('subtitle', 4)

    await page.keyboard.press('Alt+ArrowDown')
    await editor.expectCaret('subtitle', 13)
    await page.keyboard.press('Alt+ArrowUp')
    await editor.expectCaret('subtitle', 0)
  })

  test('Ctrl+K deletes to the end of the line', async ({ page, browserName }) => {
    test.skip(browserName === 'firefox', "Playwright's Firefox ignores macOS key bindings")
    const editor = await openEditor(page)
    await editor.placeCaret('title', 4)

    await page.keyboard.press('Control+k')

    expect(await editor.text('title')).toBe('Quar')
    await editor.document.expectSource(basic.replace('Quarterly report', 'Quar'))
  })

  test('Ctrl+D deletes the character after the caret, and Ctrl+H the one before', async ({ page, browserName }) => {
    test.skip(browserName === 'firefox', "Playwright's Firefox ignores macOS key bindings")
    const editor = await openEditor(page)
    await editor.placeCaret('title', 4)

    await page.keyboard.press('Control+d')
    expect(await editor.text('title')).toBe('Quarerly report')
    await editor.expectCaret('title', 4)

    await page.keyboard.press('Control+h')
    expect(await editor.text('title')).toBe('Quaerly report')
    await editor.expectCaret('title', 3)
  })

  // Today it does nothing.
  test.fixme('Ctrl+T swaps the characters on either side of the caret', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 4)

    await page.keyboard.press('Control+t')

    expect(await editor.text('title')).toBe('Quatrerly report')
    await editor.document.expectSource(basic.replace('Quarterly report', 'Quatrerly report'))
  })
})
