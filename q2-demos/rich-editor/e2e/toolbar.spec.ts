/**
 * The toolbar above the document, as in a word processor. It follows the
 * caret: its style box shows the style of the caret's line (Paragraph,
 * Heading 1, …) and its menu changes it, and its quote button is pressed
 * while the caret is in a quote and quotes or unquotes the line. Using the
 * toolbar leaves the caret where it was, on the same text. It changes the
 * lines of the body only: in the title block, it's disabled.
 *
 * Resting the mouse on a button shows a tooltip with its name and keyboard
 * shortcut, and the style menu shows each style's shortcut; the shortcuts
 * do what the buttons and the menu do.
 */

import { expect, fixture, openEditor, test } from './helpers.ts'

const mod = 'ControlOrMeta'
const toolbar = fixture('toolbar')

/** A shortcut as the editor writes it here: on a Mac, or elsewhere. */
const keys = (mac: string, other: string) => (process.platform === 'darwin' ? mac : other)

/** A shortcut as `aria-keyshortcuts` writes it here. */
const ariaKeys = (rest: string) => `${process.platform === 'darwin' ? 'Meta' : 'Control'}+${rest}`

test.describe('following the caret', () => {
  test('the style box shows the style of the caret\'s line', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')

    await editor.placeCaretInLine(0, 2)
    await expect(editor.styleBox()).toHaveText('Paragraph')
    await editor.placeCaretInLine(1, 2)
    await expect(editor.styleBox()).toHaveText('Heading 2')
    await editor.placeCaretInLine(2, 2)
    await expect(editor.styleBox()).toHaveText('Paragraph')
  })

  test('the quote button is pressed while the caret is in a quote', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')

    await editor.placeCaretInLine(0, 2)
    await expect(editor.quoteButton()).toHaveAttribute('aria-pressed', 'false')
    await editor.placeCaretInLine(2, 2)
    await expect(editor.quoteButton()).toHaveAttribute('aria-pressed', 'true')
  })

  test('in the title block, the toolbar is disabled', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')

    await editor.placeCaret('title', 2)

    await expect(editor.styleBox()).toBeDisabled()
    await expect(editor.quoteButton()).toBeDisabled()
  })
})

