/**
 * Composing text with an input method: Japanese or Chinese input, a dead
 * key for an accent, a phone's keyboard. While a composition lasts, the
 * browser itself writes the unfinished text into the page, and it can
 * change with every key. The editor takes the text when the composition
 * ends: only then is it in the source, and an undo step.
 *
 * (`Editor.compose` plays the browser's part.)
 */

import { expect, fixture, openEditor, test } from './helpers.ts'

const mod = 'ControlOrMeta'
const basic = fixture('basic')
const body = fixture('body')

test.describe('composing text', () => {
  test('the text being composed shows in the page, and is in the source only when it is final', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 16)

    await editor.compose(' に')
    await editor.compose(' にほ')
    expect(await editor.text('title')).toBe('Quarterly report にほ')
    expect(editor.document.source()).toBe(basic)

    await editor.endComposition(' 日本')
    expect(await editor.text('title')).toBe('Quarterly report 日本')
    await editor.expectCaret('title', 19)
    await editor.document.expectSource(basic.replace('Quarterly report', 'Quarterly report 日本'))
  })

  test('typing goes on after the composed text', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 9)

    await editor.compose('に')
    await editor.endComposition('日本')
    await page.keyboard.type('!')

    expect(await editor.text('title')).toBe('Quarterly日本! report')
    await editor.document.expectSource(basic.replace('Quarterly report', 'Quarterly日本! report'))
  })

  test('undo takes back the composed text', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 16)
    await editor.compose(' に')
    await editor.endComposition(' 日本')

    await page.keyboard.press(`${mod}+z`)

    expect(await editor.text('title')).toBe('Quarterly report')
    await editor.document.expectSource(basic)
  })

  test('text composed in a new, empty subtitle keeps the subtitle', async ({ page }) => {
    const editor = await openEditor(page, 'no-subtitle')
    await editor.addField('subtitle')

    await editor.compose('に')
    await editor.endComposition('日本')

    expect(await editor.text('subtitle')).toBe('日本')
    await editor.expectCaret('subtitle', 2)
    await expect.poll(() => editor.document.source()).toContain('subtitle: 日本\n')
  })

  test('text composed in a paragraph changes only that paragraph\'s line of the source', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(1, 17)

    await editor.compose(' に')
    expect(editor.document.source()).toBe(body)
    await editor.endComposition(' 日本')

    await editor.document.expectSource(body.replace('Second paragraph.', 'Second paragraph. 日本'))
  })
})

test.describe('while someone else edits', () => {
  test('their edit to another field shows when the composition ends', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 16)
    await editor.compose(' に')

    editor.document.replace('Third quarter', 'Fourth quarter')
    await editor.endComposition(' 日本')

    await editor.expectText('subtitle', 'Fourth quarter')
    expect(await editor.text('title')).toBe('Quarterly report 日本')
    await editor.document.expectSource(
      basic.replace('Quarterly report', 'Quarterly report 日本').replace('Third quarter', 'Fourth quarter'),
    )
  })

  test('their edit to the field being composed in stays, with the composed text', async ({ page }) => {
    const editor = await openEditor(page)
    await editor.placeCaret('title', 16)
    await editor.compose(' に')

    editor.document.replace('Quarterly', 'Annual')
    // Their edit has reached this editor before the composition ends.
    await page.waitForTimeout(300)
    await editor.endComposition(' 日本')

    await editor.expectText('title', 'Annual report 日本')
    await editor.expectCaret('title', 16)
    await editor.document.expectSource(basic.replace('Quarterly report', 'Annual report 日本'))
  })

  test('their edit to another paragraph stays, with the composed text', async ({ page }) => {
    const editor = await openEditor(page, 'body')
    await editor.placeCaretInParagraph(0, 16)
    await editor.compose(' に')

    editor.document.replace('Second paragraph.', 'Second paragraph, edited.')
    await page.waitForTimeout(300)
    await editor.endComposition(' 日本')

    await expect
      .poll(() => editor.paragraphs())
      .toEqual(['First paragraph. 日本', 'Second paragraph, edited.', 'Last paragraph.'])
    await editor.document.expectSource(
      body.replace('First paragraph.', 'First paragraph. 日本').replace('Second paragraph.', 'Second paragraph, edited.'),
    )
  })
})

test('nothing composed is kept while the source doesn\'t parse', async ({ page }) => {
  const editor = await openEditor(page)
  await editor.placeCaret('title', 16)
  const broken = basic.replace('title: Quarterly report', 'title: "Quarterly report')
  editor.document.replace('title: Quarterly report', 'title: "Quarterly report')
  await expect(page.locator('.visual-editor .status')).toContainText('Read-only')

  await editor.compose(' に')
  await editor.endComposition(' 日本')

  expect(await editor.text('title')).toBe('Quarterly report')
  expect(editor.document.source()).toBe(broken)
})
