# Quarto LSP Hover Support Plan

**Issue:** kyoto-jqh - LSP Phase 4: Hover Information
**Parent Epic:** kyoto-7bf - Implement Quarto LSP server (quarto lsp)
**Created:** 2026-01-20
**Status:** Deferred (pending schema integration)

## Overview

This plan covers implementing hover support for the Quarto LSP. Based on analysis of TS Quarto's hover implementation, hover is **purely schema-driven YAML intelligence** - it provides descriptions for YAML keys in frontmatter, code cell options, and project config files.

### Why Deferred

TS Quarto's hover implementation requires:
1. Schema definitions with `description` fields (short/long format)
2. Schema navigation by instance path
3. Integration with frontmatter, engine options, and project config schemas

Rust Quarto has `quarto-yaml-validation` with schema infrastructure, but:
- Schemas store `description: Option<String>` (simple string only)
- TS Quarto uses `tags["description"]` with `{short, long}` objects
- Schema registry would need to be threaded through to the hover function
- No direct import of TS Quarto's schema definitions yet

Implementing hover without schema integration would provide no functionality that matches TS Quarto.

## Prerequisites

Before implementing this phase:

- [ ] Schema integration work (may be separate epic)
  - Import or port TS Quarto schema definitions
  - Enhance `SchemaAnnotations` to handle `{short, long}` descriptions
  - Create schema registry accessible from LSP core

## Work Items

### Core Implementation

- [ ] Implement position-to-YAML-node lookup in `quarto-lsp-core`
- [ ] Create `get_hover()` function in `quarto-lsp-core`
- [ ] Hover for YAML frontmatter keys (show description from schema)
- [ ] Hover for code cell options (`#| ` lines)
- [ ] Hover for project config keys (`_quarto.yml`)
- [ ] Implement `textDocument/hover` handler in `quarto-lsp`
- [ ] Write unit tests for hover content
- [ ] Write integration tests for hover requests

### Schema Integration (if not done separately)

- [ ] Thread schema registry through to hover function
- [ ] Implement schema navigation by instance path (similar to TS Quarto's `navigateSchemaByInstancePath`)
- [ ] Extract descriptions from schema annotations
- [ ] Handle both simple string and `{short, long}` description formats

## Technical Notes

### TS Quarto Hover Implementation

Location: `external-sources/quarto-cli/src/core/lib/yaml-intelligence/hover.ts`

Key pattern:
```typescript
// Navigate schema by path
for (const matchingSchema of navigateSchemaByInstancePath(schema, navigationPath)) {
  const concreteSchema = resolveSchema(matchingSchema);
  if (concreteSchema.tags && concreteSchema.tags.description) {
    const desc = concreteSchema.tags.description;
    if (typeof desc === "string") {
      result.push(desc);
    } else {
      result.push(desc.long);  // Use long form for hover
    }
  }
}

// Format: **key**\n\ndescription
return {
  content: `**${navigationPath.slice(-1)[0]}**\n\n` + result.join("\n\n"),
  range: { ... }
};
```

### Schemas Used

| Context | Schema Source |
|---------|---------------|
| Frontmatter | `getFrontMatterSchema()` |
| Code cell options | `getEngineOptionsSchema()[engine]` |
| Project config | `getProjectConfigSchema()` |

## References

- Main LSP plan: `claude-notes/plans/2026-01-20-quarto-lsp.md`
- TS Quarto hover: `external-sources/quarto-cli/src/core/lib/yaml-intelligence/hover.ts`
- Rust schema types: `crates/quarto-yaml-validation/src/schema/types.rs`
