## Stage 3: File Rendering Setup

**File:** `src/command/render/render-files.ts`
**Function:** `renderFiles()` → `renderFileInternal()`

### What Happens

1. **Progress Setup** (skipped for single file)
   ```typescript
   const progress = options.progress ||
     (project && (files.length > 1) && !options.flags?.quiet);
   ```

2. **Temp Context Creation**
   ```typescript
   const tempContext = createTempContext();
   ```
   - Creates temporary directory for intermediate files
   - Tracks all temporary files for cleanup
   - Located in system temp directory

3. **Per-File Rendering**
   - Creates a "lifetime" for resource management
   - Calls `renderFileInternal()` for each file
   - Single file → one iteration

4. **Pandoc Renderer**
   - Default renderer: `defaultPandocRenderer()`
   - Immediately renders each executed file
   - Collects completions for finalization

**Key Source Locations:**
- renderFiles: `src/command/render/render-files.ts:289`
- renderFileInternal: `src/command/render/render-files.ts:431`

