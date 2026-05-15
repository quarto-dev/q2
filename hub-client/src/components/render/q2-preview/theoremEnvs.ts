/**
 * Map a theorem-like `ref_type` to its env class name.
 *
 * Port of `theorem_env_for` at
 * `crates/quarto-core/src/transforms/crossref_render.rs:388-400`.
 * The 8-entry table is closed: each canonical Quarto theorem-like ref
 * type has one entry, and the unknown / missing case returns the empty
 * string. `Theorem.tsx` consumes this to compute the env class that
 * sits alongside `theorem` (per `crossref_render.rs:346-352`).
 *
 * Sync convention: when the Rust mapping changes (new theorem-like
 * ref type), update both files together.
 */
export function theoremEnvFor(refType: string): string {
    switch (refType) {
        case 'thm': return 'theorem';
        case 'lem': return 'lemma';
        case 'cor': return 'corollary';
        case 'prp': return 'proposition';
        case 'cnj': return 'conjecture';
        case 'def': return 'definition';
        case 'exm': return 'example';
        case 'exr': return 'exercise';
        default:    return '';
    }
}
