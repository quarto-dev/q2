# Kyoto Project Overview

## Goal
Explore porting Quarto CLI from TypeScript/Deno to Rust.

## Current Status
**Planning/Understanding Phase** - Writing small prototypes only.

## Repository Structure

```
kyoto/
├── claude-notes/          # Notes for Claude about the project
├── external-sources/      # Source code for analysis
│   ├── quarto-cli/       # Main Quarto CLI (TypeScript/Deno)
│   ├── quarto-web/       # Quarto documentation website
│   ├── quarto-markdown/  # Rust markdown parser (already in Rust!)
│   └── quarto/           # Monorepo with LSP and VS Code extension
└── CLAUDE.md             # Guide for Claude working in external-sources/
```

## Key Findings

### 1. Quarto Architecture

**Current Components**:
- **CLI** (TypeScript/Deno): Main publishing system at `external-sources/quarto-cli/`
- **Markdown Parser** (Rust): Already exists at `external-sources/quarto-markdown/`
- **LSP** (TypeScript/Node): Language server at `external-sources/quarto/apps/lsp/`
- **VS Code Extension** (TypeScript): Editor integration at `external-sources/quarto/apps/vscode/`

### 2. LSP Architecture Problem

**Critical Issue**: The LSP has tight runtime coupling with the CLI:

```typescript
// LSP dynamically loads JS modules from CLI installation
const modulePath = path.join(resourcesPath, "editor", "tools", "vs-code.mjs");
import(fileUrl(modulePath))
```

**Implications**:
- LSP expects JavaScript modules from CLI
- YAML validation/completion loaded at runtime from CLI
- Resource files (schemas, completion data) distributed with CLI
- **This breaks if CLI is ported to Rust**

### 3. Why This Matters

The LSP is in a **separate monorepo** from the CLI but depends on CLI resources:
- Different repos, but runtime coupling
- CLI changes can break LSP
- Testing requires full CLI installation
- Coordination needed across repos

**If CLI → Rust**:
- ❌ JS modules won't exist
- ❌ Dynamic loading breaks
- ❌ YAML validation logic needs reimplementation
- ❌ Resource format may change

## Strategic Options

### Option 1: Port LSP to Rust (Integrated)
Embed LSP in Rust CLI using `tower-lsp` framework.

**Pros**: Single codebase, shared logic, no IPC overhead
**Cons**: ~6,300 LOC to port, need Rust LSP expertise
**Effort**: High (weeks to months)

### Option 2: Bridge Approach
Keep TypeScript LSP, create interface to Rust CLI.

**Pros**: Don't rewrite LSP, iterate independently
**Cons**: IPC overhead, complex interface, two languages
**Effort**: Medium

### Option 3: Hybrid
Core logic in Rust (YAML, schemas), UI features in TypeScript.

**Pros**: Best of both, reuse Rust logic
**Cons**: Still need bridge, split architecture
**Effort**: Medium-High

### Option 4: Clean Separation
Define stable data contract, LSP consumes from CLI.

**Pros**: Clean separation, language-agnostic
**Cons**: Logic duplication, sync issues
**Effort**: Medium

## Recommended Phased Approach

### Phase 1: Foundation ✅ (Current)
- [x] Study existing architecture
- [x] Document LSP dependencies
- [x] Understand design issues

### Phase 2: Bridge (Next)
- [ ] Define LSP data contract (what LSP needs from CLI)
- [ ] Implement Rust CLI compatibility layer
- [ ] Generate equivalent data to `vs-code.mjs`
- [ ] Keep existing TypeScript LSP working

### Phase 3: Evaluate
- [ ] Measure bridge performance
- [ ] Assess maintenance burden
- [ ] Decide: keep bridge vs port to Rust

### Phase 4: Long-term
- [ ] If keeping bridge: optimize, stabilize
- [ ] If porting: use `tower-lsp`, share code with CLI

## Key Advantages of Rust Port

1. **Markdown parsing already in Rust** (`quarto-markdown`)
2. **Single runtime** (vs current Deno CLI + Node LSP)
3. **Shared validation logic** (YAML schemas, completions)
4. **Better performance** (especially for LSP)
5. **Unified distribution** (single binary possible)

## Open Questions

1. What exact data does LSP need from CLI? (schemas, completions, etc.)
2. Can we generate compatible data from Rust?
3. What's the performance impact of different approaches?
4. How much LSP code is CLI-dependent vs VS Code-specific?
5. Is there value in Rust LSP for other editors (Neovim, etc.)?

## Next Steps

1. **Define LSP data contract** - catalog all CLI dependencies
2. **Prototype bridge** - can Rust CLI provide what's needed?
3. **Evaluate tower-lsp** - how feasible is Rust LSP?
4. **Plan migration** - minimize disruption to users
5. **Consider multi-editor support** - LSP could work beyond VS Code

## Resources

- **tower-lsp**: https://github.com/ebkalderon/tower-lsp
- **rust-analyzer**: Reference Rust LSP implementation
- **LSP spec**: https://microsoft.github.io/language-server-protocol/
- **quarto-markdown**: Tree-sitter parsing already in Rust
