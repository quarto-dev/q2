# Quarto Hub Kanban

A kanban board app built on top of Quarto Hub's real-time sync infrastructure. Cards are stored as level-2 headers in a QMD document, with attributes (status, type, deadline, etc.) driving the board UI.

## Context

Although this lives in `q2-demos/`, it's more than a throwaway demo. Think of it as **dogfooding**: it showcases the potential of Quarto Hub's collaborative editing stack (Automerge sync, WASM-based QMD parsing, live AST manipulation), and we plan to use it for managing aspects of q2 development itself.

When making engineering decisions here, treat it as a real application — invest in good UX, accessibility, and polish rather than cutting corners for demo purposes.

## Development

This project uses npm workspaces. Always run `npm install` from the **repo root**, not from this directory.

```bash
# From repo root
npm install

# From this directory
npm run dev          # Start dev server with HMR
npm run dev:fresh    # Clear cache and start fresh
npm run build:all    # Full build (WASM + app)
npm run test         # Unit tests
npm run test:integration  # Component integration tests
npm run test:ci      # All tests
```
