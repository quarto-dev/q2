/**
 * Compile-time declaration for the staged `wasm-qmd-parser` package
 * (crates/wasm-qmd-parser, built with `wasm-pack --target nodejs`).
 *
 * The package is a runtime artifact — built by scripts/build-qmd-parser.mjs
 * and staged into dist-bundle/node_modules by scripts/stage-qmd-parser.mjs —
 * so it can never appear in this package's node_modules. tsc typechecks
 * against this declaration; Node resolves the real files at runtime.
 * Only the entry points the MCP server uses are declared.
 */

declare module 'wasm-qmd-parser' {
  /**
   * Parse qmd text. Returns a JSON string:
   * `{"success": true, "ast": "<json string>"}` on success (the AST is the
   * pampa JSON wire format, double-encoded), or
   * `{"success": false, "error": string, "diagnostics": [{"message": string}]}`
   * on failure. `include_resolved_locations` is the string "true" to attach
   * source locations (`l: {b: {c, l, o}, e: {c, l, o}, f}`) to every node.
   */
  export function parse_qmd(input: string, include_resolved_locations: string): string;

  /**
   * Convert a document between formats. `convert(astJson, "json", "qmd")`
   * serializes a pampa JSON AST back to qmd text.
   */
  export function convert(document: string, input_format: string, output_format: string): string;
}
