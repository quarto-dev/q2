# Plan 2: @quarto/markdown + QuartoAPI Assembly

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** Phases 2A, 2B, 2D are independent. Phase 2C (wiring into engine-host) requires Plan 1a Phase 5 to have created the `@quarto/engine-host` package.
**Blocks:** Plan 4 (Julia Validation)
**Estimated sessions:** 1-2

## Overview

Create the `@quarto/markdown` TypeScript package with clean reimplementations of the markdown utilities that engine extensions need, and flesh out the QuartoAPI assembly in `@quarto/engine-host` that Plan 1a stubbed.

This plan covers everything in the QuartoAPI except `quarto.jupyter` (which is Plan 3) and the stub infrastructure (which is Plan 1a).

## Work Items

### Phase 2A: @quarto/markdown package

Clean reimplementations of markdown utilities. These are the functions exposed via `quarto.markdownRegex.*`.

- [ ] Create `ts-packages/quarto-markdown/package.json`:
  ```json
  {
    "name": "@quarto/markdown",
    "version": "0.1.0",
    "type": "module",
    "main": "src/index.ts",
    "dependencies": { "yaml": "^2.0.0" }
  }
  ```

- [ ] Implement `src/extract-yaml.ts` — `extractYaml(markdown: string) → Metadata`:
  - Find YAML front matter between `---` delimiters
  - Parse with `yaml` package
  - Support the `!expr` YAML tag (Quarto convention: treat as raw string)
  - Handle edge cases: no front matter, empty front matter, malformed YAML
  - Reference: Quarto 1's `readYamlFromMarkdown` in `src/core/yaml.ts`
  - ~50 lines + tests

- [ ] Implement `src/partition.ts` — `partition(markdown: string) → PartitionedMarkdown`:
  - Split markdown into: yaml front matter, heading (first heading if present), body
  - Uses `extractYaml` for the YAML part
  - Parse Pandoc-style attributes on code blocks (`pandocAttrParseText`)
  - Reference: Quarto 1's `partitionMarkdown` in `src/core/pandoc/pandoc-partition.ts`
  - ~200 lines + tests

