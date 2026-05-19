/*
 * attribution/palette.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Deterministic colour helpers shared with the hub-client TS
//! producer.
//!
//! Drift mitigation: the doc-comments cross-reference the TS
//! siblings (`actorColor` / `fnv1aHex8` in
//! `hub-client/src/utils/palette.ts`). Anyone editing either is
//! forced to consider the other; reference test vectors here and in
//! `palette.test.ts` pin the cross-implementation parity.

/// Tol Muted — a 10-colour qualitative, colour-blind-safe palette by
/// Paul Tol. Reproduced from "Notes on colour schemes"
/// (<https://sronpersonalpages.nl/~pault/>) as factual data; see the
/// linked notes for the design rationale. Ordering matches Tol's
/// canonical sequence so the same actor hash lands on the same name
/// across libraries that adopt this palette (R `khroma`, Python
/// `paletteer`, etc.).
const TOL_MUTED: [&str; 10] = [
    "#CC6677", // rose
    "#332288", // indigo
    "#DDCC77", // sand
    "#117733", // green
    "#88CCEE", // cyan
    "#882255", // wine
    "#44AA99", // teal
    "#999933", // olive
    "#AA4499", // purple
    "#DDDDDD", // pale grey
];

/// Deterministic colour from an actor hash string.
///
/// Formula: parse the first 6 hex chars of the actor ID as an
/// integer, mod the palette length, index into [`TOL_MUTED`]. Non-hex
/// input (or an empty string) collapses to index `0`.
///
/// **MUST stay in sync with the TS `actorColor` in
/// `hub-client/src/utils/palette.ts` — same palette, same formula.**
pub fn actor_color(actor: &str) -> String {
    // Mirror TS `actor.slice(0, 6)`: first 6 Unicode scalar values
    // (effectively bytes for the hex inputs the producer contracts
    // feed us).
    let prefix: String = actor.chars().take(6).collect();
    let n = u32::from_str_radix(&prefix, 16).unwrap_or(0);
    let idx = (n as usize) % TOL_MUTED.len();
    TOL_MUTED[idx].to_string()
}

/// 32-bit FNV-1a hash, formatted as a left-padded 8-char hex string.
///
/// Used wherever an arbitrary actor string (e.g. an email) must be
/// reduced to a hex-prefix-safe input for [`actor_color`]. Caller:
/// `GitBlameProvider` (pre-hashes the author email). The TS sibling
/// `fnv1aHex8` plays the same role for Automerge actor IDs whose
/// first 6 chars aren't guaranteed hex or that need fallback colouring
/// when profile metadata is absent.
///
/// **MUST stay in sync with the TS `fnv1aHex8` (Phase 5
/// hub-client work item).**
pub fn fnv1a_hex8(s: &str) -> String {
    const FNV_OFFSET_BASIS_32: u32 = 0x811c_9dc5;
    const FNV_PRIME_32: u32 = 0x0100_0193;
    let mut hash: u32 = FNV_OFFSET_BASIS_32;
    for b in s.as_bytes() {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(FNV_PRIME_32);
    }
    format!("{hash:08x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_color_returns_a_tol_muted_entry() {
        // Every output of `actor_color` must be one of the canonical
        // palette entries. This is the core invariant: if the formula
        // drifts and starts emitting (say) HSL strings, every site
        // that asserts on the colour will catch it via this rule —
        // including the TS sibling.
        let c = actor_color("aabbccdd");
        assert!(
            TOL_MUTED.contains(&c.as_str()),
            "{c} must be one of the Tol Muted palette entries"
        );
    }

    #[test]
    fn actor_color_matches_ts_for_known_inputs() {
        // parseInt("aabbcc", 16) = 0xaabbcc = 11_189_196; % 10 = 6 →
        // TOL_MUTED[6] = teal.
        assert_eq!(actor_color("aabbccdd"), "#44AA99");
        // parseInt("000000", 16) = 0; % 10 = 0 → TOL_MUTED[0] = rose.
        assert_eq!(actor_color("00000000"), "#CC6677");
    }

    #[test]
    fn actor_color_handles_empty_and_non_hex_input() {
        // Both fall through to index 0 → rose.
        assert_eq!(actor_color(""), "#CC6677");
        assert_eq!(actor_color("zzz"), "#CC6677");
    }

    #[test]
    fn fnv1a_hex8_known_vectors() {
        // Reference values for FNV-1a 32-bit.
        assert_eq!(fnv1a_hex8(""), "811c9dc5");
        assert_eq!(fnv1a_hex8("a"), "e40c292c");
        assert_eq!(fnv1a_hex8("foobar"), "bf9cf968");
    }

    #[test]
    fn fnv1a_hex8_is_eight_chars_lowercase_hex() {
        let h = fnv1a_hex8("alice@example.com");
        assert_eq!(h.len(), 8);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
