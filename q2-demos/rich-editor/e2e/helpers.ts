import { expect, type Frame, type Page } from '@playwright/test'

export type Field = 'title' | 'subtitle' | 'abstract'

const SELECTORS: Record<Field, string> = {
  title: 'h1.title .qe-field',
  subtitle: 'p.subtitle .qe-field',
  abstract: '.abstract .qe-field',
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

export interface Position {
  field: Field
  offset: number
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

  /**
   * Where the caret is, as text offsets within its field. `field` is null
   * when the editor doesn't have focus.
   */
  caret(): Promise<Caret> {
    return this.frame.evaluate((selectors) => {
      const fieldOf = (node: Node | null) => {
        const el = node?.nodeType === Node.ELEMENT_NODE ? (node as Element) : node?.parentElement
        const field = el?.closest('.qe-field')
        const name = Object.entries(selectors).find(([, s]) => field?.matches(s))?.[0] as Field | undefined
        return field && name ? { el: field, name } : null
      }
      const selection = document.getSelection()!
      const focused = (document.activeElement as HTMLElement | null)?.isContentEditable === true
      const range = selection.rangeCount > 0 ? selection.getRangeAt(0) : null
      const field = focused && range ? fieldOf(range.startContainer) : null
      if (!field || !range) return { field: null, start: 0, end: 0, x: null }
      const prefix = document.createRange()
      prefix.selectNodeContents(field.el)
      prefix.setEnd(range.startContainer, range.startOffset)
      const start = prefix.toString().length
      const rects = range.getClientRects()
      return {
        field: field.name,
        start,
        end: start + range.toString().length,
        x: rects.length > 0 ? rects[0].left : null,
      }
    }, SELECTORS)
  }

  /** The selection's ends, as text offsets within their fields. */
  selection(): Promise<{ anchor: Position; focus: Position } | null> {
    return this.frame.evaluate((selectors) => {
      const at = (node: Node | null, offset: number): Position | null => {
        const el = node?.nodeType === Node.ELEMENT_NODE ? (node as Element) : node?.parentElement
        const field = el?.closest('.qe-field')
        const name = Object.entries(selectors).find(([, s]) => field?.matches(s))?.[0] as Field | undefined
        if (!field || !name || !node) return null
        const prefix = document.createRange()
        prefix.selectNodeContents(field)
        prefix.setEnd(node, offset)
        return { field: name, offset: prefix.toString().length }
      }
      const s = document.getSelection()!
      const anchor = at(s.anchorNode, s.anchorOffset)
      const focus = at(s.focusNode, s.focusOffset)
      return anchor && focus ? { anchor, focus } : null
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

  /** Paste plain text at the caret. */
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
