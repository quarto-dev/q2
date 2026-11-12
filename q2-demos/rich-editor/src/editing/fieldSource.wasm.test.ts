/**
 * Runs against the real WASM module (build it first with `npm run build:wasm`).
 */

import { beforeAll, describe, expect, it } from 'vitest'
import { readFile } from 'fs/promises'
import { join } from 'path'
import { createLocalFile } from '../quarto-synced-file/localFile.ts'
import { initWasmFromBytes } from '../quarto-synced-file/wasm.ts'
import { createMetaFieldSource } from './fieldSource.ts'

beforeAll(async () => {
  const bytes = await readFile(
    join(__dirname, '../../wasm-quarto-hub-client/wasm_quarto_hub_client_bg.wasm'),
  )
  await initWasmFromBytes(bytes)
})

const DOC = `---
# draft
title: Old
author: Ana
abstract: |
  One.

  Two.
---

Body.
`

function open(text = DOC) {
  const file = createLocalFile(text)
  const source = createMetaFieldSource(file, { keys: ['title', 'subtitle', 'abstract'], multiline: ['abstract'] })
  return { file, source, text: () => file.getSnapshot().text }
}

describe('createMetaFieldSource', () => {
  it('reads the fields as plain text, leaving out absent ones', () => {
    expect(open().source.get()).toEqual({ title: 'Old', abstract: 'One.\nTwo.' })
  })

  it('writes a field into its own line and reads it back', () => {
    const { source, text } = open()
    source.set({ title: 'New *title*' })
    expect(text()).toBe(DOC.replace('title: Old', 'title: "New \\\\*title\\\\*"'))
    expect(source.get().title).toBe('New *title*')
  })

  it('reads back what the document keeps, not what was written', () => {
    const { source, text } = open()
    source.set({ title: "  It's   new " })
    expect(text()).toBe(DOC.replace('title: Old', 'title: "It\'s new"'))
    expect(source.get().title).toBe('It’s new')
  })

  it('writes nothing when the document already reads that way', () => {
    const { source, text } = open()
    source.set({ title: 'Old ', abstract: 'One.\n\nTwo.\n' })
    expect(text()).toBe(DOC)
  })

  it('removes a field written as empty or undefined', () => {
    const { source, text } = open()
    source.set({ title: ' ', abstract: undefined })
    expect(text()).toBe('---\n# draft\nauthor: Ana\n---\n\nBody.\n')
    expect(source.get()).toEqual({})
  })

  it('writes a paragraph per line', () => {
    const { source, text } = open()
    source.set({ abstract: 'One.\nTwo.\nThree.' })
    expect(text()).toBe(DOC.replace('  Two.\n', '  Two.\n\n  Three.\n'))
  })

  it('gives the same fields object until a field changes', () => {
    const { file, source } = open()
    const before = source.get()
    file.applyEdits([{ rangeOffset: DOC.indexOf('Body'), rangeLength: 4, text: 'Text' }])
    expect(source.get()).toBe(before)
    file.applyEdits([{ rangeOffset: DOC.indexOf('Old'), rangeLength: 3, text: 'New' }])
    expect(source.get()).not.toBe(before)
  })

  it('is not writable, and keeps the last fields, while the front matter is not valid YAML', () => {
    const { file, source, text } = open()
    const listener = { calls: 0 }
    source.subscribe(() => listener.calls++)
    file.applyEdits([{ rangeOffset: DOC.indexOf('Old'), rangeLength: 0, text: '"' }])
    expect(listener.calls).toBe(1)
    expect(source.writable()).toBe(false)
    expect(source.get().title).toBe('Old')

    const broken = text()
    source.set({ title: 'New' })
    expect(text()).toBe(broken)
  })
})

describe('headings in the body', () => {
  const H = (level: number) => String.fromCharCode(0xfdd0 + level)
  const BODY = `---
title: T
---

Intro.

## Results {#res .big}

Middle.

### Plain heading

End.
`
  function openBody(text = BODY) {
    const file = createLocalFile(text)
    const source = createMetaFieldSource(file, { keys: ['title'], body: 'body' })
    return { source, text: () => file.getSnapshot().text }
  }

  it('reads a heading as a line with its level\'s prefix', () => {
    expect(openBody().source.get().body).toBe(`Intro.\n${H(2)}Results\nMiddle.\n${H(3)}Plain heading\nEnd.`)
  })

  it('writes typing in a heading to its own line, keeping its attributes', () => {
    const { source, text } = openBody()
    source.set({ body: `Intro.\n${H(2)}Results so far\nMiddle.\n${H(3)}Plain heading\nEnd.` })
    expect(text()).toBe(BODY.replace('## Results {#res .big}', '## Results so far {#res .big}'))
  })

  it('writes typing in a heading without attributes without making its id up', () => {
    const { source, text } = openBody()
    source.set({ body: `Intro.\n${H(2)}Results\nMiddle.\n${H(3)}Plain headings\nEnd.` })
    expect(text()).toBe(BODY.replace('### Plain heading', '### Plain headings'))
  })

  it('writes a heading whose prefix was removed as a paragraph', () => {
    const { source, text } = openBody()
    source.set({ body: `Intro.\n${H(2)}Results\nMiddle.\nPlain heading\nEnd.` })
    expect(text()).toBe(BODY.replace('### Plain heading', 'Plain heading'))
  })

  it('writes a paragraph given a prefix as a heading', () => {
    const { source, text } = openBody()
    source.set({ body: `Intro.\n${H(2)}Results\n${H(4)}Middle.\n${H(3)}Plain heading\nEnd.` })
    expect(text()).toBe(BODY.replace('Middle.', '#### Middle.'))
  })

  it('writes a new heading, and markdown typed in it as text', () => {
    const { source, text } = openBody()
    source.set({ body: `Intro.\n${H(1)}A *new* # one\n${H(2)}Results\nMiddle.\n${H(3)}Plain heading\nEnd.` })
    expect(text()).toBe(BODY.replace('Intro.\n', 'Intro.\n\n# A \\*new\\* \\# one\n'))
    expect(openBody(text()!).source.get().body).toContain(`${H(1)}A *new* # one\n`)
  })

  it('writes an empty heading, and reads it back as one', () => {
    const { source, text } = openBody()
    source.set({ body: `Intro.\n${H(2)}Results\nMiddle.\n${H(3)}\nEnd.` })
    expect(openBody(text()!).source.get().body).toBe(`Intro.\n${H(2)}Results\nMiddle.\n${H(3)}\nEnd.`)
  })

  it('splits a heading into a heading and a paragraph', () => {
    const { source, text } = openBody()
    source.set({ body: `Intro.\n${H(2)}Results\nMiddle.\n${H(3)}Plain\nheading\nEnd.` })
    expect(text()).toBe(BODY.replace('### Plain heading', '### Plain\n\nheading'))
  })
})
