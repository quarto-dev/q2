# Julia engine extension for quarto

Quarto's Julia engine, extracted from quarto-cli into a standalone engine extension. It was originally built directly into quarto-cli and is now pulled back in via a git subtree.

## Development

To build the TypeScript engine extension:

```bash
npm ci && npm run build
```

This bundles `src/julia-engine.ts` into `_extensions/julia-engine/julia-engine.js` with esbuild (Node 24 or later). `npm test` runs the unit tests and `npm run typecheck` type-checks the sources.

## Implementation notes

The engine is plain Node: it uses `node:` built-ins only (`fs`, `path`, `child_process`, `net`, `crypto`) and runs unchanged under Quarto 1's Deno and Quarto 2's Node. `src/transport.ts` holds the control-server wire protocol (HMAC-signed requests, newline-delimited JSON replies) separately from the Quarto API so it can be unit tested against a fake server.

There is a new Quarto API for the more significant utilities, such as Jupyter and Markdown functions. Quarto passes the API down in `ExecutionEngineDiscovery.init()` - this may get called multiple times but it will always pass the same Quarto API object.

The delay in `readTransportFile()` was not awaited previously, so it wasn't effective. Fixing it required making its caller functions async.

String constants are defined in `src/constants.ts`.