- [ ] Implement `src/languages.ts` — `getLanguages(markdown: string) → Set<string>`:
  - Extract language specifiers from fenced code blocks via regex
  - Match `` ```{language} `` patterns
  - Pure regex, zero dependencies
  - Reference: Quarto 1's `languagesInMarkdown` — literally a copy, it's self-contained
  - ~30 lines + tests

- [ ] Implement `src/break-quarto-md.ts` — `breakQuartoMd(markdown: string) → QuartoMdCell[]`:
  - Split markdown into alternating code cells and markdown cells
  - Parse cell options from YAML comments within code blocks
  - **Simplified from Quarto 1**: use `yaml` package directly for cell options, no schema validation, no tree-sitter
  - Handle: fenced code blocks, shortcodes, raw blocks
  - Reference: Quarto 1's `breakQuartoMd` in `src/core/lib/break-quarto-md.ts`
  - Note: the Julia engine does NOT use this method, but the engine template does and other engines will
  - ~300 lines + tests

- [ ] Create `src/index.ts` that re-exports all functions
- [ ] Write comprehensive tests for each function. Check existing ts-packages for the test runner convention (likely Vitest or Deno test). If no convention exists, use Deno test (`deno test`) since the engine-host runs in Deno.
- [ ] Add to workspace in root `package.json` (should be automatic via `ts-packages/*` glob). Run `npm install` from the repo root after creating the package.

### Phase 2B: MappedString implementation

The `quarto.mappedString` namespace provides MappedString — the same concept
as q2's `SourceInfo`, but as a TypeScript type matching Quarto 1's interface.

Two construction paths:
1. **From source_map** (primary): The engine-host harness constructs a
   MappedString with full `.map()` provenance from the `source_map`
   byte-range entries in `TsExecuteOptions`. This is implemented in
   Plan 1a Phase 5 (harness), not here.
2. **From API calls** (secondary): Engines may also call
   `quarto.mappedString.fromFile()` or `fromString()` for their own
   purposes (e.g., reading additional files during execution). These
   create base MappedStrings with identity mapping.

- [ ] Implement `MappedString` type in `@quarto/engine-host`:
  ```typescript
  interface MappedString {
      value: string;
      fileName?: string;
      map(index: number, closest?: boolean):
          { index: number, originalString: MappedString } | undefined;
  }
  ```

- [ ] Implement `fromSourceMap(text, sourcePath, sourceMap)` — constructs
  a MappedString with `.map()` that binary-searches the byte-range pieces
  and returns references to per-file base MappedStrings. See Plan 1a
  Phase 5 for the algorithm. This is the harness-internal function used
  to build the MappedString for `options.target.markdown`.

- [ ] Implement namespace methods (QuartoAPI surface):
  - `fromString(text, fileName?) → MappedString` — base MappedString (identity mapping)
  - `fromFile(path) → MappedString` — `Deno.readTextFileSync(path)` + filename
  - `normalizeNewlines(ms) → MappedString` — replace `\r\n` with `\n`
  - `splitLines(ms) → MappedString[]` — split on newlines, each line's `.map()` delegates to parent
  - `indexToLineCol(ms, offset) → { line, column }` — offset to line/col

- [ ] Write tests — including `.map()` through source_map pieces

### Phase 2C: Remaining QuartoAPI namespaces

Flesh out the stub `quarto-api.ts` from Plan 1a Phase 5 with real implementations for all namespaces except `jupyter`.

#### quarto.path
- [ ] `absolute(path)` → `Deno.realPathSync(path)`
- [ ] `runtime(subdir)` → `join(context.runtimeDir, subdir)`
- [ ] `resource(...parts)` → `join(context.resourceDir, ...parts)`
- [ ] `toForwardSlashes(path)` → `path.replace(/\\/g, "/")`
- [ ] `dirAndStem(file)` → `[dirname(file), basename(file, extname(file))]`
- [ ] `inputFilesDir(input)` → `join(dirname(input), basename(input, ext) + "_files")`
- [ ] `isQmdFile(file)` → check extension

#### quarto.format
- [ ] Methods accept a format parameter for API compatibility with Quarto 1, but compute results from the format's `pandoc.to` string:
  ```typescript
  isHtmlCompatible: (format) => {
      const to = format?.pandoc?.to ?? context.format.pandocTo;
      return ["html", "html4", "html5", "revealjs", "s5", "slideous", "slidy",
              "epub", "epub2", "epub3"].some(f => to.startsWith(f));
  },
  isLatexOutput: (format) => {
      const to = format?.pandoc?.to ?? context.format.pandocTo;
      return ["latex", "beamer", "pdf"].some(f => to.startsWith(f));
  },
  // etc.
  ```
- [ ] Fallback to `context.format.pandocTo` when no format argument is passed (some engines call without arguments)

#### quarto.system
- [ ] `isInteractiveSession()` → `context.isInteractiveSession`
- [ ] `runningInCI()` → `context.runningInCI`
- [ ] `execProcess(options)` → wrap `Deno.Command`:
  ```typescript
  async execProcess(options) {
      const cmd = new Deno.Command(options.cmd[0], {
          args: options.cmd.slice(1),
          stdin: options.stdin ? "piped" : "null",
          stdout: "piped",
          stderr: "piped",
          env: options.env,
          cwd: options.cwd,
      });
      // ...
  }
  ```
- [ ] `pandoc(args, stdin?)` → `execProcess` with `context.pandocPath` as the command
- [ ] `tempContext()` → create temp dir, return cleanup helper
- [ ] `onCleanup(handler)` → register cleanup callback

#### quarto.console
- [ ] `info(message, options?)` → `console.error("[INFO]", message)`. The `options` parameter (`{ bold, newline, indent, ... }`) is accepted but formatting hints are best-effort in a subprocess context (no terminal control).
- [ ] `warning(message, options?)` → `console.error("[WARN]", message)`
- [ ] `error(message, options?)` → `console.error("[ERROR]", message)`
- [ ] `withSpinner(options, fn)` → log start/end messages, call fn (no actual spinner in subprocess)

#### quarto.crypto
- [ ] `md5Hash(content)` → use Web Crypto API or a small dependency. Note: Web Crypto doesn't natively support MD5. Options: use `npm:md5`, use `node:crypto` (available in Deno), or a pure-JS MD5.

#### quarto.text
- [ ] `lines(text)` → `text.split("\n")`
- [ ] `trimEmptyLines(lines, trim)` → filter empty lines from start/end
- [ ] `postProcessRestorePreservedHtml(options)` → replace preservation markers with original HTML
- [ ] `executeInlineCodeHandler(language, exec)` → (stub for now, used by knitr not julia)
- [ ] `asYamlText(metadata)` → `yaml.dump(metadata)`

- [ ] Wire all namespaces into `quarto-api.ts`
- [ ] Write tests for each namespace

### Phase 2D: @quarto/types and import map

Following Quarto 1's model, engine extensions import types via `import type { ... } from "@quarto/types"`. These are erased during the build step (bundling), so no runtime code is needed — just a `.d.ts` file referenced by the import map.

- [ ] Define our type definitions in `ts-packages/quarto-types/` (or `resources/extension-build/quarto-types.d.ts`):
  - `ExecutionEngineDiscovery`, `ExecutionEngineInstance`
  - `ExecuteOptions`, `ExecuteResult`, `ExecutionTarget`
  - `QuartoAPI` (with our namespace signatures)
  - `MappedString`, `PartitionedMarkdown`, `Metadata`
  - `EngineProjectContext`
- [ ] For compatibility with Quarto 1 extensions: our type names should match Quarto 1's
- [ ] Create `resources/extension-build/import-map.json`:
  ```json
  {
    "imports": {
      "@quarto/types": "./quarto-types.d.ts",
      "path": "jsr:@std/path",
      "fs/exists": "jsr:@std/fs/exists",
      "encoding/base64": "jsr:@std/encoding/base64"
    }
  }
  ```
- [ ] Create `resources/extension-build/deno.json`:
  ```json
  {
    "compilerOptions": { "strict": true, "lib": ["deno.ns", "DOM", "ES2021"] },
    "importMap": "./import-map.json"
  }
  ```
- [ ] Copy `quarto-types.d.ts` into `resources/extension-build/` during the build process

## Design Notes

### Why rewrite instead of extract?

Quarto 1's markdown utilities are tangled with the YAML schema/validation system (~30+ files), tree-sitter, mapped-text infrastructure, and lodash. Clean rewrites of the actual logic are ~50-300 lines per function, vs. extracting would require bringing 30+ files and stubbing their dependencies. The logic itself is straightforward — it's the plumbing that's tangled.

### YAML cell options: simplified approach

Quarto 1's `partitionCellOptions` uses the full YAML schema system to validate cell options. Our `breakQuartoMd` skips validation and just parses YAML with `js-yaml`. This means:
- Cell options with typos won't be caught at parse time
- That's fine — validation happens elsewhere in q2's pipeline
- The engine extension just needs the parsed options as a plain object

### Future: Quarto 1 adoption

These packages are designed so that Quarto 1 could eventually import them, replacing its tangled implementations. The API signatures match Quarto 1's existing interfaces. If/when Quarto 1 adopts them, it gains cleaner code and shared maintenance.

## Success Criteria

- [ ] `@quarto/markdown` package with extractYaml, partition, getLanguages, breakQuartoMd
- [ ] All QuartoAPI namespaces except jupyter implemented and tested
- [ ] MappedString with full `.map()` provenance (from source_map and from API calls)
- [ ] `fromSourceMap` reconstructs provenance from byte-range entries
- [ ] Types defined for engine extension imports
- [ ] All tests pass
