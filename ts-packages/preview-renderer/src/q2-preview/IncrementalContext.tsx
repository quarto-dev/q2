import { createContext } from 'react';

/**
 * Incremental-list state for `q2 preview` of revealjs decks — the preview-side
 * mirror of the native writer's incremental threading
 * (`crates/pampa/src/writers/html.rs`, Pandoc's `writerIncremental`). Because
 * Pandoc list items carry no `Attr`, the `fragment` class is attached by the
 * renderer; here that's the `BulletList`/`OrderedList` components reading this
 * context.
 *
 * - `enabled` — true only inside a `RevealDeck` (revealjs). When false (the
 *   html preview, q2-debug) the list components take their unchanged path, so
 *   `.incremental` is a no-op outside slides, matching the writer's gating.
 * - `incremental` — current state, flipped by `.incremental` / `.nonincremental`
 *   Divs (and the global `incremental: true` default set by `RevealDeck`).
 */
export interface IncrementalState {
    enabled: boolean;
    incremental: boolean;
}

export const IncrementalContext = createContext<IncrementalState>({
    enabled: false,
    incremental: false,
});
