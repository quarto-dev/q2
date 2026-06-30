import { defineConfig } from "vitest/config";
import path from "path";

export default defineConfig({
  // Resolve the workspace `@quarto/api` dep from its TypeScript source instead
  // of a (possibly stale) built `dist/`. Without this, a change to `@quarto/api`
  // source is invisible to these tests until `npm run build -w @quarto/api`
  // runs — every other consumer (tsc via the `types` condition, the esbuild
  // bundle via `conditions: ['source','import']`) already resolves from source.
  //
  // The `source` export condition alone is NOT honored by vitest when a `dist/`
  // exists (same caveat noted in preview-runtime/vitest.config.ts), so the
  // load-bearing piece is the directory alias: `@quarto/api` → `../quarto-api/src`
  // resolves both the bare import and every subpath (`@quarto/api/mappedString`,
  // `/platform`, …) by prefix + index resolution. `conditions` is kept as cheap
  // insurance for any future `@quarto/*` dep added here.
  resolve: {
    conditions: ["source", "import"],
    alias: {
      "@quarto/api": path.resolve(__dirname, "../quarto-api/src"),
    },
  },
  test: {
    include: ["src/**/*.test.ts"],
    exclude: ["dist/**", "node_modules/**", "**/*.deno-test.ts"],
    passWithNoTests: true,
  },
});
