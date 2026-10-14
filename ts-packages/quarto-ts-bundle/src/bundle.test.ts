import { mkdirSync, mkdtempSync, readFileSync, existsSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { bundle } from "./bundle.js";

const dirs: string[] = [];
function tmp(): string {
  const dir = mkdtempSync(join(tmpdir(), "ts-bundle-"));
  dirs.push(dir);
  return dir;
}
afterEach(() => {
  for (const d of dirs.splice(0)) rmSync(d, { recursive: true, force: true });
});

function write(root: string, files: Record<string, string>): void {
  for (const [rel, text] of Object.entries(files)) {
    const path = join(root, rel);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, text);
  }
}

/** Bundle `entry` under `root`; returns the output text. */
async function build(root: string, entry: string, aliases: Record<string, string> = {}): Promise<string> {
  const warnings: string[] = [];
  await bundle({ entry, output: "dist/out.mjs", aliases, cwd: root }, (w) => warnings.push(w));
  expect(warnings).toEqual([]);
  return readFileSync(join(root, "dist/out.mjs"), "utf8");
}

describe("bundle", () => {
  it("strips types and erases `import type`", async () => {
    const root = tmp();
    write(root, {
      "src/types.ts": "export interface Shape { n: number }\nexport type Id = string;\n",
      "src/main.ts":
        'import type { Shape, Id } from "./types.js";\n' +
        "export function area(s: Shape, id: Id): number { return s.n * s.n + id.length; }\n",
    });
    const out = await build(root, "src/main.ts");
    expect(out).toContain("function area(");
    expect(out).not.toContain("interface");
    expect(out).not.toContain(": number");
  });

  it("transforms files that use enums, which strip-only mode rejects", async () => {
    const root = tmp();
    write(root, { "src/main.ts": "export enum Color { Red, Green }\nexport const c = Color.Green;\n" });
    const out = await build(root, "src/main.ts");
    expect(out).not.toContain("enum ");
    const mod = await import(`data:text/javascript,${encodeURIComponent(out)}`);
    expect(mod.c).toBe(1);
  });

  it("leaves node: and bare built-in imports external", async () => {
    const root = tmp();
    write(root, {
      "src/main.ts":
        'import { join } from "node:path";\nimport { readFileSync } from "fs";\n' +
        "export const x = [join, readFileSync];\n",
    });
    const out = await build(root, "src/main.ts");
    expect(out).toMatch(/from 'node:path'/);
    expect(out).toMatch(/from 'fs'/);
  });

  it("bundles CommonJS dependencies into ESM that runs", async () => {
    const root = tmp();
    write(root, {
      "node_modules/cjs-dep/package.json": '{"name":"cjs-dep","version":"1.0.0","main":"index.js"}',
      "node_modules/cjs-dep/index.js":
        'const path = require("node:path");\nmodule.exports = function hello(n) { return path.basename(n) + "!"; };\n',
      "src/main.ts": 'import hello from "cjs-dep";\nexport const out: string = hello("/a/b.txt");\n',
    });
    const out = await build(root, "src/main.ts");
    const mod = await import(`data:text/javascript,${encodeURIComponent(out)}`);
    expect(mod.out).toBe("b.txt!");
  });

  it("honours the `source` export condition", async () => {
    const root = tmp();
    write(root, {
      "node_modules/@scope/pkg/package.json": JSON.stringify({
        name: "@scope/pkg",
        type: "module",
        exports: { ".": { types: "./src/index.ts", source: "./src/index.ts", import: "./dist/missing.js" } },
      }),
      "node_modules/@scope/pkg/src/index.ts": "export const v: number = 7;\n",
      "src/main.ts": 'import { v } from "@scope/pkg";\nexport const w = v;\n',
    });
    const mod = await import(`data:text/javascript,${encodeURIComponent(await build(root, "src/main.ts"))}`);
    expect(mod.w).toBe(7);
  });

  it("applies aliases to the name and to subpaths, longest name first", async () => {
    const root = tmp();
    write(root, {
      "pkgs/api/src/index.ts": "export const api = 'root';\n",
      "pkgs/api/src/claims.ts": "export const claims = 'claims';\n",
      "pkgs/special/index.ts": "export const special = 'special';\n",
      "src/main.ts":
        'import { api } from "@q/api";\nimport { claims } from "@q/api/claims";\nimport { special } from "@q/api/special";\n' +
        "export const all = [api, claims, special];\n",
    });
    const out = await build(root, "src/main.ts", {
      "@q/api": join(root, "pkgs/api/src"),
      "@q/api/special": join(root, "pkgs/special"),
    });
    for (const v of ["'root'", "'claims'", "'special'"]) expect(out).toContain(v);
  });

  it("fails on an unresolved import, naming the specifier, and writes nothing", async () => {
    const root = tmp();
    write(root, { "src/main.ts": 'import { join } from "jsr:@std/path";\nexport const x = join;\n' });
    await expect(build(root, "src/main.ts")).rejects.toThrow(/Could not resolve "jsr:@std\/path" \(imported by src\/main\.ts\)/);
    expect(existsSync(join(root, "dist/out.mjs"))).toBe(false);
  });

  it("fails on an unresolved relative import", async () => {
    const root = tmp();
    write(root, { "src/main.ts": 'import "./nope.js";\n' });
    await expect(build(root, "src/main.ts")).rejects.toThrow(/nope/);
  });

  it("is byte-stable across builds and directories, with no paths in the output", async () => {
    const files = {
      "node_modules/cjs-dep/package.json": '{"name":"cjs-dep","version":"1.0.0","main":"index.js"}',
      "node_modules/cjs-dep/index.js": "module.exports = { n: 1 };\n",
      "src/util.ts": "export const u: number = 2;\n",
      "src/main.ts": 'import dep from "cjs-dep";\nimport { u } from "./util.js";\nexport const t = dep.n + u;\n',
    };
    const outputs: string[] = [];
    for (const sub of ["a", "b/deeper"]) {
      const root = join(tmp(), sub);
      write(root, files);
      outputs.push(await build(root, "src/main.ts"));
      outputs.push(await build(root, "src/main.ts"));
      expect(outputs.at(-1)).not.toContain(root);
    }
    expect(new Set(outputs).size).toBe(1);
  });
});
