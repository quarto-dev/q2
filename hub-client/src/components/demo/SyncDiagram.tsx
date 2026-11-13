/**
 * A document (holding the counter) talking to a sync server. Waves leave
 * the document when the counter changes, and leave the tower on each ack.
 */
import { Tower, Waves } from './RadioTower';

interface Props { count: number; docLit: boolean; ackLit: boolean }

export default function SyncDiagram({ count, docLit, ackLit }: Props) {
  return (
    <svg className="sd" viewBox="0 0 400 200" width="400" height="200" aria-hidden="true">
      {/* Spans the tower's height: tip at y=77, base at y=173. */}
      <g className="sd-doc" fill="none" strokeWidth="4" strokeLinejoin="round">
        <path d="M30 77h60l20 20v76H30z" />
        <path d="M90 77v20h20" />
      </g>
      <text className={`sd-count${docLit ? ' lit' : ''}`} x="70" y="125" textAnchor="middle" dominantBaseline="middle">
        {count}
      </text>
      <g transform="translate(110 125) scale(0.5)"><Waves dir={1} lit={docLit} /></g>
      <g transform="translate(335 125) scale(0.5)"><Waves dir={-1} lit={ackLit} /></g>
      <g transform="translate(335 20) scale(0.85)"><Tower /></g>
      <text className="sd-label" x="335" y="194" textAnchor="middle">sync server</text>
    </svg>
  );
}
