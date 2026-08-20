/**
 * Tests for src/engine-loader.ts — dynamic engine-module import + validation.
 *
 * TDD: tests written before the implementation exists. Named-revert tests have
 * explicit documentation about what must go RED when the named revert is applied.
 *
 * Uses node:fs and node:os to write temp .mjs files for dynamic import.
 */

import { describe, it, expect, afterEach } from "vitest";
import { writeFileSync, unlinkSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { loadEngineModule } from "./engine-loader.js";

// ---------------------------------------------------------------------------
// Temp file management
// ---------------------------------------------------------------------------

const tempFiles: string[] = [];

function writeTempMjs(name: string, content: string): string {
  const path = join(tmpdir(), `engine-loader-test-${name}-${Date.now()}.mjs`);
  writeFileSync(path, content, "utf-8");
  tempFiles.push(path);
  return path;
}

afterEach(() => {
  for (const f of tempFiles.splice(0)) {
    if (existsSync(f)) {
      unlinkSync(f);
    }
  }
});

// ---------------------------------------------------------------------------
// Valid engine loads
// ---------------------------------------------------------------------------

describe("loadEngineModule — valid engines", () => {
  it("loads a valid engine module and returns the discovery object", async () => {
    const path = writeTempMjs(
      "valid",
      `
export default {
  name: "test-engine",
  defaultExt: ".qmd",
  defaultYaml: () => [],
  defaultContent: () => [],
  validExtensions: () => [".qmd"],
  claimsFile: () => false,
  claimsLanguage: (lang) => lang === "python",
  canFreeze: false,
  generatesFigures: false,
  launch: (ctx) => ({ name: "test-engine", canFreeze: false }),
};
`,
    );

    const result = await loadEngineModule(path);
    expect(result.name).toBe("test-engine");
    expect(typeof result.claimsLanguage).toBe("function");
    expect(typeof result.launch).toBe("function");
  });

  it("loaded engine's claimsLanguage works correctly", async () => {
    const path = writeTempMjs(
      "claims-lang",
      `
export default {
  name: "python-engine",
  defaultExt: ".py",
  defaultYaml: () => [],
  defaultContent: () => [],
  validExtensions: () => [".py"],
  claimsFile: () => false,
  claimsLanguage: (lang) => lang === "python" ? 10 : false,
  canFreeze: false,
  generatesFigures: true,
  launch: (ctx) => ({ name: "python-engine", canFreeze: false }),
};
`,
    );

    const result = await loadEngineModule(path);
    expect(result.name).toBe("python-engine");
    // The function works as expected
    expect(result.claimsLanguage("python")).toBe(10);
    expect(result.claimsLanguage("r")).toBe(false);
  });

  it("engine with optional init field is accepted", async () => {
    const path = writeTempMjs(
      "with-init",
      `
export default {
  name: "init-engine",
  defaultExt: ".qmd",
  defaultYaml: () => [],
  defaultContent: () => [],
  validExtensions: () => [],
  claimsFile: () => false,
  claimsLanguage: () => false,
  canFreeze: false,
  generatesFigures: false,
  launch: (ctx) => ({ name: "init-engine", canFreeze: false }),
  init: (quarto) => { /* stores quarto reference */ },
};
`,
    );

    const result = await loadEngineModule(path);
    expect(result.name).toBe("init-engine");
    expect(typeof result.init).toBe("function");
  });
});

// ---------------------------------------------------------------------------
// Invalid engines — validation
// ---------------------------------------------------------------------------

describe("loadEngineModule — invalid engines", () => {
  it("rejects when launch is missing", async () => {
    // Named revert: remove the export-validation block → this invalid module loads
    // without error → this assertion goes RED.
    const path = writeTempMjs(
      "no-launch",
      `
export default {
  name: "no-launch-engine",
  claimsLanguage: (lang) => false,
  // launch is intentionally absent
};
`,
    );

    await expect(loadEngineModule(path)).rejects.toThrow(/launch/i);
  });

  it("rejects when name is missing", async () => {
    const path = writeTempMjs(
      "no-name",
      `
export default {
  // name is intentionally absent
  claimsLanguage: (lang) => false,
  launch: (ctx) => ({}),
};
`,
    );

    await expect(loadEngineModule(path)).rejects.toThrow(/name/i);
  });

  it("rejects when claimsLanguage is missing", async () => {
    const path = writeTempMjs(
      "no-claims",
      `
export default {
  name: "no-claims-engine",
  // claimsLanguage is intentionally absent
  launch: (ctx) => ({}),
};
`,
    );

    await expect(loadEngineModule(path)).rejects.toThrow(/claimsLanguage/i);
  });

  it("error message names the path and the missing member", async () => {
    const path = writeTempMjs(
      "no-launch-2",
      `
export default {
  name: "bad-engine",
  claimsLanguage: () => false,
  // launch missing
};
`,
    );

    await expect(loadEngineModule(path)).rejects.toThrow(
      /engine module.*missing.*launch/i,
    );
  });

  it("rejects when name is not a string (wrong type)", async () => {
    const path = writeTempMjs(
      "bad-name-type",
      `
export default {
  name: 42,
  claimsLanguage: (lang) => false,
  launch: (ctx) => ({}),
};
`,
    );

    await expect(loadEngineModule(path)).rejects.toThrow(/name/i);
  });

  it("rejects when claimsLanguage is not a function (wrong type)", async () => {
    const path = writeTempMjs(
      "bad-claims-type",
      `
export default {
  name: "bad-claims",
  claimsLanguage: "not-a-function",
  launch: (ctx) => ({}),
};
`,
    );

    await expect(loadEngineModule(path)).rejects.toThrow(/claimsLanguage/i);
  });
});

// ---------------------------------------------------------------------------
// Missing default export
// ---------------------------------------------------------------------------

describe("loadEngineModule — missing default export", () => {
  it("rejects with a clear error when there is no default export", async () => {
    const path = writeTempMjs(
      "no-default",
      `
// Named exports only — no default
export const name = "no-default-engine";
export function launch(ctx) { return {}; }
`,
    );

    await expect(loadEngineModule(path)).rejects.toThrow(/no default export/i);
  });

  it("rejects with a clear error when default is null", async () => {
    const path = writeTempMjs(
      "null-default",
      `
export default null;
`,
    );

    await expect(loadEngineModule(path)).rejects.toThrow(/no default export/i);
  });
});
