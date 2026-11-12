/**
 * WASM module wrapper for QMD parsing.
 *
 * Provides initWasm(), parseQmdContent(), the qmd writers, and derivations
 * (deriveTitleBlockMeta) that run single render-pipeline steps on demand.
 */

import type { RustQmdJson } from '@quarto/pandoc-types'
import type { AstResponse, TitleBlockMetaResponse } from 'wasm-quarto-hub-client'

let wasmModule: typeof import('wasm-quarto-hub-client') | null = null
let initPromise: Promise<void> | null = null

/**
 * Initialize the WASM module. Safe to call multiple times.
 */
export async function initWasm(): Promise<void> {
  if (wasmModule) return
  if (initPromise) return initPromise

  initPromise = (async () => {
    const wasm = await import('wasm-quarto-hub-client')
    await wasm.default()
    wasmModule = wasm
  })()

  return initPromise
}

/**
 * Initialize the WASM module from its bytes (for Node, where the default
 * fetch-based loader doesn't work).
 */
export async function initWasmFromBytes(bytes: BufferSource): Promise<void> {
  if (wasmModule) return
  const wasm = await import('wasm-quarto-hub-client')
  await wasm.default({ module_or_path: bytes })
  wasmModule = wasm
}

/**
 * Parse QMD content into a RustQmdJson AST.
 * Returns null if parsing fails (logs warning), and also if the parser
 * read the document but reported an error about it: front matter that
 * isn't valid YAML then comes back as an AST with no metadata at all,
 * which must not be shown or written back as if the document had none.
 *
 * Must call initWasm() before first use.
 */
export function parseQmdContent(content: string): RustQmdJson | null {
  if (!wasmModule) {
    throw new Error('WASM not initialized. Call initWasm() first.')
  }

  const responseJson = wasmModule.parse_qmd_content(content)
  const response: AstResponse = JSON.parse(responseJson)

  if (!response.success || !response.ast) {
    console.warn('[wasm] Parse failed:', response.error)
    return null
  }
  const error = response.warnings?.find((w) => w.kind === 'error')
  if (error) {
    console.warn('[wasm] Parse incomplete:', error.title, error.problem)
    return null
  }

  return JSON.parse(response.ast) as RustQmdJson
}

/**
 * Convert a RustQmdJson AST back to QMD text.
 *
 * Must call initWasm() before first use.
 */
export function writeQmdFromAst(ast: RustQmdJson): string {
  if (!wasmModule) {
    throw new Error('WASM not initialized. Call initWasm() first.')
  }

  const astJson = JSON.stringify(ast)
  const responseJson = wasmModule.ast_to_qmd(astJson)
  const response: AstResponse = JSON.parse(responseJson)

  if (!response.success || !response.qmd) {
    throw new Error(`AST-to-QMD conversion failed: ${response.error}`)
  }

  return response.qmd
}

/**
 * Incrementally write a modified AST back to QMD, preserving unchanged
 * portions of the original source text verbatim.
 *
 * Must call initWasm() before first use.
 */
export function incrementalWriteQmd(originalQmd: string, newAst: RustQmdJson): string {
  if (!wasmModule) {
    throw new Error('WASM not initialized. Call initWasm() first.')
  }

  const newAstJson = JSON.stringify(newAst)
  const responseJson = wasmModule.incremental_write_qmd(originalQmd, newAstJson)
  const response: AstResponse = JSON.parse(responseJson)

  if (!response.success || !response.qmd) {
    throw new Error(`Incremental write failed: ${response.error}`)
  }

  return response.qmd
}

/**
 * Render qmd text to a standalone HTML page with the full render pipeline,
 * as `q2 render` does for a single file outside a project.
 *
 * Must call initWasm() before first use.
 */
export async function renderQmdContent(content: string): Promise<string> {
  if (!wasmModule) {
    throw new Error('WASM not initialized. Call initWasm() first.')
  }

  const response = JSON.parse(await wasmModule.render_qmd_content(content, '')) as {
    success: boolean
    html?: string
    error?: string
  }
  if (!response.success || response.html === undefined) {
    throw new Error(`Render failed: ${response.error}`)
  }
  return response.html
}

export interface TitleBlockMeta {
  /** The input metadata plus the derived title-block keys. */
  meta: RustQmdJson['meta']
  /** Normalization problems, e.g. an undefined affiliation `ref`. */
  issues: string[]
}

/**
 * Derive the metadata keys the title block reads (`by-author`,
 * `by-affiliation`, `labels`, `rendered.has-title-block`, ...) from a raw
 * parse, using the render pipeline's own `authors-normalize` logic.
 * Reads only `ast.meta`, so it costs the same at any document size.
 *
 * Must call initWasm() before first use.
 */
export function deriveTitleBlockMeta(ast: RustQmdJson): TitleBlockMeta {
  if (!wasmModule) {
    throw new Error('WASM not initialized. Call initWasm() first.')
  }

  // Only the metadata matters; send the source pool so positions survive.
  const metaOnly = { ...ast, blocks: [] }
  const response: TitleBlockMetaResponse = JSON.parse(
    wasmModule.title_block_meta(JSON.stringify(metaOnly)),
  )

  if (!response.success || !response.ast) {
    throw new Error(`Title block derivation failed: ${response.error}`)
  }

  return { meta: (JSON.parse(response.ast) as RustQmdJson).meta, issues: response.issues ?? [] }
}

/**
 * Compile the default Bootstrap + Quarto theme CSS, the stylesheet a
 * document with no `theme:` renders with. Takes about half a second the
 * first time (dart-sass is loaded on demand), so call it once and reuse
 * the result.
 *
 * Must call initWasm() before first use.
 */
export async function compileDefaultThemeCss(): Promise<string> {
  if (!wasmModule) {
    throw new Error('WASM not initialized. Call initWasm() first.')
  }
  const wasm = wasmModule

  // The sass importer reads Bootstrap's SCSS from the WASM's virtual file
  // system through these callbacks (as preview-runtime's initWasm does).
  const sass = await import('/src/wasm-js-bridge/sass.js')
  const read = (path: string): string | null => {
    const r = JSON.parse(wasm.vfs_read_file(path)) as { success: boolean; content?: string }
    return r.success && r.content !== undefined ? r.content : null
  }
  sass.setVfsCallbacks(read, (path: string) => read(path) !== null, () => {
    const r = JSON.parse(wasm.vfs_list_files()) as { success: boolean; files?: string[] }
    return r.success && r.files ? r.files : []
  })

  const response = JSON.parse(await wasm.compile_default_bootstrap_css(true)) as {
    success: boolean
    css?: string
    error?: string
  }
  if (!response.success || response.css === undefined) {
    throw new Error(`Theme CSS compilation failed: ${response.error}`)
  }
  return response.css
}
