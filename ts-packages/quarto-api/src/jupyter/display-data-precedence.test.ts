/**
 * Shared-fixture test: the same cases run against the Rust port
 * (`crates/quarto-core/src/presenter/inline.rs`, `display_data_mime_type`), so the two
 * ports of Q1 `displayDataMimeType` cannot drift apart silently.
 */

import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import type { JupyterCellOutputData } from "@quarto/types";
import { displayDataMimeType } from "./display-data.js";

interface Case {
  name: string;
  flags: { toHtml?: boolean; toLatex?: boolean; toMarkdown?: boolean };
  data: Record<string, unknown>;
  expected: string | null;
}

const fixture = JSON.parse(
  readFileSync(fileURLToPath(new URL("./__fixtures__/display-data-precedence.json", import.meta.url)), "utf8"),
) as { cases: Case[] };

describe("displayDataMimeType — shared precedence fixture", () => {
  for (const c of fixture.cases) {
    it(c.name, () => {
      const output = { data: c.data } as unknown as JupyterCellOutputData;
      const got = displayDataMimeType(output, {
        toHtml: c.flags.toHtml ?? false,
        toLatex: c.flags.toLatex ?? false,
        toMarkdown: c.flags.toMarkdown ?? false,
      });
      expect(got).toBe(c.expected);
    });
  }
});
