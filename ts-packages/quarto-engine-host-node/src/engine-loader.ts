/**
 * engine-loader — dynamic engine-module import + validation.
 *
 * Loads a TypeScript/JS engine module by absolute path, reads its default
 * export (the ExecutionEngineDiscovery), validates the required surface, and
 * returns it.
 *
 * Uses `node:url`'s `pathToFileURL` to build a `file://` URL for dynamic
 * import. This works under both Node/vitest (our test runner) and Deno
 * (which supports `node:` specifiers). No `Deno.*` API is used here.
 *
 * Required surface on the default export (per ExecutionEngineDiscovery):
 *   - name: string
 *   - claimsLanguage: function
 *   - launch: function
 *   (init? is optional)
 */

import { pathToFileURL } from "node:url";
import type { ExecutionEngineDiscovery } from "@quarto/types";

/**
 * Dynamically import an engine module from the given absolute path and
 * validate that its default export satisfies the ExecutionEngineDiscovery
 * required surface.
 *
 * @param path  Absolute path to the engine module (.mjs, .ts, .js, etc.).
 * @returns     The validated ExecutionEngineDiscovery default export.
 * @throws      Error if the module has no default export, or if any of
 *              `name`, `claimsLanguage`, or `launch` is missing / wrong type.
 */
export async function loadEngineModule(
  path: string,
): Promise<ExecutionEngineDiscovery> {
  const url = pathToFileURL(path);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const mod: Record<string, unknown> = await import(url.href);

  const discovery = mod["default"];

  // ── Validate: default export must exist ─────────────────────────────────────
  if (discovery === null || discovery === undefined) {
    throw new Error(
      `engine module ${path} has no default export (expected ExecutionEngineDiscovery)`,
    );
  }

  const disc = discovery as Record<string, unknown>;

  // ── Validate required members ────────────────────────────────────────────────
  //
  // Named revert: remove this validation block → invalid modules (missing launch,
  // name, or claimsLanguage) load without error → the "invalid engine rejects"
  // assertions go RED.
  if (typeof disc["name"] !== "string") {
    throw new Error(
      `engine module ${path} is missing required export: name (must be a string)`,
    );
  }
  if (typeof disc["claimsLanguage"] !== "function") {
    throw new Error(
      `engine module ${path} is missing required export: claimsLanguage (must be a function)`,
    );
  }
  if (typeof disc["launch"] !== "function") {
    throw new Error(
      `engine module ${path} is missing required export: launch (must be a function)`,
    );
  }

  return discovery as ExecutionEngineDiscovery;
}
