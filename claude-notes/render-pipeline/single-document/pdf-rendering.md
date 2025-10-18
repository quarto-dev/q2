## PDF Rendering: Key Differences from HTML

**What happens when `format: pdf` is specified instead of `format: html`?**

The pipeline stages 1-9 are largely identical, but **Stage 9 (Pandoc Conversion) and Stage 10 (Postprocessing)** differ significantly for PDF output.

### PDF Output Recipe Selection

**File:** `src/command/render/output.ts`
**Function:** `outputRecipe(context)`

The output recipe is selected based on format type:

```typescript
export function outputRecipe(context: RenderContext): OutputRecipe {
  const format = context.format;

  if (useQuartoLatexmk(format, options.flags)) {
    return quartoLatexmkOutputRecipe(input, output, options, format);
  } else if (useContextPdfOutputRecipe(format, options.flags)) {
    return contextPdfOutputRecipe(input, output, options, format);
  } else if (useTypstPdfOutputRecipe(format)) {
    return typstPdfOutputRecipe(input, output, options, format, context.project);
  } else {
    // Default recipe (HTML, DOCX, etc.)
    return defaultOutputRecipe(input, output, options, format);
  }
}
```

**When is LaTeX/PDF recipe used?**
```typescript
function useQuartoLatexmk(format: Format, flags?: RenderFlags) {
  const to = format.pandoc.to;
  const ext = format.render[kOutputExt] || "html";

  // If explicitly disabled
  if (format.render[kLatexAutoMk] === false) {
    return false;
  }

  // If creating PDF output via LaTeX
  if (["beamer", "pdf"].includes(to || "") && ext === "pdf") {
    const engine = pdfEngine(format.pandoc, format.render, flags);
    return isLatexPdfEngine(engine);  // lualatex, pdflatex, xelatex, tectonic
  }

  return false;
}
```

### PDF Format Defaults

**File:** `src/format/pdf/format-pdf.ts`
**Function:** `createPdfFormat()`

PDF format provides different defaults than HTML:

```typescript
export function createPdfFormat(): Format {
  return {
    execute: {
      [kFigWidth]: 5.5,      // Narrower than HTML (7)
      [kFigHeight]: 3.5,     // Smaller than HTML (5)
      [kFigFormat]: "pdf",   // Vector graphics
      [kFigDpi]: 300,        // High DPI for print
    },
    pandoc: {
      [kPdfEngine]: "lualatex",   // Default LaTeX engine
      standalone: true,
      variables: {
        graphics: true,
        tables: true,
      },
    },
    render: {
      [kOutputExt]: "pdf",
      [kPageWidth]: 6.5,     // US Letter minus margins
      [kKeepTex]: false,     // Delete .tex after compilation
      [kLatexAutoMk]: true,  // Use quarto's latexmk
      [kLatexAutoInstall]: true,  // Auto-install missing packages (TinyTeX)
      [kLatexClean]: true,   // Remove auxiliary files
      [kLatexMinRuns]: 1,
      [kLatexMaxRuns]: 10,
    },
  };
}
```

### PDF Format Extras

PDF format provides a `formatExtras` function that returns:

```typescript
formatExtras: async (...args): Promise<FormatExtras> => {
  return {
    // PDF-specific postprocessor (runs on .tex file)
    postprocessors: [pdfLatexPostProcessor],

    // Template context with LaTeX partials
    templateContext: {
      template: pdfTemplate,
      partials: [
        "doc-class.tex",
        "graphics.tex",
        "tables.tex",
        "title.tex",
        // ...
      ],
    },

    // Metadata defaults
    metadata: {
      [kDocumentClass]: "scrartcl",  // KOMA-Script article
    },
  };
}
```

### LaTeX Postprocessor

**File:** `src/format/pdf/format-pdf.ts`
**Function:** `pdfLatexPostProcessor(output: string)`

The LaTeX postprocessor modifies the `.tex` file **line-by-line** with multiple passes:

