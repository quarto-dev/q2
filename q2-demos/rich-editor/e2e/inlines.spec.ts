/**
 * Formatting in the body's text: bold, italic, underline, strikethrough,
 * highlight, code and links (links have `links.spec.ts`), in paragraphs,
 * headings, quotes and list items, shown as `q2 render` shows them and
 * edited in place. (The design is `mockups/inlines.html`, with the choices
 * "Arrow out of code", "Converts as you type" and "Popover".)
 *
 * Typing at the end of bold or italic carries it on, and at its start
 * doesn't. Code has an inside and an outside at each edge: Right at the end
 * of code steps out of it without moving on, so what's typed next is
 * outside it. A caret put somewhere by a click or the browser takes the
 * formats of the character before it.
 *
 * ⌘B, ⌘I, ⌘U, ⌘⇧X, ⌘E and ⌘⇧H (Ctrl elsewhere) turn bold, italic,
 * underline, strikethrough, code and highlight on and off: over the
 * selection, or, with nothing selected, for what's typed next. So do the
 * toolbar's buttons for bold, italic, underline and highlight. Markdown
 * typed out and closed (`**bold**`, `*italic*`, `__bold__`, `_italic_`,
 * `~~strikethrough~~`, `` `code` ``, `[text](url)`) becomes formatting;
 * Backspace right after gives the characters back.
 */

import { createDocument, expect, fixture, lineEnd, lineStart, openEditor, test } from './helpers.ts'

const mod = 'ControlOrMeta'
const inlines = fixture('inlines')
const FIRST = 'Some bold text, italic text, underlined, struck, highlighted, code and a link.'
const at = (text: string) => FIRST.indexOf(text)

test.describe('showing formatting', () => {
  test('a paragraph with formatting can be edited, and shows it as `q2 render` does', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')

    expect(await editor.lines()).toEqual([`p: ${FIRST}`, 'p: Code at the end: q2 render', 'p: Plain words.'])
    const line = editor.line(0)
    await expect(line.locator('strong')).toHaveText('bold text')
    await expect(line.locator('em')).toHaveText('italic text')
    await expect(line.locator('u')).toHaveText('underlined')
    await expect(line.locator('del')).toHaveText('struck')
    await expect(line.locator('span.mark')).toHaveText('highlighted')
    await expect(line.locator('code')).toHaveText('code')
    await expect(line.locator('a')).toHaveText('a link')
    await expect(line.locator('a')).toHaveAttribute('href', 'https://quarto.org')
  })

  test('headings, quotes and list items can be formatted too', async ({ page }) => {
    const editor = await (
      await createDocument('---\ntitle: T\n---\n\n## A **bold** heading\n\n> A *quoted* word\n\n* An `item`\n')
    ).open(page)

    expect(await editor.lines()).toEqual(['h2: A bold heading', 'blockquote: A quoted word', 'li: An item'])
    await expect(editor.line(0).locator('strong')).toHaveText('bold')
    await expect(editor.line(1).locator('em')).toHaveText('quoted')
    await expect(editor.line(2).locator('code')).toHaveText('item')
  })

  test("a paragraph with math, a citation, a note or an image can't be edited yet", async ({ page }) => {
    const editor = await (
      await createDocument('---\ntitle: T\n---\n\nArea $x^2$.\n\nAs @knuth said.\n\nA note.^[Hidden.]\n\nA ![picture](p.png).\n\nPlain.\n')
    ).open(page)

    expect(await editor.lines()).toEqual(['p: Plain.'])
  })
})

