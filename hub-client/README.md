# hub-client

Web frontend for Quarto Hub - a collaborative document editor using Quarto's WASM rendering engine.

Quarto Hub is **local-first**: it opens straight into a usable editor with no
sign-in, and projects live in your browser until you choose to connect to a
hub for sync and collaboration. See [`LOCAL-FIRST.md`](./LOCAL-FIRST.md) for the
user-facing model and [`OFFLINE.md`](./OFFLINE.md) for the offline asset cache.

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
| `npm run build:local-prod` | Build for local-prod mode (sets sync server URL) |
| `npm run typecheck` | Type-check with strict Vite-compatible settings |
| `npm run lint` | Run ESLint |
| `npm run preview` | Preview production build |
| `npm run local-prod` | Run local production mode (Node.js proxy) |
| `npm run local-prod:nginx` | Run local production mode (nginx in Docker) |
| `npm run local-prod:fresh` | Clean rebuild + run local-prod |
| `npm run local-prod:fresh:nginx` | Clean rebuild + run local-prod with nginx |

### Preflight Checks

Run `npm run preflight` after making changes to verify everything builds correctly:
- Rebuilds the WASM module (catches Rust errors)
- Type-checks TypeScript with Vite-compatible settings

This is the same check that runs before `dev:fresh`, but without starting the dev server.

**Important:** Plain `tsc --noEmit` without `-p tsconfig.app.json` uses different settings and may miss errors that will break at runtime. Always use `npm run typecheck` or `npm run preflight`.

### Testing on Mobile / Other Devices

By default the dev server only accepts connections from `localhost`. To test on a phone or tablet on the same network, you need two things:

1. **Expose the server on the network** (`--host`)
2. **Serve over HTTPS** — browsers treat `http://<ip>` as an insecure context and block APIs like `crypto.randomUUID()` and `crypto.subtle`, causing a blank white page

Start the server with both:

```bash
VITE_HTTPS=1 npm run dev:fresh -- --host
```

Vite will print the network HTTPS URL to open on your device:

```
➜  Local:   https://localhost:5173/
➜  Network: https://192.168.x.x:5173/
```

The certificate is self-signed, so the browser will show a security warning on first visit. Click through it ("Advanced" → "Proceed") — this is expected for local development.

### When to Rebuild WASM

You need to rebuild the WASM module (`npm run build:wasm` or `npm run dev:fresh`) when:

- You've made changes to `crates/wasm-quarto-hub-client`
- You've made changes to `crates/quarto-core` (transforms, pipeline, etc.)
- You've made changes to `crates/pampa` (parsing, rendering)
- You've pulled updates that include Rust changes

## Production Build / Deployment

For deploying to a remote server, use:

```bash
npm run build:all
```

This produces a complete production build in `dist/`.

## Local Production Mode

Test hub-client in a production-like setup locally:

```bash
# Build + run
npm run local-prod:fresh

# Open http://127.0.0.1:8080
```

Two modes available:
- `local-prod` - Node.js proxy (fast, recommended)
- `local-prod:nginx` - nginx in Docker (tests actual nginx config)

See [`../scripts/README.md`](../scripts/README.md) for details and troubleshooting.

### dev:fresh vs build:all

These two scripts are often confused:

| | `dev:fresh` | `build:all` |
|---|-------------|-------------|
| WASM build | Yes | Yes |
| TypeScript | Type-check only (`--noEmit`) | Full compilation (`tsc -b`) |
| Vite | Starts dev server | Production build |
| Output | None (serves on-the-fly) | `dist/` directory |

- **`dev:fresh`** is for local development. Vite transpiles TypeScript on-the-fly, so no compilation step is needed. The `preflight` check just validates types.

- **`build:all`** is for deployment. It runs `tsc -b` to compile TypeScript and `vite build` to bundle everything into `dist/`.

If you run `dev:fresh` expecting deployable output, you won't get any - you need `build:all`.

## Architecture

The hub-client uses a WASM module (`wasm-quarto-hub-client`) that provides:

- **Virtual File System (VFS)** - In-browser file storage for project files
- **QMD Rendering** - Full Quarto rendering pipeline (parsing, transforms, HTML generation)

The WASM module is symlinked from `crates/wasm-quarto-hub-client/pkg/`.

## Console debug API (`window.quartoDebug`)

When a project is loaded, hub-client installs a small read/write
debug API on `window.quartoDebug`. It is intended for developers
and agents who want to script the editor from DevTools without
clicking through menus.

### Gating

The API is installed only when **either** condition is true:

- `import.meta.env.DEV` — i.e., a Vite dev build (`npm run dev`,
  `npm run dev:fresh`).
- `localStorage.quartoDebug === '1'` — manual opt-in for one-off
  debugging against a production build. Set it from DevTools:

  ```js
  localStorage.setItem('quartoDebug', '1');
  // reload the page
  ```

  Remove with `localStorage.removeItem('quartoDebug')`.

This means production builds ship the API code (~1 KB) but never
expose `window.quartoDebug` unless the localStorage flag is set.

### Surface

```ts
window.quartoDebug = {
  project(): { id, description, indexDocId, syncServer } | null;
  listFiles(): string[];
  readFile(path): string | Uint8Array | null;
  writeFile(path, contents: string | Uint8Array, options?: { mimeType?: string }): Promise<void>;
  rerender(): Promise<{ documentPath, result, at }>;
  getActiveFile(): string | null;
  setActiveFile(path: string): void;
  lastRenderResponse(): { documentPath, result, at } | null;
  vfsList(prefix?: string): string[];
  vfsRead(path: string): Uint8Array | null;
};
```

`writeFile` routes through the same Automerge mutation path the
editor uses, so sync, presence, and the live preview all observe
the change. `lastRenderResponse()` returns the most recent render
response — including renders triggered by editor keystrokes, not
just `rerender()` calls.

For binary writes that overwrite an existing path, the API
deletes the old document first so the file keeps its name (the
underlying `createBinaryFile` is content-addressed and would
otherwise rename to `name-<hash>.ext`).

Source: `src/services/debugApi.ts`.
