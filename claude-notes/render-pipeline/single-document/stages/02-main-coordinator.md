## Stage 2: Main Render Coordinator

**File:** `src/command/render/render-shared.ts`
**Function:** `render(path, options)`

### What Happens

1. **Initialize YAML Validation**
   ```typescript
   setInitializer(initYamlIntelligenceResourcesFromFilesystem);
   await initState();
   ```
   - Loads YAML schemas from filesystem
   - Prepares validation infrastructure
   - Required before any document parsing

2. **Project Context Detection**
   ```typescript
   let context = await projectContext(path, nbContext, options);
   ```
   - For single file: `context` will be `undefined` (no `_quarto.yml` found)
   - Searches up directory tree for `_quarto.yml` or `_quarto.yaml`

3. **Single File Project Context Creation**
   ```typescript
   context = await singleFileProjectContext(path, nbContext, options.flags);
   ```

   This creates a minimal "project" context for a standalone file:
   - `dir`: Directory containing the file
   - `config`: Empty or minimal configuration
   - `files`: Just the single input file
   - `isSingleFile`: `true` (important flag!)
   - Methods: `resolveFullMarkdownForFile()`, `fileExecutionEngineAndTarget()`, etc.

4. **Invoke File Rendering**
   ```typescript
   const result = await renderFiles(
     [{ path }],
     options,
     nbContext,
     undefined,        // alwaysExecuteFiles
     undefined,        // pandocRenderer (uses default)
     context,
   );
   ```

5. **Post-Render Engine Hook**
   ```typescript
   if (!renderResult.error && engine?.postRender) {
     for (const file of renderResult.files) {
       await engine.postRender(file, renderResult.context);
     }
   }
   ```

**Key Insight:** Even a "single file" render creates a minimal project context. This unifies the code path and allows single files to use project infrastructure (engines, extensions, etc.).

**Key Source Locations:**
- Main render: `src/command/render/render-shared.ts:38`
- Single file context: `src/project/types/single-file/single-file.ts`