test.describe('typing at the edge of formatting', () => {
  test("at the end of bold carries it on, and at its start doesn't", async ({ page }) => {
    const editor = await openEditor(page, 'inlines')

    await editor.placeCaretInLine(0, at('bold text') + 'bold text'.length)
    await page.keyboard.type('ed')
    await editor.document.expectSource(inlines.replace('**bold text**', '**bold texted**'))

    await editor.placeCaretInLine(0, at('bold text'))
    await page.keyboard.type('x')
    await editor.document.expectSource(inlines.replace('**bold text**', 'x**bold texted**'))
  })

  test('at the end of code goes in it; Right first steps out of it, without moving on', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    // A click after the code puts the caret inside its end.
    const box = (await editor.line(0).locator('code').boundingBox())!
    await page.mouse.click(box.x + box.width + 1, box.y + box.height / 2)
    await editor.expectLineCaret(0, at('code') + 4)

    await page.keyboard.type('s')
    await editor.document.expectSource(inlines.replace('`code`', '`codes`'))

    await page.keyboard.press('ArrowRight')
    await editor.expectLineCaret(0, at('code') + 5)
    await page.keyboard.type(',')
    await editor.document.expectSource(inlines.replace('`code`', '`codes`,'))
    await expect(editor.line(0).locator('code')).toHaveText('codes')
  })

  test('at the end of a line, Right steps out of code too', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(1, 0)
    await page.keyboard.press(lineEnd)

    await page.keyboard.press('ArrowRight')
    await page.keyboard.type('!')

    await editor.document.expectSource(inlines.replace('`q2 render`', '`q2 render`!'))
    await expect(editor.line(1).locator('code')).toHaveText('q2 render')
    expect((await editor.lines())[1]).toBe('p: Code at the end: q2 render!')
  })

  test('at the start of code is outside it, and Right steps inside', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(0, at('code'))

    await page.keyboard.type('(')
    await editor.document.expectSource(inlines.replace('`code`', '(`code`'))

    await page.keyboard.press('ArrowRight')
    await page.keyboard.type('x')
    await editor.document.expectSource(inlines.replace('`code`', '(`xcode`'))
  })

  test('Left at the start of code steps out of it, and Right back in', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(1, 0)
    await page.keyboard.press(lineEnd)
    for (let i = 0; i < 'q2 render'.length; i++) await page.keyboard.press('ArrowLeft')

    // At the start of the code, inside it: Left steps out of it.
    await page.keyboard.press('ArrowLeft')
    await page.keyboard.type('(')
    await editor.document.expectSource(inlines.replace('`q2 render`', '(`q2 render`'))
    await page.keyboard.press('ArrowRight')
    await page.keyboard.type('$')
    await editor.document.expectSource(inlines.replace('`q2 render`', '(`$q2 render`'))
  })
})

test.describe('formatting from the keyboard', () => {
  test('⌘B with nothing selected makes what is typed next bold, and again plain', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(2, 12)

    await page.keyboard.press(`${mod}+b`)
    await page.keyboard.type(' bold')
    await page.keyboard.press(`${mod}+b`)
    await page.keyboard.type(' plain')

    await editor.document.expectSource(inlines.replace('Plain words.', 'Plain words. **bold** plain'))
    await expect(editor.line(2).locator('strong')).toHaveText('bold')
  })

  test('⌘B over a selection makes it bold, and again plain, keeping the selection', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(2, 0)
    for (let i = 0; i < 5; i++) await page.keyboard.press('Shift+ArrowRight')

    await page.keyboard.press(`${mod}+b`)
    await editor.document.expectSource(inlines.replace('Plain words.', '**Plain** words.'))
    expect(await editor.selectedText()).toBe('Plain')

    await page.keyboard.press(`${mod}+b`)
    await editor.document.expectSource(inlines)
    expect(await editor.selectedText()).toBe('Plain')
  })

  test('the other shortcuts make italic, underline, strikethrough, code and highlight', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    const select = async () => {
      await editor.placeCaretInLine(2, 6)
      for (let i = 0; i < 5; i++) await page.keyboard.press('Shift+ArrowRight')
    }
    for (const [keys, written] of [
      [`${mod}+i`, '*words*'],
      [`${mod}+u`, '[words]{.underline}'],
      [`${mod}+Shift+x`, '~~words~~'],
      [`${mod}+e`, '`words`'],
      [`${mod}+Shift+h`, '[words]{.mark}'],
    ]) {
      await select()
      await page.keyboard.press(keys)
      await editor.document.expectSource(inlines.replace('Plain words.', `Plain ${written}.`))
      await page.keyboard.press(keys)
      await editor.document.expectSource(inlines)
    }
  })

  test('Enter at the end of bold carries it on to the line it starts', async ({ page }) => {
    const document = await createDocument('---\ntitle: T\n---\n\nSome **bold**\n')
    const editor = await document.open(page)
    await editor.placeCaretInLine(0, 9)

    await page.keyboard.press('Enter')
    await page.keyboard.type('more')

    await document.expectSource('---\ntitle: T\n---\n\nSome **bold**\n\n**more**\n')
  })

  test('undo takes formatting back, and redo makes it again', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(2, 0)
    for (let i = 0; i < 5; i++) await page.keyboard.press('Shift+ArrowRight')
    await page.keyboard.press(`${mod}+b`)
    await editor.document.expectSource(inlines.replace('Plain words.', '**Plain** words.'))

    await page.keyboard.press(`${mod}+z`)
    await editor.document.expectSource(inlines)
    await page.keyboard.press(`${mod}+Shift+z`)
    await editor.document.expectSource(inlines.replace('Plain words.', '**Plain** words.'))
  })
})

