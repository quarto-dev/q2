/** SVG parts shared by the gallery icon and the sync diagram (SimpleAuthDemo). */

/** Radio tower with its tip at y=74 and base at y=180, centered on x=0. */
export function Tower() {
  return (
    <g className="rt-tower" fill="none" strokeWidth="5" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="0" cy="74" r="7" />
      <path d="M0 81v22M-28 180L-6 103h12l22 77M-16 138h32M-22 160h44M-10 118h20" />
    </g>
  );
}

/** Three arcs radiating from the origin, bulging to the right (dir 1) or left (dir -1). */
export function Waves({ dir, lit }: { dir: 1 | -1; lit: boolean }) {
  const arcs: [radius: number, x: number, halfChord: number][] = [[28, 64, 24], [44, 74, 36], [60, 84, 48]];
  return (
    <g className={`rt-waves${lit ? ' lit' : ''}`} fill="none" strokeWidth="4" strokeLinecap="round">
      {arcs.map(([r, x, h]) => (
        <path key={r} d={`M${dir * x} ${-h}a${r} ${r} 0 0 ${dir === 1 ? 1 : 0} 0 ${2 * h}`} />
      ))}
    </g>
  );
}

/** Grayed-out radio tower with waves on both sides; the gallery icon. */
export default function RadioTower({ lit }: { lit: boolean }) {
  return (
    <svg className="rt" viewBox="-155 20 310 170" width="240" height="132" aria-hidden="true">
      <g transform="translate(0 80)">
        <Waves dir={1} lit={lit} />
        <Waves dir={-1} lit={lit} />
      </g>
      <Tower />
    </svg>
  );
}
