/**
 * LoadingIndicator — the one loading indicator (Phase 3 of the UI/UX
 * modernization plan). Token-styled spinner + label, announced politely
 * to screen readers via role="status". The spinner's motion is covered
 * by the global prefers-reduced-motion rule in ui.css.
 */

import { common } from '../strings';

interface Props {
  /** Visible label; defaults to the shared "Loading…" copy. */
  label?: string;
}

export default function LoadingIndicator({ label = common.loading }: Props) {
  return (
    <div className="qh-loading" role="status">
      <span className="qh-spinner" aria-hidden="true" />
      {label}
    </div>
  );
}
