/**
 * The app around the visual editor: which document it opens, the source
 * pane beside the editor, and the line of status under the editor.
 *
 * The document is named in the URL: `?doc=<index document id>&file=<path>`,
 * with `&server=<hub's WebSocket URL>` unless the hub is the local one, and
 * `&view=visual` for the visual editor alone. `/fixture.html` opens a
 * fixture with no hub at all.
 */

import { createDocument, expect, fixture, openEditor, test } from './helpers.ts'

const basic = fixture('basic')

/** The URL of the page `open` went to, with other query parameters. */
function withQuery(url: string, change: (query: URLSearchParams) => void): string {
  const next = new URL(url)
  change(next.searchParams)
  return next.toString()
}

test.describe('which document', () => {
  test('without a document in the URL, the page says how to name one', async ({ page }) => {
    await page.goto('/')

    await expect(page.locator('.status')).toHaveText(
      'Add ?doc=<index-doc-id>&file=<path.qmd> to the URL (optionally &server=<ws-url>, default ws://127.0.0.1:8080/ws).',
    )
    await expect(page.locator('iframe')).toHaveCount(0)
  })

  test('a file the project does not have is waited for', async ({ page }) => {
    const editor = await openEditor(page)

    await page.goto(withQuery(page.url(), (query) => query.set('file', 'other.qmd')))

    await expect(page.locator('.status')).toHaveText('Waiting for other.qmd…')
    await expect(page.locator('iframe')).toHaveCount(0)
    expect(editor.document.source()).toBe(basic)
  })

  test('a document id that is not one is an error', async ({ page }) => {
    await openEditor(page)

    await page.goto(withQuery(page.url(), (query) => query.set('doc', 'not-a-document')))

    await expect(page.locator('.status.status-error')).toContainText('Connection failed')
  })
})

test.describe('the source pane', () => {
  /** Open the document with both panes: the page `open` shows has the visual editor alone. */
  async function openBothPanes(page: import('@playwright/test').Page, source = basic) {
    const document = await createDocument(source)
    await document.open(page)
    await page.goto(withQuery(page.url(), (query) => query.delete('view')))
    const frame = page.frameLocator('iframe.visual-frame')
    await frame.locator('h1.title').waitFor()
    return { document, frame, sourcePane: page.locator('textarea') }
  }

  test('is to the left of the visual editor, and holds the source text', async ({ page }) => {
    const { sourcePane } = await openBothPanes(page)

    await expect(sourcePane).toHaveValue(basic)
    const source = (await sourcePane.boundingBox())!
    const visual = (await page.locator('iframe.visual-frame').boundingBox())!
    expect(source.x + source.width).toBeLessThanOrEqual(visual.x)
  })

  test('is not there with view=visual', async ({ page }) => {
    await openEditor(page)

    await expect(page.locator('iframe.visual-frame')).toHaveCount(1)
    await expect(page.locator('textarea')).toHaveCount(0)
  })

  test('the caret starts in the visual editor, at the start of the title', async ({ page }) => {
    const { document } = await openBothPanes(page)

    await page.keyboard.type('The ')

    await document.expectSource(basic.replace('Quarterly report', 'The Quarterly report'))
  })

  test('what is typed in it shows in the visual editor, and is in the document', async ({ page }) => {
    const { document, frame, sourcePane } = await openBothPanes(page)
    await sourcePane.evaluate((el: HTMLTextAreaElement) => {
      el.focus()
      const at = el.value.indexOf('report')
      el.setSelectionRange(at, at)
    })

    await page.keyboard.type('sales ')

    await expect(frame.locator('h1.title')).toHaveText('Quarterly sales report')
    await expect(sourcePane).toHaveValue(basic.replace('Quarterly report', 'Quarterly sales report'))
    await document.expectSource(basic.replace('Quarterly report', 'Quarterly sales report'))
  })

  test('what is typed in the visual editor shows in it', async ({ page }) => {
    const { frame, sourcePane } = await openBothPanes(page)
    await frame.locator('p.subtitle .qe-field').click()
    await page.keyboard.press('ControlOrMeta+a')

    await page.keyboard.type('Fourth quarter')

    await expect(sourcePane).toHaveValue(basic.replace('Third quarter', 'Fourth quarter'))
  })

  test('says so while the source does not parse, and both panes keep working when it does again', async ({
    page,
  }) => {
    const { document, frame, sourcePane } = await openBothPanes(page)
    const parseFailed = page.locator('.editor .status')
    await expect(parseFailed).toHaveCount(0)
    await sourcePane.evaluate((el: HTMLTextAreaElement) => {
      el.focus()
      const at = el.value.indexOf('Quarterly')
      el.setSelectionRange(at, at)
    })

    // The title's quote is left open.
    await page.keyboard.type('"')
    await expect(parseFailed).toHaveText('Parse failed; showing the last good AST.')
    await expect(page.locator('.visual-editor .status')).toContainText("Read-only: the source doesn't parse.")
    await expect(frame.locator('h1.title')).toHaveText('Quarterly report')

    await page.keyboard.press('Backspace')
    await expect(parseFailed).toHaveCount(0)
    await expect(page.locator('.visual-editor .status')).not.toContainText('Read-only')
    await document.expectSource(basic)
  })
})

test.describe('the status line under the visual editor', () => {
  test('says how long the title block took to work out', async ({ page }) => {
    await openEditor(page)

    await expect(page.locator('.visual-editor .status')).toHaveText(/^Title block derived in \d+\.\d\d ms\./)
  })

  test('says so while the theme is being worked out, when the page is not styled yet', async ({ page }) => {
    await page.goto('/fixture.html')
    const status = page.locator('.visual-editor .status')

    await expect(status).toContainText('Compiling theme CSS…')
    await expect(status).not.toContainText('Compiling theme CSS…')
  })

  test('lists what is wrong with the authors', async ({ page }) => {
    const document = await createDocument(
      '---\ntitle: Notes\nauthor:\n  - name: Ana Silva\n    affiliations:\n      - ref: nowhere\n---\n',
    )
    await document.open(page)

    await expect(page.locator('.visual-editor .status')).toContainText(
      "Issues: Undefined affiliation 'nowhere' for author 'Ana Silva'.",
    )
  })
})

test.describe('a fixture, with no hub', () => {
  test('opens in the visual editor alone, and can be edited', async ({ page }) => {
    await page.goto('/fixture.html?fixture=no-subtitle')
    const frame = page.frameLocator('iframe.visual-frame')
    await expect(page.locator('textarea')).toHaveCount(0)
    await expect(frame.locator('h1.title')).toHaveText('Quarterly report')

    // The caret starts in the title here too.
    await page.keyboard.type('The ')
    await frame.getByRole('button', { name: '+ Subtitle' }).click()
    await page.keyboard.type('Draft')

    await expect(frame.locator('h1.title')).toHaveText('The Quarterly report')
    await expect(frame.locator('p.subtitle')).toHaveText('Draft')
  })

  test('is `basic` unless another is named', async ({ page }) => {
    await page.goto('/fixture.html')

    await expect(page.frameLocator('iframe.visual-frame').locator('p.subtitle')).toHaveText('Third quarter')
  })

  test('that does not exist is an error', async ({ page }) => {
    await page.goto('/fixture.html?fixture=nope')

    await expect(page.locator('.status.status-error')).toHaveText('No fixture named nope.')
  })
})
