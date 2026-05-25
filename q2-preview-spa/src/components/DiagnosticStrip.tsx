/**
 * DiagnosticStrip (Plan 7 Phase 7).
 *
 * Surfaces soft-drop warnings (Q-3-42 / Q-3-43) returned by
 * `incrementalWriteQmd` after a component-driven edit hits an atomic
 * region. The SPA has no Monaco squiggle to lean on, so this strip is
 * the only diagnostic surface for write-side warnings.
 *
 * Autosave-context spam mitigation: every keystroke triggers a render +
 * write, so a user typing over an atomic-resolved inline would re-emit
 * the same Q-3-42 on every tick. We group by source range and show the
 * first three occurrences per `(start_line, start_column, end_line,
 * end_column)`; further hits are silently dropped (the prior entries
 * stay visible). Plan 7 §"Autosave-context spam mitigation".
 *
 * The catalog messages (`Q-3-42`: "Shortcode edit dropped" + body;
 * `Q-3-43`: "Generated content edit dropped" + body) already read as
 * imperative instructions ("edit the invocation token in source
 * instead"), so DiagnosticStrip surfaces title + problem verbatim.
 */

import type { Diagnostic } from '@quarto/preview-renderer/types/diagnostic';

interface DiagnosticStripProps {
  /** Soft-drop warnings to surface. Cleared by the caller on dismiss. */
  warnings: Diagnostic[];
  /** Caller-provided dismiss handler. */
  onDismiss: () => void;
}

/**
 * Group warnings by source-range key and cap each group at 3 entries.
 * Exported for tests.
 */
export function suppressAfterThree(warnings: Diagnostic[]): Diagnostic[] {
  const counts = new Map<string, number>();
  const out: Diagnostic[] = [];
  for (const w of warnings) {
    const key = `${w.code ?? ''}:${w.start_line ?? -1}:${w.start_column ?? -1}:${w.end_line ?? -1}:${w.end_column ?? -1}`;
    const n = counts.get(key) ?? 0;
    if (n < 3) {
      out.push(w);
      counts.set(key, n + 1);
    }
  }
  return out;
}

export function DiagnosticStrip({ warnings, onDismiss }: DiagnosticStripProps) {
  if (warnings.length === 0) return null;
  const visible = suppressAfterThree(warnings);

  return (
    <div
      role="status"
      aria-live="polite"
      style={{
        position: 'absolute',
        bottom: '0.75rem',
        right: '0.75rem',
        zIndex: 10,
        maxWidth: '28rem',
        padding: '0.5rem 0.75rem',
        display: 'flex',
        flexDirection: 'column',
        gap: '0.375rem',
        border: '1px solid rgba(180, 120, 0, 0.4)',
        borderRadius: '0.375rem',
        background: 'rgba(255, 247, 217, 0.97)',
        color: 'rgba(60, 40, 0, 0.95)',
        fontSize: '0.825rem',
        lineHeight: 1.4,
        boxShadow: '0 1px 4px rgba(0, 0, 0, 0.08)',
      }}
    >
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'baseline',
          gap: '0.75rem',
        }}
      >
        <strong style={{ fontWeight: 600 }}>
          {visible.length === 1 ? '1 edit dropped' : `${visible.length} edits dropped`}
        </strong>
        <button
          type="button"
          onClick={onDismiss}
          aria-label="Dismiss warnings"
          style={{
            border: 'none',
            background: 'transparent',
            color: 'inherit',
            cursor: 'pointer',
            fontSize: '0.875rem',
            padding: '0 0.25rem',
            lineHeight: 1,
          }}
        >
          ×
        </button>
      </div>
      <ul style={{ margin: 0, padding: 0, listStyle: 'none' }}>
        {visible.map((w, i) => (
          <li key={`${w.code ?? 'd'}-${i}`} style={{ marginTop: i > 0 ? '0.25rem' : 0 }}>
            <span style={{ fontWeight: 500 }}>
              {w.code ? `${w.code}: ` : ''}
              {w.title}
            </span>
            {w.problem ? (
              <div style={{ marginTop: '0.125rem', opacity: 0.85 }}>{w.problem}</div>
            ) : null}
          </li>
        ))}
      </ul>
    </div>
  );
}
