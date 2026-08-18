/**
 * Parity tests for @quarto/api/config key lists.
 *
 * STRONG mode (local dev): external-sources/quarto-cli is a live symlink —
 * we read Q1's constants.ts and diff every resolved list against ours.
 * Deleting or altering any key in a config list must make this test RED.
 *
 * FALLBACK mode (CI, symlink absent): assert each list is non-empty and
 * contains a known anchor key. This does NOT catch a single wrong key —
 * the manual hand-diff in the task-3-report compensates.
 */

import { describe, it, expect } from "vitest";
import { readFileSync, existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

import {
  kExecuteDefaultsKeys,
  kRenderDefaultsKeys,
  kPandocDefaultsKeys,
  kIdentifierDefaultsKeys,
  kLanguageDefaultsKeys,
} from "./index.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

// Path to Q1 constants.ts:
// src/config/ (1 up) → src/ (1 up) → quarto-api/ (1 up) → ts-packages/ (1 up) → repo root
// → external-sources/quarto-cli/src/config/constants.ts
const Q1_CONSTANTS_PATH = resolve(
  __dirname,
  "../../../..",
  "external-sources/quarto-cli/src/config/constants.ts",
);

const STRONG_MODE = existsSync(Q1_CONSTANTS_PATH);

// ─── helpers ──────────────────────────────────────────────────────────────────

/**
 * Parse the Q1 constants.ts source text and resolve the five kXxxDefaultsKeys
 * arrays to their string values. Does NOT import the file — avoids Deno .ts
 * extension issues. Regex-based extraction.
 */
function resolveQ1Constants(source: string): {
  kExecuteDefaultsKeys: string[];
  kRenderDefaultsKeys: string[];
  kPandocDefaultsKeys: string[];
  kIdentifierDefaultsKeys: string[];
  kLanguageDefaultsKeys: string[];
} {
  // Build a map: symbol name → string value
  // Matches: export const kFoo = "bar";
  // Intentionally STRICT — only single-line string-literal `export const` bindings
  // resolve. If Q1 ever defines a key via a computed/multi-line expression, the
  // symbol lookup throws "Symbol not found", reddening this test for a non-parity
  // reason; that is a deliberate loud failure prompting a re-sync of this parser.
  const symbolMap = new Map<string, string>();
  const bindingRe = /^export\s+const\s+(\w+)\s*=\s*"([^"]+)"/gm;
  let m: RegExpExecArray | null;
  while ((m = bindingRe.exec(source)) !== null) {
    symbolMap.set(m[1], m[2]);
  }

  /**
   * Find the array body for `export const <name> = [...];` and resolve each
   * element (symbol reference or inline string literal) to its string value.
   */
  function extractArray(name: string): string[] {
    const arrayRe = new RegExp(
      `export\\s+const\\s+${name}\\s*=\\s*\\[([\\s\\S]*?)\\];`,
    );
    const arrayMatch = arrayRe.exec(source);
    if (!arrayMatch) {
      throw new Error(`Could not find ${name} in Q1 constants.ts`);
    }
    const body = arrayMatch[1];

    const result: string[] = [];
    // Each item is either an identifier (symbol ref) or an inline string literal
    const itemRe = /(\w+)|"([^"]+)"/g;
    let item: RegExpExecArray | null;
    while ((item = itemRe.exec(body)) !== null) {
      if (item[1]) {
        const resolved = symbolMap.get(item[1]);
        if (resolved === undefined) {
          throw new Error(
            `Symbol '${item[1]}' not found in Q1 constants.ts symbol map`,
          );
        }
        result.push(resolved);
      } else {
        // inline string literal (e.g. "defaults", "filters", etc.)
        result.push(item[2]);
      }
    }
    return result;
  }

  return {
    kExecuteDefaultsKeys: extractArray("kExecuteDefaultsKeys"),
    kRenderDefaultsKeys: extractArray("kRenderDefaultsKeys"),
    kPandocDefaultsKeys: extractArray("kPandocDefaultsKeys"),
    kIdentifierDefaultsKeys: extractArray("kIdentifierDefaultsKeys"),
    kLanguageDefaultsKeys: extractArray("kLanguageDefaultsKeys"),
  };
}

// ─── strong-mode parity tests ─────────────────────────────────────────────────

