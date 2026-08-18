/**
 * metadataAsFormat — partition a q2 merged-metadata map into Q1's nested Format shape.
 *
 * Faithful port of external-sources/quarto-cli/src/config/metadata.ts:165-239.
 * Pure logic — no IO, no Deno.*.
 */

import type { Format } from "@quarto/types";
import {
  kExecuteDefaultsKeys,
  kRenderDefaultsKeys,
  kPandocDefaultsKeys,
  kIdentifierDefaultsKeys,
} from "@quarto/api/config";
import type { TsFormatInfo } from "./types.js";

// ---------------------------------------------------------------------------
// Local constants — values verified against
// external-sources/quarto-cli/src/config/constants.ts
// and external-sources/quarto-cli/src/format/markdown/format-markdown-consts.ts
// ---------------------------------------------------------------------------

/** Bin name constants (the six top-level Format keys). */
const kIdentifierDefaults = "identifier";
const kRenderDefaults = "render";
const kExecuteDefaults = "execute";
const kPandocDefaults = "pandoc";
const kLanguageDefaults = "language";
const kPandocMetadata = "metadata";

/** constants.ts:70 — key within execute that holds the "enabled" boolean. */
const kExecuteEnabled = "enabled"; // Q1 constants.ts line 70

/** constants.ts:618 — server metadata key. */
const kServer = "server"; // Q1 constants.ts line 618

/** constants.ts:76-77 — ipynb filter keys. */
const kIpynbFilter = "ipynb-filter";   // Q1 constants.ts line 76
const kIpynbFilters = "ipynb-filters"; // Q1 constants.ts line 77

/**
 * format-markdown-consts.ts:7-18 — the full commonmark variant string that
 * replaces a bare "gfm" prefix in the `variant` render option.
 *
 * Computed as kGfmCommonmarkExtensions.join("") where:
 *   kGfmCommonmarkExtensions = [
 *     "+autolink_bare_uris", "+emoji", "+footnotes",
 *     "+gfm_auto_identifiers", "+pipe_tables", "+strikeout",
 *     "+task_lists", "+tex_math_dollars",
 *   ]
 */
const kGfmCommonmarkVariant =
  "+autolink_bare_uris+emoji+footnotes+gfm_auto_identifiers+pipe_tables+strikeout+task_lists+tex_math_dollars";

// The six bin names as a set for O(1) Stage-1 lookup.
const BIN_NAMES = new Set([
  kIdentifierDefaults,
  kRenderDefaults,
  kExecuteDefaults,
  kPandocDefaults,
  kLanguageDefaults,
  kPandocMetadata,
]);

// ---------------------------------------------------------------------------
// Main export
// ---------------------------------------------------------------------------

/**
 * Partition q2's merged metadata map into Q1's nested `Format` shape.
 *
 * Algorithm mirrors Q1 `metadataAsFormat` (metadata.ts:165-239):
 *   Stage 1  — peel nested bin blocks (identifier/render/execute/pandoc/language/metadata).
 *   Stage 2  — classify remaining flat keys into the four key-list bins (no language branch).
 *   Tail     — three normalizations: server→{type}, ipynb coalesce, gfm variant expansion.
 *   Finish   — merge explicit identifier (explicit fields win).
 */
