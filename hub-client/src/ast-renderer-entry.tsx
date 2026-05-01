/**
 * Entry point for the AST renderer iframe.
 * This is loaded by ast-renderer.html and handles postMessage communication.
 */

import { createRoot } from 'react-dom/client';
import { Ast, componentRegistry } from './components/render/ReactAstDebugRenderer';
import * as ReactAstDebugRendererModule from './components/render/ReactAstDebugRenderer';
import React from 'react';
import { Deck, Slide } from '@revealjs/react';
import 'reveal.js/reveal.css';
import 'reveal.js/theme/white.css';
import katex from 'katex';
import 'katex/dist/katex.min.css';

let root: ReturnType<typeof createRoot> | null = null;
let customRegistry: Record<string, React.ComponentType<any>> = {};
let componentsLoading = false;

interface UpdateAstPayload {
  astJson: string;
  currentFilePath: string;
}

// Handle messages from parent window
window.addEventListener('message', async (event) => {
  // In production, verify event.origin for security

  if (event.data.type === 'LOAD_CUSTOM_COMPONENTS') {
    componentsLoading = true;
    await loadCustomComponents(event.data.componentsCode);
    componentsLoading = false;
  } else if (event.data.type === 'UPDATE_AST') {
    // Wait for components to finish loading before rendering
    if (componentsLoading) {
      await new Promise(resolve => {
        const check = setInterval(() => {
          if (!componentsLoading) {
            clearInterval(check);
            resolve(undefined);
          }
        }, 50);
      });
    }
    updateAst(event.data.payload);
  }
});

/**
 * Load custom components from transpiled JS code using dynamic imports
 */
async function loadCustomComponents(componentsCode: Record<string, string>) {
  customRegistry = {};

  // Make React and other dependencies available globally for the imported modules
  (window as any).React = React;
  (window as any).__REACT_AST_DEBUG_RENDERER__ = ReactAstDebugRendererModule;
  (window as any).RevealReact = { Deck, Slide };
  (window as any).katex = katex;

  for (const [componentName, code] of Object.entries(componentsCode)) {
    try {
      // Create a blob URL for the transpiled code
      const blob = new Blob([code], { type: 'application/javascript' });
      const url = URL.createObjectURL(blob);

      try {
        // Dynamically import the module
        const module = await import(url);

        // Extract all exported components
        customRegistry = { ...componentRegistry, ...module }

        console.log(`[AstIframe] Loaded custom component: ${componentName}`);
      } finally {
        // Clean up the blob URL
        URL.revokeObjectURL(url);
      }
    } catch (err) {
      console.error(`[AstIframe] Failed to load custom component ${componentName}:`, err);
    }
  }
}

function updateAst(payload: UpdateAstPayload) {
  const {
    astJson,
    currentFilePath,
  } = payload;

  // Merge custom components with defaults (custom overrides defaults)
  const mergedRegistry = { ...componentRegistry, ...customRegistry } as Record<string, (props: any) => React.ReactNode>;

  const rootElement = document.getElementById('root');
  if (!rootElement) {
    console.error('Root element not found');
    return;
  }

  try {
    // Create root only once
    if (!root) {
      root = createRoot(rootElement);
    }

    // Render the Ast component
    root.render(
      <Ast
        astJson={astJson}
        currentFilePath={currentFilePath}
        onNavigateToDocument={(path, anchor) => {
          window.parent.postMessage({
            type: 'NAVIGATE_TO_DOCUMENT',
            path,
            anchor
          }, '*');
        }}
        setAst={(newAst) => {
          window.parent.postMessage({
            type: 'SET_AST',
            ast: newAst
          }, '*');
        }}
        registry={mergedRegistry}
      />
    );
  } catch (err) {
    console.error('Failed to render AST:', err);
    rootElement.innerHTML = `
      <div style="padding: 20px; color: red;">
        <strong>Render Error:</strong>
        <pre>${err instanceof Error ? err.message : String(err)}</pre>
      </div>
    `;
  }
}

// Signal that the iframe is ready to receive messages
window.parent.postMessage({ type: 'IFRAME_READY' }, '*');
