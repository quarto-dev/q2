/**
 * Parent half of the render-components pipeline for the q2-preview SPA
 * (GH #402 / bd-ue80chl0). Mirrors hub-client's `ReactRenderer.tsx`
 * flow — meta walk → `resolveComponentPath` → content lookup →
 * `transpileTSX` — using the same shared helpers, so the two preview
 * surfaces cannot drift.
 *
 * The shared transpiler (`@quarto/preview-renderer/utils/tsxTranspiler`,
 * which pulls `@babel/standalone`, ~3 MB min) is imported DYNAMICALLY
 * and only when the document actually lists components: Vite splits it
 * into a lazy chunk, so documents without `render-components:` never
 * fetch or parse babel. Keep every import of that module dynamic — a
 * static import anywhere in the SPA graph would fold babel into the
 * main chunk.
 */

import { resolveComponentPath } from '@quarto/preview-renderer/utils/componentPath';
import { extractRenderComponentPaths } from '@quarto/preview-renderer/utils/renderComponents';
import type { Diagnostic } from '@quarto/preview-renderer/types/diagnostic';

export interface CustomComponentsResult {
  /**
   * Compiled JS keyed by the ORIGINAL `render-components` entry string
   * (hub-client parity — the iframe logs these keys verbatim). Entries
   * that fail (missing file, transpile error) are omitted; the failure
   * is reported in `warnings` instead.
   */
  code: Record<string, string>;
  /**
   * Component-pipeline failures, shaped as render diagnostics: per the
   * plan's Q3 decision, "compiling a component" is part of "rendering",
   * so these merge into the render-warnings overlay lane.
   */
  warnings: Diagnostic[];
}

/**
 * Referentially-stable result for documents without `render-components`.
 * PreviewApp keeps this exact object in state for the common case so
 * the iframe's `customComponentsCode` prop identity never churns (and
 * `LOAD_CUSTOM_COMPONENTS` is never re-posted) across ordinary edits.
 */
export const EMPTY_CUSTOM_COMPONENTS: CustomComponentsResult = {
  code: {},
  warnings: [],
};

/**
 * Cheap, stable effect key for the transpile pipeline: the JSON string
 * of the document's resolved `render-components` path list, or `''`
 * when the document has none (or `astJson` is absent/unparseable).
 *
 * String-stable across `.qmd` keystrokes that don't touch the list —
 * that's the Q1 cadence decision: per-keystroke edits must not
 * accumulate babel runs. Re-transpilation is triggered only by this
 * key changing or by a `.tsx` file being touched (the caller's
 * `tsxTick`).
 */
export function extractComponentPathsKey(astJson: string | null): string {
  if (!astJson) return '';
  let ast: unknown;
  try {
    ast = JSON.parse(astJson);
  } catch {
    return '';
  }
  const paths = extractRenderComponentPaths(ast);
  return paths.length > 0 ? JSON.stringify(paths) : '';
}

/**
 * Resolve, read, and transpile the document's custom components.
 *
 * @param componentPathsKey key from {@link extractComponentPathsKey}
 * @param currentFilePath   project-root-relative path of the document
 *                          (relative entries resolve against its dir)
 * @param getContent        text-file lookup, keyed by project-root-
 *                          relative path without a leading slash (the
 *                          SPA passes `getFileContent` from
 *                          `@quarto/preview-runtime`)
 */
export async function buildCustomComponentsCode(
  componentPathsKey: string,
  currentFilePath: string,
  getContent: (path: string) => string | null,
): Promise<CustomComponentsResult> {
  if (!componentPathsKey) {
    return EMPTY_CUSTOM_COMPONENTS;
  }
  const componentPaths = JSON.parse(componentPathsKey) as string[];

  // Lazy chunk: babel is only loaded once a document actually lists
  // components. Subsequent calls hit the module cache.
  const { transpileTSX } = await import(
    '@quarto/preview-renderer/utils/tsxTranspiler'
  );

  const code: Record<string, string> = {};
  const warnings: Diagnostic[] = [];
  for (const path of componentPaths) {
    const lookupPath = resolveComponentPath(path, currentFilePath);
    const tsxCode = getContent(lookupPath);
    if (tsxCode === null || tsxCode === undefined) {
      console.warn(`[PreviewApp] Component file not found: ${path}`);
      warnings.push({
        kind: 'warning',
        title: `render-components: file not found: ${path}`,
        problem: `The document lists \`${path}\` under \`render-components\`, but no synced file exists at \`${lookupPath}\`. The built-in component will be used instead.`,
        hints: [],
        details: [],
      });
      continue;
    }
    try {
      code[path] = transpileTSX(tsxCode);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      console.error(`[PreviewApp] Failed to transpile component ${path}:`, err);
      warnings.push({
        kind: 'warning',
        title: `render-components: transpile error in ${path}`,
        problem: message,
        hints: [],
        details: [],
      });
    }
  }

  return { code, warnings };
}
