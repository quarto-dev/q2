/**
 * Format → pipeline-kind mapping (q2-preview Plan 1).
 *
 * Single source of truth on the JS side for which Quarto pipeline a
 * given format string drives. Mirrors the `pipeline_kind` field on
 * the Rust `Format` struct (`crates/quarto-core/src/format.rs`),
 * populated by `Format::from_format_string` from the same lookup
 * table that `builtin_pseudo_format` uses. Changes to either side
 * should land together.
 *
 * Used today by `ReactPreview.tsx::doRender` to choose between the
 * AST-only entry point (`parseQmdToAst`, for q2-debug / q2-slides)
 * and the full pipeline (`renderPageInProject`, for q2-preview).
 * Plan 7 will extend it for the edit-back path: when q2-preview
 * round-trip lands, the writer wrapper takes a `pipelineKind`
 * argument that flows through this helper.
 *
 * Returns `'preview'` for q2-preview today; `undefined` for every
 * other format (q2-debug, q2-slides, html, pdf, etc.). The future
 * `'baseline'` value is reserved for Plan 7's write-side
 * baseline-vs-preview distinction; callers handling render-time
 * dispatch should treat anything but `'preview'` as the standard
 * (HTML or AST-only) path.
 */
export type PipelineKind = 'preview';

export function pipelineKindForFormat(format: string): PipelineKind | undefined {
  switch (format) {
    case 'q2-preview':
      return 'preview';
    default:
      return undefined;
  }
}
