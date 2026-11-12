/**
 * How the body's blocks are shown. A paragraph of plain text is a
 * paragraph that can be edited, and a heading of plain text a heading that
 * can be edited (`headings.spec.ts`). Every other block the editor knows is
 * shown as `q2 render` would show it, roughly (no syntax highlighting, no
 * rendered math), and can't be edited. A block it doesn't know is a box
 * with the name of its kind.
 *
 * (That the editable parts match `q2 render` exactly is in
 * `render-parity.spec.ts`.)
 */

import { createDocument, expect, openEditor, test } from './helpers.ts'

const shown = (editor: { frame: import('@playwright/test').Frame }, selector: string) =>
  editor.frame.locator(`#quarto-document-content > ${selector}`)

test.describe('paragraphs', () => {
  test('a paragraph of plain text can be edited, and no other paragraph can', async ({ page }) => {
    const editor = await openEditor(page, 'blocks')

    expect(await editor.paragraphs()).toEqual([
      'Plain text, with “quotes” and an author’s apostrophe.',
      'Last words.',
    ])
  })

  test('quotes and apostrophes are plain text, shown curly as `q2 render` shows them', async ({ page }) => {
    const editor = await openEditor(page, 'blocks')
    await editor.placeCaretInParagraph(0, 0)
    await page.keyboard.type('Some ')

    // Someone else sees the same paragraph, still editable.
    const other = await editor.join()
    expect((await other.paragraphs())[0]).toBe('Some Plain text, with “quotes” and an author’s apostrophe.')
    // The edited paragraph is written with the quotes it showed.
    await expect
      .poll(() => editor.document.source())
      .toContain('\n\nSome Plain text, with “quotes” and an author\'s apostrophe.\n\n')
  })

  test('a paragraph with any formatting is shown with it', async ({ page }) => {
    const editor = await openEditor(page, 'blocks')
    const p = shown(editor, 'p:not(.qe-field)')

    await expect(p).toHaveText(
      'Text with emphasis, strong, strikeout, super, sub, code, x^2, a link, a span, small caps and underline.',
    )
    await expect(p.locator('em')).toHaveText('emphasis')
    await expect(p.locator('strong')).toHaveText('strong')
    await expect(p.locator('s')).toHaveText('strikeout')
    await expect(p.locator('sup')).toHaveText('super')
    await expect(p.locator('sub')).toHaveText('sub')
    await expect(p.locator('code')).toHaveText('code')
    // Math is shown as its source.
    await expect(p.locator('span.math')).toHaveText('x^2')
    await expect(p.locator('a.external')).toHaveText('a link')
    await expect(p.locator('a.external')).toHaveAttribute('href', 'https://example.com')
    await expect(p.locator('span.mark')).toHaveText('a span')
    await expect(p.locator('span.smallcaps')).toHaveText('small caps')
    await expect(p.locator('u')).toHaveText('underline')
  })

  test('quotes and an image in a paragraph with formatting are shown, and a citation and a note are not', async ({ page }) => {
    const editor = await (
      await createDocument(
        '---\ntitle: Notes\n---\n\nAn *emphasised* "quote" and \'another\', see [@smith2020], ![a picture](picture.png){.wide} and a note.^[Hidden.]\n',
      )
    ).open(page)
    const p = shown(editor, 'p')

    // Quotes are curly; a citation and a note aren't shown.
    await expect(p).toHaveText('An emphasised “quote” and ‘another’, see ,  and a note.')
    await expect(p.locator('img.wide')).toHaveAttribute('src', 'picture.png')
    await expect(p.locator('img.wide')).toHaveAttribute('alt', 'a picture')
    expect(await editor.paragraphs()).toEqual([])
  })

  test('a line break within a paragraph is shown, and the paragraph can\'t be edited', async ({ page }) => {
    const editor = await (await createDocument('---\ntitle: Notes\n---\n\nOne\\\ntwo.\n')).open(page)

    await expect(shown(editor, 'p')).toHaveText('Onetwo.')
    await expect(shown(editor, 'p').locator('br')).toHaveCount(1)
    expect(await editor.paragraphs()).toEqual([])
  })
})