test.describe('the toolbar', () => {
  test('has bold, italic, underline and highlight after the style box, and link after them', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')

    const names = await editor.toolbar().getByRole('button').evaluateAll((buttons) => buttons.map((b) => b.getAttribute('aria-label')))
    expect(names).toEqual(['Undo', 'Redo', 'Text style', 'Bold', 'Italic', 'Underline', 'Highlight', 'Link', 'Bulleted list', 'Numbered list', 'Quote'])
  })

  test("its format buttons are pressed for the caret's formats", async ({ page }) => {
    const editor = await openEditor(page, 'inlines')

    await editor.placeCaretInLine(0, at('bold text') + 2)
    await expect(editor.formatButton('Bold')).toHaveAttribute('aria-pressed', 'true')
    await expect(editor.formatButton('Italic')).toHaveAttribute('aria-pressed', 'false')

    await editor.placeCaretInLine(0, at('highlighted') + 2)
    await expect(editor.formatButton('Highlight')).toHaveAttribute('aria-pressed', 'true')
    await expect(editor.formatButton('Bold')).toHaveAttribute('aria-pressed', 'false')
  })

  test('its format buttons turn formats on and off over the selection', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(2, 0)
    for (let i = 0; i < 5; i++) await page.keyboard.press('Shift+ArrowRight')

    await editor.formatButton('Bold').click()
    await editor.document.expectSource(inlines.replace('Plain words.', '**Plain** words.'))
    await editor.formatButton('Highlight').click()
    await editor.document.expectSource(inlines.replace('Plain words.', '**[Plain]{.mark}** words.'))
    await expect(editor.formatButton('Highlight')).toHaveAttribute('aria-pressed', 'true')
    await editor.formatButton('Bold').click()
    await editor.formatButton('Highlight').click()
    await editor.document.expectSource(inlines)
    await editor.formatButton('Underline').click()
    await editor.document.expectSource(inlines.replace('Plain words.', '[Plain]{.underline} words.'))
    // The caret stays in the document, on the same text.
    expect(await editor.selectedText()).toBe('Plain')
  })

  test("in the title block, they're disabled", async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaret('title', 2)

    await expect(editor.formatButton('Bold')).toBeDisabled()
    await expect(editor.linkButton()).toBeDisabled()
  })
})

