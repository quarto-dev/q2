/**
 * The render pipeline's diagnostics for a project file: what the preview
 * reports through `onDiagnosticsChange`, obtained without a preview by
 * rendering the page through the runtime's queued project render. This is
 * the full set the app underlines — parse errors plus everything the
 * transforms find against the project (links to files that don't exist,
 * unresolved cross-references, unknown shortcodes, ...), which the LSP's
 * analysis pipeline alone does not run.
 *
 * Debounced on `content`; a render superseded by a newer edit is dropped
 * silently, like the live preview does.
 */

import { useEffect, useRef, useState } from 'react';
import type { Diagnostic } from '@quarto/preview-renderer/types/diagnostic';
import { RenderSupersededError, renderPageInProjectWithAttribution } from '@quarto/preview-runtime';

const EMPTY: Diagnostic[] = [];

export function useRenderDiagnostics(path: string | null, content: string, enabled = true, debounceMs = 300): Diagnostic[] {
  // Tagged with the path they were rendered for, so a path switch shows
  // nothing until its own render lands.
  const [result, setResult] = useState<{ path: string; diagnostics: Diagnostic[] } | null>(null);
  const requestRef = useRef(0);

  useEffect(() => {
    if (!enabled || path === null) return;
    const request = ++requestRef.current;
    const timer = window.setTimeout(() => {
      void renderPageInProjectWithAttribution(path, undefined, null, undefined, true)
        .then((response) => {
          if (request !== requestRef.current) return;
          setResult({ path, diagnostics: [...(response.diagnostics ?? []), ...(response.warnings ?? [])] });
        })
        .catch((err: unknown) => {
          if (err instanceof RenderSupersededError) return;
          console.error('render diagnostics failed:', err);
        });
    }, debounceMs);
    return () => window.clearTimeout(timer);
  }, [path, content, enabled, debounceMs]);

  return enabled && result !== null && result.path === path ? result.diagnostics : EMPTY;
}
