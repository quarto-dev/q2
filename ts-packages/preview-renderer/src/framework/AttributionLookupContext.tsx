import { createContext, useContext } from 'react';

/**
 * Per-node attribution record resolved from `astContext.attribution`
 * + `astContext.attributionActors` (Phase 5c). Keyed by the node's
 * source-info pool id (`s` field on every JSON node). Carries the
 * per-record `time` so the hover badge can render a relative
 * timestamp ("3m ago", "2d ago").
 */
export interface NodeAttributionIdentity {
    actor: string;
    name: string;
    color: string;
    /**
     * Last-touch time. Unix milliseconds (Automerge) or seconds
     * (git blame) — consumers normalise both via the `< 1e12` heuristic.
     */
    time: number;
}

/**
 * Lookup from source-info pool id (`s`) to the resolved identity
 * for that node. `null` when attribution is off — every consumer
 * site short-circuits in that case, leaving the renderer's
 * existing output unchanged.
 *
 * Provided by `framework/Ast.tsx` once per AST render. Format
 * dispatchers (q2-debug today, q2-preview eventually) consume via
 * `useNodeAttribution(node)` and decide their own visual treatment.
 */
export const AttributionLookupContext = createContext<
    Map<number, NodeAttributionIdentity> | null
>(null);

/**
 * Look up the attribution identity for the AST node currently being
 * rendered, if any. Returns `null` when attribution is off, when the
 * node has no `s` field, or when the lookup has no entry for that
 * `s`. Callers can then apply colour / hover state conditionally.
 */
export function useNodeAttribution(
    node: { s?: number } | undefined,
): NodeAttributionIdentity | null {
    const lookup = useContext(AttributionLookupContext);
    if (!lookup || node == null || node.s == null) return null;
    return lookup.get(node.s) ?? null;
}