test.describe('typing markdown', () => {
  test('bold, italic, strikethrough and code become formatting when they are closed', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(2, 12)

    await page.keyboard.type(' **b** *i* __B__ _I_ ~~s~~ `c` after')

    await editor.document.expectSource(inlines.replace('Plain words.', 'Plain words. **b** *i* **B** *I* ~~s~~ `c` after'))
    expect((await editor.lines())[2]).toBe('p: Plain words. b i B I s c after')
    await expect(editor.line(2).locator('strong')).toHaveText(['b', 'B'])
    await expect(editor.line(2).locator('code')).toHaveText('c')
  })

  test('a link typed out becomes a link', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(2, 12)

    await page.keyboard.type(' [Quarto](https://quarto.org) site')

    await editor.document.expectSource(inlines.replace('Plain words.', 'Plain words. [Quarto](https://quarto.org) site'))
    await expect(editor.line(2).locator('a')).toHaveText('Quarto')
  })

  test('Backspace right after gives the characters back', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(2, 12)
    await page.keyboard.type(' **b**')
    await expect(editor.line(2).locator('strong')).toHaveText('b')

    await page.keyboard.press('Backspace')

    expect((await editor.lines())[2]).toBe('p: Plain words. **b**')
    await expect(editor.line(2).locator('strong')).toHaveCount(0)
    await editor.document.expectSource(inlines.replace('Plain words.', 'Plain words. \\*\\*b\\*\\*'))
  })

  test('an underscore inside a word is typed as text', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(2, 12)

    await page.keyboard.type(' snake_case_name')

    expect((await editor.lines())[2]).toBe('p: Plain words. snake_case_name')
    await expect(editor.line(2).locator('em')).toHaveCount(0)
  })

  test('markdown typed in code stays as typed', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(1, 0)
    await page.keyboard.press(lineEnd)

    await page.keyboard.type(' **x**')

    await expect(editor.line(1).locator('code')).toHaveText('q2 render **x**')
  })
})

test.describe('deleting', () => {
  test('Backspace into bold, then typing, types bold', async ({ page }) => {
    const document = await createDocument('---\ntitle: T\n---\n\n**Bold** x\n')
    const editor = await document.open(page)
    await editor.placeCaretInLine(0, 6)

    await page.keyboard.press('Backspace')
    await page.keyboard.press('Backspace')
    await page.keyboard.type('er')

    await document.expectSource('---\ntitle: T\n---\n\n**Bolder**\n')
  })

  test('deleting the last character of bold leaves bold on for what is typed next', async ({ page }) => {
    const document = await createDocument('---\ntitle: T\n---\n\nSome **b** words\n')
    const editor = await document.open(page)
    await editor.placeCaretInLine(0, 6)

    await page.keyboard.press('Backspace')
    await page.keyboard.type('x')

    await document.expectSource('---\ntitle: T\n---\n\nSome **x** words\n')
  })

  test('deleting a selection across formatting keeps the formatting of what is left', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(0, at('bold text') + 4)
    for (let i = 0; i < ' text, italic'.length; i++) await page.keyboard.press('Shift+ArrowRight')

    await page.keyboard.press('Backspace')

    await editor.document.expectSource(inlines.replace('**bold text**, *italic text*', '**bold** *text*'))
  })
})

test.describe('the clipboard', () => {
  test('copy takes the plain text', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(0, at('bold text'))
    for (let i = 0; i < 'bold text, italic'.length; i++) await page.keyboard.press('Shift+ArrowRight')

    expect(await editor.copy()).toBe('bold text, italic')
  })

  test("pasted text takes the caret's formats", async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(0, at('bold text') + 4)

    await editor.paste(' and')

    await editor.document.expectSource(inlines.replace('**bold text**', '**bold and text**'))
  })
})

test.describe('with collaborators', () => {
  test('someone formats a word in the source, and it shows formatted, my caret where it was', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    await editor.placeCaretInLine(2, 12)

    editor.document.replace('Plain words.', '**Plain** words.')

    await expect(editor.line(2).locator('strong')).toHaveText('Plain')
    await editor.expectLineCaret(2, 12)
    await page.keyboard.type('!')
    await editor.document.expectSource(inlines.replace('Plain words.', '**Plain** words.!'))
  })

  test('formatting I make shows for someone else', async ({ page }) => {
    const editor = await openEditor(page, 'inlines')
    const other = await editor.join()
    await editor.placeCaretInLine(2, 0)
    for (let i = 0; i < 5; i++) await page.keyboard.press('Shift+ArrowRight')

    await page.keyboard.press(`${mod}+i`)

    await expect(other.line(2).locator('em')).toHaveText('Plain')
  })
})

test('Home and End keep to the line, past its formatting', async ({ page }) => {
  const editor = await openEditor(page, 'inlines')
  await editor.placeCaretInLine(0, 3)

  await page.keyboard.press(lineEnd)
  await editor.expectLineCaret(0, FIRST.length)
  await page.keyboard.press(lineStart)
  await editor.expectLineCaret(0, 0)
})