```typescript
async function pdfLatexPostProcessor(output: string): Promise<PostprocessResult> {
  const texPath = output;  // e.g., "doc.tex"
  let lines = Deno.readTextFileSync(texPath).split("\n");

  // Pass 1: Sidecaption processing
  lines = processSidecaptions(lines);

  // Pass 2: Callout float handling
  lines = processCalloutFloats(lines);

  // Pass 3: Table column margins
  lines = processTableMargins(lines);

  // Pass 4: GUID replacement (for cross-references)
  lines = replaceGuids(lines);

  // Pass 5: Bibliography processing (biblatex, natbib, citeproc)
  lines = processBibliography(lines, format);

  // Pass 6: Margin citations
  lines = processMarginCitations(lines);

  // Pass 7: Footnote → sidenote conversion
  lines = convertFootnotesToSidenotes(lines);

  // Pass 8: Code annotation processing
  lines = processCodeAnnotations(lines);

  // Pass 9: Caption footnote extraction
  lines = extractCaptionFootnotes(lines);

  // Write modified .tex file
  Deno.writeTextFileSync(texPath, lines.join("\n"));

  return { supporting: [], resources: [] };
}
```

**Example transformations:**

1. **Margin Citations:**
   ```latex
   % Before:
   \cite[see][p. 10]{smith2020}

   % After (for margin citations):
   \margincite[see][p. 10]{smith2020}
   ```

2. **Footnote → Sidenote:**
   ```latex
   % Before:
   Some text\footnote{This is a note}.

   % After (if sidenotes enabled):
   Some text\sidenote{This is a note}.
   ```

3. **Caption Footnotes:**
   ```latex
   % Before:
   \caption{My figure\footnote{Source: ...}}

   % After:
   \caption{My figure\footnotemark}
   \footnotetext{Source: ...}
   ```

### LaTeX Output Recipe

**File:** `src/command/render/latexmk/latexmk.ts`
**Function:** `quartoLatexmkOutputRecipe()`

The PDF recipe is fundamentally different from HTML:

```typescript
export function quartoLatexmkOutputRecipe(
  input: string,
  finalOutput: string,
  options: RenderOptions,
  format: Format,
): OutputRecipe {
  const outputDir = format.render[kLatexOutputDir];

  const generate = async (
    input: string,
    format: Format,
    pandocOptions: PandocOptions,
  ): Promise<string> => {
    const mkOptions: LatexmkOptions = {
      input,
      engine: pdfEngine(format.pandoc, format.render, pandocOptions.flags),
      autoInstall: format.render[kLatexAutoInstall],
      minRuns: format.render[kLatexMinRuns],
      maxRuns: format.render[kLatexMaxRuns],
      tinyTex: format.render[kLatexTinyTex],
      texInputDirs: format.render[kLatexInputPaths] || [],
      outputDir: outputDir,
      clean: !options.flags?.debug && format.render[kLatexClean] !== false,
      quiet: pandocOptions.flags?.quiet,
    };

    // Run latexmk
    return generatePdf(mkOptions);
  };

  const computePath = (texStem: string, inputDir: string, format: Format) => {
    const mkOutputdir = format.render[kLatexOutputDir];
    return mkOutputdir
      ? join(mkOutputdir, texStem + ".pdf")
      : join(inputDir, texStem + ".pdf");
  };

  return texToPdfOutputRecipe(
    input,
    finalOutput,
    options,
    format,
    "latex",  // Pandoc should output LaTeX
    { generate, computePath },
    outputDir,
  );
}
```

### TeX to PDF Recipe

**File:** `src/command/render/output-tex.ts`
**Function:** `texToPdfOutputRecipe()`

This recipe encapsulates the LaTeX → PDF workflow:

```typescript
export function texToPdfOutputRecipe(
  input: string,
  finalOutput: string,
  options: RenderOptions,
  format: Format,
  pdfIntermediateTo: string,  // "latex"
  pdfGenerator: PdfGenerator,
  pdfOutputDir?: string | null,
): OutputRecipe {
  const [inputDir, inputStem] = dirAndStem(input);

  // Create tex-safe filename (avoid LaTeX-unfriendly characters)
  const texStem = texSafeFilename(inputStem);
  const output = texStem + ".tex";

  // The complete() hook runs AFTER pandoc
  const complete = async (pandocOptions: PandocOptions) => {
    const input = join(inputDir, output);  // e.g., "doc.tex"

    // Run latexmk to compile .tex → .pdf
    const pdfOutput = await pdfGenerator.generate(input, format, pandocOptions);

    // Keep .tex if requested
    const compileTex = join(inputDir, output);
    if (!format.render[kKeepTex]) {
      safeRemoveSync(compileTex);
    }

    // Copy or write PDF to final output location
    if (finalOutput) {
      if (finalOutput === kStdOut) {
        writeFileToStdout(pdfOutput);
        safeRemoveSync(pdfOutput);
      } else {
        const outputPdf = expandPath(finalOutput);
        if (normalize(pdfOutput) !== normalize(outputPdf)) {
          Deno.renameSync(pdfOutput, outputPdf);
        }
      }
      return normalizeOutputPath(input, finalOutput);
    } else {
      return normalizeOutputPath(input, pdfOutput);
    }
  };

  // Tweak Pandoc writer (use "latex" instead of "pdf")
  const to = format.pandoc.to === "pdf" ? pdfIntermediateTo : format.pandoc.to;

  return {
    output,           // "doc.tex"
    keepYaml: false,
    args: options.pandocArgs || [],
    format: {
      ...format,
      pandoc: {
        ...format.pandoc,
        to,  // "latex" instead of "pdf"
      },
    },
    complete,
    finalOutput: pdfOutput ? relative(inputDir, pdfOutput) : undefined,
  };
}
```

