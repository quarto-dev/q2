/**
 * Runs against the real WASM module (build it first with `npm run build:wasm`).
 */

import { beforeAll, describe, expect, it } from 'vitest'
import { readFile } from 'fs/promises'
import { join } from 'path'
import {
  deriveTitleBlockMeta,
  initWasmFromBytes,
  parseQmdContent,
} from '../quarto-synced-file/wasm.ts'
import { metaText, setMetaText } from './meta.ts'

type MetaValue = { t: string; c: unknown }

function text(v: unknown): string | undefined {
  const m = v as MetaValue | undefined
  if (m?.t === 'MetaString') return m.c as string
  if (m?.t === 'MetaInlines') {
    return (m.c as MetaValue[]).map((i) => (i.t === 'Str' ? i.c : i.t === 'Space' ? ' ' : '')).join('')
  }
  return undefined
}

beforeAll(async () => {
  const bytes = await readFile(
    join(__dirname, '../../wasm-quarto-hub-client/wasm_quarto_hub_client_bg.wasm'),
  )
  await initWasmFromBytes(bytes)
})

const ast = () => parseQmdContent('---\ntitle: Old\nauthor: Ana\n---\n\nBody.\n')!

describe('setMetaText', () => {
  it('sets a text field that the title block derivation reads back', () => {
    const meta = setMetaText(ast().meta, 'title', 'New title')
    const derived = deriveTitleBlockMeta({ ...ast(), meta }).meta
    expect(text(derived.title)).toBe('New title')
  })

  it('keeps an emptied field present, so the title block stays', () => {
    const doc = parseQmdContent('---\ntitle: Old\n---\n')!
    const meta = setMetaText(doc.meta, 'title', '')
    const derived = deriveTitleBlockMeta({ ...doc, meta }).meta
    const rendered = derived.rendered as unknown as { c: Record<string, MetaValue> }
    expect(rendered.c['has-title-block']).toMatchObject({ t: 'MetaBool', c: true })
  })

  it('removes a field when given undefined', () => {
    const meta = setMetaText(ast().meta, 'title', undefined)
    expect(meta.title).toBeUndefined()
    expect(meta.author).toBeDefined()
  })

  it('does not mutate its input', () => {
    const original = ast().meta
    const before = JSON.stringify(original)
    setMetaText(original, 'title', 'x')
    expect(JSON.stringify(original)).toBe(before)
  })
})

describe('metaText', () => {
  it('flattens inline markup the way the title block shows it', () => {
    const doc = parseQmdContent('---\ntitle: "*Big* `x` [news](u)"\nsubtitle: plain\n---\n')!
    expect(metaText(doc.meta.title)).toBe('Big x news')
    expect(metaText(doc.meta.subtitle)).toBe('plain')
    expect(metaText(doc.meta.author)).toBeUndefined()
  })
})