describe("@quarto/api/config — key-list parity (STRONG mode)", () => {
  if (!STRONG_MODE) {
    it.skip("SKIPPED — external-sources/quarto-cli not present (CI fallback mode)", () => {});
    return;
  }

  const q1Source = readFileSync(Q1_CONSTANTS_PATH, "utf-8");
  const q1 = resolveQ1Constants(q1Source);

  it("resolves Q1 key lists without error", () => {
    expect(q1.kExecuteDefaultsKeys.length).toBeGreaterThan(0);
  });

  // Sorted-array compare (not Set): catches a removed/added key, a value
  // mutation, AND a lost duplicate (multiplicity) — the duplicate language
  // keys the port deliberately preserves. Order is functionally irrelevant
  // (these back a membership lookup), so sorting avoids spurious failures.
  it("kExecuteDefaultsKeys matches Q1 as a sorted list — removing a key or losing a duplicate turns this RED", () => {
    expect([...kExecuteDefaultsKeys].sort()).toEqual(
      [...q1.kExecuteDefaultsKeys].sort(),
    );
  });

  it("kRenderDefaultsKeys matches Q1 as a sorted list — removing a key or losing a duplicate turns this RED", () => {
    expect([...kRenderDefaultsKeys].sort()).toEqual(
      [...q1.kRenderDefaultsKeys].sort(),
    );
  });

  it("kPandocDefaultsKeys matches Q1 as a sorted list — removing a key or losing a duplicate turns this RED", () => {
    expect([...kPandocDefaultsKeys].sort()).toEqual(
      [...q1.kPandocDefaultsKeys].sort(),
    );
  });

  it("kIdentifierDefaultsKeys matches Q1 as a sorted list — removing a key or losing a duplicate turns this RED", () => {
    expect([...kIdentifierDefaultsKeys].sort()).toEqual(
      [...q1.kIdentifierDefaultsKeys].sort(),
    );
  });

  it("kLanguageDefaultsKeys matches Q1 as a sorted list — removing a key or losing a duplicate turns this RED", () => {
    expect([...kLanguageDefaultsKeys].sort()).toEqual(
      [...q1.kLanguageDefaultsKeys].sort(),
    );
  });
});

// ─── fallback-mode parity tests ───────────────────────────────────────────────

describe("@quarto/api/config — key-list parity (FALLBACK mode, CI)", () => {
  if (STRONG_MODE) {
    it.skip("SKIPPED — running in STRONG mode (external-sources present)", () => {});
    return;
  }

  it("kExecuteDefaultsKeys is non-empty and contains anchor key 'fig-width'", () => {
    expect(kExecuteDefaultsKeys.length).toBeGreaterThan(0);
    expect(kExecuteDefaultsKeys).toContain("fig-width");
  });

  it("kRenderDefaultsKeys is non-empty and contains anchor key 'keep-tex'", () => {
    expect(kRenderDefaultsKeys.length).toBeGreaterThan(0);
    expect(kRenderDefaultsKeys).toContain("keep-tex");
  });

  it("kPandocDefaultsKeys is non-empty and contains anchor key 'to'", () => {
    expect(kPandocDefaultsKeys.length).toBeGreaterThan(0);
    expect(kPandocDefaultsKeys).toContain("to");
  });

  it("kIdentifierDefaultsKeys is non-empty and contains anchor key 'target-format'", () => {
    expect(kIdentifierDefaultsKeys.length).toBeGreaterThan(0);
    expect(kIdentifierDefaultsKeys).toContain("target-format");
  });

  it("kLanguageDefaultsKeys is non-empty and contains anchor key 'toc-title-document'", () => {
    expect(kLanguageDefaultsKeys.length).toBeGreaterThan(0);
    expect(kLanguageDefaultsKeys).toContain("toc-title-document");
  });
});

// ─── no Deno / node: in config source ─────────────────────────────────────────

describe("@quarto/api/config — no Deno or node: in source", () => {
  const configSrcPath = resolve(__dirname, "./index.ts");

  it("config source contains no Deno. references", () => {
    const src = readFileSync(configSrcPath, "utf-8");
    expect(src).not.toMatch(/\bDeno\./);
  });

  it("config source contains no node: imports", () => {
    const src = readFileSync(configSrcPath, "utf-8");
    expect(src).not.toMatch(/from\s+["']node:/);
  });
});