### PDF Generation (Latexmk)

**File:** `src/command/render/latexmk/pdf.ts`
**Function:** `generatePdf(mkOptions: LatexmkOptions)`

This is the core PDF compilation logic, handling multiple LaTeX runs:

```typescript
export async function generatePdf(mkOptions: LatexmkOptions): Promise<string> {
  if (!mkOptions.quiet) {
    logProgress("\nRendering PDF");
    logProgress(`running ${mkOptions.engine.pdfEngine} - 1`);
  }

  const [cwd, stem] = dirAndStem(mkOptions.input);
  const workingDir = mkOptions.outputDir ? join(cwd, mkOptions.outputDir) : cwd;

  // Ensure working directory exists
  if (!existsSync(workingDir)) {
    Deno.mkdirSync(workingDir);
  } else {
    cleanup(workingDir, stem);  // Clean auxiliary files
  }

  // Determine if auto-install is available (TexLive)
  const allowUpdate = await hasTexLive();
  mkOptions.autoInstall = mkOptions.autoInstall && allowUpdate;

  // Create TexLive context
  const texLive = await texLiveContext(mkOptions.tinyTex !== false);
  const pkgMgr = packageManager(mkOptions, texLive);

  // PASS 1: Initial compilation
  const response = await initialCompileLatex(
    mkOptions.input,
    mkOptions.engine,
    pkgMgr,
    texLive,
    mkOptions.outputDir,
    mkOptions.texInputDirs,
    mkOptions.quiet,
  );
  const initialCompileNeedsRerun = needsRecompilation(response.log);

  // PASS 2: Generate index (if .idx file exists)
  const indexIntermediateFile = indexIntermediate(workingDir, stem);
  let indexCreated = false;
  if (indexIntermediateFile) {
    info("  Re-compiling document for index");
    await runPdfEngine(mkOptions.input, mkOptions.engine, texLive, ...);

    indexCreated = await makeIndexIntermediates(
      indexIntermediateFile,
      pkgMgr,
      texLive,
      mkOptions.engine.indexEngine,
      mkOptions.engine.indexEngineOpts,
      mkOptions.quiet,
    );
  }

  // PASS 3: Generate bibliography (if .aux/.bcf file exists)
  const bibliographyCreated = await makeBibliographyIntermediates(
    mkOptions.input,
    mkOptions.engine.bibEngine || "citeproc",
    pkgMgr,
    texLive,
    mkOptions.outputDir,
    mkOptions.texInputDirs,
    mkOptions.quiet,
  );

  // PASS 4: Recompile until complete (or max runs)
  const minRuns = (mkOptions.minRuns || 1) - 1;
  const maxRuns = (mkOptions.maxRuns || 10) - 1;
  if (
    (indexCreated || bibliographyCreated || minRuns || initialCompileNeedsRerun) &&
    maxRuns > 0
  ) {
    await recompileLatexUntilComplete(
      mkOptions.input,
      mkOptions.engine,
      pkgMgr,
      mkOptions.minRuns || 1,
      maxRuns,
      texLive,
      mkOptions.outputDir,
      mkOptions.texInputDirs,
      mkOptions.quiet,
    );
  }

  // Cleanup auxiliary files
  if (mkOptions.clean) {
    cleanup(workingDir, stem);
  }

  return mkOptions.outputDir
    ? join(mkOptions.outputDir, stem + ".pdf")
    : join(cwd, stem + ".pdf");
}
```

**What `recompileLatexUntilComplete` does:**

