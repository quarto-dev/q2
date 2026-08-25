import { transform } from '@babel/standalone';

// EXPERIMENTAL functionality for custom render components
//
// Moved from `hub-client/src/services/tsxTranspiler.ts` (GH #402 /
// bd-ue80chl0 Phase 1) so hub-client and the q2-preview SPA share one
// transpiler. hub-client imports this module statically; the SPA imports
// it dynamically (`await import(...)`) so `@babel/standalone` lands in a
// lazy chunk that documents without `render-components:` never load.
// Nothing in the iframe entry graph may import this module — it would
// pull babel into the iframe bundle.

/**
 * Transpile TSX code to JavaScript
 * @param tsxCode - The TSX source code to transpile
 * @returns The transpiled JavaScript code
 */
export function transpileTSX(tsxCode: string): string {
  try {
    // Transpile TSX to JS
    const result = transform(tsxCode, {
      presets: ['typescript', 'react'],
      filename: 'component.tsx',
    });

    if (!result.code) {
      throw new Error('Transpilation produced no output');
    }

    return result.code;
  } catch (err) {
    console.error('TSX transpilation error:', err);
    throw new Error(`Failed to transpile TSX: ${err instanceof Error ? err.message : String(err)}`);
  }
}
