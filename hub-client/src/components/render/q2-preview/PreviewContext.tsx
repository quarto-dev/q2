import { createContext } from 'react';

/**
 * q2-preview-specific context. Carries values that don't belong on the
 * framework's `RegistryContext` because q2-debug doesn't need them.
 *
 * Today: `currentFilePath` for resolving relative image paths and qmd
 * link targets in q2-preview's leaves (Plan 2B's `Image`, link handlers).
 *
 * The default value is `null` — leaves should treat absence as a bug
 * (every q2-preview render is mounted under a `PreviewContext.Provider`
 * by `entry.tsx`'s `PreviewRoot`).
 */
export interface PreviewContextValue {
    currentFilePath: string;
}

export const PreviewContext = createContext<PreviewContextValue | null>(null);
