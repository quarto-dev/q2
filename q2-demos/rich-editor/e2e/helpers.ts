/**
 * What the e2e tests drive: a qmd document on a sync hub, opened in the
 * visual editor by one or more collaborators, each in a browser of their
 * own. The tests read and edit the document's source text directly too
 * (`Document`), as a collaborator in a source editor would.
 *
 * Import `test` from here, not from Playwright: it disconnects each test's
 * collaborators when the test ends.
 */

import { readFileSync } from 'node:fs'
import { test as base, expect, type Frame, type Page } from '@playwright/test'
import { createSyncClient, setSyncLogger, type SyncClient } from '../../../ts-packages/quarto-sync-client/src/index.ts'
import { startTestHub, type TestHub } from '../../../ts-packages/quarto-sync-client/src/test-hub.ts'

export { expect }

/**
 * The keys that take the caret to the start and end of its line. On a Mac,
 * Home and End scroll the page and leave the caret where it is.
 */
export const lineStart = process.platform === 'darwin' ? 'Meta+ArrowLeft' : 'Home'
export const lineEnd = process.platform === 'darwin' ? 'Meta+ArrowRight' : 'End'

export type Field = 'title' | 'subtitle' | 'abstract'

const SELECTORS: Record<Field, string> = {
  title: 'h1.title .qe-field',
  subtitle: 'p.subtitle .qe-field',
  abstract: '.abstract .qe-field',
}

/** The body's editable paragraphs. */
const PARAGRAPH = '#quarto-document-content > p.qe-field'

/** The row of "+" items that add the title-block fields a document doesn't have. */
const WIDGET = '.qe-widget'

const FILE = 'index.qmd'

// The sync client's connection progress isn't the tests' output.
setSyncLogger(() => {})

// One hub per worker process, started when a test first needs it.
let hub: Promise<TestHub> | null = null
const cleanups: (() => Promise<unknown>)[] = []

export const test = base.extend<{ disconnectCollaborators: void }>({
  disconnectCollaborators: [
    async ({}, use) => {
      await use()
      for (const cleanup of cleanups.splice(0).reverse()) await cleanup().catch(() => {})
    },
    { auto: true },
  ],
})

/** The text of `e2e/fixtures/<name>.qmd`. */
export function fixture(name: string): string {
  return readFileSync(new URL(`./fixtures/${name}.qmd`, import.meta.url), 'utf-8')
}

/**
 * A qmd document on the hub, and a collaborator who works on its source
 * text: reads it, and edits it without a visual editor.
 */
export class Document {
  constructor(
    private readonly hub: TestHub,
    private readonly client: SyncClient,
    readonly indexDocId: string,
  ) {}

  /** The document's source text now, as this collaborator has it. */
  source(): string {
    return this.client.getFileContent(FILE) ?? ''
  }

  /** Wait until the source text is `expected`. */
  async expectSource(expected: string) {
    await expect.poll(() => this.source()).toBe(expected)
  }

  /** Replace the first occurrence of `find` in the source text. */
  replace(find: string, replacement: string) {
    const at = this.source().indexOf(find)
    if (at < 0) throw new Error(`The source has no ${JSON.stringify(find)}:\n${this.source()}`)
    this.client.applyEditorOperations(FILE, [{ rangeOffset: at, rangeLength: find.length, text: replacement }])
  }

  /** Open the document in the visual editor on `page`, themed and ready. */
  async open(page: Page): Promise<Editor> {
    // The hub must have the document before a browser asks for it.
    if (!(await this.hub.hubHasHeadsOf(this.client))) throw new Error('The hub never received the document.')
    const query = new URLSearchParams({ server: this.hub.url, doc: this.indexDocId, file: FILE, view: 'visual' })
    await page.goto(`/?${query}`)
    await page.locator('iframe.visual-frame').waitFor()
    const frame = page.frameLocator('iframe.visual-frame')
    // The "+" row is there for every document, with a title block or not.
    await frame.locator(WIDGET).waitFor({ state: 'attached' })
    await expect(page.locator('.visual-editor .status')).not.toContainText('Compiling')
    const handle = await page.locator('iframe.visual-frame').elementHandle()
    const doc = (await handle!.contentFrame())!
    return new Editor(page, doc, this)
  }
}

