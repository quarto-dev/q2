/**
 * Runs against the real WASM module (build it first with `npm run build:wasm`).
 */

import { beforeAll, describe, expect, it } from 'vitest'
import { readFile } from 'fs/promises'
import { join } from 'path'
import { createDiskFile } from './diskFile.ts'
import { initWasmFromBytes } from './wasm.ts'

beforeAll(async () => {
  const bytes = await readFile(join(__dirname, '../../wasm-quarto-hub-client/wasm_quarto_hub_client_bg.wasm'))
  await initWasmFromBytes(bytes)
})

const DOC = 'One.\n\nTwo.\n'

/** A file whose saves finish at once, and the texts it saved. */
function open(text = DOC) {
  const saved: string[] = []
  const file = createDiskFile(text, async (t) => {
    saved.push(t)
  })
  return { file, saved, text: () => file.getSnapshot().text }
}

/** A file whose saves finish only when the test says so. */
function openSlow(text = DOC) {
  const saves: { text: string; done: () => void }[] = []
  const file = createDiskFile(text, (t) => new Promise<void>((done) => saves.push({ text: t, done })))
  return { file, saves, text: () => file.getSnapshot().text }
}

const tick = () => new Promise((r) => setTimeout(r, 0))

describe('createDiskFile', () => {
  it('starts with the text, parsed, and saves nothing', () => {
    const { file, saved } = open()
    expect(file.getSnapshot()).toMatchObject({ text: DOC, parseError: false, connected: true, connecting: false })
    expect(file.getSnapshot().ast).not.toBeNull()
    expect(saved).toEqual([])
  })

  it('saves each edit', () => {
    const { file, saved, text } = open()
    file.applyEdits([{ rangeOffset: 4, rangeLength: 0, text: '!' }])
    expect(text()).toBe('One.!\n\nTwo.\n')
    expect(saved).toEqual(['One.!\n\nTwo.\n'])
  })

  it('takes a change made on disk, and notifies, without saving it back', () => {
    const { file, saved, text } = open()
    let notified = 0
    file.subscribe(() => notified++)
    file.applyDiskText('One.\n\nTwo, changed.\n')
    expect(text()).toBe('One.\n\nTwo, changed.\n')
    expect(notified).toBe(1)
    expect(saved).toEqual([])
  })

  it('ignores the disk having what it already has', () => {
    const { file, saved } = open()
    let notified = 0
    file.subscribe(() => notified++)
    file.applyDiskText(DOC)
    expect(notified).toBe(0)
    expect(saved).toEqual([])
  })

  it('goes on from a change made on disk', () => {
    const { file, saved, text } = open()
    file.applyDiskText('Zero.\n\nOne.\n\nTwo.\n')
    file.applyEdits([{ rangeOffset: text()!.length - 1, rangeLength: 0, text: '!' }])
    expect(text()).toBe('Zero.\n\nOne.\n\nTwo.!\n')
    expect(saved).toEqual(['Zero.\n\nOne.\n\nTwo.!\n'])
  })

  it('saves one text at a time, the latest', async () => {
    const { file, saves } = openSlow()
    file.applyEdits([{ rangeOffset: 4, rangeLength: 0, text: '!' }])
    file.applyEdits([{ rangeOffset: 5, rangeLength: 0, text: '?' }])
    file.applyEdits([{ rangeOffset: 6, rangeLength: 0, text: '.' }])
    expect(saves.map((s) => s.text)).toEqual(['One.!\n\nTwo.\n'])
    saves[0].done()
    await tick()
    expect(saves.map((s) => s.text)).toEqual(['One.!\n\nTwo.\n', 'One.!?.\n\nTwo.\n'])
  })

  it('merges a change made on disk with edits not saved yet, and saves the result', async () => {
    const { file, saves, text } = openSlow()
    file.applyEdits([{ rangeOffset: 4, rangeLength: 0, text: '!' }]) // saving
    file.applyEdits([{ rangeOffset: 0, rangeLength: 0, text: '>' }]) // waits for that save
    // The disk changes, from what the first save wrote.
    file.applyDiskText('One.!\n\nTwo, changed.\n')
    expect(text()).toBe('>One.!\n\nTwo, changed.\n')
    saves[0].done()
    await tick()
    expect(saves.at(-1)!.text).toBe('>One.!\n\nTwo, changed.\n')
  })

  it('keeps the last good parse when the text stops parsing', () => {
    const { file } = open()
    const ast = file.getSnapshot().ast
    file.applyDiskText('---\ntitle: [\n---\n')
    expect(file.getSnapshot().parseError).toBe(true)
    expect(file.getSnapshot().ast).toBe(ast)
  })
})
