## Before Committing

**Read `claude-notes/instructions/review.md` and complete the checklist before every commit.** This is mandatory - do not skip the review step.

## General

- Try hard to avoid "TODO" comments in the code base. If are running low on context and you do have to add it, make sure there's a braid strand (even if low-priority) to track the TODO, and add the strand id to the TODO line.

## Rust: HashMap and Determinism

**CRITICAL**: This codebase requires deterministic output for snapshot tests and reproducible builds.

1. **Never use `std::collections::HashMap` or `rustc_hash::FxHashMap` in structs that are serialized** (anything with `#[derive(Serialize)]`). Use `hashlink::LinkedHashMap` instead - it preserves insertion order.

2. **Default to `LinkedHashMap`** for any map where you iterate over entries or where the map could eventually be serialized. When in doubt, use `LinkedHashMap`.

3. **`FxHashMap` is only acceptable for**:
   - Internal caches keyed by pointers or indices
   - Lookup-only maps where you never iterate over keys/values
   - Performance-critical code where you've verified order doesn't affect output

4. **Red flags to watch for**:
   - `#[serde(flatten)]` on HashMap fields (serialization order becomes non-deterministic)
   - HashMap fields in structs that implement `Serialize`
   - Iterating over HashMap entries in any code path that affects output

## hub-client (TypeScript/React)

When making changes to `hub-client/`:

1. **At the start of a dev session**, run `npm install` from the repo root to ensure all workspace dependencies are up to date. This is fast and prevents test failures from missing packages added by other contributors.

2. **If anything fails with `The package "@esbuild/<platform>" could not be found`** (or a similar missing-platform-binary error from rollup/swc/parcel): the *lockfile* is missing that package's optional platform dependencies, which breaks every machine and CI — not just your checkout. Fix the artifact, not your machine: delete the affected package's block from `package-lock.json` (e.g. `jq 'del(.packages["node_modules/esbuild"])' package-lock.json`), run `npm install` from the repo root so npm re-resolves it and records the full `@esbuild/*` platform set, verify with a clean `npm ci`, and **commit the lockfile change**. Do NOT stop at `npm install --no-save @esbuild/darwin-arm64` — that is a machine-local band-aid that leaves CI and colleagues broken. (Incident 2026-08-19: top-level `esbuild@0.28.0` entered the lockfile with zero platform entries — only vite's nested copy had them — so every `npm ci` produced a broken esbuild.)

3. **After making TypeScript changes**, run preflight checks:
   ```bash
   cd hub-client && npm run preflight
   ```
   This builds WASM and type-checks with Vite-compatible settings.

4. **Type imports**: Use `import type` for type-only imports (interfaces, type aliases). Vite's esbuild transformer requires this due to `verbatimModuleSyntax: true`.
   ```typescript
   // Correct
   import { useCallback } from 'react';
   import type { RefObject } from 'react';

   // Wrong - will fail at runtime in Vite
   import { useCallback, RefObject } from 'react';
   ```

5. **Don't use plain `tsc --noEmit`** - it uses different settings and misses errors. Always use `npm run typecheck` or `npm run preflight`.

## WASM (wasm-quarto-hub-client)

When making changes to the WASM module (`crates/wasm-quarto-hub-client/`):

1. **CRITICAL: Test WASM changes with Node.js BEFORE claiming they work**. The fastest way to verify WASM behavior is with a Node.js test script, NOT by opening the browser. See `crates/wasm-quarto-hub-client/README.md` for details.

2. **Create or update `hub-client/test-wasm.mjs`** to test new WASM functionality:
   ```javascript
   import { readFile } from 'fs/promises';
   import { dirname, join } from 'path';
   import { fileURLToPath } from 'url';

   const __dirname = dirname(fileURLToPath(import.meta.url));
   const wasmPath = join(__dirname, 'node_modules/wasm-quarto-hub-client/wasm_quarto_hub_client_bg.wasm');

   // Load and test the WASM module
   const wasm = await import('wasm-quarto-hub-client');
   const wasmBytes = await readFile(wasmPath);
   await wasm.default(wasmBytes);

   // Test your functionality here
   const result = await wasm.render_qmd_content(content, '');
   console.log(JSON.parse(result));
   ```

3. **Build WASM before testing**:
   ```bash
   cd hub-client && npm run build:wasm
   ```

4. **The WASM crate is excluded from the workspace** - it has its own `Cargo.toml` dependencies. If you add a new dependency to the WASM code, you must add it to `crates/wasm-quarto-hub-client/Cargo.toml`, not just the workspace root.
