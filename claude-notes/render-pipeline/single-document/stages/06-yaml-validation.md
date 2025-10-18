## Stage 6: YAML Validation

**File:** `src/core/schema/validate-document.ts`
**Function:** `validateDocument(context)`

### What Happens

```typescript
const validate = context.format.render?.["validate-yaml"];
if (validate !== false) {
  const validationResult = await validateDocument(context);
  if (validationResult.length) {
    throw new RenderInvalidYAMLError();
  }
}
```

### 6.1 Schema Loading

The validator uses JSON schemas defined in `src/resources/schema/`:

```
resources/schema/
├── document-*.yml          # Document-level schemas
├── project-*.yml           # Project-level schemas
├── format-*.yml            # Format-specific schemas
└── definitions.yml         # Shared definitions
```

### 6.2 Validation Process

1. **Determine Schema**
   ```typescript
   const engineName = context.engine.name;
   const formatName = context.format.identifier[kTargetFormat];

   const schema = await getFrontMatterSchema(engineName, formatName);
   ```

2. **Validate Metadata**
   ```typescript
   const errors = [];

   // Validate document-level metadata
   errors.push(...validateAgainstSchema(context.target.metadata, schema));

   // Validate format-specific metadata
   for (const format of Object.keys(context.target.metadata.format || {})) {
     errors.push(...validateAgainstSchema(
       context.target.metadata.format[format],
       formatSchema(format)
     ));
   }
   ```

3. **Error Reporting**
   - Uses `MappedString` to report errors at correct source locations
   - Provides helpful error messages with YAML paths
   - Includes suggestions for common mistakes

### 6.3 Special Cases

**Expression Tags** (`!expr`):
```typescript
// YAML like: fig-cap: !expr paste("Hello", "World")
// Validator skips type checking for !expr tagged values

if (value.tag === "!expr") {
  return; // Skip validation
}
```

**Key Source Locations:**
- validateDocument: `src/core/schema/validate-document.ts`
- Schema definitions: `src/resources/schema/`

