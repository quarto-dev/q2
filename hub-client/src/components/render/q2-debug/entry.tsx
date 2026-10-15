/**
 * Entry point for the q2-debug renderer iframe.
 *
 * Loaded by `/q2-debug.html`; speaks the postMessage protocol with the
 * parent (`Q2DebugIframe`). The document is rendered without React: the
 * fresh parse is diffed against the AST already on screen, the edit script
 * is applied to that AST in place, and the DOM is patched to match —
 * see `dom/document.ts`. Untouched nodes keep both their AST objects and
 * their DOM, and every highlight is derived from the script. An AST caret
 * (`dom/caret.ts`) rides on that stability: click text to place it on a
 * node, use the arrow keys to move it, and watch it stay put across edits.
 * Typing and Backspace edit the AST through the caret; the edited AST is
 * posted back as `SET_AST`, and the parent writes it to the QMD.
 *
 * Round trip of an edit: the caret applies it to the document at once,
 * then the parent writes the QMD, re-renders, and sends the fresh parse
 * back. That parse reconciles to no change (or just the parser's
 * normalisation). What must not happen is a render of an *older* state
 * arriving after a newer keystroke: it would undo the keystroke on
 * screen until its own render arrived. So each edit carries a sequence
 * number, the parent acknowledges it on the render that reflects it, and
 * while an edit is unacknowledged other updates are held (the latest one
 * is kept and applied if no acknowledgement comes within `HOLD_MS`, e.g.
 * because a collaborator's change replaced ours before the render).
 */

import './dom/debug.css';
import type { PandocAST } from '@quarto/preview-renderer/framework';
import { describeOp, type Op } from '@quarto/preview-renderer/astdiff';
import {
    makeIframeMessageDispatcher,
    type IframeMessage,
} from '../iframeMessageDispatch';
import { DebugDocument } from './dom/document';
import { AstCaret } from './dom/caret';

interface UpdateAstPayload {
  astJson: string;
  currentFilePath: string;
  /** The current user, for the caret's flag. */
  user?: { name: string; color: string } | null;
  /** The `editSeq` of the caret edit this render reflects, if any. */
  editAck?: number | null;
}

/** How long to hold updates waiting for an edit's acknowledgement. */
const HOLD_MS = 2000;

/** Last edit script applied, exposed for debugging / e2e inspection. */
declare global {
  interface Window {
    __Q2_DEBUG_AST_OPS__?: Op[];
  }
}

let doc: DebugDocument | null = null;
let caret: AstCaret | null = null;
let currentPath: string | null = null;
/** Sequence number of the last edit sent, and the last one acknowledged. */
let sentSeq = 0;
let ackedSeq = 0;
/** The latest update held back while an edit is unacknowledged. */
let held: UpdateAstPayload | null = null;
let holdTimer: ReturnType<typeof setTimeout> | null = null;

function sendEdit(ast: PandocAST): void {
  sentSeq++;
  window.parent.postMessage({ type: 'SET_AST', ast, editSeq: sentSeq }, '*');
  if (holdTimer !== null) clearTimeout(holdTimer);
  holdTimer = setTimeout(() => {
    holdTimer = null;
    ackedSeq = sentSeq;
    if (held) applyUpdate(held);
    held = null;
  }, HOLD_MS);
}

function updateAst(payload: UpdateAstPayload): void {
  if (sentSeq > ackedSeq && payload.editAck !== sentSeq) {
    held = payload;
    return;
  }
  if (payload.editAck === sentSeq) {
    ackedSeq = sentSeq;
    held = null;
    if (holdTimer !== null) { clearTimeout(holdTimer); holdTimer = null; }
  }
  applyUpdate(payload);
}

// Shared dispatcher gates UPDATE_AST on the in-flight LOAD_CUSTOM_COMPONENTS
// promise so messages run in arrival order (see ../iframeMessageDispatch.ts).
const dispatch = makeIframeMessageDispatcher({
  loadCustomComponents: async (componentsCode) => {
    // User render components are React components; this renderer has no
    // React. Accepted and ignored so the parent's protocol is unchanged.
    const names = Object.keys(componentsCode);
    if (names.length > 0) {
      console.warn('[q2-debug] custom render components are not supported by the DOM renderer; ignoring', names);
    }
  },
  updateAst: (payload) => updateAst(payload as UpdateAstPayload),
});

window.addEventListener('message', (event) => {
  // In production, verify event.origin for security
  dispatch(event.data as IframeMessage);
});

function applyUpdate(payload: UpdateAstPayload) {
  const { astJson, currentFilePath, user } = payload;
  const rootElement = document.getElementById('root');
  if (!rootElement) {
    console.error('Root element not found');
    return;
  }
  try {
    let fresh: PandocAST;
    try {
      fresh = JSON.parse(astJson) as PandocAST;
    } catch (err) {
      throw new Error(`Failed to parse AST: ${err instanceof Error ? err.message : String(err)}`);
    }
    if (!doc) {
      const container = document.createElement('div');
      container.className = 'pandoc-content-debug';
      rootElement.replaceChildren(container);
      doc = new DebugDocument(container);
      // Click to place, arrows to move; attached to an AST node + offset,
      // and migrated through edits by the applier's report (dom/caret.ts).
      caret = new AstCaret(doc);
      caret.onEdit = sendEdit;
      (window as unknown as { __Q2_DEBUG_CARET__?: AstCaret }).__Q2_DEBUG_CARET__ = caret;
    }
    if (currentPath !== currentFilePath) {
      // A diff against a different document would be large and meaningless.
      doc.reset();
      currentPath = currentFilePath;
    }
    caret?.setUser(user ?? null);
    const result = doc.update(fresh);
    window.__Q2_DEBUG_AST_OPS__ = result.ops;
    if (result.reused) {
      console.debug(`[q2-debug] applied ${result.ops.length} AST op(s)`, result.ops.map(describeOp));
    }
  } catch (err) {
    console.error('Failed to render AST:', err);
    doc = null;
    caret = null;
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
