/**
 * Runs against the real WASM module (build it first with `npm run build:wasm`).
 */

import { beforeAll, describe, expect, it } from 'vitest'
import { readFile } from 'fs/promises'
import { join } from 'path'
import { createLocalFile } from '../quarto-synced-file/localFile.ts'
import { initWasmFromBytes } from '../quarto-synced-file/wasm.ts'
import { createMetaFieldSource } from './fieldSource.ts'
import { CODE, EMPH, formatted, HIGHLIGHT, STRONG, UNDERLINE } from './inlines.ts'
import { LINK_CLOSE, LINK_OPEN, LINK_TEXT } from '../engine/marks.ts'

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

  describe('keys that stay when empty (keepEmpty)', () => {
    function openKept(text = DOC) {
      const file = createLocalFile(text)
      const source = createMetaFieldSource(file, {
        keys: ['title', 'subtitle', 'abstract'],
        multiline: ['abstract'],
        keepEmpty: ['title', 'subtitle', 'abstract'],
      })
      return { file, source, text: () => file.getSnapshot().text }
    }

    it('reads one that is empty as empty, not absent', () => {
      const { source } = openKept(DOC.replace('title: Old', 'title: ""'))
      expect(source.get()).toEqual({ title: '', abstract: 'One.\nTwo.' })
    })

    it('writes an emptied one as an empty value, on its line', () => {
      const { source, text } = openKept()
      source.set({ title: ' ' })
      expect(text()).toBe(DOC.replace('title: Old', 'title: ""'))
      expect(source.get().title).toBe('')
    })

    it('adds one, empty', () => {
      const { source, text } = openKept()
      source.set({ subtitle: '' })
      expect(text()).toContain('\nsubtitle: ""\n')
      expect(source.get().subtitle).toBe('')
    })

    it('removes one written as undefined', () => {
      const { source, text } = openKept(DOC.replace('title: Old', 'title: ""'))
      source.set({ title: undefined })
      expect(text()).toBe(DOC.replace('title: Old\n', ''))
      expect(source.get().title).toBeUndefined()
    })

    it('gives a document with no front matter one', () => {
      const { source, text } = openKept('Body.\n')
      source.set({ title: '' })
      expect(text()).toBe('---\ntitle: ""\n---\n\nBody.\n')
      source.set({ title: 'Notes' })
      expect(text()).toBe('---\ntitle: Notes\n---\n\nBody.\n')
    })

    it('gives an empty document a title', () => {
      const { source, text } = openKept('')
      source.set({ title: 'Notes' })
      expect(text()).toContain('title: Notes\n')
      expect(source.get()).toEqual({ title: 'Notes' })
    })
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

describe('numbered lists in the body', () => {
  const N = '﷙'
  const BODY = `---
title: T
---

Intro.

3) One.
4) Two.

End.
`
  function openBody(text = BODY) {
    const file = createLocalFile(text)
    const source = createMetaFieldSource(file, { keys: ['title'], body: 'body' })
    return { source, text: () => file.getSnapshot().text }
  }

  it('reads a numbered list as a line per item, with the numbered prefix', () => {
    expect(openBody().source.get().body).toBe(`Intro.\n${N}One.\n${N}Two.\nEnd.`)
  })

  // The qmd writer writes a changed list whole, two spaces after a number
  // below 10.
  it('writes an edited list from its start number, with its delimiter', () => {
    const { source, text } = openBody()
    source.set({ body: `Intro.\n${N}One.\n${N}Two!\nEnd.` })
    expect(text()).toBe(BODY.replace('3) One.\n4) Two.', '3)  One.\n4)  Two!'))
  })

  it('writes a new list numbered 1. from 1, and an empty item, and reads them back', () => {
    const { source, text } = openBody('---\ntitle: T\n---\n\nIntro.\n')
    source.set({ body: `Intro.\n${N}A\n${N}\n${N}B` })
    expect(text()).toBe('---\ntitle: T\n---\n\nIntro.\n\n1.  A\n2.\n3.  B\n')
    expect(openBody(text()!).source.get().body).toBe(`Intro.\n${N}A\n${N}\n${N}B`)
  })
})

describe('formatting in the body', () => {
  const BODY = `---
title: T
---

Some **bold**, *italic*, [under]{.underline}, [lit]{.mark}, \`code\` and [a link](https://q.org).

End.
`
  const link = (url: string, text: string) => LINK_OPEN + url + LINK_TEXT + text + LINK_CLOSE
  const line = `Some ${formatted(STRONG, 'bold')}, ${formatted(EMPH, 'italic')}, ${formatted(UNDERLINE, 'under')}, ${formatted(HIGHLIGHT, 'lit')}, ${formatted(CODE, 'code')} and ${link('https://q.org', 'a link')}.`
  function openBody(text = BODY) {
    const file = createLocalFile(text)
    const source = createMetaFieldSource(file, { keys: ['title'], body: 'body' })
    return { source, text: () => file.getSnapshot().text }
  }

  it('reads a formatted paragraph as a line with its formatting', () => {
    expect(openBody().source.get().body).toBe(`${line}\nEnd.`)
  })

  it('writes an edit of a formatted paragraph as markdown, and reads it back', () => {
    const { source, text } = openBody()
    const edited = line.replace(formatted(STRONG, 'bold'), formatted(STRONG, 'bolder'))
    source.set({ body: `${edited}\nEnd.` })
    expect(text()).toBe(BODY.replace('**bold**', '**bolder**'))
    expect(openBody(text()!).source.get().body).toBe(`${edited}\nEnd.`)
  })

  it('writes new formatting of every kind', () => {
    const { source, text } = openBody('---\ntitle: T\n---\n\nEnd.\n')
    source.set({ body: `${formatted(STRONG, `a ${formatted(EMPH, 'b')}`)} ${formatted(HIGHLIGHT, 'c')} ${link('u', 'd')}\nEnd.` })
    expect(text()).toBe('---\ntitle: T\n---\n\n**a _b_** [c]{.mark} [d](u)\n\nEnd.\n')
  })

  it("doesn't write an edit that wouldn't read back", () => {
    const { source, text } = openBody('---\ntitle: T\n---\n\nEnd.\n')
    // Italic inside bold, in a word: the qmd writer writes `**a_b_**`.
    source.set({ body: `${formatted(STRONG, `a${formatted(EMPH, 'b')}`)}\nEnd.` })
    expect(text()).toBe('---\ntitle: T\n---\n\nEnd.\n')
  })
})
