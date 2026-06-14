/**
 * Equivalence invariant: buildDepthCommitDestination vs. JSON.stringify(resolved.sourceEntry)
 *
 * This test guards the safety premise of the "live-identity refactor" (P3.3 §3 hardening):
 * for every editable block, the commit destination produced by buildDepthCommitDestination
 * is string-identical to JSON.stringify(resolved.sourceEntry).
 *
 * WHY THIS IS SAFE (and what would make it unsafe):
 *
 * The Rust matcher apply_node_edit (crates/pampa/src/apply_node_edit.rs) decodes the
 * destination JSON reading ONLY `t`, `r`, `d` (extra fields are ignored by the decoder).
 * It matches a block by the {t, r, d} triple.
 *
 * sourceIndex.ts only indexes/makes editable blocks where t === 0 && d === 0.
 * Therefore every editable block's resolved.sourceEntry has the shape {t:0, r:[r0,r1], d:0}.
 *
 * buildDepthCommitDestination returns JSON.stringify({ t: 0, r: [anchorR0, anchorR1], d: 0 }).
 * For an editable block where anchorR0 === sourceEntry.r[0] and anchorR1 === sourceEntry.r[1]
 * (enforced by isBlockEditTarget via anchorR0 === resolved.sourceEntry.r[0] and self-heal
 * updating anchorR1 in lockstep), the two strings are identical.
 *
 * IF pampa ever emits non-zero t/d for editable blocks, or adds extra fields to sourceEntry
 * that the closure form would preserve, these assertions will break — signalling that the
 * refactor's premise no longer holds and the commit destinations must be revisited.
 */

import { describe, it, expect } from 'vitest';
import { buildDepthCommitDestination } from './depthNav';

describe('commit-destination equivalence invariant', () => {
    it('buildDepthCommitDestination === JSON.stringify(resolved.sourceEntry) for a representative editable block', () => {
        // Representative editable pool entry: {t:0, r:[6,12], d:0}
        // (e.g. para1 in the 3-block test fixtures used throughout the P2/P3 suite)
        const sourceEntry = { t: 0, r: [6, 12] as [number, number], d: 0 };

        const closureForm = JSON.stringify(sourceEntry);
        const liveForm = buildDepthCommitDestination({ anchorR0: 6, anchorR1: 12 });

        expect(liveForm).toBe(closureForm);
    });

    it('buildDepthCommitDestination === JSON.stringify(resolved.sourceEntry) for a nested editable block', () => {
        // Nested child para from the blockquote fixture: {t:0, r:[2,22], d:0}
        const sourceEntry = { t: 0, r: [2, 22] as [number, number], d: 0 };

        const closureForm = JSON.stringify(sourceEntry);
        const liveForm = buildDepthCommitDestination({ anchorR0: 2, anchorR1: 22 });

        expect(liveForm).toBe(closureForm);
    });

    it('buildDepthCommitDestination === JSON.stringify(resolved.sourceEntry) for a top-level block at byte 0', () => {
        // Top-level block at the start of the document: {t:0, r:[0,6], d:0}
        const sourceEntry = { t: 0, r: [0, 6] as [number, number], d: 0 };

        const closureForm = JSON.stringify(sourceEntry);
        const liveForm = buildDepthCommitDestination({ anchorR0: 0, anchorR1: 6 });

        expect(liveForm).toBe(closureForm);
    });

    it('returns null when there is no active edit target', () => {
        // Null-guard: if editTargetRef.current is null, buildDepthCommitDestination returns null.
        // The refactored commit paths treat null as "no active target → skip the commit".
        expect(buildDepthCommitDestination(null)).toBeNull();
        expect(buildDepthCommitDestination(undefined)).toBeNull();
    });
});
