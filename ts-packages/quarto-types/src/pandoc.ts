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
 * Mapped type that allows any of the valid include locations
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