test.describe('the style box', () => {
  test('its menu checks the caret\'s line\'s style', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(1, 2)

    await editor.styleBox().click()

    await expect(editor.styleMenu().getByRole('menuitemradio', { checked: true })).toHaveAccessibleName('Heading 2')
  })

  test('its menu shows each style\'s keyboard shortcut', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)

    await editor.styleBox().click()

    for (const [name, digit] of [['Paragraph', 0], ['Heading 1', 1], ['Heading 2', 2], ['Heading 3', 3], ['Heading 4', 4]] as const) {
      const item = editor.styleMenu().getByRole('menuitemradio', { name, exact: true })
      await expect(item.locator('kbd')).toHaveText(keys(`⌥⌘${digit}`, `Ctrl+Alt+${digit}`))
      await expect(item).toHaveAttribute('aria-keyshortcuts', ariaKeys(`Alt+${digit}`))
    }
  })

  test('its check mark is the same size whichever style is checked', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(1, 2)

    await editor.styleBox().click()

    const checked = editor.styleMenu().getByRole('menuitemradio', { checked: true })
    const paragraph = editor.styleMenu().getByRole('menuitemradio', { name: 'Paragraph' })
    const markSize = await checked.evaluate((el) => getComputedStyle(el, '::before').fontSize)
    const itemSize = await paragraph.evaluate((el) => getComputedStyle(el).fontSize)
    expect(markSize).toBe(itemSize)
  })

  test('opening its menu changes nothing in the document', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    const before = await editor.frame.locator('#quarto-document-content').innerHTML()

    await editor.styleBox().click()
    await expect(editor.styleMenu()).toBeVisible()

    expect(await editor.frame.locator('#quarto-document-content').innerHTML()).toBe(before)
  })

  test('choosing a heading makes the paragraph a heading, with the caret where it was', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 5)

    await editor.chooseStyle('Heading 1')

    await expect(editor.styleMenu()).toBeHidden()
    expect((await editor.lines())[0]).toBe('h1: Intro.')
    await editor.document.expectSource(toolbar.replace('Intro.', '# Intro.'))
    await page.keyboard.type('!')
    await editor.document.expectSource(toolbar.replace('Intro.', '# Intro!.'))
  })

  test('choosing Paragraph makes a heading a paragraph', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(1, 2)

    await editor.chooseStyle('Paragraph')

    expect((await editor.lines())[1]).toBe('p: Results')
    await editor.document.expectSource(toolbar.replace('## Results', 'Results'))
  })

  test('choosing another level changes the heading\'s level', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(1, 2)

    await editor.chooseStyle('Heading 3')

    await editor.document.expectSource(toolbar.replace('## Results', '### Results'))
  })

  test('in a quote, a heading can\'t be chosen', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(2, 2)

    await editor.styleBox().click()

    await expect(editor.styleMenu().getByRole('menuitemradio', { name: 'Heading 1' })).toBeDisabled()
    await expect(editor.styleMenu().getByRole('menuitemradio', { name: 'Paragraph' })).toBeEnabled()
  })

  test('a selection over several lines changes each of them', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    await page.keyboard.press('Shift+ArrowDown')

    await editor.chooseStyle('Heading 3')

    await editor.document.expectSource(toolbar.replace('Intro.', '### Intro.').replace('## Results', '### Results'))
  })

  test('undo gives the line back its style', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    await editor.chooseStyle('Heading 1')
    await editor.document.expectSource(toolbar.replace('Intro.', '# Intro.'))

    await page.keyboard.press(`${mod}+z`)

    await editor.document.expectSource(toolbar)
  })

  test('Escape closes its menu and changes nothing', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    await editor.styleBox().click()

    await page.keyboard.press('Escape')

    await expect(editor.styleMenu()).toBeHidden()
    await page.keyboard.type('!')
    await editor.document.expectSource(toolbar.replace('Intro.', 'In!tro.'))
  })

  test('clicking the style box again closes its menu', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    await editor.styleBox().click()

    await editor.styleBox().click()

    await expect(editor.styleMenu()).toBeHidden()
  })

  test('clicking the document closes its menu', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    await editor.styleBox().click()

    await editor.placeCaretInLine(3, 1)

    await expect(editor.styleMenu()).toBeHidden()
  })
})

test.describe('the quote button', () => {
  test('quotes the caret\'s paragraph, with the caret where it was', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(3, 2)

    await editor.quoteButton().click()

    expect((await editor.lines())[3]).toBe('blockquote: End.')
    await editor.document.expectSource(toolbar.replace('End.', '> End.'))
    await expect(editor.quoteButton()).toHaveAttribute('aria-pressed', 'true')
    await page.keyboard.type('!')
    await editor.document.expectSource(toolbar.replace('End.', '> En!d.'))
  })

  test('in a quote, makes it a paragraph again', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(2, 2)

    await editor.quoteButton().click()

    expect((await editor.lines())[2]).toBe('p: A quote.')
    await editor.document.expectSource(toolbar.replace('> A quote.', 'A quote.'))
  })

  test('is disabled on a heading', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')

    await editor.placeCaretInLine(1, 2)

    await expect(editor.quoteButton()).toBeDisabled()
  })
})

test.describe('the undo and redo buttons', () => {
  test('are disabled before anything is edited', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)

    await expect(editor.undoButton()).toBeDisabled()
    await expect(editor.redoButton()).toBeDisabled()
  })

  test('undo takes back the last edit, with the caret where it was', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    await page.keyboard.type('xy')
    await editor.document.expectSource(toolbar.replace('Intro.', 'Inxytro.'))

    await editor.undoButton().click()

    await editor.document.expectSource(toolbar)
    await expect(editor.undoButton()).toBeDisabled()
    await page.keyboard.type('!')
    await editor.document.expectSource(toolbar.replace('Intro.', 'In!tro.'))
  })

  test('redo makes the undone edit again', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(3, 2)
    await editor.quoteButton().click()
    await editor.undoButton().click()
    await editor.document.expectSource(toolbar)
    await expect(editor.redoButton()).toBeEnabled()

    await editor.redoButton().click()

    await editor.document.expectSource(toolbar.replace('End.', '> End.'))
    await expect(editor.redoButton()).toBeDisabled()
    await expect(editor.undoButton()).toBeEnabled()
  })

  test('another edit after an undo leaves nothing to redo', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    await page.keyboard.type('x')
    await editor.undoButton().click()
    await expect(editor.redoButton()).toBeEnabled()

    await page.keyboard.type('y')

    await expect(editor.redoButton()).toBeDisabled()
  })

  test('work in the title block too', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaret('title', 5)
    await page.keyboard.type('!')
    await editor.document.expectSource(toolbar.replace('Field notes', 'Field! notes'))

    await editor.undoButton().click()

    await editor.document.expectSource(toolbar)
    await editor.redoButton().click()
    await editor.document.expectSource(toolbar.replace('Field notes', 'Field! notes'))
  })
})

