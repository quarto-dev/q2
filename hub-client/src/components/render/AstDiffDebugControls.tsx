import { useCallback, useRef, useState } from 'react';
import { diffAsts, parseQmdContentSync, isWasmReady } from '@quarto/preview-runtime';
import { Q2PreviewIframe } from '@quarto/preview-renderer/iframe/Q2PreviewIframe';

/**
 * Debug controls for AST diff annotation (claude-notes/plans/
 * 2026-07-22-ast-diff-annotation.md).
 *
 * "Snapshot AST" parses the editor's current content (parse tier, via the
 * synchronous `parse_qmd_content` WASM entry point) and stores the AST JSON;
 * "Compare" diffs snapshot → current via the WASM `diff_asts_to_qmd` export,
 * logs the change-annotated qmd to the console, and opens a modal rendering
 * the annotated AST **directly** with the q2-preview iframe renderer — no
 * qmd round-trip, so boundary spaces inside `[++ …]`/`[-- …]` marks are
 * preserved exactly. Green/red diff styling is injected client-side as a
 * RawBlock `<style>` prepended to the AST: `.added`/`.removed` divs and
 * `quarto-insert`/`quarto-delete` spans (the desugared editorial marks).
 */
export interface AstDiffDebugControlsProps {
  /** The editor's current qmd source text. */
  content: string;
}

/** Diff styles, injected as a raw-HTML block at the top of the diff AST. */
const DIFF_STYLE_HTML = [
  '<style>',
  'div.added { background: #d3f2d3; border-radius: 4px; padding: 2px 6px; }',
  'div.removed { background: #f8d2d2; border-radius: 4px; padding: 2px 6px; }',
  'span.quarto-insert { background: #d3f2d3; border-radius: 3px; white-space: pre-wrap; }',
  'span.quarto-delete { background: #f8d2d2; border-radius: 3px; white-space: pre-wrap; }',
  '</style>',
].join('\n');

/** Prepend the diff `<style>` block to the AST as a RawBlock. */
function injectDiffStyles(astJson: string): string {
  const ast = JSON.parse(astJson);
  ast.blocks = [{ t: 'RawBlock', c: ['html', DIFF_STYLE_HTML] }, ...(ast.blocks ?? [])];
  return JSON.stringify(ast);
}

export function AstDiffDebugControls({ content }: AstDiffDebugControlsProps) {
  const snapshotRef = useRef<string | null>(null);
  const [hasSnapshot, setHasSnapshot] = useState(false);
  const [diff, setDiff] = useState<{ astJson: string; qmd: string; themeFp: string } | null>(null);

  const parseQmd = useCallback((source: string): string | null => {
    if (!isWasmReady()) {
      console.warn('[ast-diff] WASM renderer not ready yet; try again in a moment.');
      return null;
    }
    const response = parseQmdContentSync(source);
    if (!response.success || !response.ast) {
      console.error('[ast-diff] Failed to parse document:', response.error);
      return null;
    }
    return response.ast;
  }, []);

  const handleSnapshot = useCallback(() => {
    const ast = parseQmd(content);
    if (!ast) return;
    snapshotRef.current = ast;
    setHasSnapshot(true);
    console.log('[ast-diff] Snapshot taken; edit the document and press Compare.');
  }, [parseQmd, content]);

  const handleCompare = useCallback(() => {
    if (!snapshotRef.current) {
      console.warn('[ast-diff] No snapshot yet; press "Snapshot AST" first.');
      return;
    }
    const current = parseQmd(content);
    if (!current) return;
    try {
      const { qmd, astJson } = diffAsts(snapshotRef.current, current);
      console.log('[ast-diff] Annotated diff (snapshot → current):\n' + qmd);
      // A fresh fingerprint makes the iframe load the theme CSS the last
      // real render left at the default VFS artifact path — this is what
      // gives the modal the normal preview fonts/styles.
      setDiff({ astJson: injectDiffStyles(astJson), qmd, themeFp: `ast-diff-${Date.now()}` });
    } catch (err) {
      console.error('[ast-diff] Failed to diff ASTs:', err);
    }
  }, [parseQmd, content]);

  return (
    <>
      <div className="preview-status-bar ast-diff-debug-bar" role="group" aria-label="AST diff debug">
        <span className="preview-status-label">AST diff (debug)</span>
        <div className="preview-status-actions">
          <button type="button" onClick={handleSnapshot} title="Save the current AST state as the diff baseline">
            Snapshot AST
          </button>
          <button
            type="button"
            onClick={handleCompare}
            disabled={!hasSnapshot}
            title="Show the snapshot → current diff (also logged to the console as qmd)"
          >
            Compare
          </button>
        </div>
      </div>
      {diff && (
        <div
          onClick={() => setDiff(null)}
          style={{
            position: 'fixed',
            inset: 0,
            background: 'rgba(0, 0, 0, 0.4)',
            zIndex: 1000,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
          }}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            style={{
              background: '#fff',
              borderRadius: 8,
              width: 'min(860px, 92vw)',
              height: '85vh',
              display: 'flex',
              flexDirection: 'column',
              boxShadow: '0 8px 30px rgba(0,0,0,0.3)',
              overflow: 'hidden',
            }}
          >
            <div
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
                padding: '8px 14px',
                borderBottom: '1px solid #ddd',
              }}
            >
              <strong>AST diff (snapshot → current)</strong>
              <button type="button" onClick={() => setDiff(null)} aria-label="Close diff modal">
                ✕
              </button>
            </div>
            <div style={{ flex: 1, minHeight: 0 }}>
              <Q2PreviewIframe
                astJson={diff.astJson}
                currentFilePath=""
                setAst={() => {}}
                editingDisabled
                themeFingerprint={diff.themeFp}
              />
            </div>
          </div>
        </div>
      )}
    </>
  );
}
