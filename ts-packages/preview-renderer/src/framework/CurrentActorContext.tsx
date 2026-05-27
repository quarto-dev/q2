import { createContext, useContext } from 'react';

/**
 * Current viewer's Automerge actor id, threaded into the iframe by
 * the parent (`Q2PreviewIframe` → `entry.tsx`) so user TSX can match
 * `actor === me` against `useNodeAttribution(node).actor`.
 *
 * `null` when the producer has no actor yet (e.g. the project hasn't
 * finished initialising, or the format isn't driven by an Automerge
 * document at all). User TSX should treat `null` as "I don't know
 * who I am" and avoid mine-vs-not branches in that state.
 *
 * Sourced from `@quarto/preview-runtime#getActorId()` in the parent.
 * Demo design (2026-05-25 plan) piggybacks on the `UPDATE_AST`
 * payload; long-term the value belongs in `astContext.currentActor`
 * alongside `astContext.attribution*` (Plan 5 follow-up).
 */
export const CurrentActorContext = createContext<string | null>(null);

/**
 * Hook for user TSX (rendered inside the q2-preview iframe) to read
 * the current viewer's Automerge actor id. Pairs with
 * `useNodeAttribution(node).actor` for "is this contribution mine?"
 * checks in interactive overrides like the reactji-toggle demo.
 */
export function useCurrentActor(): string | null {
    return useContext(CurrentActorContext);
}
