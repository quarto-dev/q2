/** Minimal type declarations for wasm-quarto-hub-client: only what this app uses. */
declare module 'wasm-quarto-hub-client' {
  export type AstResponse =
    | { success: true; ast: string }
    | { success: false; error?: string; diagnostics?: unknown[] }

  export function parse_qmd_content(content: string): AstResponse
  export default function init(): Promise<void>
}
