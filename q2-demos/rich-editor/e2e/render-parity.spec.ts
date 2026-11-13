/**
 * The visual editor shows a document as `q2 render` would: for each
 * fixture, the editor's page holds the same HTML as the rendered page,
 * once the editor's own markup (the elements and attributes that make
 * fields editable) is taken out, and with the default theme's stylesheet
 * every element is the same size in the same place.
 *
 * The reference is rendered in the same browser by the WASM module's full
 * render pipeline (`render_qmd_content`), whose `<main>` is the same as
 * `q2 render`'s for these fixtures. Only what the editor can edit is
 * compared (the title block, and paragraphs and quotes of plain text); the read-only
 * blocks are rendered roughly and are left out.
 *
 * The fixtures open in `/fixture.html`, with no hub.
 */

import { readFileSync } from 'node:fs'
import { expect, test, type Page } from '@playwright/test'

/** Fixtures whose rendering should match `q2 render`'s. */
const MATCHING = ['title-block', 'paragraphs', 'long-title', 'no-subtitle', 'wrapped', 'quote', 'rules']

test.describe('the HTML', () => {
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
})

test.describe('the layout', () => {
  for (const name of MATCHING) {
    test(`${name}: everything is the size and in the place it is in the rendered page`, async ({ page }) => {
      await expectLayoutParity(page, name)
    })
  }

  test('the page has the default theme', async ({ page }) => {
    const frame = await openFixture(page, 'title-block')

    // A few of the things the theme decides.
    const style = (selector: string) =>
      frame.locator(selector).evaluate((el) => {
        const s = getComputedStyle(el)
        return { size: s.fontSize, weight: s.fontWeight, display: s.display }
      })
    expect(await style('body')).toEqual({ size: '17px', weight: '400', display: 'block' })
    expect(await style('h1.title')).toEqual({ size: '34px', weight: '600', display: 'block' })
    expect(await style('p.subtitle')).toEqual({ size: '21.25px', weight: '300', display: 'block' })
    expect(await style('.quarto-title-meta-author')).toMatchObject({ display: 'grid' })
    // The article's column is narrower than the page, and centred in it.
    const main = (await frame.locator('main').boundingBox())!
    const body = (await frame.locator('body').boundingBox())!
    expect(main.width).toBeLessThan(body.width - 100)
    expect(Math.abs(main.x - body.x - (body.x + body.width - main.x - main.width))).toBeLessThan(2)
  })
})

test('headings are shown in sections, as `q2 render` does', async ({ page }) => {
  test.fixme(
    true,
    'Headings are flat lines: `q2 render` puts each in a <section> with what follows it, and marks them `anchored`',
  )
  await expectRenderParity(page, 'headings')
})

test.fixme('a document with a theme of its own has that theme', async ({ page }) => {
  const frame = await openFixture(page, 'themed')

  // Today every document has the default theme. (`darkly` is dark.)
  expect(await frame.locator('body').evaluate((el) => getComputedStyle(el).backgroundColor)).toBe('rgb(34, 34, 34)')
})

/** Open a fixture in the visual editor, themed and ready. */
async function openFixture(page: Page, name: string) {
  await page.goto(`/fixture.html?fixture=${name}`)
  const frame = page.frameLocator('iframe.visual-frame')
  await frame.locator('[data-field]').first().waitFor()
  await expect(page.locator('.visual-editor .status')).not.toContainText('Compiling')
  return frame
}

/** The fixture rendered to a page of HTML, as `q2 render` renders it. */
function render(page: Page, name: string): Promise<string> {
  const qmd = readFileSync(new URL(`./fixtures/${name}.qmd`, import.meta.url), 'utf-8')
  return page.evaluate(async (qmd) => {
    const wasm = await import('/src/quarto-synced-file/wasm.ts')
    await wasm.initWasm()
    return wasm.renderQmdContent(qmd)
  }, qmd)
}

/**
 * The rendered page, with the default theme's stylesheet, goes in a frame
 * of its own, as large as the editor's; every element of its `<main>` is
 * then where the editor has it.
 */
async function expectLayoutParity(page: Page, name: string) {
  const frame = await openFixture(page, name)
  const rendered = await render(page, name)
  await page.evaluate(async (html) => {
    const wasm = await import('/src/quarto-synced-file/wasm.ts')
    // (Without the byte-order mark the stylesheet's text starts with.)
    const css = (await wasm.compileDefaultThemeCss()).replace(/^\uFEFF/, '')
    const doc = new DOMParser().parseFromString(html, 'text/html')
    for (const el of [...doc.querySelectorAll('link[rel=stylesheet], script')]) el.remove()
    const style = doc.createElement('style')
    style.textContent = css
    doc.head.append(style)
    const editor = document.querySelector('iframe.visual-frame')!.getBoundingClientRect()
    const reference = document.createElement('iframe')
    reference.id = 'reference'
    reference.style.cssText = `position: fixed; left: 0; top: 0; border: 0; width: ${editor.width}px; height: ${editor.height}px; visibility: hidden;`
    // The editor's frame has a border.
    const visual = document.querySelector<HTMLElement>('iframe.visual-frame')!
    visual.style.border = '0'
    reference.style.width = `${visual.getBoundingClientRect().width}px`
    reference.style.height = `${visual.getBoundingClientRect().height}px`
    reference.srcdoc = `<!doctype html>${doc.documentElement.outerHTML}`
    await new Promise((loaded) => {
      reference.addEventListener('load', loaded, { once: true })
      document.body.append(reference)
    })
  }, rendered)

  const reference = await page.frameLocator('#reference').locator('body').evaluate(boxes)
  await expect.poll(() => frame.locator('body').evaluate(boxes)).toEqual(reference)
}

/**
 * Where everything in the page's `<main>` is: a line for each element
 * (other than the editor's own, which a field renders in place of its
 * text), with its box to the nearest half pixel.
 *
 * Runs in the page; it must not refer to anything outside itself.
 */
function boxes(body: HTMLElement) {
  const main = body.querySelector('main')
  if (!main) return null
  const half = (n: number) => Math.round(n * 2) / 2
  return [main, ...main.querySelectorAll('*')]
    .filter((el) => !el.matches('span.qe-field, .qe-field > br'))
    .map((el) => {
      const r = el.getBoundingClientRect()
      const classes = [...el.classList].filter((c) => c !== 'qe-field').join('.')
      return `${el.tagName.toLowerCase()}${classes && '.' + classes} ${half(r.x)},${half(r.y)} ${half(r.width)}x${half(r.height)}`
    })
}

async function expectRenderParity(page: Page, name: string) {
  const frame = await openFixture(page, name)
  const rendered = await render(page, name)

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