```typescript
async function recompileLatexUntilComplete(
  input: string,
  engine: PdfEngine,
  pkgMgr: PackageManager,
  minRuns: number,
  maxRuns: number,
  texLive: TexLiveContext,
  outputDir?: string,
  texInputDirs?: string[],
  quiet?: boolean,
) {
  let runCount = 0;
  minRuns = minRuns - 1;  // Already ran once

  while (true) {
    if (runCount >= maxRuns) {
      warning(`maximum number of runs (${maxRuns}) reached`);
      break;
    }

    if (!quiet) {
      logProgress(`running ${engine.pdfEngine} - ${runCount + 2}`);
    }

    const result = await runPdfEngine(input, engine, texLive, ...);

    if (!result.result.success) {
      displayError("Error compiling latex", result.log, result.result);
      return Promise.reject();
    } else {
      runCount++;

      // Check if recompilation is needed
      if (
        (existsSync(result.log) && needsRecompilation(result.log)) ||
        runCount < minRuns
      ) {
        continue;  // Run again
      }
      break;  // Done
    }
  }
}
```

**When is recompilation needed?**

The log file is parsed for indicators:

```typescript
function needsRecompilation(logFile: string): boolean {
  const logText = Deno.readTextFileSync(logFile);

  // LaTeX warnings that require recompilation:
  const indicators = [
    "Rerun to get cross-references right",
    "Rerun to get citations correct",
    "Rerun to get outlines right",
    "There were undefined references",
    "Label(s) may have changed",
  ];

  return indicators.some(indicator => logText.includes(indicator));
}
```

### Auto-Installation of Missing Packages

**File:** `src/command/render/latexmk/pdf.ts`
**Function:** `initialCompileLatex()`

If compilation fails due to missing packages, Quarto can auto-install them:

```typescript
async function initialCompileLatex(
  input: string,
  engine: PdfEngine,
  pkgMgr: PackageManager,
  texLive: TexLiveContext,
  outputDir?: string,
  texInputDirs?: string[],
  quiet?: boolean,
) {
  let packagesUpdated = false;

  while (true) {
    // Run PDF engine
    const response = await runPdfEngine(input, engine, texLive, ...);
    const success = response.result.code === 0 && existsSync(response.output);

    if (success) {
      // Check for hyphenation warnings
      const logText = Deno.readTextFileSync(response.log);
      const missingHyphenationFile = findMissingHyphenationFiles(logText);
      if (missingHyphenationFile && pkgMgr.autoInstall) {
        if (await pkgMgr.installPackages([missingHyphenationFile])) {
          continue;  // Retry
        }
      }
      return Promise.resolve(response);

    } else if (pkgMgr.autoInstall) {
      // Update package manager first time
      if (!packagesUpdated) {
        if (!quiet) logProgress("updating tlmgr");
        await pkgMgr.updatePackages(false, true);

        if (!quiet) logProgress("updating existing packages");
        await pkgMgr.updatePackages(true, false);
        packagesUpdated = true;
      }

      // Find and install missing packages
      const packagesInstalled = await findAndInstallPackages(
        pkgMgr,
        response.log,
        response.result.stderr,
        quiet,
      );

      if (packagesInstalled) {
        continue;  // Retry
      } else {
        displayError("missing packages (automatic installation failed)", ...);
        return Promise.reject();
      }
    } else {
      displayError("missing packages (automatic installed disabled)", ...);
      return Promise.reject();
    }
  }
}
```

### Pipeline Comparison: HTML vs PDF

| Stage | HTML | PDF |
|-------|------|-----|
| **1-8** | Identical | Identical |
| **9. Pandoc** | `pandoc --to html` | `pandoc --to latex` |
| **Output** | `.html` file | `.tex` file |
| **9.5 Postprocessors** | HTML postprocessors (DOM manipulation) | LaTeX postprocessor (line-by-line text manipulation) |
| **10. Recipe Complete** | No additional processing | **Runs latexmk** |
| **10.1 Bibliography** | N/A (handled by Pandoc citeproc) | Runs bibtex/biber if needed |
| **10.2 Index** | N/A | Runs makeindex if needed |
| **10.3 Compilation** | N/A | Runs lualatex/pdflatex 1-10 times |
| **10.4 Auto-install** | N/A | Installs missing LaTeX packages via TinyTeX |
| **10.5 Cleanup** | Removes temp files | Removes .aux, .log, .toc, .out, .bbl, etc. |
| **Final Output** | `.html` file | `.pdf` file |

### Key Insight: PDF as Multi-Stage Compilation

Unlike HTML (which is a single Pandoc invocation followed by DOM postprocessing), PDF rendering is a **multi-stage compilation process**:

