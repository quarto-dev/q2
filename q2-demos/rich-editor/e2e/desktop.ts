/**
 * The desktop app's page (`desktop.html`) in a browser, with the test as
 * the app's Rust side (`src-tauri/`): a file on disk that the page opens
 * with `open_file` and saves with `save_file`, and `file-changed` events
 * when the file changes on disk. The Rust side's own tests cover how it
 * does those things; these cover the page's use of them.
 *
 * The page talks to Tauri through `window.__TAURI_INTERNALS__`, which the
 * init script below provides, as `@tauri-apps/api/mocks` does.
 */

import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { expect, type Frame, type Page } from '@playwright/test'

export class DesktopFile {
  constructor(
    readonly page: Page,
    readonly frame: Frame,
    readonly path: string,
  ) {}

  /** The file's text on disk now. */
  text(): string {
    return readFileSync(this.path, 'utf8')
  }

  /** Wait until the file on disk holds `expected`. */
  async expectText(expected: string) {
    await expect.poll(() => this.text()).toBe(expected)
  }

  /** Something other than the app changes the file, and the app notices. */
  async changeOnDisk(text: string) {
    writeFileSync(this.path, text)
    await this.page.evaluate((t) => (window as unknown as FakeTauri).__fakeTauriEmit('file-changed', t), text)
  }

  /** The window's title, as the page set it. */
  windowTitle(): Promise<string | null> {
    return this.page.evaluate(() => (window as unknown as FakeTauri).__fakeTauriTitle)
  }
}

interface FakeTauri {
  __fakeTauriEmit(event: string, payload: unknown): void
  __fakeTauriTitle: string | null
}

/** Open the desktop page on a new file named `name` holding `text`. */
export async function openDesktop(page: Page, name: string, text: string): Promise<DesktopFile> {
  const path = join(mkdtempSync(join(tmpdir(), 'rich-editor-desktop-')), name)
  writeFileSync(path, text)
  await page.exposeFunction('__fakeTauriOpenFile', () => ({ path, name, text: readFileSync(path, 'utf8') }))
  await page.exposeFunction('__fakeTauriSaveFile', (saved: string) => writeFileSync(path, saved))
  await page.addInitScript(() => {
    type Callback = (data: unknown) => void
    const w = window as unknown as Record<string, any>
    const callbacks = new Map<number, Callback>()
    const listeners = new Map<string, number[]>()
    let nextId = 1
    w.__fakeTauriTitle = null
    w.__fakeTauriEmit = (event: string, payload: unknown) => {
      for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ event, id, payload })
    }
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener(event: string, id: number) {
        listeners.set(event, (listeners.get(event) ?? []).filter((l) => l !== id))
        callbacks.delete(id)
      },
    }
    w.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } },
      transformCallback(callback: Callback, once = false) {
        const id = nextId++
        callbacks.set(id, (data) => {
          if (once) callbacks.delete(id)
          callback(data)
        })
        return id
      },
      unregisterCallback: (id: number) => callbacks.delete(id),
      async invoke(cmd: string, args: Record<string, any>) {
        switch (cmd) {
          case 'open_file':
            return w.__fakeTauriOpenFile()
          case 'save_file':
            return w.__fakeTauriSaveFile(args.text)
          case 'plugin:event|listen':
            listeners.set(args.event, [...(listeners.get(args.event) ?? []), args.handler])
            return args.handler
          case 'plugin:event|unlisten':
            return null
          case 'plugin:window|set_title':
            w.__fakeTauriTitle = args.value
            return null
          default:
            throw new Error(`The fake Tauri has no command ${cmd}`)
        }
      },
    }
  })
  await page.goto('/desktop.html')
  await page.locator('.visual-editor[data-ready]').waitFor()
  const handle = await page.locator('iframe.visual-frame').elementHandle()
  return new DesktopFile(page, (await handle!.contentFrame())!, path)
}
