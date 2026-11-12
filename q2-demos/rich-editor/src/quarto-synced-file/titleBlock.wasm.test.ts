/**
 * Runs against the real WASM module (build it first with `npm run build:wasm`).
 */

import { beforeAll, describe, expect, it } from 'vitest'
import { readFile } from 'fs/promises'
import { join } from 'path'
import {
  initWasmFromBytes,
  parseQmdContent,
  deriveTitleBlockMeta,
  compileDefaultThemeCss,
} from './wasm.ts'

beforeAll(async () => {
  const bytes = await readFile(
    join(__dirname, '../../wasm-quarto-hub-client/wasm_quarto_hub_client_bg.wasm'),
  )
  await initWasmFromBytes(bytes)
})

const DOC = `---
title: "Quarterly *report*"
author:
  - name: Ana Silva
    affiliations: [{ref: posit}]
  - name: Ben Okafor
affiliations:
  - id: posit
    name: Posit
---

Body.
`

type Meta = Record<string, { t: string; c: unknown }>

/** Read a MetaMap/MetaList path from the JSON meta, returning the leaf. */
function at(meta: Meta, path: (string | number)[]): unknown {
  let cur: unknown = meta
  for (const key of path) {
    if (cur && typeof cur === 'object' && 't' in cur) {
      cur = (cur as unknown as { c: unknown }).c
    }
    cur = (cur as Record<string | number, unknown>)?.[key]
  }
  return cur
}

function plain(value: unknown): string {
  const v = value as { t: string; c: unknown }
  if (v?.t === 'MetaString') return v.c as string
  if (v?.t === 'MetaInlines') {
    return (v.c as { t: string; c?: unknown }[])
      .map((i) => (i.t === 'Str' ? i.c : i.t === 'Space' ? ' ' : ''))
      .join('')
  }
  return String(v?.c ?? '')
}

describe('deriveTitleBlockMeta', () => {
  it('derives the keys PreviewTitleBlock reads from a raw parse', () => {
    const ast = parseQmdContent(DOC)!
    expect(ast.meta['by-author']).toBeUndefined()

    const { meta, issues } = deriveTitleBlockMeta(ast)
    const m = meta as unknown as Meta

    expect(issues).toEqual([])
    expect(at(m, ['rendered', 'has-title-block'])).toMatchObject({ t: 'MetaBool', c: true })
    expect(plain(at(m, ['by-author', 0, 'name', 'literal']))).toBe('Ana Silva')
    expect(plain(at(m, ['by-author', 0, 'affiliations', 0, 'name']))).toBe('Posit')
    expect(plain(at(m, ['by-author', 1, 'name', 'literal']))).toBe('Ben Okafor')
    expect(plain(at(m, ['labels', 'authors']))).toBe('Authors')
    // The raw keys are kept.
    expect(m.title.t).toBe('MetaInlines')
  })

  it('reports an undefined affiliation ref as an issue', () => {
    const ast = parseQmdContent('---\nauthor:\n  - name: A\n    affiliations: [{ref: nope}]\n---\n')!
    const { issues } = deriveTitleBlockMeta(ast)
    expect(issues.length).toBe(1)
    expect(issues[0]).toContain('nope')
  })
})

describe('compileDefaultThemeCss', () => {
  it('compiles the default theme, including the title-block styles', async () => {
    const css = await compileDefaultThemeCss()
    expect(css).toContain('.quarto-title-meta')
    expect(css).toContain('--bs-body-font-family')
  })
})