export function metadataAsFormat(formatInfo: TsFormatInfo): Format {
  // Initialize all six bins empty.
  const format: Format = {
    identifier: {},
    render: {},
    execute: {},
    pandoc: {},
    language: {},
    metadata: {},
  };

  // Use a type-unsafe alias so we can index by bin name string (matches Q1's pattern).
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const f = format as Record<string, any>;

  // -------------------------------------------------------------------------
  // Stage 1 — peel nested bins.
  // If a top-level key is one of the six bin names, merge its value into that bin.
  // Exception: `execute: <boolean>` → set format.execute[kExecuteEnabled] = value.
  // Any other scalar under a bin name is dropped (Q1 behavior — it spreads null/etc.).
  // -------------------------------------------------------------------------
  const remainingKeys: string[] = [];

  for (const key of Object.keys(formatInfo.metadata)) {
    if (BIN_NAMES.has(key)) {
      const value = formatInfo.metadata[key];
      if (typeof value === "boolean") {
        // Special case: execute: true/false → sets enabled flag.
        if (key === kExecuteDefaults) {
          f[kExecuteDefaults] = f[kExecuteDefaults] ?? {};
          f[kExecuteDefaults][kExecuteEnabled] = value;
        }
        // Boolean under any other bin name: drop (match Q1).
      } else if (typeof value === "object" && value !== null && !Array.isArray(value)) {
        // Plain map: merge into the bin.
        f[key] = { ...f[key], ...(value as Record<string, unknown>) };
      }
      // else: non-object, non-boolean scalar under a bin name — drop (Q1 spreads; guard is fine).
    } else {
      remainingKeys.push(key);
    }
  }

  // -------------------------------------------------------------------------
  // Stage 2 — classify remaining flat keys.
  // Order: identifier → render → execute → pandoc → metadata (no language branch).
  // Move-not-duplicate: each key lands in exactly ONE bin.
  // -------------------------------------------------------------------------
  for (const key of remainingKeys) {
    const value = formatInfo.metadata[key];
    if (kIdentifierDefaultsKeys.includes(key)) {
      format.identifier[key as keyof typeof format.identifier] =
        value as string;
    } else if (kRenderDefaultsKeys.includes(key)) {
      format.render[key] = value;
    } else if (kExecuteDefaultsKeys.includes(key)) {
      format.execute[key] = value;
    } else if (kPandocDefaultsKeys.includes(key)) {
      format.pandoc[key] = value;
    } else {
      format.metadata[key] = value;
    }
  }

  // -------------------------------------------------------------------------
  // Tail normalization 1 — server: string → { type: string }.
  // -------------------------------------------------------------------------
  if (typeof format.metadata[kServer] === "string") {
    format.metadata[kServer] = { type: format.metadata[kServer] as string };
  }

  // -------------------------------------------------------------------------
  // Tail normalization 2 — ipynb-filter coalesce into ipynb-filters.
  // Both keys are in kExecuteDefaultsKeys → both arrive in format.execute.
  // -------------------------------------------------------------------------
  const filter = format.execute[kIpynbFilter];
  if (typeof filter === "string") {
    const existing = format.execute[kIpynbFilters];
    format.execute[kIpynbFilters] = Array.isArray(existing) ? existing : [];
    (format.execute[kIpynbFilters] as string[]).push(filter);
    delete format.execute[kIpynbFilter];
  }

  // -------------------------------------------------------------------------
  // Tail normalization 3 — expand gfm alias in variant.
  // -------------------------------------------------------------------------
  if (typeof format.render["variant"] === "string") {
    format.render["variant"] = (format.render["variant"] as string).replace(
      /^gfm/,
      kGfmCommonmarkVariant,
    );
  }

  // -------------------------------------------------------------------------
  // Merge explicit identifier — explicit fields win over flat-key classified.
  // Skip undefined fields (don't write extension-name: undefined).
  // -------------------------------------------------------------------------
  const explicitNonEmpty: Partial<typeof formatInfo.identifier> = {};
  for (const [k, v] of Object.entries(formatInfo.identifier)) {
    if (v !== undefined) {
      (explicitNonEmpty as Record<string, string>)[k] = v as string;
    }
  }
  format.identifier = { ...format.identifier, ...explicitNonEmpty };

  return format;
}

// ---------------------------------------------------------------------------
// Execute-visibility defaults
// ---------------------------------------------------------------------------

/**
 * Q1's base execute-visibility defaults (`format/formats-shared.ts:210-217`):
 * the keys that gate whether a cell — and its code / output / warnings — is
 * rendered. Values are the format-agnostic BASE (`error:false`, everything else
 * `true`).
 *
 * Why this exists in the host: `metadataAsFormat` is a faithful port of Q1's
 * partition-only `metadataAsFormat` (config/metadata.ts) — Q1 merges the writer
 * format's `execute` defaults into the document metadata during *format
 * resolution*, BEFORE that partition runs, so its engines always see a Format
 * with these keys populated. q2 has no writer-format-defaults merge yet, so the
 * engine-visible `format.execute` arrives with only the keys present in the
 * document frontmatter. The Julia engine (the first real consumer of
 * `jupyterToMarkdown`) then finds `format.execute.include` / `.output` /
 * `.echo` undefined, and `includeCell` / `includeOutput` (tags.ts `shouldInclude`)
 * drop every executed cell — the rendered body comes out empty.
 *
 * Applying the base defaults here — only for keys the document did not set —
 * restores Q1's "cells render by default" behaviour without touching the
 * partition port.
 *
 * KNOWN DIVERGENCE (documented for Plan 4 Phase 4F/4G): Q1's per-writer overrides
 * (HTML / PDF set `echo:false`, `warning:false`) are NOT applied here — only the
 * format-agnostic base. Under q2 an HTML render therefore currently echoes cell
 * source by default (`echo:true`) where Q1's HTML would hide it. Closing that gap
 * belongs with a real writer-format-defaults layer, not this host shim.
 */
const kExecuteVisibilityDefaults: Record<string, boolean> = {
  eval: true,
  echo: true,
  output: true,
  warning: true,
  include: true,
  error: false,
};

/**
 * Fill absent execute-visibility keys on `format.execute` with Q1's base
 * defaults (see {@link kExecuteVisibilityDefaults}). Mutates and returns
 * `format`. Keys the document already set (any value, including `false`) are
 * left untouched.
 */
export function applyExecuteDefaults(format: Format): Format {
  const execute = format.execute as Record<string, unknown>;
  for (const [key, value] of Object.entries(kExecuteVisibilityDefaults)) {
    if (execute[key] === undefined) {
      execute[key] = value;
    }
  }
  return format;
}
