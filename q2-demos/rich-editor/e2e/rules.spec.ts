/**
 * A horizontal rule is a block of its own in the body: it can't be
 * edited, and the caret steps over it, but it can be made and removed.
 * Typing `---` (or `***` or `___`) at the start of a paragraph makes one,
 * as in qmd, and Backspace at the start of the line after it, Delete at
 * the end of the line before it, or deleting a selection across it,
 * removes it.
 */

import { createDocument, expect, fixture, openEditor, test } from './helpers.ts'

const mod = 'ControlOrMeta'
const rules = fixture('rules')

test.describe('showing rules', () => {
  test('a rule is shown as a rule between the lines around it, with no text to edit', async ({ page }) => {
    const editor = await openEditor(page, 'rules')

    expect(await editor.blocks()).toEqual(['p: Intro.', 'hr: ', 'p: Middle.', 'hr: ', 'p: End.'])
    expect(await editor.lines()).toEqual(['p: Intro.', 'p: Middle.', 'p: End.'])
  })

  test('editing a paragraph leaves the rules as they are written', async ({ page }) => {
    const source = '---\ntitle: Notes\n---\n\nIntro.\n\n***\n\nMiddle.\n\n- - -\n\nEnd.\n'
    const document = await createDocument(source)
    const editor = await document.open(page)
    await editor.placeCaretInLine(2, 4)

    await page.keyboard.type('!')

    await document.expectSource(source.replace('End.', 'End.!'))
  })

  // The qmd writer pairs the rules before an edit with those after it by
  // what they are, and every rule is the same, so it can give one rule's
  // spelling to another.
  test.fixme('making or removing a rule leaves the others as they are written', async ({ page }) => {
    const source = '---\ntitle: Notes\n---\n\nIntro.\n\n---\n\nMiddle.\n\n***\n\nEnd.\n'
    const document = await createDocument(source)
    const editor = await document.open(page)
    await editor.placeCaretInLine(1, 0)

    await page.keyboard.press('Backspace')

    // Today the rule left is written `---`.
    await document.expectSource(source.replace('---\n\nMiddle.', 'Middle.'))
  })
})

test.describe('moving past a rule', () => {
  test('Right and Left step over it', async ({ page }) => {
    const editor = await openEditor(page, 'rules')
    await editor.placeCaretInLine(0, 6)

    await page.keyboard.press('ArrowRight')
    await editor.expectLineCaret(1, 0)

    await page.keyboard.press('ArrowLeft')
    await editor.expectLineCaret(0, 6)
  })

  test('Down and Up step over it', async ({ page }) => {
    const editor = await openEditor(page, 'rules')
    await editor.placeCaretInLine(0, 2)

    await page.keyboard.press('ArrowDown')
    await page.keyboard.type('x')
    await expect.poll(async () => (await editor.lines())[1]).toMatch(/^p: M.*x/)

    await page.keyboard.press('ArrowUp')
    await page.keyboard.type('y')
    await expect.poll(async () => (await editor.lines())[0]).toMatch(/^p: I.*y/)
  })
})

test.describe('making a rule', () => {
  test('--- at the start of a paragraph puts a rule before it, and the caret stays on its text', async ({ page }) => {
    const editor = await openEditor(page, 'rules')
    await editor.placeCaretInLine(2, 0)

    await page.keyboard.type('---')

    expect(await editor.blocks()).toEqual(['p: Intro.', 'hr: ', 'p: Middle.', 'hr: ', 'hr: ', 'p: End.'])
    await editor.document.expectSource(rules.replace('End.', '---\n\nEnd.'))
    await editor.expectLineCaret(2, 0)
  })

  test('in a new paragraph, what is typed next goes after it', async ({ page }) => {
    const editor = await openEditor(page, 'rules')
    await editor.placeCaretInLine(2, 4)

    await page.keyboard.press('Enter')
    await page.keyboard.type('---After')

    expect((await editor.blocks()).slice(4)).toEqual(['p: End.', 'hr: ', 'p: After'])
    await editor.document.expectSource(rules.replace('End.\n', 'End.\n\n---\n\nAfter\n'))
  })

  test('*** and ___ make one too, written as ---', async ({ page }) => {
    const editor = await openEditor(page, 'rules')
    await editor.placeCaretInLine(2, 4)

    await page.keyboard.press('Enter')
    await page.keyboard.type('***A')
    await page.keyboard.press('Enter')
    await page.keyboard.type('___B')

    expect((await editor.blocks()).slice(4)).toEqual(['p: End.', 'hr: ', 'p: A', 'hr: ', 'p: B'])
    await editor.document.expectSource(rules.replace('End.\n', 'End.\n\n---\n\nA\n\n---\n\nB\n'))
  })

  test('undo gives back the typed dashes, as text', async ({ page }) => {
    const editor = await openEditor(page, 'rules')
    await editor.placeCaretInLine(2, 4)
    await page.keyboard.press('Enter')
    await page.keyboard.type('---')
    expect((await editor.blocks()).slice(4)).toEqual(['p: End.', 'hr: ', 'p: '])

    await page.keyboard.press(`${mod}+z`)

    expect((await editor.blocks()).slice(4)).toEqual(['p: End.', 'p: ---'])
    await editor.expectLineCaret(3, 3)
    await editor.document.expectSource(rules.replace('End.\n', 'End.\n\n\\-\\-\\-\n'))
  })

  test('Backspace right after gives back the typed dashes, as text', async ({ page }) => {
    const editor = await openEditor(page, 'rules')
    await editor.placeCaretInLine(2, 4)
    await page.keyboard.press('Enter')
    await page.keyboard.type('---')
    expect((await editor.blocks()).slice(4)).toEqual(['p: End.', 'hr: ', 'p: '])

    await page.keyboard.press('Backspace')

    expect((await editor.blocks()).slice(4)).toEqual(['p: End.', 'p: ---'])
    await editor.expectLineCaret(3, 3)
    await editor.document.expectSource(rules.replace('End.\n', 'End.\n\n\\-\\-\\-\n'))
  })

  test('--- that isn\'t at the start is text', async ({ page }) => {
    const editor = await openEditor(page, 'rules')
    await editor.placeCaretInLine(2, 4)

    await page.keyboard.type(' ---')

    expect((await editor.blocks()).slice(4)).toEqual(['p: End. ---'])
  })

  test('--- in a heading, a quote or the title is text', async ({ page }) => {
    const editor = await openEditor(page, 'rules')
    await editor.placeCaretInLine(1, 0)
    await page.keyboard.type('# ---')
    await editor.placeCaretInLine(2, 0)
    await page.keyboard.type('> ---')
    await editor.placeCaret('title', 0)
    await page.keyboard.type('---')

    expect((await editor.lines()).slice(1)).toEqual(['h1: ---Middle.', 'blockquote: ---End.'])
    await editor.expectText('title', '---Field notes')
  })
})

