import { expect, type Frame, type Page } from '@playwright/test'

export type Field = 'title' | 'subtitle'

const SELECTORS: Record<Field, string> = {
  title: 'h1.title .qe-field',
  subtitle: 'p.subtitle .qe-field',
}

/** Open the visual editor on `e2e/fixtures/<fixture>.qmd`, themed and ready. */
export async function openEditor(page: Page, fixture = 'basic') {
  await page.goto(`/fixture.html?fixture=${fixture}`)
  await page.locator('iframe.visual-frame').waitFor()
  const frame = page.frameLocator('iframe.visual-frame')
  await frame.locator('#title-block-header').waitFor()
  await expect(page.locator('.visual-editor .status')).not.toContainText('Compiling')
  const handle = await page.locator('iframe.visual-frame').elementHandle()
  const doc = (await handle!.contentFrame())!
  return new Editor(page, doc)
}

export interface Caret {
  /** The focused field, or null when no field has focus. */
  field: Field | null
  start: number
  end: number
  /** The caret's horizontal position in the frame, when it has one. */
  x: number | null
}

export class Editor {
  constructor(
    readonly page: Page,
    readonly frame: Frame,
  ) {}

  field(field: Field) {
    return this.frame.locator(SELECTORS[field])
  }

  async text(field: Field): Promise<string | null> {
    const locator = this.field(field)
    return (await locator.count()) === 0 ? null : await locator.textContent()
  }

  /** Click inside a field, `x` pixels from its left edge on its first line. */
  async clickAt(field: Field, x: number) {
    const box = (await this.field(field).boundingBox())!
    const lineHeight = await this.field(field).evaluate((el) => parseFloat(getComputedStyle(el).lineHeight))
    await this.field(field).click({ position: { x, y: Math.min(box.height, lineHeight) / 2 } })
  }

  /** Click the page below the title block, outside every field. */
  async clickAway() {
    const box = (await this.frame.locator('#title-block-header').boundingBox())!
    await this.page.mouse.click(box.x + 5, box.y + box.height + 40)
  }

  /** Where the caret is, as text offsets within its field. */
  caret(): Promise<Caret> {
    return this.frame.evaluate((selectors) => {
      const active = document.activeElement
      const field =
        (Object.entries(selectors).find(([, s]) => active?.matches(s))?.[0] as Field | undefined) ?? null
      const selection = document.getSelection()!
      if (!field || selection.rangeCount === 0) return { field, start: 0, end: 0, x: null }
      const range = selection.getRangeAt(0)
      const prefix = document.createRange()
      prefix.selectNodeContents(active!)
      prefix.setEnd(range.startContainer, range.startOffset)
      const start = prefix.toString().length
      const rects = range.getClientRects()
      return {
        field,
        start,
        end: start + range.toString().length,
        x: rects.length > 0 ? rects[0].left : null,
      }
    }, SELECTORS)
  }

  /** The bounding boxes of everything in the title block, to detect layout shifts. */
  layout(): Promise<string[]> {
    return this.frame.evaluate(() =>
      [...document.querySelectorAll('#title-block-header, #title-block-header *')].map((el) => {
        const r = el.getBoundingClientRect()
        return `${el.tagName}.${el.className} ${r.x},${r.y},${r.width},${r.height}`
      }),
    )
  }

  /** The number of visual lines a field occupies. */
  lineCount(field: Field): Promise<number> {
    return this.field(field).evaluate((el) => {
      const lh = parseFloat(getComputedStyle(el).lineHeight)
      return Math.round(el.getBoundingClientRect().height / lh)
    })
  }

  /** Paste plain text into the focused field. */
  paste(text: string) {
    return this.frame.evaluate((text) => {
      const data = new DataTransfer()
      data.setData('text/plain', text)
      document.activeElement!.dispatchEvent(
        new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }),
      )
    }, text)
  }
}
