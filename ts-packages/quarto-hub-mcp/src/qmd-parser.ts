/**
 * The qmd parser for AST-aware tools (CAP-11), loaded from the staged
 * `wasm-qmd-parser` package (spike S-1: the same pampa build the web
 * preview uses, compiled with `--target nodejs`).
 *
 * Loading is lazy and singleton: the 4.6 MB wasm instantiates once, on the
 * first AST-aware call (~4 ms), never on the plain-CRUD path. The dynamic
 * import keeps esbuild from trying to bundle the package — it stays
 * external and resolves from dist-bundle/node_modules at runtime (the
 * keyring pattern).
 *
 * Tests point `QUARTO_QMD_PARSER_SPEC` at the crate's pkg directory (set by
 * the vitest global setup): the package is a build artifact, never an npm
 * dependency, so it can never resolve through workspace node_modules.
 */

export interface QmdParser {
  /**
   * Parse qmd text into the pampa JSON AST (with source locations), or
   * null when the parser rejects the input or its payload is malformed.
   */
  parse(text: string): unknown | null;
}

let singleton: Promise<QmdParser> | null = null;

export function loadQmdParser(): Promise<QmdParser> {
  singleton ??= (async () => {
    const spec = process.env['QUARTO_QMD_PARSER_SPEC'] ?? 'wasm-qmd-parser';
    let mod: typeof import('wasm-qmd-parser');
    try {
      mod = (await import(spec)) as typeof import('wasm-qmd-parser');
    } catch (err) {
      const detail = err instanceof Error ? err.message : String(err);
      throw new Error(
        `the qmd parser package could not be loaded (${detail}). ` +
          'Build it with `node scripts/build-qmd-parser.mjs` ' +
          '(wasm-pack + a wasm32-capable clang required).',
      );
    }
    return {
      parse(text: string): unknown | null {
        try {
          const out = JSON.parse(mod.parse_qmd(text, 'true')) as {
            success?: unknown;
            ast?: unknown;
          };
          if (out.success !== true || typeof out.ast !== 'string') return null;
          return JSON.parse(out.ast) as unknown;
        } catch {
          return null;
        }
      },
    };
  })();
  return singleton;
}
