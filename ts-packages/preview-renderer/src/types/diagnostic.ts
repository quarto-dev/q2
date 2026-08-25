/**
 * Diagnostic types for structured error/warning information from WASM.
 *
 * These types match the JSON structure returned by wasm-quarto-hub-client's
 * render_qmd() function. Line and column numbers are 1-based to match Monaco.
 *
 * @deprecated These types are being phased out in favor of the LSP-style
 * types in `types/intelligence.ts`. The new types use:
 * - camelCase field names
 * - Nested Range/Position objects
 * - 0-based positions (LSP specification)
 *
 * Use `types/intelligence.ts` and `intelligenceService.ts` for new code.
 * This file will be removed once render_qmd is updated to use the new format.
 */

export type DiagnosticKind = 'error' | 'warning' | 'info' | 'note';

export interface DiagnosticDetail {
  kind: 'error' | 'info' | 'note';
  content: string;
  start_line?: number;
  start_column?: number;
  end_line?: number;
  end_column?: number;
}

export interface Diagnostic {
  kind: DiagnosticKind;
  title: string;
  code?: string;
  problem?: string;
  hints: string[];
  start_line?: number;
  start_column?: number;
  end_line?: number;
  end_column?: number;
  details: DiagnosticDetail[];
  /**
   * Pre-rendered ariadne source-context snippet (bd-352bh). Present
   * when the diagnostic has a `start_line` and the producer had a
   * SourceContext available (i.e. always, for diagnostics produced
   * by `diagnostic_to_json`). Same text the `q2 render` CLI prints
   * to stdout — render verbatim in a `<pre>` block with `stripAnsi`
   * for the rich source-context view. Falls back to the structured
   * fields (title / code / problem / start_line) for compact
   * display when absent.
   */
  rendered?: string;
}

/**
 * A Pass-1 failure (parse / metadata error) for a project file
 * other than the active page (bd-rqba). See `services/wasmRenderer.ts`
 * `Pass1Failure` for the canonical definition; this is a
 * minimal mirror so this header type stays self-contained.
 */
export interface Pass1Failure {
  source_file: string;
  error: string;
  diagnostics: Diagnostic[];
}

/**
 * One outstanding editorial comment on the rendered page, from the
 * document's Pass-1 `DocumentProfile` (bd-0rsk07il, GH #445).
 * Matches `quarto_core::document_profile::JsonComment`: positions are
 * 1-based (Monaco convention); `file` is the source file the mark
 * maps to, which for a comment inside an `{{< include >}}` is the
 * included file rather than the active document.
 */
export interface RenderComment {
  /** Plain-text comment content. */
  text: string;
  /** In-band author (`author=` attribute), when stamped. */
  author?: string;
  /** In-band ISO 8601 timestamp (`date=` attribute), when stamped. */
  date?: string;
  /** Remaining attr key-value pairs, authored order. */
  attributes?: [string, string][];
  /** Path of the source file the mark's span maps to. */
  file?: string;
  start_line?: number;
  start_column?: number;
  end_line?: number;
  end_column?: number;
}

/**
 * Render response from WASM with structured diagnostics.
 */
export interface RenderResponse {
  success: boolean;
  error?: string;
  html?: string;
  /**
   * Serialized Pandoc AST JSON for the q2-preview format.
   * Populated by the q2-preview pipeline branch, `undefined` for
   * HTML / error responses. Consumers dispatch on which of `html`
   * / `ast_json` is present (typically via
   * `pipelineKindForFormat(format)` to decide which is expected).
   */
  ast_json?: string;
  /**
   * Untransformed Pandoc AST JSON — the `qmd_to_pandoc` output
   * captured immediately after `ParseDocumentStage`, before any
   * `AstTransformsStage`. Populated alongside `ast_json` for
   * q2-preview renders; `undefined` for HTML / error responses.
   * Round-tripped from the frontend as the baseline for
   * `apply_node_edit` (target-incremental-writes Phase 1).
   */
  untransformed_ast_json?: string;
  /** Structured diagnostics (errors) with line/column information for Monaco. */
  diagnostics?: Diagnostic[];
  /** Structured warnings with line/column information for Monaco. */
  warnings?: Diagnostic[];
  /** Sibling-page Pass-1 failures (bd-rqba). */
  pass1_failures?: Pass1Failure[];
  /**
   * Compiled theme CSS fingerprint (Plan 2A item 11). Recovered
   * from the `css:theme:<fingerprint>` artifact key. The hub-client
   * iframe wrapper dedupes blob-URL re-mints when this hasn't
   * changed. `undefined` when no theme artifact was produced
   * (errors, q2-debug, themeless single-doc).
   */
  theme_fingerprint?: string;
  /**
   * Outstanding editorial comments on the active page, in source
   * order (bd-0rsk07il, GH #445). Absent when the document has none
   * (consumers treat absent as zero) and on error responses. The
   * comment-toggle badge reads `comments.length`.
   */
  comments?: RenderComment[];
}

/**
 * Payload sent via `setAst` for q2-preview node edits (Plan 2b).
 *
 * Two channels — the parent routes on `channel`:
 *
 * - `'text'`: raw QMD the user typed.  Parent calls `parse_qmd_content(newText)`
 *   then `apply_node_edit`.  Used by built-in editing (Para, Header textarea).
 *
 * - `'subtree'`: replacement block already in Pandoc JSON form.  Parent passes
 *   it straight to `apply_node_edit` (no re-parse).  Used by render-component
 *   editing (drag, comment, kanban) via `commitSubtreeEdit`.
 *
 * `destinationSourceInfoJson` is `JSON.stringify(resolved.sourceEntry)` in
 * both channels.
 */
export type PreviewNodeEditPayload =
  | {
      __isPreviewNodeEdit: true;
      channel: 'text';
      /** JSON-serialized SourceInfo VALUE of the edited node (not a pool id). */
      destinationSourceInfoJson: string;
      /** Raw QMD text the user typed. */
      newText: string;
    }
  | {
      __isPreviewNodeEdit: true;
      channel: 'subtree';
      /** JSON-serialized SourceInfo VALUE of the edited node (not a pool id). */
      destinationSourceInfoJson: string;
      /** Pandoc JSON of the replacement block (full Pandoc document; only `blocks[0]` is used). */
      modifiedSubtreeJson: string;
    };
