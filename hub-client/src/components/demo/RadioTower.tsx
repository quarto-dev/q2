/** Grayed-out radio tower with three waves on each side (see SimpleAuthDemo). */
export default function RadioTower({ lit }: { lit: boolean }) {
  const waves = [
    'M64 56a28 28 0 0 1 0 48', 'M74 44a44 44 0 0 1 0 72', 'M84 32a60 60 0 0 1 0 96',
    'M-64 56a28 28 0 0 0 0 48', 'M-74 44a44 44 0 0 0 0 72', 'M-84 32a60 60 0 0 0 0 96',
  ];
  return (
    <svg className={`rt${lit ? ' lit' : ''}`} viewBox="-155 20 310 170" width="240" height="132" aria-hidden="true">
      <g className="rt-waves" fill="none" strokeWidth="4" strokeLinecap="round">
        {waves.map((d) => <path key={d} d={d} />)}
      </g>
      <g className="rt-tower" fill="none" strokeWidth="5" strokeLinecap="round" strokeLinejoin="round">
        <circle cx="0" cy="74" r="7" />
        <path d="M0 81v22M-28 180L-6 103h12l22 77M-16 138h32M-22 160h44M-10 118h20" />
      </g>
    </svg>
  );
}