test.describe('tooltips', () => {
  // Each button, and its tooltip's shortcut (none for the style box).
  const BUTTONS = [
    { name: 'Undo', keys: keys('⌘Z', 'Ctrl+Z'), aria: ariaKeys('Z') },
    { name: 'Redo', keys: keys('⇧⌘Z', 'Ctrl+Y'), aria: keys('Meta+Shift+Z', 'Control+Y') },
    { name: 'Text style', keys: null, aria: null },
    { name: 'Bold', keys: keys('⌘B', 'Ctrl+B'), aria: ariaKeys('B') },
    { name: 'Italic', keys: keys('⌘I', 'Ctrl+I'), aria: ariaKeys('I') },
    { name: 'Underline', keys: keys('⌘U', 'Ctrl+U'), aria: ariaKeys('U') },
    { name: 'Highlight', keys: keys('⇧⌘H', 'Ctrl+Shift+H'), aria: ariaKeys('Shift+H') },
    { name: 'Link', keys: keys('⌘K', 'Ctrl+K'), aria: ariaKeys('K') },
    { name: 'Bulleted list', keys: keys('⇧⌘8', 'Ctrl+Shift+8'), aria: ariaKeys('Shift+8') },
    { name: 'Numbered list', keys: keys('⇧⌘7', 'Ctrl+Shift+7'), aria: ariaKeys('Shift+7') },
    { name: 'Quote', keys: keys('⇧⌘9', 'Ctrl+Shift+9'), aria: ariaKeys('Shift+9') },
  ]

  for (const b of BUTTONS) {
    test(`resting on ${b.name} shows its name${b.keys ? ' and shortcut' : ''}`, async ({ page }) => {
      const editor = await openEditor(page, 'toolbar')
      await editor.placeCaretInLine(0, 2)

      await editor.hoverOver(editor.toolbar().getByRole('button', { name: b.name, exact: true }))

      await expect(editor.tooltip()).toBeVisible()
      await expect(editor.tooltip().locator('span').first()).toHaveText(b.name)
      if (b.keys) await expect(editor.tooltip().locator('kbd')).toHaveText(b.keys)
      else await expect(editor.tooltip().locator('kbd')).toHaveCount(0)
    })
  }

  test('a disabled button shows its tooltip too', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    await expect(editor.redoButton()).toBeDisabled()

    await editor.hoverOver(editor.redoButton())

    await expect(editor.tooltip().locator('span').first()).toHaveText('Redo')
  })

  test('the first button\'s tooltip stays within the toolbar', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)

    await editor.hoverOver(editor.undoButton())

    await expect(editor.tooltip()).toBeVisible()
    const tip = (await editor.tooltip().boundingBox())!
    const bar = (await editor.toolbar().boundingBox())!
    expect(tip.x).toBeGreaterThanOrEqual(bar.x)
  })

  test('moving to another button shows its tooltip instead', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    await editor.hoverOver(editor.formatButton('Bold'))
    await expect(editor.tooltip()).toBeVisible()

    await editor.hoverOver(editor.formatButton('Italic'))

    await expect(editor.tooltip().locator('span').first()).toHaveText('Italic')
    await expect(editor.tooltip()).toHaveCount(1)
  })

  test('moving off the toolbar hides the tooltip', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    await editor.hoverOver(editor.formatButton('Bold'))
    await expect(editor.tooltip()).toBeVisible()

    await editor.hoverOver(editor.line(3))

    await expect(editor.tooltip()).toBeHidden()
  })

  test('pressing the button hides its tooltip', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    await editor.hoverOver(editor.quoteButton())
    await expect(editor.tooltip()).toBeVisible()

    await page.mouse.down()
    await page.mouse.up()

    await expect(editor.tooltip()).toBeHidden()
    await editor.document.expectSource(toolbar.replace('Intro.', '> Intro.'))
  })

  test('the style box shows no tooltip while its menu is open', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    await editor.styleBox().click()

    await editor.hoverOver(editor.styleBox())
    await page.waitForTimeout(1000)

    await expect(editor.tooltip()).toBeHidden()
  })

  test('the buttons give their shortcuts to assistive technology, and leave the browser\'s tooltip out', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')

    for (const b of BUTTONS) {
      const button = editor.toolbar().getByRole('button', { name: b.name, exact: true })
      if (b.aria) await expect(button).toHaveAttribute('aria-keyshortcuts', b.aria)
      else await expect(button).not.toHaveAttribute('aria-keyshortcuts')
      await expect(button).not.toHaveAttribute('title')
    }
  })
})

