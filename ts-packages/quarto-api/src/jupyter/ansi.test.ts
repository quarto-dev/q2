/**
 * `stripAnsiCode` against the table shared with the Rust presenter
 * (`crates/quarto-core/tests/fixtures/ansi/strip-cases.json`, OPR-7g). Both sides must agree
 * on every row.
 */
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { stripAnsiCode } from "./to-markdown.js";

const table = JSON.parse(
  readFileSync(
    fileURLToPath(
      new URL(
        "../../../../crates/quarto-core/tests/fixtures/ansi/strip-cases.json",
        import.meta.url,
      ),
    ),
    "utf8",
  ),
) as { name: string; input: string; expected: string }[];

describe("stripAnsiCode (shared table)", () => {
  for (const c of table) {
    it(c.name, () => {
      expect(stripAnsiCode(c.input)).toBe(c.expected);
    });
  }
});
