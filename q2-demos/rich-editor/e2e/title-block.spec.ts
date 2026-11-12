/**
 * Editing the title and subtitle in the visual editor: the caret, Enter,
 * the arrow keys, undo, adding and removing fields, and what an empty field
 * shows. What the edits do to the source text is in `source.spec.ts`.
 */

import { createDocument, expect, fixture, openEditor, test, lineStart, lineEnd } from './helpers.ts'

const mod = 'ControlOrMeta'
const basic = fixture('basic')

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

  test('nor has the page around it, and its text is not spell-checked', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)

    // What has the focus is the whole document, not the field.
    const surface = await editor.frame.evaluate(() => {
      const el = document.activeElement as HTMLElement
      return { tag: el.tagName, outline: getComputedStyle(el).outlineStyle, spellcheck: el.spellcheck }
    })
    expect(surface).toEqual({ tag: 'MAIN', outline: 'none', spellcheck: false })
  })

  test('a double click selects a word', async ({ page }) => {
    const editor = await openEditor(page)
    const box = (await editor.field('title').boundingBox())!

    await editor.field('title').dblclick({ position: { x: 30, y: box.height / 2 } })

    // (With the space after it, on some systems.)
    expect((await editor.selectedText()).trim()).toBe('Quarterly')
  })

  test('a triple click selects the field\'s text, and typing replaces it and nothing else', async ({ page }) => {
    const editor = await openEditor(page)
    const box = (await editor.field('title').boundingBox())!

    await editor.field('title').click({ clickCount: 3, position: { x: 30, y: box.height / 2 } })
    expect((await editor.selectedText()).trim()).toBe('Quarterly report')
    await page.keyboard.type('Annual')

    expect(await editor.text('title')).toBe('Annual')
    expect(await editor.text('subtitle')).toBe('Third quarter')
    await editor.document.expectSource(basic.replace('Quarterly report', 'Annual'))
  })
})

test.describe('typing', () => {
  test('typing updates the field as you type', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press(lineEnd)
    await page.keyboard.type(' (draft)')

    expect(await editor.text('title')).toBe('Quarterly report (draft)')
    expect(await editor.caret()).toMatchObject({ field: 'title', start: 24, end: 24 })
  })

  test('Backspace deletes a whole character, however it is encoded', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press(lineEnd)
    // A thumbs-up with a skin tone: two code points, four UTF-16 units.
    await page.keyboard.insertText(' 👍🏽')
    expect(await editor.text('title')).toBe('Quarterly report 👍🏽')

    await page.keyboard.press('Backspace')

    expect(await editor.text('title')).toBe('Quarterly report ')
  })

  test('Delete deletes the character after the caret, a whole one too', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 9)
    await page.keyboard.insertText('👍🏽')
    await editor.placeCaret('title', 9)

    await page.keyboard.press('Delete')
    expect(await editor.text('title')).toBe('Quarterly report')
    await editor.expectCaret('title', 9)

    await page.keyboard.press('Delete')
    expect(await editor.text('title')).toBe('Quarterlyreport')
    await editor.expectCaret('title', 9)
  })

  test('Tab types nothing', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 9)

    await page.keyboard.press('Tab')

    expect(await editor.text('title')).toBe('Quarterly report')
    expect(editor.document.source()).toBe(basic)
  })

  test('the keys for bold and italic format nothing', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.select('title', 0, 9)

    await page.keyboard.press(`${mod}+b`)
    await page.keyboard.press(`${mod}+i`)

    expect(await editor.field('title').innerHTML()).toBe('Quarterly report')
    expect(await editor.selectedText()).toBe('Quarterly')
    expect(editor.document.source()).toBe(basic)
  })

  test('a correction from the system (autocorrect, a spelling suggestion) replaces the text it is for', async ({
    page,
    browserName,
  }) => {
    test.skip(browserName === 'webkit', "WebKit can't make the event a correction arrives as")
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press(lineEnd)

    // The system replaces "Quarterly" with "Yearly", wherever the caret is.
    await editor.field('title').evaluate((el) => {
      const text = el.firstChild!
      const range = new StaticRange({ startContainer: text, startOffset: 0, endContainer: text, endOffset: 9 })
      document.activeElement!.dispatchEvent(
        new InputEvent('beforeinput', {
          inputType: 'insertReplacementText',
          data: 'Yearly',
          targetRanges: [range],
          bubbles: true,
          cancelable: true,
        }),
      )
    })

    expect(await editor.text('title')).toBe('Yearly report')
    await editor.document.expectSource(fixture('basic').replace('Quarterly report', 'Yearly report'))
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
    await page.keyboard.press(lineEnd)
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

  test('Enter with a selection deletes it and moves to the next field', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.select('title', 9, 16)

    await page.keyboard.press('Enter')

    expect(await editor.text('title')).toBe('Quarterly')
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

  test('a new subtitle left empty goes away when the editor loses the focus, too', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.clickAt('title', 60)
    await page.keyboard.press('Enter')
    expect(await editor.text('subtitle')).toBe('')

    await editor.clickAway()

    await editor.expectText('subtitle', null)
    expect((await editor.caret()).field).toBeNull()
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
    await page.keyboard.press(lineEnd)

    await page.keyboard.press('ArrowRight')

    expect(await editor.caret()).toMatchObject({ field: 'subtitle', start: 0, end: 0 })
  })

  test('Left at the start of the subtitle goes to the end of the title', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('subtitle', 30)
    await page.keyboard.press(lineStart)

    await page.keyboard.press('ArrowLeft')

    expect(await editor.caret()).toMatchObject({ field: 'title', start: 16, end: 16 })
  })

  test('Right at the end of the document stays put', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaretInParagraph(0, 10)

    await page.keyboard.press('ArrowRight')

    await editor.expectParagraphCaret(0, 10)
  })

  test('Left and Backspace at the start of the document stay put, and delete nothing', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 0)

    await page.keyboard.press('ArrowLeft')
    await editor.expectCaret('title', 0)
    await page.keyboard.press('Backspace')
    await editor.expectCaret('title', 0)

    expect(await editor.text('title')).toBe('Quarterly report')
    expect(editor.document.source()).toBe(basic)
  })
})