test.describe('other blocks', () => {
  test('a heading has its level, its id and its classes', async ({ page }) => {
    const editor = await openEditor(page, 'blocks')

    await expect(shown(editor, 'h1#first.unnumbered')).toHaveText('One')
    await expect(shown(editor, 'h6#six')).toHaveText('Six')
  })

  test('a code block is its text, with its language as a class', async ({ page }) => {
    const editor = await openEditor(page, 'blocks')

    await expect(shown(editor, 'pre.python > code')).toHaveText('print("hi")')
  })

  test('a block quote holds its blocks', async ({ page }) => {
    const editor = await openEditor(page, 'blocks')

    await expect(shown(editor, 'blockquote.blockquote > p')).toHaveText('A quote.')
  })

  test('a bullet list and a numbered list hold their items, and the numbered one starts at its number', async ({
    page,
  }) => {
    const editor = await openEditor(page, 'blocks')

    await expect(shown(editor, 'ul > li')).toHaveText(['Bullet one', 'Bullet two'])
    await expect(shown(editor, 'ol > li')).toHaveText(['Three', 'Four'])
    await expect(shown(editor, 'ol')).toHaveAttribute('start', '3')
  })

  test('a div has its id and classes, and holds its blocks', async ({ page }) => {
    const editor = await openEditor(page, 'blocks')

    await expect(shown(editor, 'div#note.aside > p')).toHaveText('Inside a div.')
  })

  test('a horizontal rule is a rule', async ({ page }) => {
    const editor = await openEditor(page, 'blocks')

    await expect(shown(editor, 'hr')).toHaveCount(1)
  })

  test('a block the editor doesn\'t know (a table, a figure) is a box with the name of its kind', async ({ page }) => {
    const editor = await openEditor(page, 'blocks')

    await expect(shown(editor, '.qe-unknown')).toHaveText(['Table', 'Figure'])
  })

  test('the blocks are in the order of the source', async ({ page }) => {
    const editor = await openEditor(page, 'blocks')

    expect((await editor.blocks()).map((block) => block.split(':')[0])).toEqual([
      'p', 'p', 'h1', 'h6', 'pre', 'blockquote', 'ul', 'ol', 'div', 'hr', 'div', 'div', 'p',
    ])
  })
})

test.describe('what blocks hold', () => {
  const source = [
    '---',
    'title: Notes',
    '---',
    '',
    '* One',
    '  * Nested',
    '* Two',
    '',
    '# A *formatted* heading',
    '',
    '```{r}',
    '1 + 1',
    '```',
    '',
    '$$',
    'x^2',
    '$$',
    '',
    '<div class="x">raw</div>',
    '',
    '{{< pagebreak >}}',
    '',
    'Text with <b>raw</b> HTML and a {{< kbd X >}} shortcode.',
    '',
  ].join('\n')

  test('a list in a list item is shown in it', async ({ page }) => {
    const editor = await (await createDocument(source)).open(page)

    await expect(shown(editor, 'ul > li')).toHaveText(['OneNested', 'Two'])
    await expect(shown(editor, 'ul > li > ul > li')).toHaveText('Nested')
  })

  test('a list with blank lines between its items holds the same items as one without', async ({ page }) => {
    const editor = await (await createDocument('---\ntitle: Notes\n---\n\n* One\n\n* Two\n')).open(page)

    await expect(shown(editor, 'ul > li')).toHaveText(['One', 'Two'])
  })

  test('a heading is shown with its formatting', async ({ page }) => {
    const editor = await (await createDocument(source)).open(page)

    await expect(shown(editor, 'h1')).toHaveText('A formatted heading')
    await expect(shown(editor, 'h1 em')).toHaveText('formatted')
  })

  test('a code cell is its code', async ({ page }) => {
    const editor = await (await createDocument(source)).open(page)

    await expect(shown(editor, 'pre > code')).toHaveText('1 + 1')
  })

  test('display math is shown as its source, in a paragraph', async ({ page }) => {
    const editor = await (await createDocument(source)).open(page)

    await expect(shown(editor, 'p > span.math')).toHaveText(/^\s*x\^2\s*$/)
  })

  test('raw HTML is a box, as a block the editor doesn\'t know is', async ({ page }) => {
    const editor = await (await createDocument(source)).open(page)

    const boxes = shown(editor, '.qe-unknown')
    await expect(boxes.first()).toHaveText('RawBlock')
    expect(new Set(await boxes.allTextContents())).toEqual(new Set(['RawBlock']))
    await expect(editor.frame.locator('div.x')).toHaveCount(0)
  })

  test('a shortcode and raw inline HTML are not shown, and their paragraph can\'t be edited', async ({ page }) => {
    const editor = await (await createDocument(source)).open(page)

    expect(await editor.blocks()).toContain('p: Text with raw HTML and a  shortcode.')
    expect(await editor.paragraphs()).toEqual([])
  })

  test('a paragraph whose text has the character that stands for an object can\'t be edited', async ({ page }) => {
    const editor = await (await createDocument('---\ntitle: Notes\n---\n\nAn ￼ here.\n\nPlain.\n')).open(page)

    expect(await editor.blocks()).toEqual(['p: An ￼ here.', 'p: Plain.'])
    expect(await editor.paragraphs()).toEqual(['Plain.'])
  })
})
