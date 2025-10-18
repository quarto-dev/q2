## Stage 1: CLI Entry Point

**File:** `src/command/render/cmd.ts`
**Function:** `renderCommand.action()`

### What Happens

1. **Argument Parsing** (cliffy Command framework)
   - Input file: `doc.qmd`
   - Flags: `--to`, `--output`, `--execute`, etc.
   - Pandoc arguments: everything after file name starting with `-`

2. **Flag Normalization**
   ```typescript
   // Handle edge cases like --foo=bar
   const normalizedArgs = [];
   for (const arg of args) {
     const equalSignIndex = arg.indexOf("=");
     if (equalSignIndex > 0 && arg.startsWith("-")) {
       normalizedArgs.push(arg.slice(0, equalSignIndex));
       normalizedArgs.push(arg.slice(equalSignIndex + 1));
     }
   }
   ```

3. **Parse Render Flags**
   - Extract Quarto-specific flags (execute, cache, freeze, etc.)
   - Separate from pandoc-specific arguments
   - `flags = await parseRenderFlags(args)`

4. **Create Services**
   ```typescript
   const services = renderServices(notebookContext());
   // Services include:
   // - temp: Temporary file management
   // - notebook: Jupyter notebook context
   // - extension: Extension loading
   ```

5. **Invoke Main Render**
   ```typescript
   renderResult = await render(renderResultInput, {
     services,
     flags,
     pandocArgs: args,
     useFreezer: flags.useFreezer === true,
     setProjectDir: true,
   });
   ```

**Key Source Locations:**
- Command definition: `src/command/render/cmd.ts:23`
- Action handler: `src/command/render/cmd.ts:129`
- Flag parsing: `src/command/render/flags.ts`