test.describe('undo', () => {
  test('typing and then deleting are separate steps', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press(lineEnd)
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
    await page.keyboard.press(lineEnd)
    await page.keyboard.type('!')
    await page.keyboard.press(`${mod}+z`)

    await page.keyboard.press(`${mod}+Shift+z`)

    expect(await editor.text('title')).toBe('Quarterly report!')
    expect(await editor.caret()).toMatchObject({ field: 'title', start: 17 })
  })

  test('typing in two places is two steps', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press(lineEnd)
    await page.keyboard.type('X')
    await page.keyboard.press(lineStart)
    await page.keyboard.type('Y')

    await page.keyboard.press(`${mod}+z`)

    expect(await editor.text('title')).toBe('Quarterly reportX')
  })

  test('typing after a pause of a second is a new step', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press(lineEnd)
    await page.keyboard.type(' one')
    await page.waitForTimeout(1200)
    await page.keyboard.type(' two')

    await page.keyboard.press(`${mod}+z`)
    expect(await editor.text('title')).toBe('Quarterly report one')
    await page.keyboard.press(`${mod}+z`)
    expect(await editor.text('title')).toBe('Quarterly report')
  })

  test('an edit after an undo leaves nothing to redo', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press(lineEnd)
    await page.keyboard.type(' one')
    await page.keyboard.press(`${mod}+z`)
    await page.keyboard.type('!')

    await page.keyboard.press(`${mod}+Shift+z`)

    expect(await editor.text('title')).toBe('Quarterly report!')
  })

  test('a paste is its own step', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press(lineEnd)
    await page.keyboard.type(' a')
    await editor.paste('b')
    expect(await editor.text('title')).toBe('Quarterly report ab')

    await page.keyboard.press(`${mod}+z`)

    expect(await editor.text('title')).toBe('Quarterly report a')
  })

  test('undo with no field focused restores the text and puts the caret back', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press(lineEnd)
    await page.keyboard.type(' (draft)')
    await editor.clickAway()
    expect((await editor.caret()).field).toBeNull()

    await page.keyboard.press(`${mod}+z`)

    expect(await editor.text('title')).toBe('Quarterly report')
    expect(await editor.caret()).toMatchObject({ field: 'title', start: 16, end: 16 })
  })

  test('elsewhere than on a Mac, the keys are Ctrl+Z to undo, and Ctrl+Y or Ctrl+Shift+Z to redo', async ({
    page,
  }) => {
    // The editor goes by the system the browser says it is on.
    await page.addInitScript(() => Object.defineProperty(navigator, 'platform', { get: () => 'Win32' }))
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press(lineEnd)
    await page.keyboard.type('!')

    await page.keyboard.press('Control+z')
    expect(await editor.text('title')).toBe('Quarterly report')
    await page.keyboard.press('Control+y')
    expect(await editor.text('title')).toBe('Quarterly report!')
    await page.keyboard.press('Control+z')
    await page.keyboard.press('Control+Shift+z')
    expect(await editor.text('title')).toBe('Quarterly report!')
  })

  test('undo in one field can undo an edit in another', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press(lineEnd)
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

  test('the title goes away like any other field, and the rest of the title block stays', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.clickAt('title', 60)
    await page.keyboard.press(`${mod}+a`)
    await page.keyboard.press('Backspace')

    await editor.clickAt('subtitle', 30)

    expect(await editor.text('title')).toBeNull()
    expect(await editor.text('subtitle')).toBe('Third quarter')
    await expect(editor.frame.locator('#title-block-header')).toContainText('Ana Silva')
    await editor.document.expectSource(basic.replace('title: Quarterly report\n', ''))
  })
})

