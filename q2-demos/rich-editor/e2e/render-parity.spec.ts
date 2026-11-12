/**
 * The visual editor shows a document as `q2 render` would: for each
 * fixture, the editor's page holds the same HTML as the rendered page,
 * once the editor's own markup (the elements and attributes that make
 * fields editable) is taken out.
 *
 * The reference is rendered in the same browser by the WASM module's full
 * render pipeline (`render_qmd_content`), whose `<main>` is the same as
 * `q2 render`'s for these fixtures. Only what the editor can edit is
 * compared (the title block and paragraphs of plain text); the read-only
 * blocks are rendered roughly and are left out.
 *
 * The fixtures open in `/fixture.html`, with no hub.
 */

import { readFileSync } from 'node:fs'
import { expect, test, type Page } from '@playwright/test'

/** Fixtures whose rendering should match `q2 render`'s. */
const MATCHING = ['title-block', 'paragraphs', 'long-title', 'no-subtitle', 'wrapped']

// The comparison is of HTML, which doesn't depend on the browser engine.
test.skip(({ browserName }) => browserName !== 'chromium', 'Runs in Chromium only')

for (const name of MATCHING) {
  test(`${name}: the editor's page matches the rendered page`, async ({ page }) => {
    await expectRenderParity(page, name)
  })
}

test('a date is shown formatted, as `q2 render` does', async ({ page }) => {
  test.fixme(true, 'The title block shows the date as written (2026-11-12, not November 12, 2026)')
  await expectRenderParity(page, 'date')
})

test("an abstract's paragraphs are shown as paragraphs", async ({ page }) => {
  test.fixme(true, 'The abstract is one <p>, with a line break between paragraphs')
  await expectRenderParity(page, 'abstract-paragraphs')
})

test('headings are shown in sections, as `q2 render` does', async ({ page }) => {
  test.fixme(
    true,
    'Headings are flat lines: `q2 render` puts each in a <section> with what follows it, and marks them `anchored`',
  )
  await expectRenderParity(page, 'headings')
})

async function expectRenderParity(page: Page, name: string) {
  await page.goto(`/fixture.html?fixture=${name}`)
  const frame = page.frameLocator('iframe.visual-frame')
  await frame.locator('[data-field]').first().waitFor()

  const qmd = readFileSync(new URL(`./fixtures/${name}.qmd`, import.meta.url), 'utf-8')
  const rendered = await page.evaluate(async (qmd) => {
    const wasm = await import('/src/quarto-synced-file/wasm.ts')
    await wasm.initWasm()
    return wasm.renderQmdContent(qmd)
  }, qmd)

  const editor = await frame.locator('body').evaluate(snapshot)
  const reference = await page.evaluate(
    ([html, snapshotSource]) => {
      const doc = new DOMParser().parseFromString(html, 'text/html')
      return (new Function(`return ${snapshotSource}`)() as typeof snapshot)(doc.body)
    },
    [rendered, snapshot.toString()] as const,
  )
  expect(editor).toEqual(reference)
}

/**
 * The page's wrapper elements and its `<main>` as comparable HTML: without
 * the editor's markup (`data-*` attributes, the `qe-field` class, the
 * element a field renders in place of its text, the `<br>` that keeps an
 * empty field a line tall, `contenteditable`, inline styles), and with
 * whitespace collapsed as the browser displays it.
 *
 * Runs in the page; it must not refer to anything outside itself.
 */
function snapshot(body: HTMLElement) {
  const main = body.querySelector('main')
  if (!main) return null
  const clone = main.cloneNode(true) as HTMLElement
  for (const field of [...clone.querySelectorAll('span.qe-field')]) field.replaceWith(...field.childNodes)
  for (const br of [...clone.querySelectorAll('.qe-field > br:last-child')]) br.remove()
  for (const el of [clone, ...clone.querySelectorAll('*')]) {
    for (const { name } of [...el.attributes]) {
      if (name.startsWith('data-') || name === 'contenteditable' || name === 'style') el.removeAttribute(name)
    }
    el.classList.remove('qe-field')
    if (el.getAttribute('class') === '') el.removeAttribute('class')
  }
  const content = body.querySelector('#quarto-content')
  return {
    body: body.className,
    content: content?.className,
    main: { id: main.id, className: main.className },
    html: clone.innerHTML
      .replace(/\s+/g, ' ')
      .replace(/> </g, '><')
      .trim()
      .replace(/></g, '>\n<')
      .split('\n'),
  }
}
