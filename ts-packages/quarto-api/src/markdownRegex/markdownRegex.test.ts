/**
 * @quarto/api — markdownRegex namespace tests
 *
 * Pure assertions — all functions are pure (no I/O), no fake host needed.
 *
 * Q1 test files mirrored:
 *   - tests/unit/pandoc-partition.test.ts   (partitionMarkdown, languagesWithClasses)
 *   - tests/unit/break-quarto-md/break-quarto-md.test.ts (breakQuartoMd)
 *   - tests/unit/yaml.test.ts               (readYamlFromMarkdown)
 *
 * All assertions are real-value checks per the seam spec:
 *   - Named revert: emptying/short-circuiting the function body makes the assertion RED.
 *   - A test that only checks "returns a string / doesn't throw" is REJECTED.
 */

import { describe, it, expect } from "vitest";
import {
  extractYaml,
  partition,
  getLanguages,
  getLanguagesWithClasses,
  breakQuartoMd,
} from "./index.js";

// ── extractYaml ───────────────────────────────────────────────────────────────
// Mirrors Q1 tests/unit/yaml.test.ts

describe("markdownRegex.extractYaml", () => {
  it("extracts front-matter YAML from a simple document", () => {
    const md = `---
title: My Title
author: Alice
---

Some body text.
`;
    const result = extractYaml(md);
    // Named revert: return {} → title assertion fails (RED)
    expect(result["title"]).toBe("My Title");
    expect(result["author"]).toBe("Alice");
  });

  it("returns an empty object for markdown with no YAML", () => {
    const md = "Just some plain text.\n\nNo front matter here.";
    const result = extractYaml(md);
    expect(Object.keys(result).length).toBe(0);
  });

  it("returns an empty object for empty string", () => {
    const result = extractYaml("");
    expect(Object.keys(result).length).toBe(0);
  });

  it("parses nested YAML values", () => {
    const md = `---
format:
  html:
    toc: true
---
body
`;
    const result = extractYaml(md);
    // Named revert: return {} → assertion fails (RED)
    expect(typeof result["format"]).toBe("object");
    const format = result["format"] as Record<string, unknown>;
    expect(typeof format["html"]).toBe("object");
  });

  it("ignores HTML comments when extracting YAML", () => {
    const md = `---
title: Visible
---
<!-- a comment -->
body
`;
    const result = extractYaml(md);
    expect(result["title"]).toBe("Visible");
  });
});

// ── partition ─────────────────────────────────────────────────────────────────
// Mirrors Q1 tests/unit/pandoc-partition.test.ts ("partitionYaml")

describe("markdownRegex.partition", () => {
  it("correctly partitions front matter, heading, and body (Q1 mirror)", () => {
    const frontMatter = "---\ntitle: foo\n---";
    const headingText = "## Hello World {#cool .foobar foo=bar}";
    const markdown = "\n\nThis is a paragraph\n\n:::{#refs}\n:::\n";
    const markdownStr = `${frontMatter}\n${headingText}${markdown}`;

    const partmd = partition(markdownStr);

    // Refs div
    // Named revert: containsRefs always false → RED
    expect(partmd.containsRefs).toBe(true);

    // Body markdown
    // Named revert: markdown = "" → RED
    expect(partmd.markdown).toBe(markdown);

    // Front-matter YAML
    // Named revert: yaml = undefined → RED
    expect(partmd.yaml?.["title"]).toBe("foo");
    expect(Object.keys(partmd.yaml!).length).toBe(1);

    // Heading
    // Named revert: headingText = undefined → RED
    expect(partmd.headingText).toBe("Hello World");

    // Heading attributes
    // Named revert: headingAttr = undefined → RED
    expect(partmd.headingAttr?.id).toBe("cool");
    expect(partmd.headingAttr?.classes).toContain("foobar");
    expect(partmd.headingAttr?.keyvalue[0][0]).toBe("foo");
    expect(partmd.headingAttr?.keyvalue[0][1]).toBe("bar");
  });

  it("handles document without front matter", () => {
    const md = "# A Heading\n\nSome content.";
    const result = partition(md);
    // Named revert: yaml = something → RED
    expect(result.yaml).toBeUndefined();
    expect(result.headingText).toBe("A Heading");
  });

  it("srcMarkdownNoYaml contains body text without the YAML block", () => {
    const md = "---\ntitle: Test\n---\n\nBody text.";
    const result = partition(md);
    // Named revert: srcMarkdownNoYaml = "" or full string → RED
    expect(result.srcMarkdownNoYaml).toContain("Body text.");
    expect(result.srcMarkdownNoYaml).not.toContain("title:");
  });
});

// ── getLanguages ──────────────────────────────────────────────────────────────

