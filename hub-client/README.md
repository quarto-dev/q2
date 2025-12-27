# hub-client

Web frontend for Quarto Hub - a collaborative document editor using Quarto's WASM rendering engine.

## Prerequisites

- Node.js 18+
- Rust toolchain with `wasm32-unknown-unknown` target (`rustup target add wasm32-unknown-unknown`)
- `wasm-pack` (`cargo install wasm-pack`)
- LLVM (macOS only: `brew install llvm`)

## Development

### Quick Start (Fresh Build)

To rebuild everything and start the dev server:

```bash
npm run dev:fresh
```

This will:
1. Rebuild the WASM module from `crates/wasm-quarto-hub-client`
2. Start the Vite dev server

### Regular Development

If you haven't changed any Rust code, you can skip the WASM rebuild:

```bash
npm run dev
```

### Available Scripts

| Script | Description |
|--------|-------------|
| `npm run dev` | Start Vite dev server (uses existing WASM) |
| `npm run dev:fresh` | Run preflight checks, then start dev server |
| `npm run preflight` | Build WASM + typecheck (run this during development) |
| `npm run build` | Build TypeScript and Vite for production |
| `npm run build:wasm` | Rebuild only the WASM module |
| `npm run build:all` | Rebuild WASM + production build |
| `npm run typecheck` | Type-check with strict Vite-compatible settings |
| `npm run lint` | Run ESLint |
| `npm run preview` | Preview production build |

### Preflight Checks

Run `npm run preflight` after making changes to verify everything builds correctly:
- Rebuilds the WASM module (catches Rust errors)
- Type-checks TypeScript with Vite-compatible settings

This is the same check that runs before `dev:fresh`, but without starting the dev server.

**Important:** Plain `tsc --noEmit` without `-p tsconfig.app.json` uses different settings and may miss errors that will break at runtime. Always use `npm run typecheck` or `npm run preflight`.

### When to Rebuild WASM

You need to rebuild the WASM module (`npm run build:wasm` or `npm run dev:fresh`) when:

- You've made changes to `crates/wasm-quarto-hub-client`
- You've made changes to `crates/quarto-core` (transforms, pipeline, etc.)
- You've made changes to `crates/pampa` (parsing, rendering)
- You've pulled updates that include Rust changes

## Architecture

The hub-client uses a WASM module (`wasm-quarto-hub-client`) that provides:

- **Virtual File System (VFS)** - In-browser file storage for project files
- **QMD Rendering** - Full Quarto rendering pipeline (parsing, transforms, HTML generation)

The WASM module is symlinked from `crates/wasm-quarto-hub-client/pkg/`.
