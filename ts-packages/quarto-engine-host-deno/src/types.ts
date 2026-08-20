/**
 * @quarto/engine-host-deno — JSON wire protocol types
 *
 * Source of truth: `crates/quarto-core/src/engine/ts_protocol.rs`
 *
 * Wire-shape conventions (enforced by serde on the Rust side):
 *   - Struct fields: camelCase on the wire (`#[serde(rename_all = "camelCase")]`).
 *   - Message enum tags: internal `type` field with explicit per-variant renames.
 *   - `TsLanguageClaim`: internal `kind` tag, lowercase variant names.
 *   - `TsFormatIdentifier`: explicit kebab-case per-field renames; `extension-name`
 *     omitted when None.
 *   - `TsMetadataValue`: untagged (each variant serializes as a bare JSON value).
 *
 * TypeScript field names in this file match the camelCase WIRE names (not the
 * Rust snake_case identifiers). When in doubt, the Rust file is authoritative.
 */

import type { HtmlDependency } from "@quarto/types";

// ==================== Correlation envelope ====================

/** A `ToEngine` message wrapped with a correlation id allocated by the Rust host.
 *  Wire shape: `{ "id": N, "msg": { "type": …, … } }`.
 *  The `msg` is deliberately NOT flattened — flatten round-trips poorly with
 *  internally-tagged enums in serde. */
export interface Request {
  id: number;
  msg: ToEngine;
}

/** A `FromEngine` message wrapped with the id of the request it answers. */
export interface Response {
  id: number;
  msg: FromEngine;
}

// ==================== Message enums ====================

/** Messages from Rust (q2) → Deno engine host. */
export type ToEngine =
  | { type: "init"; global: HostGlobalConfig }
  | { type: "loadEngine"; enginePath: string }
  | { type: "launchEngine"; engine: string; project: EngineProjectContext }
  | { type: "shutdown" }
  | { type: "claimsLanguage"; engine: string; language: string; firstClass?: string | null }
  | { type: "claimsFile"; engine: string; file: string; ext: string }
  | { type: "markdownForFile"; engine: string; file: string }
  | { type: "execute"; engine: string; options: TsExecuteOptions }
  | { type: "intermediateFiles"; engine: string; input: string }
  | { type: "dependencies"; engine: string; options: TsDependenciesOptions }
  | { type: "cancel"; target: number };

/** Messages from Deno engine host → Rust (q2). */
export type FromEngine =
  | { type: "loaded"; discovery: LoadEngineResult }
  | { type: "launched"; instance: LaunchEngineResult }
  | { type: "error"; message: string; stack?: string | null }
  | { type: "claimsLanguageResult"; result: TsLanguageClaim | null }
  | { type: "claimsFileResult"; result: boolean }
  | { type: "markdownForFileResult"; result: TsMappedStringWithMap }
  | { type: "executeResult"; result: TsExecuteResult }
  | { type: "intermediateFilesResult"; result: string[] | null }
  | { type: "dependenciesResult"; includes: TsPandocIncludes }
  | { type: "cancelled" };

// ==================== Lifecycle response payloads ====================

/** Response to `loadEngine` — discovery surface (cheap to obtain).
 *  `generatesFigures` lives ONLY at the discovery tier, not on `LaunchEngineResult`. */
export interface LoadEngineResult {
  name: string;
  validExtensions: string[];
  generatesFigures: boolean;
  canFreeze: boolean;
  quartoRequired?: string | null;
}

/** Response to `launchEngine` — instance metadata available after `launch()`.
 *  Only field: `canFreeze`. `generatesFigures` is absent (it moved to discovery). */
export interface LaunchEngineResult {
  canFreeze: boolean;
}

// ==================== Language claim ====================

/** Kind-tagged language claim returned by `claimsLanguage`. */
export type TsLanguageClaim =
  | { kind: "primary"; priority: number }
  | { kind: "interop"; priority: number }
  | { kind: "fallback"; priority: number };

// ==================== Engine host context ====================

