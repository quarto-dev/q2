/**
 * The app around the visual editor: which document it opens, the source
 * pane the toolbar's Show source button shows beside the editor, the
 * connection to the hub, the line of status under the editor, and the
 * layout in a narrow window.
 *
 * The document is named in the URL: `?doc=<index document id>&file=<path>`,
 * with `&server=<hub's WebSocket URL>` unless it's the public automerge
 * sync server. Without a document, the page makes one from the tutorial.
 * `/fixture.html` opens a fixture with no hub at all.
 */

import { readFileSync } from 'node:fs'
import { createDocument, expect, fixture, hubUrl, joinDocument, openEditor, test } from './helpers.ts'

const basic = fixture('basic')
const tutorial = readFileSync(new URL('../src/tutorial.qmd', import.meta.url), 'utf-8')

/** The URL of the page `open` went to, with other query parameters. */
function withQuery(url: string, change: (query: URLSearchParams) => void): string {
  const next = new URL(url)
  change(next.searchParams)
  return next.toString()
}

test.describe('which document', () => {
  test('without a document in the URL, a new one is made from the tutorial, on the server, and the URL names it', async ({
    page,
  }) => {
    const server = await hubUrl()

    await page.goto(`/?${new URLSearchParams({ server })}`)

    const title = page.frameLocator('iframe.visual-frame').locator('h1.title')
    await expect(title).toHaveText('Welcome to the Quarto visual editor')
    const query = new URL(page.url()).searchParams
    expect(query.get('server')).toBe(server)
    expect(query.get('file')).toBe('index.qmd')
    // Another collaborator finds it on the server.
    const document = await joinDocument(query.get('doc')!)
    await document.expectSource(tutorial)
    // Reloading opens it again, rather than making another.
    await page.reload()
    await expect(title).toHaveText('Welcome to the Quarto visual editor')
    expect(page.url()).toContain(`doc=${query.get('doc')}`)
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
  /** Open the document, and show the source pane with the Show source button. */
  async function openBothPanes(page: import('@playwright/test').Page, source = basic) {
    const document = await createDocument(source)
    await document.open(page)
    await page.getByRole('button', { name: 'Show source' }).click()
    const frame = page.frameLocator('iframe.visual-frame')
    return { document, frame, sourcePane: page.locator('textarea') }
  }

  test('is not there until the Show source button is pressed, and the editor is a page wide, in the middle', async ({ page }) => {
    await page.setViewportSize({ width: 1400, height: 900 })
    await openEditor(page)

    await expect(page.locator('iframe.visual-frame')).toHaveCount(1)
    await expect(page.locator('textarea')).toHaveCount(0)
    await expect(page.getByRole('button', { name: 'Show source' })).toBeVisible()
    const visual = (await page.locator('.visual-box').boundingBox())!
    expect(Math.abs(visual.width - 816)).toBeLessThan(2)
    expect(Math.abs(visual.x - (1400 - (visual.x + visual.width)))).toBeLessThan(2)
  })

  test('is to the right of the visual editor, and holds the source text', async ({ page }) => {
    const { sourcePane } = await openBothPanes(page)

    // The button is named for what it does now.
    await expect(page.getByRole('button', { name: 'Hide source' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Show source' })).toHaveCount(0)
    await expect(sourcePane).toHaveValue(basic)
    const source = (await sourcePane.boundingBox())!
    const visual = (await page.locator('iframe.visual-frame').boundingBox())!
    expect(visual.x + visual.width).toBeLessThanOrEqual(source.x)
  })

  test('goes when the Hide source button is pressed, and the caret stays where it was', async ({ page }) => {
    const document = await createDocument(basic)
    const editor = await document.open(page)
    await editor.placeCaret('title', 4)

    await page.getByRole('button', { name: 'Show source' }).click()
    await expect(page.locator('textarea')).toHaveCount(1)
    await page.getByRole('button', { name: 'Hide source' }).click()

    await expect(page.locator('textarea')).toHaveCount(0)
    await expect(page.getByRole('button', { name: 'Show source' })).toBeVisible()
    await editor.expectCaret('title', 4)
  })

  test('is not offered on a fixture, which has no source pane', async ({ page }) => {
    await page.goto('/fixture.html')

    await expect(page.frameLocator('iframe.visual-frame').locator('h1.title')).toBeVisible()
    await expect(page.getByRole('button', { name: /source/ })).toHaveCount(0)
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

test.describe('the connection to the hub', () => {
  /** Route the page's connections to the hub through the test, which can cut them. */
  async function cuttableHub(page: import('@playwright/test').Page) {
    const open: { close: () => Promise<void> }[] = []
    let down = false
    await page.routeWebSocket(/\/\/127\.0\.0\.1:\d+\/ws/, (ws) => {
      if (down) {
        void ws.close()
        return
      }
      const server = ws.connectToServer()
      open.push({ close: () => server.close() })
    })
    return {
      /** Close every connection, and refuse new ones until `restore`. */
      async cut() {
        down = true
        for (const socket of open.splice(0)) await socket.close()
      },
      /** Close every connection, and let the page connect again. */
      async drop() {
        for (const socket of open.splice(0)) await socket.close()
      },
      restore() {
        down = false
      },
    }
  }

  /** Open the page of a real document, on a hub that isn't there. */
  async function openOnNoHub(page: import('@playwright/test').Page) {
    await (await createDocument(basic)).open(page)
    await page.goto(withQuery(page.url(), (query) => query.set('server', 'ws://127.0.0.1:1/ws')))
  }

  test('while connecting, the page says so', async ({ page }) => {
    await openOnNoHub(page)

    await expect(page.locator('.status')).toHaveText('Connecting to ws://127.0.0.1:1/ws…')
    await expect(page.locator('iframe')).toHaveCount(0)
  })

  // Today it says it's connecting for 10–15 s, then "Disconnected." for ever.
  test.fixme('a hub that can\'t be reached is an error that names it', async ({ page }) => {
    test.setTimeout(60000)
    await openOnNoHub(page)

    await expect(page.locator('.status.status-error')).toContainText('ws://127.0.0.1:1/ws', { timeout: 30000 })
  })

  test('when the connection drops, the editor stays, and catches up with the document once it reconnects', async ({ page }) => {
    const hub = await cuttableHub(page)
    const document = await createDocument(basic)
    await document.open(page)
    const title = page.frameLocator('iframe.visual-frame').locator('h1.title')

    await hub.drop()
    document.replace('Quarterly report', 'Annual report')

    // It tries again after a few seconds; the editor stays all along.
    await expect(page.locator('.visual-editor[data-offline]')).toHaveCount(1)
    await expect(page.locator('iframe.visual-frame')).toHaveCount(1)
    await expect(title).toHaveText('Annual report', { timeout: 15000 })
    await expect(page.locator('.visual-editor[data-offline]')).toHaveCount(0)
    await page.frameLocator('iframe.visual-frame').locator('h1.title .qe-field').click()
    await page.keyboard.type('!')
    await expect.poll(() => document.source()).toContain('!')
  })

  test('while the connection is down, the editor stays editable, with its caret, saying nothing, and what was typed reaches the document once it is back', async ({ page }) => {
    const hub = await cuttableHub(page)
    const document = await createDocument(basic)
    const editor = await document.open(page)
    await editor.placeCaret('title', 4)

    await hub.cut()

    await expect(page.locator('.visual-editor[data-offline]')).toHaveCount(1)
    // Nothing says so on the page (yet).
    await expect(page.locator('.visual-editor .status')).toHaveText('')
    await editor.expectCaret('title', 4)
    await page.keyboard.type('x')
    expect(await editor.text('title')).toBe('Quarxterly report')
    expect(document.source()).toContain('title: Quarterly report')

    hub.restore()
    await expect(page.locator('.visual-editor[data-offline]')).toHaveCount(0, { timeout: 15000 })
    await expect.poll(() => document.source()).toContain('title: Quarxterly report')
    await editor.expectCaret('title', 5)
  })

  test('edits made on both sides while the connection is down are merged once it is back', async ({ page }) => {
    const hub = await cuttableHub(page)
    const document = await createDocument(basic)
    const editor = await document.open(page)
    await editor.placeCaret('title', 4)

    await hub.cut()
    await expect(page.locator('.visual-editor[data-offline]')).toHaveCount(1)
    await page.keyboard.type('x')
    document.replace('Third quarter', 'Third quarter, revised')

    hub.restore()
    await expect.poll(() => document.source(), { timeout: 15000 }).toContain('title: Quarxterly report')
    expect(document.source()).toContain('subtitle: Third quarter, revised')
    await expect(page.frameLocator('iframe.visual-frame').locator('p.subtitle')).toHaveText('Third quarter, revised')
    await editor.expectCaret('title', 5)
  })
})

test.describe('a narrow window', () => {
  async function openNarrow(page: import('@playwright/test').Page) {
    const document = await createDocument(basic)
    await page.setViewportSize({ width: 700, height: 800 })
    await document.open(page)
    await page.getByRole('button', { name: 'Show source' }).click()
  }

  test('has the source pane below the visual editor, each as wide as the window', async ({ page }) => {
    await openNarrow(page)

    const source = (await page.locator('textarea').boundingBox())!
    const visual = (await page.locator('iframe.visual-frame').boundingBox())!
    expect(visual.y + visual.height).toBeLessThanOrEqual(source.y)
    expect(Math.abs(source.x - visual.x)).toBeLessThan(2)
    expect(Math.abs(source.width - visual.width)).toBeLessThan(4)
  })

  // Today it is a few lines tall (68px).
  test.fixme('has a source pane tall enough to read', async ({ page }) => {
    await openNarrow(page)

    expect((await page.locator('textarea').boundingBox())!.height).toBeGreaterThanOrEqual(300)
  })
})

test.describe('the status line under the visual editor', () => {
  test('is empty while all is well', async ({ page }) => {
    await openEditor(page)

    await expect(page.locator('.visual-editor .status')).toHaveText('')
  })

  test('says so while the theme is being worked out, when the page is not styled yet', async ({ page }) => {
    await page.goto('/fixture.html')
    const status = page.locator('.visual-editor .status')

    await expect(status).toContainText('Compiling theme CSS…')
    await expect(status).not.toContainText('Compiling theme CSS…')
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
