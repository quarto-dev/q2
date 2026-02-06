/**
 * WASM module wrapper for QMD parsing.
 *
 * Provides initWasm() and parseQmdContent() for use with quarto-sync-client's
 * ASTOptions.
 */

import type { RustQmdJson } from '@quarto/pandoc-types'
import type { AstResponse } from 'wasm-quarto-hub-client'

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
 * Parse QMD content into a RustQmdJson AST.
 * Returns null if parsing fails (logs warning).
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
