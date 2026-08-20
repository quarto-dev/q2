/**
 * @quarto/api — text namespace tests (PURE)
 *
 * Mirrors Q1 test cases from: external-sources/quarto-cli/tests/unit/text.test.ts
 * All assertions are value-based per the §2aa seam-spec (not "returns a string").
 * Named revert that reddens each test is noted inline.
 */

import { describe, it, expect } from "vitest";
import {
  lines,
  trimEmptyLines,
  lineColToIndex,
  executeInlineCodeHandler,
  asYamlText,
  postProcessRestorePreservedHtml,
} from "./index.js";
import type { PostProcessOptions } from "@quarto/types";

// ─── lines ────────────────────────────────────────────────────────────────────

describe("text.lines", () => {
  it("splits on LF", () => {
    // Revert body to `return []` → RED
    expect(lines("a\nb\nc")).toEqual(["a", "b", "c"]);
  });

  it("splits on CRLF", () => {
    expect(lines("a\r\nb\r\nc")).toEqual(["a", "b", "c"]);
  });

  it("splits on bare CR", () => {
    expect(lines("a\rb\rc")).toEqual(["a", "b", "c"]);
  });

  it("single line (no newline) returns array of one element", () => {
    expect(lines("hello")).toEqual(["hello"]);
  });

  it("empty string returns array with one empty string", () => {
    // mirrors Q1 `lines('')` behaviour
    expect(lines("")).toEqual([""]);
  });

  it("trailing newline produces trailing empty string element", () => {
    expect(lines("a\nb\n")).toEqual(["a", "b", ""]);
  });
});

// ─── trimEmptyLines ───────────────────────────────────────────────────────────

describe("text.trimEmptyLines", () => {
  it("trims both leading and trailing by default ('all')", () => {
    // Revert to `return lines` → RED
    expect(trimEmptyLines(["", "  ", "a", "b", "", "  "])).toEqual(["a", "b"]);
  });

  it("trims only leading when trim='leading'", () => {
    expect(trimEmptyLines(["", "a", "b", ""], "leading")).toEqual([
      "a",
      "b",
      "",
    ]);
  });

  it("trims only trailing when trim='trailing'", () => {
    expect(trimEmptyLines(["", "a", "b", ""], "trailing")).toEqual([
      "",
      "a",
      "b",
    ]);
  });

  it("returns [] when all lines are empty", () => {
    expect(trimEmptyLines(["", "  ", "\t"])).toEqual([]);
  });

  it("returns [] on empty input", () => {
    expect(trimEmptyLines([])).toEqual([]);
  });

  it("preserves middle empty lines", () => {
    expect(trimEmptyLines(["a", "", "b"])).toEqual(["a", "", "b"]);
  });
});

// ─── lineColToIndex ───────────────────────────────────────────────────────────

describe("text.lineColToIndex", () => {
  const text = "hello\nworld\n!";

  it("line 0, col 0 maps to index 0", () => {
    // Revert lineOffset math → RED
    const toIndex = lineColToIndex(text);
    expect(toIndex({ line: 0, column: 0 })).toBe(0);
  });

  it("line 0, col 3 maps to index 3 (within first line)", () => {
    const toIndex = lineColToIndex(text);
    expect(toIndex({ line: 0, column: 3 })).toBe(3);
  });

  it("line 1, col 0 maps to index 6 (start of 'world')", () => {
    // "hello\n" is 6 chars; 'world' starts at index 6
    const toIndex = lineColToIndex(text);
    expect(toIndex({ line: 1, column: 0 })).toBe(6);
  });

  it("line 1, col 5 maps to index 11 (end of 'world')", () => {
    const toIndex = lineColToIndex(text);
    expect(toIndex({ line: 1, column: 5 })).toBe(11);
  });

  it("line 2, col 0 maps to index 12", () => {
    // "hello\nworld\n" is 12 chars; '!' starts at index 12
    const toIndex = lineColToIndex(text);
    expect(toIndex({ line: 2, column: 0 })).toBe(12);
  });

  it("handles CRLF line endings", () => {
    const crlf = "ab\r\ncd";
    const toIndex = lineColToIndex(crlf);
    // "ab\r\n" is 4 chars; 'c' starts at index 4
    expect(toIndex({ line: 1, column: 0 })).toBe(4);
  });
});

// ─── executeInlineCodeHandler ─────────────────────────────────────────────────

describe("text.executeInlineCodeHandler", () => {
  it("replaces a matching inline code expression with the exec result", () => {
    // Revert replaceAll to identity → RED
    const handler = executeInlineCodeHandler("python", (expr) =>
      expr === "1+1" ? "2" : undefined,
    );
    expect(handler("result: `{python} 1+1`")).toBe("result: 2");
  });

  it("preserves the original span when exec returns undefined", () => {
    const handler = executeInlineCodeHandler("python", () => undefined);
    expect(handler("result: `{python} expr`")).toBe("result: `{python} expr`");
  });

  it("trims whitespace from the expression before passing to exec", () => {
    const received: string[] = [];
    const handler = executeInlineCodeHandler("r", (expr) => {
      received.push(expr);
      return "42";
    });
    handler("val: `{r}  x + y  `");
    expect(received).toEqual(["x + y"]);
  });

  it("handles multiple expressions in one string", () => {
    const handler = executeInlineCodeHandler("r", (expr) =>
      expr === "a" ? "1" : expr === "b" ? "2" : undefined,
    );
    expect(handler("`{r} a` and `{r} b`")).toBe("1 and 2");
  });

  it("does not match expressions for a different language", () => {
    const handler = executeInlineCodeHandler("python", (expr) => expr);
    // No substitution — language token is 'r', not 'python'
    expect(handler("`{r} expr`")).toBe("`{r} expr`");
  });
});

// ─── asYamlText ───────────────────────────────────────────────────────────────

describe("text.asYamlText", () => {
  it("serializes a flat metadata object to YAML", () => {
    // Revert stringify to JSON.stringify → RED (different syntax)
    const out = asYamlText({ title: "My Doc", draft: true });
    expect(out).toContain("title: My Doc");
    expect(out).toContain("draft: true");
  });

  it("uses 2-space indentation for nested objects", () => {
    const out = asYamlText({ author: { name: "Alice", affiliation: "ACME" } });
    // 2-space indent: the nested key is indented 2 spaces
    expect(out).toMatch(/author:\n {2}name:/);
  });

  it("does not sort keys (order preserved)", () => {
    const out = asYamlText({ z: 1, a: 2 });
    const zIdx = out.indexOf("z:");
    const aIdx = out.indexOf("a:");
    // z must come before a
    expect(zIdx).toBeLessThan(aIdx);
  });

  it("serializes an empty object to '{}\n' (value assertion)", () => {
    // yaml.stringify({}) returns '{}\n' — verify the real output
    // Revert body to return "" or "{}" → RED
    expect(asYamlText({})).toBe("{}\n");
  });
});

// ─── postProcessRestorePreservedHtml (accepted-untested STUB) ─────────────────

// accepted-untested (Plan 4b-D): not built yet (Plan 2 B2 stub). Loose guard
// only: it fails LOUD (throws) rather than silently no-op'ing. NOT an
// assertion that throwing is the desired end state — a future real
// implementation must be free to stop throwing without reddening this test.
describe("text.postProcessRestorePreservedHtml (accepted-untested stub)", () => {
  it("throws rather than silently no-op'ing", () => {
    expect(() =>
      postProcessRestorePreservedHtml(
        {} as unknown as PostProcessOptions,
      ),
    ).toThrow(/not yet implemented/i);
  });
});