/** Process-stable config, delivered once on `init` at spawn. */
export interface HostGlobalConfig {
  resourceDir: string;
  runtimeDir: string;
  dataDir: string;
  pandocPath?: string | null;
  isInteractiveSession: boolean;
  runningInCi: boolean;
  quartoVersion: string;
}

/** Per-render project context, carried on each `launchEngine`. */
export interface EngineProjectContext {
  projectDir?: string | null;
  isSingleFile: boolean;
  config?: Record<string, TsMetadataValue> | null;
  outputDir?: string | null;
}

// ==================== Mapped string with source map ====================

/** Used in `markdownForFileResult` (non-QMD file conversion). */
export interface TsMappedStringWithMap {
  value: string;
  fileName?: string | null;
  sourceMap: TsSourceMapEntry[];
}

// ==================== Pandoc types ====================

export interface TsPandocIncludes {
  inHeader?: string[] | null;
  beforeBody?: string[] | null;
  afterBody?: string[] | null;
}

// ==================== Format info ====================

/** Q1-compatible `FormatIdentifier` with kebab-case wire keys.
 *  No `rename_all` on the Rust struct — each key is explicitly renamed.
 *  `extension-name` is omitted from the wire when None/absent. */
export interface TsFormatIdentifier {
  "base-format": string;
  "target-format": string;
  "display-name": string;
  "extension-name"?: string;
}

export interface TsFormatInfo {
  identifier: TsFormatIdentifier;
  metadata: Record<string, TsMetadataValue>;
}

// ==================== Metadata value ====================

/** JSON-shaped metadata value. Each variant serializes as a bare JSON value
 *  (`#[serde(untagged)]` on the Rust side). */
export type TsMetadataValue =
  | boolean
  | number
  | string
  | null
  | TsMetadataValue[]
  | { [key: string]: TsMetadataValue };

// ==================== Source map ====================

/** Byte-range source-map entry. `source: null` marks an unmappable piece. */
export interface TsSourceMapEntry {
  start: number;
  length: number;
  source: TsSourcePosition | null;
}

export interface TsSourcePosition {
  file: string;
  fileOffset: number;
}

// ==================== Execute options ====================

/** Options sent to the engine with each `execute` message. */
export interface TsExecuteOptions {
  input: string;
  sourcePath: string;
  format: TsFormatInfo;
  tempDir: string;
  cwd: string;
  projectDir?: string | null;
  libDir: string;
  quiet: boolean;
  handledLanguages: string[];
  params?: Record<string, TsMetadataValue> | null;
  sourceMap: TsSourceMapEntry[];
  /** Whether to resolve dependencies inline (true, Q1 default) or defer them.
   *  Absent key on the wire deserializes as true (Rust `#[serde(default = "default_true")]`). */
  dependencies: boolean;
}

// ==================== Dependencies options ====================

/** Options sent to the engine with each `dependencies` message (deferred-dependency path). */
export interface TsDependenciesOptions {
  input: string;
  sourcePath: string;
  sourceMap: TsSourceMapEntry[];
  format: TsFormatInfo;
  output: string;
  tempDir: string;
  libDir?: string | null;
  projectDir?: string | null;
  dependencies: TsMetadataValue[];
  quiet: boolean;
  // NOTE: no resourceDir — ambient via Init.global
}

// ==================== Execute result ====================

/** Result returned by the engine after execution.
 *  `htmlDependencies` uses the shared `HtmlDependency` type from `@quarto/types`.
 *  The wire requires `stylesheets`/`scripts` to be present as arrays; the harness
 *  will normalize/default to `[]` at serialization time (later task). */
export interface TsExecuteResult {
  markdown: string;
  supporting: string[];
  filters: string[];
  includes?: TsPandocIncludes | null;
  htmlDependencies: HtmlDependency[];
  metadata?: Record<string, TsMetadataValue> | null;
  pandoc?: Record<string, TsMetadataValue> | null;
  resourceFiles: string[];
  preserve: Record<string, string>;
  postProcess: boolean;
  engineDependencies?: Record<string, TsMetadataValue[]> | null;
  // NOTE: there is NO `engine` field on the wire result.
}

// Re-export HtmlDependency so consumers can import it from this package.
export type { HtmlDependency };
