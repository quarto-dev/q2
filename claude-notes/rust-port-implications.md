# Rust Port Implications and Considerations

## Current State Summary

The Quarto ecosystem currently consists of:

1. **quarto-cli**: Main CLI (Deno/TypeScript) - external-sources/quarto-cli/
2. **quarto-markdown**: Rust markdown parser - external-sources/quarto-markdown/
3. **quarto LSP**: TypeScript/Node LSP server - external-sources/quarto/apps/lsp/
4. **quarto VS Code**: TypeScript extension - external-sources/quarto/apps/vscode/

## Critical Dependencies

### LSP → CLI Dependencies
The LSP currently has tight runtime coupling with quarto-cli:

```typescript
// Loads YAML validation from CLI resources
const modulePath = path.join(resourcesPath, "editor", "tools", "vs-code.mjs");
```

This means:
- LSP expects JavaScript modules from CLI
- YAML schemas/completions loaded from CLI installation
- Can't work without CLI present

### What Breaks with Rust CLI

If quarto-cli is ported to Rust:
1. ❌ `vs-code.mjs` won't exist (TypeScript → Rust)
2. ❌ Dynamic JS module loading breaks
3. ❌ YAML validation logic needs reimplementation
4. ❌ Attribute/completion data format may change
5. ❌ Resource paths/structure likely different

## Strategic Options for LSP

### Option 1: Port LSP to Rust (Integrated)
**Embed LSP functionality directly in the Rust CLI**

**Pros**:
- Single codebase, single language
- No cross-process/cross-language boundaries
- Shared YAML validation, schema logic
- Easier to keep in sync
- Can use tower-lsp or similar Rust LSP framework

**Cons**:
- Large scope (6,300+ LOC to port)
- Need Rust LSP expertise
- VS Code extension still needs to bundle/launch it

**Effort**: High (several weeks to months)

### Option 2: Keep TypeScript LSP with Rust CLI Bridge
**Maintain separate TypeScript LSP, create interface to Rust CLI**

**Approaches**:
- **2a. JSON/RPC bridge**: Rust CLI exposes LSP data via JSON-RPC or similar
- **2b. Shared library**: Rust CLI compiles to native lib with C FFI, Node addon
- **2c. Subprocess**: LSP shells out to Rust CLI for data

**Pros**:
- Don't need to rewrite LSP
- Leverage existing TypeScript LSP code
- Can iterate on Rust CLI independently

**Cons**:
- Performance overhead (IPC/FFI/subprocess)
- Complex interface design
- Two languages to maintain
- Testing complexity

**Effort**: Medium (design interface, implement bridge)

### Option 3: Hybrid - Core in Rust, Features in TypeScript
**Move critical LSP logic to Rust, keep UI features in TypeScript**

What to port:
- YAML validation/completion (already in CLI)
- Schema handling
- Document parsing/analysis

What to keep in TypeScript:
- VS Code integration
- Middleware/virtual docs
- UI-specific features

**Pros**:
- Reuse Rust CLI logic directly
- Keep VS Code-specific code in TypeScript
- Performance benefits for heavy lifting

**Cons**:
- Still need bridge/interface
- Split logic between languages
- Complex architecture

**Effort**: Medium-High

### Option 4: Full Separation with Contract
**Keep LSP separate, define stable data contract with CLI**

- Rust CLI provides LSP data via files/API
- TypeScript LSP consumes standardized format
- No code sharing, just data exchange

**Pros**:
- Clean separation of concerns
- Independent evolution
- Language-agnostic interface

**Cons**:
- Duplicated logic (parsing, validation)
- Harder to keep schemas in sync
- Performance overhead

**Effort**: Medium

## Recommended Approach

**Phased strategy**:

### Phase 1: Foundation (Immediate)
1. **Port CLI to Rust** (already exploring with kyoto project)
2. **Extract LSP data contract**: Document what LSP needs from CLI
   - YAML schemas
   - Completion data
   - Validation rules
   - Resource metadata

### Phase 2: Bridge (Short-term - works with existing LSP)
3. **Implement compatibility layer in Rust CLI**:
   - Generate `vs-code.mjs` equivalent data as JSON
   - Provide CLI command for LSP queries
   - Maintain resource files in compatible format

This keeps existing TypeScript LSP working while buying time.

### Phase 3: LSP Decision (Medium-term)
4. **Evaluate**:
   - Performance of bridge approach
   - Development velocity
   - Maintenance burden

5. **Choose**:
   - If bridge works well → Keep Option 2
   - If performance/complexity issues → Move to Option 1 (Rust LSP)

### Phase 4: Long-term
6. **If porting to Rust**:
   - Use `tower-lsp` framework (well-maintained Rust LSP library)
   - Share code with CLI where possible
   - Keep VS Code extension as thin client

## Key Considerations

### 1. **YAML Schema Management**
Currently duplicated between CLI and LSP. Rust port should:
- Centralize schema definitions
- Generate completions/validation from single source
- Expose to both CLI and LSP (however implemented)

### 2. **Markdown Parsing**
`quarto-markdown` already exists in Rust. LSP should:
- Reuse this parser (Option 1)
- Or consume its output via interface (Options 2-4)

### 3. **Virtual Document Handling**
VS Code LSP does complex virtual doc management for embedded code. This is:
- VS Code-specific functionality
- Best kept in TypeScript
- Doesn't need to move to Rust

### 4. **Performance**
LSP performance matters. Consider:
- How often CLI is called
- IPC/FFI overhead
- Caching strategies

### 5. **Distribution**
Current: VS Code extension bundles TypeScript LSP
- With Rust LSP: Would bundle Rust binary (platform-specific)
- With bridge: Bundle TypeScript LSP + interface to Rust CLI
- Consider bundle size, platform support

## Action Items

1. ✅ Document LSP architecture (done)
2. ⏭️ **Define LSP data contract** - what does LSP need from CLI?
3. ⏭️ **Prototype bridge approach** - can Rust CLI provide what's needed?
4. ⏭️ **Evaluate tower-lsp** - how hard is Rust LSP implementation?
5. ⏭️ **Plan migration** - phased approach to minimize disruption

## Resources

- **tower-lsp**: https://github.com/ebkalderon/tower-lsp (Rust LSP framework)
- **rust-analyzer**: Reference implementation of complex Rust LSP
- **LSP specification**: https://microsoft.github.io/language-server-protocol/
- **quarto-markdown**: Already has tree-sitter parsing in Rust

## Questions to Answer

1. What exact data does LSP need from CLI? (schema, completions, etc.)
2. Can we generate compatible data from Rust?
3. What's the performance impact of different approaches?
4. How much LSP code is truly CLI-dependent vs VS Code-specific?
5. Is there value in a Rust LSP that could be used by other editors (Neovim, etc.)?
