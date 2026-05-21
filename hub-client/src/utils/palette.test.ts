import { describe, it, expect } from 'vitest';

import { actorColor, fnv1aHex8 } from './palette';

// Drift-mitigation: bit-for-bit parity with the Rust siblings in
// `crates/quarto-core/src/attribution/palette.rs`. A divergence here
// is a producer/consumer drift bug; the rendered colour for a given
// actor would no longer match between the replay drawer and the
// Attribution overlay.

describe('actorColor', () => {
  it('matches the Rust `actor_color` formula for known inputs', () => {
    // parseInt("aabbcc", 16) = 0xaabbcc = 11_189_196; % 10 = 6;
    // TOL_MUTED[6] = teal.
    expect(actorColor('aabbccdd')).toBe('#44AA99');
    // parseInt("000000", 16) = 0; % 10 = 0; TOL_MUTED[0] = rose.
    expect(actorColor('00000000')).toBe('#CC6677');
  });

  it('handles empty and non-hex input (NaN -> index 0)', () => {
    expect(actorColor('')).toBe('#CC6677');
    expect(actorColor('zzz')).toBe('#CC6677');
  });
});

describe('fnv1aHex8', () => {
  it('matches Rust FNV-1a 32-bit reference vectors bit-for-bit', () => {
    // Same vectors pinned in palette.rs::tests::fnv1a_hex8_known_vectors.
    expect(fnv1aHex8('')).toBe('811c9dc5');
    expect(fnv1aHex8('a')).toBe('e40c292c');
    expect(fnv1aHex8('foobar')).toBe('bf9cf968');
  });

  it('emits an 8-char lowercase hex string', () => {
    const h = fnv1aHex8('alice@example.com');
    expect(h).toHaveLength(8);
    expect(h).toMatch(/^[0-9a-f]{8}$/);
  });
});