```
Markdown
  → Pandoc → .tex file
  → LaTeX postprocessor → modified .tex file
  → lualatex run 1 → .aux, .log, .pdf (draft)
  → bibtex/biber → .bbl (bibliography)
  → makeindex → .ind (index)
  → lualatex run 2 → .pdf (with refs)
  → lualatex run 3 → .pdf (stable cross-refs)
  → ...
  → lualatex run N → .pdf (final)
```

Each LaTeX run reads auxiliary files (`.aux`, `.toc`, `.lof`, `.lot`) and may update them, requiring subsequent runs until the output stabilizes.

### Implications for Rust Port

1. **LaTeX Postprocessing Needs Line-by-Line Text Manipulation**
   ```rust
   // Unlike HTML (which needs DOM parsing), PDF needs string processing
   pub fn pdf_latex_postprocessor(tex_path: &Path, format: &Format) -> Result<()> {
     let content = std::fs::read_to_string(tex_path)?;
     let mut lines: Vec<String> = content.lines().map(String::from).collect();

     lines = process_sidecaptions(lines);
     lines = process_callout_floats(lines);
     lines = process_bibliography(lines, format);
     lines = convert_footnotes_to_sidenotes(lines);
     // ...

     std::fs::write(tex_path, lines.join("\n"))?;
     Ok(())
   }
   ```

2. **PDF Recipe Must Handle Multi-Run Compilation**
   ```rust
   pub struct PdfRecipe {
     tex_file: PathBuf,
     engine: PdfEngine,
     min_runs: usize,
     max_runs: usize,
   }

   impl OutputRecipe for PdfRecipe {
     async fn complete(&self, options: PandocOptions) -> Result<PathBuf> {
       // Run latexmk
       let pdf_path = generate_pdf(LatexmkOptions {
         input: &self.tex_file,
         engine: &self.engine,
         min_runs: self.min_runs,
         max_runs: self.max_runs,
         auto_install: true,
         clean: true,
       }).await?;

       // Remove .tex unless keep-tex is true
       if !options.format.render.keep_tex {
         std::fs::remove_file(&self.tex_file)?;
       }

       Ok(pdf_path)
     }
   }
   ```

3. **Log File Parsing for Recompilation Detection**
   ```rust
   pub fn needs_recompilation(log_path: &Path) -> Result<bool> {
     let log_text = std::fs::read_to_string(log_path)?;

     const INDICATORS: &[&str] = &[
       "Rerun to get cross-references right",
       "Rerun to get citations correct",
       "There were undefined references",
       "Label(s) may have changed",
     ];

     Ok(INDICATORS.iter().any(|ind| log_text.contains(ind)))
   }
   ```

4. **Package Manager Integration**
   ```rust
   pub struct TinyTexPackageManager {
     tinytex_bin: PathBuf,
   }

   impl PackageManager for TinyTexPackageManager {
     async fn install_packages(&self, packages: &[String]) -> Result<bool> {
       for package in packages {
         let result = tokio::process::Command::new(&self.tinytex_bin)
           .arg("install")
           .arg(package)
           .output()
           .await?;

         if !result.status.success() {
           return Ok(false);
         }
       }
       Ok(true)
     }
   }
   ```

5. **Auxiliary File Cleanup**
   ```rust
   pub fn cleanup_latex_artifacts(working_dir: &Path, stem: &str) -> Result<()> {
     const AUX_EXTENSIONS: &[&str] = &[
       "log", "aux", "idx", "ind", "ilg", "toc", "lof", "lot",
       "bcf", "blg", "bbl", "fls", "out", "nav", "snm", "vrb",
       "xwm", "brf", "run.xml",
     ];

     for ext in AUX_EXTENSIONS {
       let aux_file = working_dir.join(format!("{}.{}", stem, ext));
       if aux_file.exists() {
         std::fs::remove_file(aux_file)?;
       }
     }

     // Also cleanup missfont.log
     let missfont = working_dir.join("missfont.log");
     if missfont.exists() {
       std::fs::remove_file(missfont)?;
     }

     Ok(())
   }
   ```

**Key Source Locations:**
- PDF format: `src/format/pdf/format-pdf.ts`
- LaTeX recipe: `src/command/render/latexmk/latexmk.ts`
- TeX to PDF: `src/command/render/output-tex.ts`
- PDF generation: `src/command/render/latexmk/pdf.ts`
- Output recipe selection: `src/command/render/output.ts`