test.describe('removing a rule', () => {
  test('Backspace at the start of the paragraph after it removes it', async ({ page }) => {
    const editor = await openEditor(page, 'rules')
    await editor.placeCaretInLine(1, 0)

    await page.keyboard.press('Backspace')

    expect(await editor.blocks()).toEqual(['p: Intro.', 'p: Middle.', 'hr: ', 'p: End.'])
    await editor.document.expectSource(rules.replace('---\n\nMiddle.', 'Middle.'))
    await editor.expectLineCaret(1, 0)
  })

  test('Backspace again joins the paragraph to the one before', async ({ page }) => {
    const editor = await openEditor(page, 'rules')
    await editor.placeCaretInLine(1, 0)

    await page.keyboard.press('Backspace')
    await page.keyboard.press('Backspace')

    expect(await editor.blocks()).toEqual(['p: Intro.Middle.', 'hr: ', 'p: End.'])
    await editor.expectLineCaret(0, 6)
  })

  test('Delete at the end of the paragraph before it removes it', async ({ page }) => {
    const editor = await openEditor(page, 'rules')
    await editor.placeCaretInLine(1, 7)

    await page.keyboard.press('Delete')

    expect(await editor.blocks()).toEqual(['p: Intro.', 'hr: ', 'p: Middle.', 'p: End.'])
    await editor.document.expectSource(rules.replace('Middle.\n\n---\n\n', 'Middle.\n\n'))
    await editor.expectLineCaret(1, 7)
  })

  test('deleting a selection across it removes it', async ({ page }) => {
    const editor = await openEditor(page, 'rules')
    await editor.placeCaretInLine(0, 3)
    await editor.shiftClickParagraph(1, 3)

    await page.keyboard.press('Backspace')

    expect(await editor.blocks()).toEqual(['p: Intdle.', 'hr: ', 'p: End.'])
    await editor.document.expectSource(rules.replace('Intro.\n\n---\n\nMiddle.', 'Intdle.'))
  })

  test('undo brings it back', async ({ page }) => {
    const editor = await openEditor(page, 'rules')
    await editor.placeCaretInLine(1, 0)
    await page.keyboard.press('Backspace')
    expect(await editor.blocks()).toEqual(['p: Intro.', 'p: Middle.', 'hr: ', 'p: End.'])

    await page.keyboard.press(`${mod}+z`)

    expect(await editor.blocks()).toEqual(['p: Intro.', 'hr: ', 'p: Middle.', 'hr: ', 'p: End.'])
    await editor.document.expectSource(rules)
    await editor.expectLineCaret(1, 0)
  })
})

test.describe('copying and pasting rules', () => {
  test('copying across a rule puts --- on the clipboard, on a line of its own', async ({ page }) => {
    const editor = await openEditor(page, 'rules')
    await editor.placeCaretInLine(0, 0)
    await editor.shiftClickParagraph(1, 7)

    expect(await editor.copy()).toBe('Intro.\n---\nMiddle.')
  })

  test('a pasted --- is text', async ({ page }) => {
    const editor = await openEditor(page, 'rules')
    await editor.placeCaretInLine(2, 4)

    await editor.paste('\n---')

    expect((await editor.blocks()).slice(4)).toEqual(['p: End.', 'p: ---'])
  })
})

test.describe('rules and collaborators', () => {
  test('a rule added in the source shows up, and the caret stays on its text', async ({ page }) => {
    const editor = await openEditor(page, 'rules')
    await editor.placeCaretInLine(2, 2)

    editor.document.replace('End.', '---\n\nEnd.')

    await expect.poll(() => editor.blocks()).toEqual(['p: Intro.', 'hr: ', 'p: Middle.', 'hr: ', 'hr: ', 'p: End.'])
    await editor.expectLineCaret(2, 2)
    await page.keyboard.type('x')
    await editor.document.expectSource(rules.replace('End.', '---\n\nEnxd.'))
  })

  test('a rule removed in the source goes, and the caret stays on its text', async ({ page }) => {
    const editor = await openEditor(page, 'rules')
    await editor.placeCaretInLine(1, 3)

    editor.document.replace('---\n\nMiddle.', 'Middle.')

    await expect.poll(() => editor.blocks()).toEqual(['p: Intro.', 'p: Middle.', 'hr: ', 'p: End.'])
    await editor.expectLineCaret(1, 3)
  })
})
