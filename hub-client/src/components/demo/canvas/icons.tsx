/** Small logos and glyphs for canvas node headers. */

/** The WebAssembly mark: purple tile, "WA" (shape after webassembly.org's logo). */
export function WasmLogo({ size = 40 }: { size?: number }) {
  return (
    <svg viewBox="0 0 64 64" width={size} height={size} xmlns="http://www.w3.org/2000/svg" aria-label="WebAssembly">
      <path d="M6 2h52a4 4 0 0 1 4 4v52a4 4 0 0 1-4 4H6a4 4 0 0 1-4-4V6a4 4 0 0 1 4-4z" fill="#654ff0" />
      <path d="M24 2v2a8 8 0 0 0 16 0V2z" fill="#ffffff" fillOpacity="0.25" />
      <text x="32" y="44" textAnchor="middle" fontFamily="system-ui, sans-serif" fontWeight="700" fontSize="24" fill="#ffffff">WA</text>
    </svg>
  );
}

/** A key-value store: a 2×3 grid of cells. */
export function KeyValueIcon({ size = 18 }: { size?: number }) {
  return (
    <svg viewBox="0 0 20 30" width={size} height={size * 1.5} xmlns="http://www.w3.org/2000/svg" aria-label="key-value store">
      {[0, 1, 2].map((row) => [0, 1].map((col) => (
        <rect key={`${row}-${col}`} x={1 + col * 9} y={1 + row * 9.5} width="8" height="8.5" rx="1.5"
          fill={col === 0 ? '#e9d5ff' : '#f3f4f6'} stroke="#6b7280" strokeWidth="1" />
      )))}
    </svg>
  );
}
