import { transform } from '@babel/standalone';

// EXPERIMENTAL functionality for custom render components

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
