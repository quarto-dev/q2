# Quarto LSP Architecture Findings

## Current Implementation Overview

The Quarto LSP is currently implemented in TypeScript within the quarto monorepo at `apps/lsp/`.

### Repository Structure

- **Main monorepo**: `/external-sources/quarto/`
  - `apps/lsp/`: The LSP server implementation (~6,300 LOC)
  - `apps/vscode/`: VS Code extension that launches the LSP
  - `packages/quarto-core/`: Shared Quarto-specific functionality
  - `packages/editor-server/`: Editor-related server functionality
  - `packages/core/`: Core shared utilities

### LSP Implementation Details

**Location**: `apps/lsp/src/`

**Key components**:
- `index.ts`: Main LSP server entry point, handles initialization and LSP protocol
- `quarto.ts`: Quarto-specific functionality (YAML completions, attribute completions)
- `workspace.ts`: Workspace management for multi-file projects
- `config.ts`: Configuration handling
- `diagnostics.ts`: Error/warning reporting
- `service/`: Language service implementations
  - `providers/`: LSP feature providers
    - `completion/`: Code completion
    - `hover/`: Hover information (math, YAML, images, references)
    - `document-links.ts`: Link detection and navigation
    - `document-symbols.ts`: Outline/symbols
    - `definitions.ts`: Go-to-definition
    - `references.ts`: Find references
    - `folding.ts`: Code folding
    - `smart-select.ts`: Smart selection
    - `diagnostics.ts`: Diagnostics

**Dependencies**:
- `vscode-languageserver`: LSP protocol implementation
- `quarto-core`: Shared Quarto functionality (context, metadata, markdown parsing)
- `editor-server`: Editor services (Zotero, preferences)
- `core`: General utilities
- `js-yaml`: YAML parsing
- `markdown-it`: Markdown parsing

### VS Code Integration

**Location**: `apps/vscode/src/lsp/client.ts`

The VS Code extension:
1. Loads the compiled LSP server from `out/lsp/lsp.js`
2. Launches it as a Node process using IPC transport
3. Uses middleware to intercept and enhance LSP requests:
   - Virtual document handling for embedded code (Python, R, etc.)
   - Embedded code completion delegation
   - Hover enhancement with image previews
   - Signature help for embedded languages

## Critical Design Issues

### 1. **Tight Coupling with quarto-cli**

The LSP currently depends on the Quarto CLI being installed and accessible:

```typescript
// From quarto.ts - loads YAML completion module from CLI resources
const modulePath = path.join(resourcesPath, "editor", "tools", "vs-code.mjs");
import(fileUrl(modulePath))
```

**Problem**: The LSP dynamically imports a JavaScript module from the Quarto CLI installation for YAML schema validation and completions. This creates a runtime dependency on the CLI.

### 2. **Resource Dependency**

The LSP loads various resources from the CLI installation:
- YAML schemas and validation
- Attribute completions (`attrs.yml`)
- Editor tools

**Problem**: These resources are maintained in quarto-cli, not the monorepo. Any schema changes require CLI updates.

### 3. **Monorepo Location**

The LSP is in a separate monorepo from quarto-cli, but:
- It depends on quarto-cli resources at runtime
- The VS Code extension packages the LSP
- Updates require coordinating across repos

**Problem**: Split architecture makes it hard to keep LSP in sync with CLI changes.

### 4. **TypeScript/Node Runtime**

The LSP is written in TypeScript and runs in Node.js:
- Bundled with VS Code extension (~6,300 LOC + dependencies)
- Requires Node runtime
- Separate from CLI (which is Deno/TypeScript)

**Problem**: Different runtime environment from CLI creates deployment complexity.

### 5. **Language Service Duplication**

Some language analysis is duplicated:
- Markdown parsing (using `markdown-it` in LSP)
- YAML parsing (using `js-yaml`)
- Schema validation (loaded from CLI)

**Problem**: Logic exists in multiple places with different implementations.

## Key Dependencies Chain

```
VS Code Extension
    ↓
LSP Server (TypeScript/Node)
    ↓
├── quarto-core package (monorepo)
├── editor-server package (monorepo)
└── Quarto CLI Resources (external)
    └── editor/tools/vs-code.mjs (YAML validation)
    └── editor/tools/attrs.yml (attribute completions)
```

## Architecture Pain Points

1. **Cross-repo coordination**: LSP in monorepo, but depends on CLI resources
2. **Runtime loading**: Dynamic import of modules from CLI at runtime
3. **Resource synchronization**: Schema/completion data lives in CLI
4. **Deployment complexity**: LSP bundled with extension, needs CLI installed
5. **Testing isolation**: Hard to test LSP without full CLI installation

## Implications for Rust Port

If the CLI is ported to Rust, the current LSP architecture will break because:

1. The LSP expects JavaScript modules from the CLI (`vs-code.mjs`)
2. YAML schema validation logic is in the CLI's TypeScript codebase
3. Resource files are distributed with the CLI

**Options**:
1. **Port LSP to Rust**: Include LSP functionality in the Rust CLI
2. **Bridge approach**: Rust CLI exposes data/services that TypeScript LSP consumes
3. **Embed in CLI**: Make LSP a component of the CLI binary
4. **Keep separate**: Maintain TypeScript LSP with new interface to Rust CLI

The current architecture suggests option 1 or 3 would be cleanest, avoiding cross-language/cross-process boundaries.
