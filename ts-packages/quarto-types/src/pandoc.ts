// parity: vendored from external-sources/quarto-cli/packages/quarto-types
/**
 * Pandoc types for Quarto
 */

/**
 * Valid Pandoc include locations
 */
export type PandocIncludeLocation =
  | "include-in-header"
  | "include-before-body"
  | "include-after-body";

/**
 * Pandoc includes for headers, body, etc.
 * Mapped type that allows any of the valid include locations.
 *
 * **Field-name note (T-Gate-parity finding):** This Q1-compatible type uses
 * kebab-case string keys (`"include-in-header"`, `"include-before-body"`,
 * `"include-after-body"`), while the wire type `TsPandocIncludes` in
 * `quarto-engine-host-deno/src/types.ts` uses camelCase identifiers
 * (`inHeader`, `beforeBody`, `afterBody`).  The harness's `renameIncludes()`
 * helper (host.ts) converts between the two at the protocol boundary —
 * engines always receive and return `PandocIncludes`; the wire never exposes
 * the Q1 kebab-case keys.
 *
 * Both `QuartoAPI["jupyter"].widgetDependencyIncludes` and
 * `DependenciesResult.includes` (execution.ts) use this same `PandocIncludes`
 * type, ensuring a consistent Q1-compatible surface for engine authors.
 */
export type PandocIncludes = {
  [K in PandocIncludeLocation]?: string[];
};

/**
 * Structured HTML dependency manifest.
 * Mirrors the Rust `TsHtmlDependency` wire type from `ts_protocol.rs`.
 * `stylesheets` and `scripts` are optional here (the harness normalizes to []
 * at serialization time when needed).
 */
export interface HtmlDependency {
  name: string;
  stylesheets?: string[];
  scripts?: string[];
}
