/**
 * What editing in the visual editor does to the document's source text.
 *
 * The source is the document: every edit is in it as soon as it's made,
 * and an edit changes only the lines of the field it was made in. The
 * tests marked `fixme` say what we want and don't have yet.
 */

import { expect, fixture, openEditor, test } from './helpers.ts'

const mod = 'ControlOrMeta'
const formatted = fixture('formatted')
const basic = fixture('basic')

test.describe('what an edit changes', () => {
  test('opening a document and moving around in it changes nothing', async ({ page }) => {
    const editor = await openEditor(page, 'formatted')
    await editor.clickAt('title', 60)
    await page.keyboard.press('ArrowDown')
    await page.keyboard.press(`${mod}+a`)
    await page.keyboard.press('End')
    await editor.clickAway()

    expect(editor.document.source()).toBe(formatted)
  })

  test('typing in the title changes only the title\'s line', async ({ page }) => {
    const editor = await openEditor(page, 'formatted')
    await editor.placeCaret('title', 9)
    await page.keyboard.type(' sales')

    await editor.document.expectSource(
      formatted.replace('title: Quarterly report', 'title: Quarterly sales report'),
    )
  })

  test('each keystroke is in the source as it is typed', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 16)

    await page.keyboard.type('s')
    await editor.document.expectSource(basic.replace('Quarterly report', 'Quarterly reports'))
    await page.keyboard.press('Backspace')
    await editor.document.expectSource(basic)
  })

  test('the first edit of a field rewrites that field\'s line in the standard form', async ({ page }) => {
    const editor = await openEditor(page, 'formatted')
    await editor.placeCaret('subtitle', 13)
    await page.keyboard.type('!')

    // The quotes, the extra spaces and the comment on the line go; every
    // other line stays.
    await editor.document.expectSource(
      formatted.replace('subtitle:   "Third quarter"   # to confirm', 'subtitle: Third quarter!'),
    )
  })

  test.fixme('an edit keeps the quotes and the comment on its line', async ({ page }) => {
    const editor = await openEditor(page, 'formatted')
    await editor.placeCaret('subtitle', 13)
    await page.keyboard.type('!')

    await editor.document.expectSource(formatted.replace('"Third quarter"', '"Third quarter!"'))
  })

  test('typing in the abstract changes only the abstract\'s lines', async ({ page }) => {
    const editor = await openEditor(page, 'formatted')
    await editor.field('abstract').click()
    await page.keyboard.press(`${mod}+ArrowDown`)
    await page.keyboard.type(' Briefly.')

    await editor.document.expectSource(formatted.replace('Then we look ahead.', 'Then we look ahead. Briefly.'))
  })

  test('a line in the abstract is a paragraph in the source', async ({ page }) => {
    const editor = await openEditor(page, 'formatted')
    await editor.field('abstract').click()
    await page.keyboard.press(`${mod}+ArrowDown`)

    // An empty line isn't a paragraph: nothing is written for it.
    await page.keyboard.press('Enter')
    expect(editor.document.source()).toBe(formatted)

    await page.keyboard.type('Last.')
    await editor.document.expectSource(formatted.replace('Then we look ahead.\n', 'Then we look ahead.\n\n  Last.\n'))
  })

  test('undo puts the source back exactly', async ({ page }) => {
    const editor = await openEditor(page, 'formatted')
    await editor.placeCaret('title', 9)
    await page.keyboard.type(' sales')
    await editor.document.expectSource(formatted.replace('Quarterly report', 'Quarterly sales report'))

    await page.keyboard.press(`${mod}+z`)
    await editor.document.expectSource(formatted)

    await page.keyboard.press(`${mod}+Shift+z`)
    await editor.document.expectSource(formatted.replace('Quarterly report', 'Quarterly sales report'))
  })

  test('an edit across fields changes each field\'s line', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 10)
    for (let i = 0; i < 12; i++) await page.keyboard.press('Shift+ArrowRight')
    await page.keyboard.type('X')

    await editor.document.expectSource(
      basic.replace('title: Quarterly report', 'title: Quarterly X').replace('subtitle: Third quarter', 'subtitle: quarter'),
    )
  })
})

