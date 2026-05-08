/**
 * Entry point for the q2-debug renderer iframe.
 *
 * Loaded by `/q2-debug.html` and handles the postMessage protocol with
 * the parent (Q2DebugIframe). Mounts the framework's <Ast> with
 * q2-debug's registry as the format-side defaults, layered with any
 * user-TSX overrides loaded via LOAD_CUSTOM_COMPONENTS.
 */

import { createRoot } from 'react-dom/client';
import React from 'react';
import { Deck, Slide } from '@revealjs/react';
import 'reveal.js/reveal.css';
import 'reveal.js/theme/white.css';
import katex from 'katex';
import 'katex/dist/katex.min.css';

import { Ast, Node, renderChildren, renderNode } from '../framework';
import type { FormatRegistry } from '../framework';
import {
    Block,
    Inline,
    Para, Plain, Header, CodeBlock, BulletList, OrderedList,
    BlockQuote, Div, HorizontalRule, RawBlock, Figure,
    Str, Space, SoftBreak, LineBreak,
    Emph, Strong, Code, Link, Image, Span, Quoted,
    blockStyle, inlineStyle,
    q2DebugRegistry,
} from '.';
import { buildCustomRegistry, type ComponentExports } from '../../../utils/customRegistry';

// Set the renderer-surface global at module top so importing this
// module is sufficient to populate `window.__REACT_AST_DEBUG_RENDERER__`
// — the framework-primitive parity test (Plan 2A item 14) relies on
// being able to read both globals via plain module imports without
// firing `LOAD_CUSTOM_COMPONENTS` setup messages. `React`, `katex`,
// and `RevealReact` stay lazy in `loadCustomComponents` since they're
// tied to dynamic user-TSX imports (the right time to set them).
(window as any).__REACT_AST_DEBUG_RENDERER__ = {
    renderChildren, renderNode, Node,
    Block, Inline,
    Para, Plain, Header, CodeBlock, BulletList, OrderedList,
    BlockQuote, Div, HorizontalRule, RawBlock, Figure,
    Str, Space, SoftBreak, LineBreak,
    Emph, Strong, Code, Link, Image, Span, Quoted,
    q2DebugRegistry, blockStyle, inlineStyle,
};

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
 * Load custom components from transpiled JS code using dynamic imports.
 */
async function loadCustomComponents(componentsCode: Record<string, string>) {
  // Make React, RevealReact, and katex available as globals for user
  // TSX modules. The renderer-surface global is set at module top
  // (see above); these three remain lazy because they're tied to
  // dynamic user-TSX imports — `loadCustomComponents` is the right
  // moment to materialize them.
  (window as any).React = React;
  (window as any).RevealReact = { Deck, Slide };
  (window as any).katex = katex;

  const loadedModules: ComponentExports[] = [];
  for (const [componentName, code] of Object.entries(componentsCode)) {
    try {
      const blob = new Blob([code], { type: 'application/javascript' });
      const url = URL.createObjectURL(blob);
      try {
        const module = await import(url);
        loadedModules.push(module as ComponentExports);
        console.log(`[Q2DebugIframe] Loaded custom component: ${componentName}`);
      } finally {
        URL.revokeObjectURL(url);
      }
    } catch (err) {
      console.error(`[Q2DebugIframe] Failed to load custom component ${componentName}:`, err);
    }
  }

  customRegistry = buildCustomRegistry(loadedModules);
}

function updateAst(payload: UpdateAstPayload) {
  const {
    astJson,
    currentFilePath,
  } = payload;

  // Merge q2-debug defaults with any user-TSX overrides. The cast asserts
  // the merged result satisfies the FormatRegistry contract; the override
  // side is babel-transpiled user code and runtime-trusted.
  const mergedRegistry: FormatRegistry = {
    ...q2DebugRegistry,
    ...customRegistry,
  } as FormatRegistry;

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