describe("markdownRegex.getLanguages", () => {
  it("returns the set of languages from code cells", () => {
    const md = `
\`\`\`{python}
x = 1
\`\`\`

\`\`\`{r}
y <- 2
\`\`\`

\`\`\`{python}
z = 3
\`\`\`
`;
    const langs = getLanguages(md);
    // Named revert: return empty Set → RED
    expect(langs.has("python")).toBe(true);
    expect(langs.has("r")).toBe(true);
    // Deduplicated (python appears twice)
    expect(langs.size).toBe(2);
  });

  it("returns empty set for document with no code cells", () => {
    const md = "Just text.\n\n```\nplain block\n```\n";
    const langs = getLanguages(md);
    // Named revert: return Set with any value → RED
    expect(langs.size).toBe(0);
  });
});

// ── getLanguagesWithClasses ───────────────────────────────────────────────────
// Mirrors Q1 tests/unit/pandoc-partition.test.ts ("languagesWithClasses - dot-joined syntax")

describe("markdownRegex.getLanguagesWithClasses", () => {
  it("handles dot-joined syntax {python.marimo} — no separate class (Q1 mirror)", () => {
    const md = `\`\`\`{python.marimo}
x = 1
\`\`\`

\`\`\`{python .foo}
y = 2
\`\`\`
`;
    const result = getLanguagesWithClasses(md);

    // {python.marimo} → language "python.marimo", no class
    // Named revert: result doesn't have "python.marimo" → RED
    expect(result.has("python.marimo")).toBe(true);
    expect(result.get("python.marimo")).toBeUndefined();

    // {python .foo} → language "python", class "foo"
    // Named revert: result.get("python") ≠ "foo" → RED
    expect(result.has("python")).toBe(true);
    expect(result.get("python")).toBe("foo");
  });

  it("returns empty map for doc with no code cells", () => {
    const result = getLanguagesWithClasses("No code blocks here.");
    expect(result.size).toBe(0);
  });

  it("does not duplicate languages (first occurrence wins)", () => {
    const md = `\`\`\`{python .first}
a = 1
\`\`\`
\`\`\`{python .second}
b = 2
\`\`\`
`;
    const result = getLanguagesWithClasses(md);
    // First occurrence wins
    // Named revert: second wins → RED
    expect(result.get("python")).toBe("first");
    expect(result.size).toBe(1);
  });
});

// ── breakQuartoMd ─────────────────────────────────────────────────────────────
// Mirrors Q1 tests/unit/break-quarto-md/break-quarto-md.test.ts

