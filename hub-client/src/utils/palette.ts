/**
 * Deterministic actor-colour helpers shared between the replay drawer
 * (`useReplayMode` / `ReplayDrawer`) and the attribution producer
 * (`useAttribution`). Both must produce identical visual output for the
 * same actor — colours seen during replay must match colours seen on
 * Attribution overlays.
 *
 * **Drift discipline.** Both functions MUST stay bit-for-bit identical
 * with their Rust siblings in
 * `crates/quarto-core/src/attribution/palette.rs`. The Rust side
 * colours git-blame author emails (via `--attribution=git`); this TS
 * side colours Automerge actor IDs. They share `actor_color` /
 * `actorColor` and `fnv1a_hex8` / `fnv1aHex8` definitions; the test
 * suite in `palette.test.ts` pins reference vectors that match
 * `palette.rs::tests`.
 */

/**
 * Tol Muted — a 10-colour qualitative, colour-blind-safe palette by
 * Paul Tol. Reproduced from "Notes on colour schemes"
 * (https://sronpersonalpages.nl/~pault/) as factual data; see the
 * linked notes for the design rationale. Ordering matches Tol's
 * canonical sequence so the same actor hash lands on the same name
 * across libraries that adopt this palette (R `khroma`, Python
 * `paletteer`, etc.).
 *
 * MUST stay in sync with `TOL_MUTED` in the Rust sibling
 * `crates/quarto-core/src/attribution/palette.rs`.
 */
const TOL_MUTED: readonly string[] = [
  '#CC6677', // rose
  '#332288', // indigo
  '#DDCC77', // sand
  '#117733', // green
  '#88CCEE', // cyan
  '#882255', // wine
  '#44AA99', // teal
  '#999933', // olive
  '#AA4499', // purple
  '#DDDDDD', // pale grey
] as const;

/**
 * Deterministic colour from an actor hash string.
 *
 * Formula: parse the first 6 hex chars of the actor ID as an integer,
 * mod the palette length, index into `TOL_MUTED`. Non-hex input (or
 * an empty string) collapses to index `0`.
 */
export function actorColor(actor: string): string {
  const n = parseInt(actor.slice(0, 6), 16);
  const idx = (Number.isNaN(n) ? 0 : n) % TOL_MUTED.length;
  return TOL_MUTED[idx];
}

/**
 * 32-bit FNV-1a hash, formatted as a left-padded 8-char hex string.
 *
 * Used to reduce an arbitrary actor string (e.g. an Automerge actor
 * ID whose first 6 chars aren't guaranteed hex) to a hex-prefix-safe
 * input for `actorColor`. Caller: the attribution producer in
 * `useAttribution`, when synthesising the `(name, color)` fallback
 * identity for actors with no profile metadata.
 *
 * Not cryptographic; deterministic and well-distributed for colour
 * purposes.
 */
export function fnv1aHex8(s: string): string {
  // Hash UTF-8 bytes to match Rust's `s.bytes()` iteration. JS strings
  // are UTF-16 internally; using `charCodeAt` directly would diverge
  // from Rust on any non-ASCII character.
  const bytes = TEXT_ENCODER.encode(s);
  let hash = 0x811c9dc5;
  for (let i = 0; i < bytes.length; i++) {
    hash ^= bytes[i];
    // Multiply by 0x01000193 (16777619), masked back into a u32 each
    // iteration. `Math.imul` does 32-bit multiplication; `>>> 0`
    // coerces to unsigned.
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, '0');
}

const TEXT_ENCODER = new TextEncoder();
