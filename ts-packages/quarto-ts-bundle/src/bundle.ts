/**
 * The bundler behind `q2 call build-ts-extension`: rollup (the `@rollup/wasm-node`
 * build, so no per-platform native package) with node-resolve, commonjs, a
 * TypeScript type-stripping step and an alias step.
 *
 * Requirements (all were met by the esbuild this replaces):
 * - ESM output for Node >= 24, `node:*` imports left external;
 * - the `source` + `import` export conditions, so `@quarto/*` workspace
 *   packages bundle from their TypeScript sources;
 * - bundled CommonJS dependencies;
 * - an import that cannot be resolved FAILS (rollup's default is a warning
 *   that leaves it external);
 * - byte-stable output: rollup embeds no paths, timestamps or hashes in a
 *   single-file ESM build, so the same inputs give the same bytes from any
 *   directory.
 */

import { rollup, type Plugin, type RollupLog } from "@rollup/wasm-node";
import commonjs from "@rollup/plugin-commonjs";
import nodeResolve from "@rollup/plugin-node-resolve";
import { realpathSync } from "node:fs";
import { stripTypeScriptTypes } from "node:module";
import { join, relative, resolve } from "node:path";

export interface BundleJob {
  /** Entry module (relative to `cwd`, or absolute). */
  entry: string;
  /** Output file (relative to `cwd`, or absolute). */
  output: string;
  /** Import aliases: specifier (exact or `<specifier>/...` prefix) -> absolute path. */
  aliases: Record<string, string>;
  /** Directory relative paths resolve against. Defaults to `process.cwd()`. */
  cwd?: string;
}

/** A failed bundle, with a message fit to show to the extension author. */
export class BundleError extends Error {}

const TS_FILE = /\.[mc]?ts$/;

/**
 * Erase TypeScript types with Node's built-in stripper (`module.stripTypeScriptTypes`).
 *
 * Strip mode replaces types with whitespace, so positions are kept and no
 * source map is needed. Syntax that is not erasable (enums, namespaces,
 * parameter properties) is rejected by strip mode; those files are
 * transformed instead. The output of both modes was identical across Node
 * 24.0.2 (amaro 0.5.2) and 24.5.0 (amaro 1.1.1) for every real engine source.
 */
function stripTypes(): Plugin {
  return {
    name: "strip-typescript-types",
    transform(code, id) {
      if (!TS_FILE.test(id) || id.endsWith(".d.ts")) return null;
      try {
        return { code: stripTypeScriptTypes(code, { mode: "strip" }), map: null };
      } catch (e) {
        if ((e as { code?: string }).code !== "ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX") throw e;
        return { code: stripTypeScriptTypes(code, { mode: "transform" }), map: null };
      }
    },
  };
}

/**
 * esbuild-style aliases: `name` matches the specifier `name` and anything
 * under `name/`; the longest name wins.
 */
function aliases(table: Record<string, string>): Plugin {
  const entries = Object.entries(table).sort(([a], [b]) => b.length - a.length);
  return {
    name: "alias",
    resolveId(source, importer, options) {
      for (const [name, target] of entries) {
        if (source === name || source.startsWith(`${name}/`)) {
          const rest = source.slice(name.length);
          return this.resolve(join(target, rest), importer, { ...options, skipSelf: true });
        }
      }
      return null;
    },
  };
}

/** `id` relative to `cwd` for messages; ids are real paths, so `cwd` is resolved first. */
function describe(cwd: string, id: string | undefined): string {
  if (!id) return "the entry point";
  const rel = relative(realpathSync(cwd), id);
  return (rel.startsWith("..") ? id : rel).replaceAll("\\", "/");
}

export async function bundle(job: BundleJob, warn: (line: string) => void = console.error): Promise<void> {
  const cwd = job.cwd ?? process.cwd();
  const output = resolve(cwd, job.output);
  const build = await rollup({
    input: resolve(cwd, job.entry),
    external: (id) => id.startsWith("node:"),
    onwarn(log: RollupLog) {
      if (log.code === "UNRESOLVED_IMPORT") {
        // Rollup leaves the import external and carries on; a bundle that
        // imports something it did not include cannot run.
        const importer = describe(cwd, log.id ?? log.ids?.[0]);
        throw new BundleError(`Could not resolve "${log.exporter}" (imported by ${importer})`);
      }
      // Import cycles are normal in real code and esbuild stays quiet about them.
      if (log.code === "CIRCULAR_DEPENDENCY") return;
      warn(`warning: ${log.message}`);
    },
    plugins: [
      aliases(job.aliases),
      nodeResolve({
        rootDir: cwd,
        exportConditions: ["source", "import"],
        extensions: [".ts", ".mts", ".cts", ".js", ".mjs", ".cjs"],
        preferBuiltins: true,
      }),
      stripTypes(),
      commonjs(),
    ],
  });
  try {
    await build.write({ file: output, format: "es", sourcemap: false });
  } finally {
    await build.close();
  }
}