test.describe('typed text is text', () => {
  test('markdown characters are written escaped, and read back as typed', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 16)
    await page.keyboard.type(' *new* [x](y) `z`')

    expect(await editor.text('title')).toBe('Quarterly report *new* [x](y) `z`')
    await editor.document.expectSource(
      basic.replace('title: Quarterly report', 'title: "Quarterly report \\\\*new\\\\* \\\\[x\\\\](y) \\\\`z\\\\`"'),
    )
    // Someone else opening the document sees the same characters.
    const other = await editor.join()
    expect(await other.text('title')).toBe('Quarterly report *new* [x](y) `z`')
  })

  test('text that YAML would read differently is quoted', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 16)
    await page.keyboard.type(': costs #1')

    await editor.document.expectSource(
      basic.replace('title: Quarterly report', 'title: "Quarterly report: costs \\\\#1"'),
    )
    const other = await editor.join()
    expect(await other.text('title')).toBe('Quarterly report: costs #1')
  })

  test('a title that starts like a list stays a title', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.select('title', 0, 16)
    await page.keyboard.type('- 1. Costs')

    expect(await editor.text('title')).toBe('- 1. Costs')
    const other = await editor.join()
    expect(await other.text('title')).toBe('- 1. Costs')
  })
})

test.describe('spaces the source does not keep', () => {
  test('a space typed at the end shows at once and is written with the next character', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 16)

    await page.keyboard.type(' ')
    expect(await editor.text('title')).toBe('Quarterly report ')
    await editor.expectCaret('title', 17)
    expect(editor.document.source()).toBe(basic)

    await page.keyboard.type('2026')
    expect(await editor.text('title')).toBe('Quarterly report 2026')
    await editor.document.expectSource(basic.replace('Quarterly report', 'Quarterly report 2026'))
  })

  test('a field shows what the source says once the caret leaves it', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 16)
    await page.keyboard.type('  ')
    expect(await editor.text('title')).toBe('Quarterly report  ')

    await page.keyboard.press('ArrowDown')

    expect(await editor.text('title')).toBe('Quarterly report')
    expect(editor.document.source()).toBe(basic)
  })
})

test.describe('adding and removing fields', () => {
  test('a new subtitle is written when it gets text, not before', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    const source = fixture('no-subtitle')
    await editor.placeCaret('title', 16)

    await page.keyboard.press('Enter')
    expect(await editor.text('subtitle')).toBe('')
    expect(editor.document.source()).toBe(source)

    await page.keyboard.type('Draft')
    await expect.poll(() => editor.document.source()).toContain('\nsubtitle: Draft\n')
  })

  test.fixme('a new subtitle goes on the line under the title', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.placeCaret('title', 16)
    await page.keyboard.press('Enter')
    await page.keyboard.type('Draft')

    // Today it goes after the last key.
    await editor.document.expectSource(
      fixture('no-subtitle').replace('title: Quarterly report\n', 'title: Quarterly report\nsubtitle: Draft\n'),
    )
  })

  test('a new subtitle left empty is never written', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.placeCaret('title', 16)
    await page.keyboard.press('Enter')
    await page.keyboard.press('ArrowUp')

    expect(await editor.text('subtitle')).toBeNull()
    expect(editor.document.source()).toBe(fixture('no-subtitle'))
  })

  test('emptying a field removes its line, and only its line', async ({ page }) => {
    const editor = await openEditor(page, 'formatted')
    await editor.select('subtitle', 0, 13)
    await page.keyboard.press('Backspace')

    await editor.document.expectSource(formatted.replace('subtitle:   "Third quarter"   # to confirm\n', ''))
    // The source never holds an empty field, even while the caret is in it.
    expect(await editor.text('subtitle')).toBe('')
  })

  test.fixme('retyping an emptied field puts it back on its line', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.select('title', 0, 16)
    await page.keyboard.press('Backspace')
    await page.keyboard.type('Annual report')

    // Today the title comes back after the last key.
    await editor.document.expectSource(basic.replace('Quarterly report', 'Annual report'))
  })

  test('replacing a field\'s whole text keeps it on its line', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.select('title', 0, 16)
    await page.keyboard.type('Annual report')

    await editor.document.expectSource(basic.replace('Quarterly report', 'Annual report'))
  })
})