test.describe('the keyboard shortcuts', () => {
  test(`${mod}+Alt+1 to 4 make the caret's line a heading, with the caret where it was`, async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)

    await page.keyboard.press(`${mod}+Alt+Digit1`)
    await editor.document.expectSource(toolbar.replace('Intro.', '# Intro.'))
    await expect(editor.styleBox()).toHaveText('Heading 1')
    await page.keyboard.press(`${mod}+Alt+Digit4`)
    await editor.document.expectSource(toolbar.replace('Intro.', '#### Intro.'))

    await page.keyboard.type('!')
    await editor.document.expectSource(toolbar.replace('Intro.', '#### In!tro.'))
  })

  test(`${mod}+Alt+0 makes a heading a paragraph`, async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(1, 2)

    await page.keyboard.press(`${mod}+Alt+Digit0`)

    await editor.document.expectSource(toolbar.replace('## Results', 'Results'))
  })

  test('a heading can\'t be made in a quote, as in the menu', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(2, 2)

    await page.keyboard.press(`${mod}+Alt+Digit1`)
    await page.keyboard.type('!')

    await editor.document.expectSource(toolbar.replace('> A quote.', '> A !quote.'))
  })

  test(`${mod}+Shift+8 makes the caret's line a bullet item, and a paragraph again`, async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(3, 2)

    await page.keyboard.press(`${mod}+Shift+Digit8`)
    await editor.document.expectSource(toolbar.replace('End.', '* End.'))
    await expect(editor.listButton()).toHaveAttribute('aria-pressed', 'true')

    await page.keyboard.press(`${mod}+Shift+Digit8`)
    await editor.document.expectSource(toolbar)
  })

  test(`${mod}+Shift+7 makes the caret's line a numbered item`, async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(3, 2)

    await page.keyboard.press(`${mod}+Shift+Digit7`)

    await editor.document.expectSource(toolbar.replace('End.', '1.  End.'))
    await page.keyboard.type('!')
    await editor.document.expectSource(toolbar.replace('End.', '1.  En!d.'))
  })

  test(`${mod}+Shift+9 quotes the caret's line, and unquotes it`, async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(3, 2)

    await page.keyboard.press(`${mod}+Shift+Digit9`)
    await editor.document.expectSource(toolbar.replace('End.', '> End.'))

    await page.keyboard.press(`${mod}+Shift+Digit9`)
    await editor.document.expectSource(toolbar)
  })

  test('a list can\'t be made on a heading, as with the button', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(1, 2)

    await page.keyboard.press(`${mod}+Shift+Digit8`)
    await page.keyboard.type('!')

    await editor.document.expectSource(toolbar.replace('## Results', '## Re!sults'))
  })

  test('in the title block, they change nothing', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaret('title', 5)

    await page.keyboard.press(`${mod}+Alt+Digit1`)
    await page.keyboard.press(`${mod}+Shift+Digit9`)
    await page.keyboard.type('!')

    await editor.document.expectSource(toolbar.replace('Field notes', 'Field! notes'))
  })

  test('undo takes a shortcut\'s change back', async ({ page }) => {
    const editor = await openEditor(page, 'toolbar')
    await editor.placeCaretInLine(0, 2)
    await page.keyboard.press(`${mod}+Alt+Digit2`)
    await editor.document.expectSource(toolbar.replace('Intro.', '## Intro.'))

    await page.keyboard.press(`${mod}+z`)

    await editor.document.expectSource(toolbar)
  })
})
