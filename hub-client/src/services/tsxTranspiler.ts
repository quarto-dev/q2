import { transform } from '@babel/standalone';
import * as React from 'react';
import * as ReactAstDebugRendererModule from '../components/render/ReactAstDebugRenderer';

// EXPERIMENTAL functionality for custom render components

/**
 * Transpile and load TSX code as a dynamic ES module
 * @param tsxCode - The TSX source code to transpile
 * @returns An object containing the exported components
 */
export async function transpileAndImportTSX(tsxCode: string): Promise<Record<string, React.ComponentType<any>>> {
  try {
    // Store modules globally for access
    (window as any).React = React;
    (window as any).__REACT_AST_DEBUG_RENDERER__ = ReactAstDebugRendererModule;

    // Transpile TSX to JS
    const result = transform(tsxCode, {
      presets: ['typescript', 'react'],
      filename: 'component.tsx',
    });

    if (!result.code) {
      throw new Error('Transpilation produced no output');
    }

    // Create a blob URL for the transpiled code
    const blob = new Blob([result.code], { type: 'application/javascript' });
    const url = URL.createObjectURL(blob);

    try {
      // Dynamically import the module
      const module = await import(/* @vite-ignore */ url);

      // Return the exports
      return module;
    } finally {
      // Clean up the blob URL
      URL.revokeObjectURL(url);
    }
  } catch (err) {
    console.error('TSX transpilation/import error:', err);
    throw new Error(`Failed to transpile/import TSX: ${err instanceof Error ? err.message : String(err)}`);
  }
}