test.describe('an empty field', () => {
  const NAMES = { title: 'Title', subtitle: 'Subtitle', abstract: 'Abstract' } as const

  for (const field of ['title', 'subtitle', 'abstract'] as const) {
    test(`an emptied ${field} shows “${NAMES[field]}”, faintly, until something is typed`, async ({ page }) => {
      const editor = await openEditor(page, 'abstract')
      const placeholder = () =>
        editor.field(field).evaluate((el) => {
          const s = getComputedStyle(el, '::before')
          return { content: s.content, color: s.color, position: s.position }
        })
      const left = (await editor.field(field).boundingBox())!.x
      if (field === 'abstract') await editor.placeCaretAtEnd(field)
      else await editor.clickAt(field, 20)
      await page.keyboard.press(`${mod}+a`)
      await page.keyboard.press('Backspace')
      expect(await editor.text(field)).toBe('')

      // It's out of the flow of the text: the caret is at the start of the
      // line, where the first character will go, not after it.
      expect(await placeholder()).toEqual({
        content: `"${NAMES[field]}"`,
        color: 'rgb(173, 181, 189)',
        position: 'absolute',
      })
      expect((await editor.field(field).boundingBox())!.x).toBe(left)

      await page.keyboard.type('x')
      expect(await editor.text(field)).toBe('x')
      expect((await placeholder()).content).toBe('none')
    })
  }
})

test.describe('formatting in the title block', () => {
  const source =
    '---\ntitle: "A *bold* claim, with `code`"\nsubtitle: See [the site](https://example.com)\n---\n\nBody text.\n'

  test('is not shown: a field is its text', async ({ page }) => {
    const editor = await (await createDocument(source)).open(page)

    expect(await editor.text('title')).toBe('A bold claim, with code')
    expect(await editor.text('subtitle')).toBe('See the site')
    expect(await editor.field('title').innerHTML()).toBe('A bold claim, with code')
    expect(await editor.field('subtitle').innerHTML()).toBe('See the site')
  })

  test.fixme('stays in the source when the field is edited', async ({ page }) => {
    const document = await createDocument(source)
    const editor = await document.open(page)
    await editor.placeCaret('title', 23)
    await page.keyboard.type('!')

    // Today the title is written as the text it showed: `A bold claim, with code!`.
    await document.expectSource(source.replace('`code`', '`code`!'))
  })
})

test.describe('titles written other ways', () => {
  test('a title written as a number is its text, and can be edited', async ({ page }) => {
    const document = await createDocument('---\ntitle: 2024\n---\n\nBody.\n')
    const editor = await document.open(page)
    expect(await editor.text('title')).toBe('2024')
    await editor.expectCaret('title', 0)

    await page.keyboard.press(lineEnd)
    await page.keyboard.type(' review')

    expect(await editor.text('title')).toBe('2024 review')
    await expect.poll(() => document.source()).toContain('title: 2024 review\n')
  })

  test('a title written as a block of text is its text, and can be edited', async ({ page }) => {
    const document = await createDocument('---\ntitle: |\n  Block scalar title\n---\n\nBody.\n')
    const editor = await document.open(page)
    expect(await editor.text('title')).toBe('Block scalar title')

    await page.keyboard.press(lineEnd)
    await page.keyboard.type('!')

    expect(await editor.text('title')).toBe('Block scalar title!')
    await expect.poll(() => document.source()).toContain('Block scalar title!')
  })

  test('a title that is not text (a list) is not shown, and there is no title to edit', async ({ page }) => {
    const editor = await (await createDocument('---\ntitle: [a, b]\n---\n\nBody.\n')).open(page)

    expect(await editor.text('title')).toBeNull()
    await expect(editor.frame.locator('#title-block-header')).toHaveCount(0)
    expect(await editor.blocks()).toEqual(['p: Body.'])
  })
})

test.describe('a subtitle without a title', () => {
  for (const [name, front] of [
    ['no title', 'subtitle: Only a subtitle'],
    ['an empty title', 'title: ""\nsubtitle: Only a subtitle'],
  ]) {
    test(`with ${name}, the subtitle is shown, and the caret starts in it`, async ({ page }) => {
      const editor = await (await createDocument(`---\n${front}\n---\n\nBody.\n`)).open(page)

      expect(await editor.text('title')).toBeNull()
      expect(await editor.text('subtitle')).toBe('Only a subtitle')
      await expect.poll(() => editor.caret()).toMatchObject({ field: 'subtitle', start: 0, end: 0 })
    })
  }
})

test.describe('the rest of the title block', () => {
  const source = '---\ntitle: Notes\ndescription: A description\nkeywords: [alpha, beta]\ncategories: [news]\n---\n\nBody.\n'

  test('a description, keywords and categories are shown', async ({ page }) => {
    const editor = await (await createDocument(source)).open(page)
    const header = editor.frame.locator('#title-block-header')

    await expect(header.getByText('A description')).toBeVisible()
    await expect(header.getByText('alpha, beta')).toBeVisible()
    await expect(header.getByText('news')).toBeVisible()
  })

  test('and can\'t be edited', async ({ page }) => {
    const document = await createDocument(source)
    const editor = await document.open(page)

    for (const text of ['A description', 'alpha, beta', 'news']) {
      await editor.frame.locator('#title-block-header').getByText(text).click()
      await page.keyboard.type('x')
    }

    await expect(editor.frame.locator('#title-block-header').getByText('A description')).toHaveText('A description')
    expect(document.source()).toBe(source)
  })
})