describe("markdownRegex.breakQuartoMd", () => {
  it("splits a simple doc into raw (front-matter) + markdown cells", async () => {
    const qmd = `---
title: Test
---

Hello world.
`;
    const result = await breakQuartoMd(qmd);
    // Named revert: cells not an array → RED
    expect(Array.isArray(result.cells)).toBe(true);
    // Named revert: return 0 cells → RED
    expect(result.cells.length).toBeGreaterThanOrEqual(1);
    // Front-matter must be a raw cell
    // Named revert: return markdown cell instead of raw → RED
    expect(result.cells[0].cell_type).toBe("raw");
    // Must include a markdown cell for the body
    // Named revert: drop markdown cells → RED
    const hasMarkdown = result.cells.some((c) => c.cell_type === "markdown");
    expect(hasMarkdown).toBe(true);
  });

  it("splits a doc with front matter + markdown + code into expected cell count (Q1 mirror)", async () => {
    const qmd = `---
title: mermaid test
format: html
---

## Some title

Some text

\`\`\`{mermaid}
graph TD;
    A-->B;
\`\`\`

A cell that shouldn't be rendered by mermaid:

\`\`\`mermaid
Do not touch this, please.
\`\`\`
`;
    const cells = (await breakQuartoMd(qmd, false)).cells;
    // Q1 test asserts exactly 4 cells for this shape
    // Named revert: return fewer/more cells → RED
    expect(cells.length).toBe(4);
    // The last cell is markdown (the non-executable mermaid block is part of markdown)
    // Named revert: last cell is code → RED
    expect(
      (cells[cells.length - 1].sourceVerbatim.value.startsWith("```")),
    ).toBe(false);
  });

  it("identifies cell types correctly: raw for front matter, code for code cell", async () => {
    const qmd = `---
title: Hello
---

Some text.

\`\`\`{python}
x = 1
\`\`\`
`;
    const cells = (await breakQuartoMd(qmd, false)).cells;

    // Should have: raw (front matter), markdown, code
    const cellTypes = cells.map((c) => {
      if (typeof c.cell_type === "string") return c.cell_type;
      if ("language" in c.cell_type && c.cell_type.language === "_directive")
        return "directive";
      return (c.cell_type as { language: string }).language;
    });

    // Named revert: no raw cell → RED
    expect(cellTypes).toContain("raw");
    // Named revert: no python code cell → RED
    expect(cellTypes).toContain("python");
    // Named revert: no markdown → RED
    expect(cellTypes).toContain("markdown");
  });

  it("code cell source contains the actual code (not the backtick fence)", async () => {
    const qmd = `\`\`\`{r}
1 + 1
\`\`\`
`;
    const cells = (await breakQuartoMd(qmd, false)).cells;
    const codeCell = cells.find(
      (c) =>
        typeof c.cell_type === "object" &&
        "language" in c.cell_type &&
        (c.cell_type as { language: string }).language === "r",
    );
    // Named revert: source is empty or contains the backticks → RED
    expect(codeCell).toBeDefined();
    expect(codeCell!.source.value.trim()).toBe("1 + 1");
  });

  it("handles dot-joined language syntax (Q1 mirror)", async () => {
    const qmd = `\`\`\`{python.marimo}
x = 1
\`\`\`
`;
    const cells = (await breakQuartoMd(qmd, false)).cells;
    // Named revert: cell not found or language wrong → RED
    expect(cells.length).toBeGreaterThanOrEqual(1);
    const codeCell = cells.find(
      (c) =>
        typeof c.cell_type === "object" &&
        "language" in c.cell_type,
    );
    expect(codeCell).toBeDefined();
    expect(
      (codeCell!.cell_type as { language: string }).language,
    ).toBe("python.marimo");
  });

  it("nested code blocks are not split into separate cells (Q1 mirror)", async () => {
    const qmd = `---
title: nested test
---

Some text.

\`\`\`\`{.markdown}
\`\`\`{mermaid}
graph TD;
    A-->B;
\`\`\`
\`\`\`\`

Then some text.
`;
    const cells = (await breakQuartoMd(qmd, false)).cells;
    // Q1 test asserts exactly 2 cells
    // Named revert: > 2 cells (split the nested block) → RED
    expect(cells.length).toBe(2);
  });

  it("HR lines are not treated as YAML delimiters (Q1 mirror)", async () => {
    const qmd = `---
title: "Untitled"
format: html
---


Hello, an hr.

---

Hello, another thing.

---

And what about this?
`;
    const cells = (await breakQuartoMd(qmd, false)).cells;
    // Q1 test: cells.length <= 2 OR cells[2] is markdown
    // Named revert: treat HR as YAML delimiter → extra raw cells
    const valid =
      cells.length <= 2 ||
      cells[2].cell_type === "markdown";
    expect(valid).toBe(true);
  });

  it("sourceVerbatim covers the entire cell including fences", async () => {
    const qmd = `\`\`\`{python}
x = 1
\`\`\`
`;
    const cells = (await breakQuartoMd(qmd, false)).cells;
    const codeCell = cells.find(
      (c) =>
        typeof c.cell_type === "object" &&
        "language" in c.cell_type &&
        (c.cell_type as { language: string }).language === "python",
    );
    expect(codeCell).toBeDefined();
    // sourceVerbatim must start with the opening fence
    // Named revert: sourceVerbatim = source (no fences) → RED
    expect(codeCell!.sourceVerbatim.value).toContain("```{python}");
    expect(codeCell!.sourceVerbatim.value).toContain("```");
  });

  it("knitr-style option lines yield yaml: undefined (knitr guard — Q1 mirror)", async () => {
    // knitr-style: #| echo=TRUE, fig.width=5  (key=value, not key: value)
    // Q1 guessChunkOptionsFormat returns "knitr" → partitionCellOptionsMapped
    // must NOT yaml-parse them and must return yaml: undefined.
    // Revert: remove the guessChunkOptionsFormat guard → yaml becomes a mis-parse → RED
    const qmd = `\`\`\`{r}
#| echo=TRUE, fig.width=5
1 + 1
\`\`\`
`;
    const cells = (await breakQuartoMd(qmd, false)).cells;
    const codeCell = cells.find(
      (c) =>
        typeof c.cell_type === "object" &&
        "language" in c.cell_type &&
        (c.cell_type as { language: string }).language === "r",
    );
    expect(codeCell).toBeDefined();
    // The knitr-format options must NOT be parsed as YAML
    // Named revert: parse the options → options becomes an object → RED
    expect(codeCell!.options).toBeUndefined();
  });

  it("knitr-style multi-line options also yield yaml: undefined", async () => {
    // Multiple knitr-style option lines — all key=value
    const qmd = `\`\`\`{r}
#| echo=TRUE,
#| fig.width=5
x <- 1
\`\`\`
`;
    const cells = (await breakQuartoMd(qmd, false)).cells;
    const codeCell = cells.find(
      (c) =>
        typeof c.cell_type === "object" &&
        "language" in c.cell_type &&
        (c.cell_type as { language: string }).language === "r",
    );
    expect(codeCell).toBeDefined();
    expect(codeCell!.options).toBeUndefined();
  });
});
