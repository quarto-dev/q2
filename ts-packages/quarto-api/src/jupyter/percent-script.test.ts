/**
 * @quarto/api/jupyter — percent-script tests
 *
 * Mirrors Q1 semantics from:
 *   external-sources/quarto-cli/src/core/jupyter/percent.ts
 *
 * Frozen Test Seam Spec:
 *
 * Row 16 (`isPercentScript` detection, both polarities — the `false`
 * code-only case is the binding discriminator):
 *   - a `# %%` code-only `.jl` file => `isPercentScript(host, file, [".jl"])`
 *     is `false`.
 *   - a `# %% [markdown]` `.jl` file => `true`.
 *   Named revert: loosen the `[markdown|raw]`-marker requirement to a bare
 *   `%%` (e.g. drop the `\[(markdown|raw)\]` group from the detection
 *   regex) => the code-only case is (wrongly) detected `true` => RED.
 *
 * Row 17 (`percentScriptToMarkdown` markdown-cell emission):
 *   `# %% [markdown]\n# Hello` => the `# Hello` line renders as a markdown
 *   cell (plain "Hello" text), NOT a ```{julia} code fence.
 *   Named revert: remove the `type === "markdown"` branch (falls through to
 *   the code-cell branch) => emitted as a ```{julia} fence => RED.
 *
 * Plus: the `.q` default-exts unit (binds the `kJupyterPercentScriptExtensions`
 * default including `.q`) and a non-percent-script file (no `%%` markers at
 * all) => `false`.
 */

import { describe, it, expect } from "vitest";
import type { PlatformHost } from "../platform/index.js";
import { isPercentScript, percentScriptToMarkdown } from "./percent-script.js";

// ─── in-memory fs host, keyed by path (local to this test file) ───────────

function makeHost(files: Record<string, string>): Pick<PlatformHost, "fs"> {
  return {
    fs: {
      readTextFileSync: (path: string) => {
        const content = files[path];
        if (content === undefined) {
          throw new Error(`makeHost: no canned content for path ${path}`);
        }
        return content;
      },
      writeFileSync: () => {
        throw new Error("writeFileSync: not implemented in this fake");
      },
      exists: () => true,
      ensureDir: () => {},
      makeTempDir: () => "/tmp/fake",
      makeTempFile: () => "/tmp/fake-file",
      remove: () => {},
      walk: () => [],
    },
  };
}

describe("isPercentScript", () => {
  // Row 16
  it("does NOT detect a code-only `# %%` .jl script (no [markdown]/[raw] marker)", () => {
    const host = makeHost({
      "/code-only.jl": ["# %%", "1 + 1", ""].join("\n"),
    });
    expect(isPercentScript(host, "/code-only.jl", [".jl"])).toBe(false);
  });

  it("detects a `# %% [markdown]` .jl script", () => {
    const host = makeHost({
      "/has-markdown.jl": ["# %% [markdown]", "# Hello", ""].join("\n"),
    });
    expect(isPercentScript(host, "/has-markdown.jl", [".jl"])).toBe(true);
  });

  it("detects a `# %% [raw]` .jl script", () => {
    const host = makeHost({
      "/has-raw.jl": ["# %% [raw]", "# some raw content", ""].join("\n"),
    });
    expect(isPercentScript(host, "/has-raw.jl", [".jl"])).toBe(true);
  });

  it("defaults exts to include `.q` when omitted", () => {
    const host = makeHost({
      "/script.q": ["/ %% [markdown]", "/ Hello", ""].join("\n"),
    });
    // no `exts` argument => must fall back to kJupyterPercentScriptExtensions,
    // which includes ".q"
    expect(isPercentScript(host, "/script.q")).toBe(true);
  });

  it("returns false for a non-percent-script file (no %% markers at all)", () => {
    const host = makeHost({
      "/plain.py": ["print('hi')", ""].join("\n"),
    });
    expect(isPercentScript(host, "/plain.py")).toBe(false);
  });

  it("returns false when the extension is not in the allowed list", () => {
    const host = makeHost({
      "/notes.txt": ["# %% [markdown]", "# Hello", ""].join("\n"),
    });
    expect(isPercentScript(host, "/notes.txt", [".jl"])).toBe(false);
  });
});

describe("percentScriptToMarkdown", () => {
  // Row 17
  it("emits a [markdown] percent cell as markdown content, not a code fence", () => {
    const host = makeHost({
      "/doc.jl": "# %% [markdown]\n# Hello",
    });
    const result = percentScriptToMarkdown(host, "/doc.jl");
    expect(result).toContain("Hello");
    expect(result).not.toContain("```");
  });

  it("emits an ordinary %% cell as a fenced code cell", () => {
    const host = makeHost({
      "/code.jl": "# %%\n1 + 1",
    });
    const result = percentScriptToMarkdown(host, "/code.jl");
    expect(result).toContain("```{julia}");
    expect(result).toContain("1 + 1");
  });
});
