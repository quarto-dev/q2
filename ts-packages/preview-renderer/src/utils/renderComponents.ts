/**
 * Shared meta walk for the `render-components:` front-matter key
 * (GH #402 / bd-ue80chl0). Extracted verbatim from hub-client's
 * `ReactRenderer.tsx` so both parent surfaces (hub-client and the
 * q2-preview SPA) read the key identically.
 *
 * The key parses to a MetaList of MetaInlines; each entry's path is the
 * first inline's `Str` content. Entries that don't resolve to a
 * non-empty string are dropped. This deliberately tolerates mid-typing
 * states:
 *  - `render-components:\n  -` — the bare bullet has no value and
 *    parses to `null`;
 *  - an empty MetaInlines — the user typed the path-string-open
 *    delimiter but no content yet.
 * Without the filter, downstream `resolveComponentPath(undefined, …)`
 * would throw inside the host's render path.
 *
 * A non-list value (scalar `render-components: foo.tsx`) yields `[]`:
 * mapping over the MetaInlines' inline nodes never produces a string at
 * `.c[0].c`, so every entry is filtered out. Same behavior as the
 * original hub-client walk.
 */
export function extractRenderComponentPaths(ast: unknown): string[] {
  const rawPaths: unknown[] =
    (ast as any)?.meta?.['render-components']?.c?.map?.(
      (o: any) => o?.c?.[0]?.c,
    ) ?? [];
  return rawPaths.filter(
    (p): p is string => typeof p === 'string' && p.length > 0,
  );
}