/** Put a document with the given source text on the hub. */
export async function createDocument(source: string): Promise<Document> {
  hub ??= startTestHub()
  const testHub = await hub
  const client = createSyncClient({
    onFileAdded() {},
    onFileChanged() {},
    onBinaryChanged() {},
    onFileRemoved() {},
    onConnectionChange() {},
    onError(error) {
      console.error('[e2e collaborator]', error.message)
    },
  })
  const project = await client.createNewProject({
    syncServer: testHub.url,
    files: [{ path: FILE, content: source, contentType: 'text' }],
    peerTimeoutMs: 10000,
    storage: 'memory',
  })
  cleanups.push(() => client.disconnect())
  return new Document(testHub, client, project.indexDocId)
}

/** Open the visual editor on a new document made from `e2e/fixtures/<name>.qmd`. */
export async function openEditor(page: Page, name = 'basic'): Promise<Editor> {
  const document = await createDocument(fixture(name))
  return document.open(page)
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

/** One collaborator's visual editor, in a browser page of its own. */
export class Editor {
  constructor(
    readonly page: Page,
    readonly frame: Frame,
    readonly document: Document,
  ) {}

  /** Another collaborator opens the same document, in a browser of their own. */
  async join(): Promise<Editor> {
    const context = await this.page.context().browser()!.newContext()
    cleanups.push(() => context.close())
    return this.document.open(await context.newPage())
  }

  /** Put the caret in a one-line field, `offset` characters in. */
  async placeCaret(field: Field, offset: number) {
    await this.clickAt(field, 5)
    await this.page.keyboard.press(lineStart)
    for (let i = 0; i < offset; i++) await this.page.keyboard.press('ArrowRight')
  }

  /** Put the caret at the end of a field's text, with a click after its last line. */
  async placeCaretAtEnd(field: Field) {
    const box = (await this.field(field).boundingBox())!
    await this.field(field).click({ position: { x: box.width - 2, y: box.height - 4 } })
  }

  /** Select the characters from `from` to `to` in a one-line field. */
  async select(field: Field, from: number, to: number) {
    await this.placeCaret(field, from)
    for (let i = from; i < to; i++) await this.page.keyboard.press('Shift+ArrowRight')
  }

  /** The text the selection covers. */
  selectedText(): Promise<string> {
    return this.frame.evaluate(() => String(document.getSelection() ?? ''))
  }

  /** Wait until the caret is at `offset` in `field`. */
  async expectCaret(field: Field, offset: number) {
    await expect.poll(() => this.caret()).toMatchObject({ field, start: offset, end: offset })
  }

  /** Wait until a field shows `expected` (null: the field is absent). */
  async expectText(field: Field, expected: string | null) {
    await expect.poll(() => this.text(field)).toBe(expected)
  }

  field(field: Field) {
    return this.frame.locator(SELECTORS[field])
  }

  /** A field's text now, or null when the field is absent. */
  text(field: Field): Promise<string | null> {
    // In one step: a collaborator's edit can remove the field at any time.
    return this.frame.evaluate((selector) => document.querySelector(selector)?.textContent ?? null, SELECTORS[field])
  }

  /** Click inside a field, `x` pixels from its left edge on its first line. */
  async clickAt(field: Field, x: number) {
    const box = (await this.field(field).boundingBox())!
    const lineHeight = await this.field(field).evaluate((el) => parseFloat(getComputedStyle(el).lineHeight))
    await this.field(field).click({ position: { x, y: Math.min(box.height, lineHeight) / 2 } })
  }

  /** Click the page below the document, outside every field. */
  async clickAway() {
    const frame = (await this.page.locator('iframe.visual-frame').boundingBox())!
    await this.page.mouse.click(frame.x + frame.width / 2, frame.y + frame.height - 10)
  }

  /** The body's blocks as shown, each as `tag: text`. */
  blocks(): Promise<string[]> {
    return this.frame.evaluate(() =>
      [...document.querySelectorAll('#quarto-document-content > :not(header)')].map(
        (el) => `${el.tagName.toLowerCase()}: ${el.textContent}`,
      ),
    )
  }

  /** The text of each of the body's editable paragraphs. */
  paragraphs(): Promise<string[]> {
    return this.frame.evaluate(
      (selector) => [...document.querySelectorAll(selector)].map((el) => el.textContent ?? ''),
      PARAGRAPH,
    )
  }

  /** Put the caret in the body's `index`th editable paragraph (of one line), `offset` characters in. */
  async placeCaretInParagraph(index: number, offset: number) {
    await this.frame.locator(PARAGRAPH).nth(index).click({ position: { x: 5, y: 5 } })
    await this.page.keyboard.press(lineStart)
    for (let i = 0; i < offset; i++) await this.page.keyboard.press('ArrowRight')
  }

  /** Extend the selection to `offset` characters into the body's `index`th editable paragraph. */
  async shiftClickParagraph(index: number, offset: number) {
    const point = await this.frame.evaluate(
      ([selector, index, offset]) => {
        const text = document.querySelectorAll(selector as string)[index as number].firstChild!
        const range = document.createRange()
        range.setStart(text, offset as number)
        range.setEnd(text, offset as number)
        const rect = range.getBoundingClientRect()
        return { x: rect.left, y: rect.top + rect.height / 2 }
      },
      [PARAGRAPH, index, offset] as const,
    )
    const frame = (await this.page.locator('iframe.visual-frame').boundingBox())!
    await this.page.keyboard.down('Shift')
    await this.page.mouse.click(frame.x + point.x + 1, frame.y + point.y)
    await this.page.keyboard.up('Shift')
  }

  /** Wait until the caret is `offset` characters into the body's `index`th editable paragraph. */
  async expectParagraphCaret(index: number, offset: number) {
    await expect
      .poll(() =>
        this.frame.evaluate((selector) => {
          const selection = document.getSelection()!
          const focused = (document.activeElement as HTMLElement | null)?.isContentEditable === true
          const node = selection.focusNode
          const el = node?.nodeType === Node.ELEMENT_NODE ? (node as Element) : node?.parentElement
          const paragraph = el?.closest(selector)
          if (!focused || !selection.isCollapsed || !paragraph || !node) return null
          const prefix = document.createRange()
          prefix.selectNodeContents(paragraph)
          prefix.setEnd(node, selection.focusOffset)
          return {
            index: [...document.querySelectorAll(selector)].indexOf(paragraph),
            offset: prefix.toString().length,
          }
        }, PARAGRAPH),
      )
      .toEqual({ index, offset })
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

  /** The row of "+" items above the title block. */
  widget() {
    return this.frame.locator(WIDGET)
  }

  /** The "+" item that adds `field`. */
  widgetItem(field: Field) {
    return this.widget().getByRole('button', { name: `+ ${field[0].toUpperCase()}${field.slice(1)}` })
  }

  /** The "+" items' labels, in order. */
  widgetItems(): Promise<string[]> {
    return this.widget().getByRole('button').allTextContents()
  }

  /** Whether the "+" row is showing (it is always there, but invisible when not). */
  widgetShown(): Promise<boolean> {
    return this.widget().evaluate((el) => getComputedStyle(el).opacity === '1')
  }

  /** Wait until the "+" row is showing, or not. */
  async expectWidgetShown(shown: boolean) {
    await expect.poll(() => this.widgetShown()).toBe(shown)
  }

  /** The label of the "+" item with the focus, or null when none has it. */
  focusedWidgetItem(): Promise<string | null> {
    return this.frame.evaluate((selector) => {
      const el = document.activeElement
      return el?.closest(selector) ? (el.textContent ?? '') : null
    }, WIDGET)
  }

  /** Wait until the "+" item labelled `label` has the focus (null: none has it). */
  async expectFocusedWidgetItem(label: string | null) {
    await expect.poll(() => this.focusedWidgetItem()).toBe(label)
  }

  /** Add a field with a click on its "+" item, showing the row first as a person would, by pointing at it. */
  async addField(field: Field) {
    await this.hoverTitleBlock()
    await this.widgetItem(field).click()
  }

  /** Point at the top of the page, where the title block is (or would be). */
  async hoverTitleBlock() {
    const box = (await this.frame.locator('#quarto-document-content').boundingBox())!
    await this.page.mouse.move(box.x + 20, box.y + 4)
  }

  /** Move the pointer off the document. */
  async pointAway() {
    const frame = (await this.page.locator('iframe.visual-frame').boundingBox())!
    await this.page.mouse.move(frame.x + frame.width / 2, frame.y + frame.height - 10)
  }

  /** The top of the body's first block, to detect layout shifts. */
  bodyTop(): Promise<number | null> {
    return this.frame.evaluate(
      () => document.querySelector('#quarto-document-content > :not(header)')?.getBoundingClientRect().top ?? null,
    )
  }

  /** Whether a field's placeholder is showing. */
  placeholderShown(field: Field): Promise<boolean> {
    return this.field(field).evaluate((el) => {
      const content = getComputedStyle(el, '::before').content
      return el.textContent === '' && content !== 'none' && content !== ''
    })
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
      // Set on the event, not given to its constructor: Firefox gives a
      // constructed paste event an empty clipboard of its own.
      const event = new ClipboardEvent('paste', { bubbles: true, cancelable: true })
      Object.defineProperty(event, 'clipboardData', { value: data })
      document.activeElement!.dispatchEvent(event)
    }, text)
  }
}
