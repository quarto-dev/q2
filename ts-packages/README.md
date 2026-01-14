# TypeScript Packages

This directory contains TypeScript packages associated with the Kyoto Rust workspace.
These packages are part of an npm workspace defined in the root `package.json`.

## Packages

- **annotated-qmd** (`@quarto/annotated-qmd`): Converts pampa JSON output
  to AnnotatedParse structures compatible with quarto-cli's YAML validation infrastructure.

- **quarto-automerge-schema** (`@quarto/quarto-automerge-schema`): Automerge schema types
  for Quarto collaborative documents. Defines the structure of documents stored in Automerge.

- **quarto-sync-client** (`@quarto/quarto-sync-client`): Automerge sync client for Quarto
  collaborative documents. Provides real-time document synchronization with a callback-based API.

## Development

All packages are part of the npm workspace. Install dependencies from the repo root:

```bash
# From repo root
npm install

# Build all packages
npm run build

# Build a specific package
npm run build -w @quarto/quarto-sync-client
```

## Architecture

The sync packages follow a layered design:

```
@quarto/quarto-automerge-schema   (types only, no runtime deps)
            ↓
@quarto/quarto-sync-client        (sync logic with callbacks)
            ↓
hub-client                        (application with VFS/WASM)
```

This separation allows other applications to use the sync client with their own
storage/VFS implementations.
