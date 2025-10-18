# Book Project Rendering Analysis

**Analysis Date**: October 11, 2025
**Analyzed Command**: `quarto render` from directory with `_quarto.yml` containing `project: type: book`

## Executive Summary

Book projects in Quarto **inherit from website projects** and add book-specific functionality. The key innovation is **dual rendering modes**: multi-file (HTML, each chapter separate) vs single-file (PDF/EPUB/DOCX, all chapters merged). Books get all website features (navigation, search, sitemap) plus chapter management, automatic numbering, cross-references, and unified bibliography.

**Critical Design Principle**: Book projects use `inheritsType: websiteProjectType.type`, meaning they delegate most functionality to the website project type and only override/extend specific behaviors.

### Comparison with Other Project Types

| Feature | Single Document | Website | Book |
|---------|----------------|---------|------|
| Project Context | None | Yes | Yes |
| Pre-Render Hook | N/A | Navigation init | Navigation init (inherited) |
| Per-File Rendering | Independent | Shared nav state | **Two modes**: multi-file OR single-file |
| Chapter Management | N/A | N/A | **Chapters, parts, appendices** |
| Automatic Numbering | Optional | N/A | **Yes** (1, 2, 3... or A, B, C) |
| Cross-references | File-scoped | File-scoped | **Project-wide** (post-render fixup) |
| Bibliography | File-scoped | File-scoped | **Project-wide** (post-render fixup) |
| Post-Render Hook | N/A | Sitemap, search, listings | **+ crossrefs, bibliography** |
| Output Modes | 1 (single file) | 1 (multi-file) | **2 (multi + single)** |

---

## Stage 0: Project Detection

**Same as website projects** - see `website-project-rendering.md` Stage 0.

---

## Stage 1: Pre-Render Hook

**Delegated to website project type**:

```typescript
function bookPreRender(context: ProjectContext): Promise<void> {
  // Special date handling (if date is "today" or "last-modified", resolve it)
  const date = bookConfig(kDate, context.config);
  if (context.config && isSpecialDate(date)) {
    setBookConfig(
      kDate,
      parseSpecialDate(context.files.input, date),
      context.config,
    );
  }

  // Delegate to website pre-render (builds navigation state)
  if (websiteProjectType.preRender) {
    return websiteProjectType.preRender(context);
  } else {
    return Promise.resolve();
  }
}
```

**What happens**:
1. Resolve special dates (e.g., `date: today` → actual date)
2. Call `websiteProjectType.preRender()` to build global navigation state (navbar, sidebar, footer)

**Book-specific config translation** happens in `bookProjectConfig`:

```typescript
export async function bookProjectConfig(
  project: ProjectContext,
  config: ProjectConfig,
) {
  // Copy book config into website config
  const site = (config[kWebsite] || {}) as Record<string, unknown>;
  const book = config[kBook] as Record<string, unknown>;

  if (book) {
    site[kSiteTitle] = book[kSiteTitle];
    site[kSiteFavicon] = book[kSiteFavicon];
    site[kSiteUrl] = book[kSiteUrl];
    site[kSiteRepoUrl] = book[kSiteRepoUrl];
    // ... 15+ more fields copied
  }

  // Build sidebar from book chapters
  site[kSiteSidebar] = site[kSiteSidebar] || {};
  const siteSidebar = site[kSiteSidebar] as Metadata;
  siteSidebar[kContents] = [];

  const bookContents = bookConfig(kBookChapters, config);
  if (Array.isArray(bookContents)) {
    siteSidebar[kContents] = bookChaptersToSidebarItems(bookContents);
  }

  const bookAppendix = bookConfig(kBookAppendix, config);
  if (Array.isArray(bookAppendix)) {
    siteSidebar[kContents].concat([
      chapterToSidebarItem({
        part: language[kSectionTitleAppendices],
        chapters: bookAppendix,
      }),
    ]);
  }

  // Build render list with chapter numbers and types
  const renderItems = await bookRenderItems(project, language, config);
  book[kBookRender] = renderItems;  // Save detailed list
  config.project[kProjectRender] = renderItems
    .filter((target) => !!target.file)
    .map((target) => target.file!);  // Extract file list

  // Add download/sharing tools to sidebar
  const tools = [];
  tools.push(...downloadTools(projectDir, config, language));
  tools.push(...sharingTools(config, language));
  siteSidebar[kBookTools] = tools;

  // Delegate to website config (which handles navbar, sidebar, etc.)
  return await websiteProjectConfig(project, config);
}
```

**Key data structure: BookRenderItem[]**

```typescript
interface BookRenderItem {
  type: "index" | "chapter" | "appendix" | "part";
  depth: number;
  text?: string;        // Part titles
  file?: string;        // Chapter file paths
  number?: number;      // Chapter numbers (1, 2, 3... or undefined for unnumbered)
}

// Example render items for a 4-chapter book with appendix:
[
  { type: "index", file: "index.md", depth: 0, number: undefined },
  { type: "chapter", file: "intro.md", depth: 0, number: 1 },
  { type: "chapter", file: "methods.md", depth: 0, number: 2 },
  { type: "chapter", file: "results.md", depth: 0, number: 3 },
  { type: "appendix", text: "Appendices", depth: 0 },  // Divider
  { type: "chapter", file: "appendix-a.md", depth: 1, number: 1 },  // Letter A
]
```

**Timing**: 100-500ms (parse chapters, build navigation, construct render list)

---

## Stages 2-10: File Rendering

**MAJOR DIVERGENCE**: Book projects use a **custom Pandoc renderer** (`bookPandocRenderer`) that handles two rendering modes.

### Rendering Mode Determination

```typescript
function isMultiFileBookFormat(format: Format) {
  const extension = format.extensions?.book as BookExtension;
  if (extension) {
    return !!extension.multiFile;
  } else {
    return false;
  }
}

// HTML format extension (multi-file):
{
  extensions: {
    book: {
      multiFile: true,  // Each chapter is separate HTML
    }
  }
}

// PDF format extension (single-file):
{
  extensions: {
    book: {
      multiFile: false,  // All chapters merged into one PDF
    }
  }
}
```

### Mode A: Multi-File Rendering (HTML, AsciiDoc)

**Behavior**: Each chapter renders separately as individual HTML files (similar to website pages).

**Per-chapter processing**:

```typescript
onRender: async (format: string, file: ExecutedFile, quiet: boolean) => {
  if (isMultiFileBookFormat(file.context.format)) {
    const partitioned = partitionMarkdown(file.executeResult.markdown);
    const fileRelative = pathWithForwardSlashes(
      relative(project.dir, file.context.target.source),
    );

    const isIndex = isBookIndexPage(fileRelative);  // index.md?

    if (isIndex) {
      // INDEX PAGE: Add book-level metadata
      file.recipe.format = withBookTitleMetadata(
        file.recipe.format,
        project.config,
      );
      // Add cover image if configured
      const coverImage = bookConfig(kBookCoverImage, project.config);
      if (coverImage) {
        file.executeResult.markdown =
          `![](${coverImage}){.quarto-cover-image}\n\n` +
          file.executeResult.markdown;
      }

    } else {
      // CHAPTER PAGE: Add chapter number to title
      const chapterInfo = chapterInfoForInput(project, fileRelative);
      // chapterInfo = { number: 2, appendix: false, labelPrefix: "2" }

      file.recipe.format = withChapterMetadata(
        file.recipe.format,
        partitioned.headingText,     // "Introduction"
        partitioned.headingAttr,     // { id: "intro", classes: [] }
        chapterInfo,                 // { number: 2, ... }
        project.config,
      );
      // Results in title: "2  Introduction"

      // Remove first heading from markdown (now in title)
      file.executeResult.markdown = partitioned.markdown;
    }

    // Render this file immediately
    const renderCompletion = await renderPandoc(file, quiet);
    renderCompletions.push(renderCompletion);
  }
}
```

**Chapter title formatting** (`withChapterMetadata`):

```typescript
export function withChapterMetadata(
  format: Format,
  headingText: string,
  headingAttr?: PandocAttr,
  chapterInfo?: ChapterInfo,
  config?: ProjectConfig,
) {
  format = ld.cloneDeep(format);

  // Set title with chapter number
  format.metadata[kTitle] = formatChapterTitle(
    format,
    headingText,
    headingAttr,
    chapterInfo,
  );

  // Set crossref metadata
  format.metadata[kCrossref] = format.metadata[kCrossref] || {};
  const crossref = format.metadata[kCrossref] as Metadata;

  if (headingAttr?.id) {
    crossref[kCrossrefChapterId] = headingAttr.id;
  }

  if (chapterInfo) {
    // Set chapter number offset for section numbering
    format.pandoc[kNumberOffset] = [chapterInfo.number];

    if (chapterInfo.appendix) {
      crossref[kCrossrefChaptersAppendix] = true;
      crossref[kCrossrefChaptersAlpha] = true;  // Use letters (A, B, C)
    }
  } else {
    format.pandoc[kNumberSections] = false;  // Unnumbered chapter
  }

  return format;
}

export function formatChapterTitle(
  format: Format,
  label: string,
  attr?: PandocAttr,
  info?: ChapterInfo,
) {
  if (!info) {
    return `[${label}]{#${attr.id} .quarto-section-identifier}`;
  }

  if (info.appendix) {
    // Appendix: "Appendix A — Details"
    const title = "Appendix";
    const delim = " —";
    return `${title} ${info.labelPrefix}${delim} ${label}`;
  } else {
    // Regular chapter: "[2]{.chapter-number}  [Introduction]{.chapter-title}"
    return `[${info.labelPrefix}]{.chapter-number}\u00A0 [${label}]{.chapter-title}`;
  }
}
```

**Result**: Each chapter is a separate HTML file with proper titles, navigation, and cross-reference anchors.

---

### Mode B: Single-File Rendering (PDF, EPUB, DOCX)

**Behavior**: Accumulate all executed files, merge their markdown, then render once as a unified document.

**Phase 1: Accumulation** (`onRender`):

```typescript
onRender: async (format: string, file: ExecutedFile, quiet: boolean) => {
  if (!isMultiFileBookFormat(file.context.format)) {
    // Don't render yet - just accumulate
    executedFiles[format] = executedFiles[format] || [];
    executedFiles[format].push(file);
  }
}
```

**Phase 2: Merging** (`onComplete` → `renderSingleFileBook` → `mergeExecutedFiles`):

```typescript
async function mergeExecutedFiles(
  project: ProjectContext,
  options: RenderOptions,
  files: ExecutedFile[],
): Promise<ExecutedFile> {
  const context = safeCloneDeep(files[0].context);

  // Set output file name
  const outputStem = bookOutputStem(project.dir, project.config);
  context.format.pandoc[kOutputFile] = `${outputStem}.${
    context.format.render[kOutputExt]
  }`;

  const renderItems = bookConfigRenderItems(project.config);

  // MERGE ALL CHAPTER MARKDOWN
  const markdown = renderItems.reduce(
    (markdown: string, item: BookRenderItem) => {
      if (item.file) {
        const file = files.find((file) =>
          file.context.target.source === join(project.dir, item.file)
        );
        if (file) {
          const partitioned = partitionMarkdown(file.executeResult.markdown);

          // Resolve title markdown (always level 1 heading)
          const titleMarkdown = resolveTitleMarkdown(partitioned);

          // Extract front matter for title block
          const titleBlockMarkdown = resolveTitleBlockMarkdown(
            partitioned.yaml,
          );

          // Body markdown (without heading)
          const bodyMarkdown = partitioned.yaml?.title
            ? partitioned.srcMarkdownNoYaml
            : partitioned.markdown;

          // Add metadata comment for tracking
          const itemMarkdown = bookItemMetadata(project, item, file) +
            titleMarkdown +
            titleBlockMarkdown +
            bodyMarkdown;

          return markdown + itemMarkdown;
        }
      } else if (item.type === kBookItemPart || item.type === kBookItemAppendix) {
        // Part dividers
        const partMarkdown = `# ${item.text}\n\n`;
        return markdown + `\n\n::: {.quarto-book-part}\n${partMarkdown}\n:::\n\n`;
      }

      return markdown;
    },
    "",
  );

  // Merge other fields
  const supporting = files.flatMap((file) => file.executeResult.supporting);
  const filters = ld.uniq(files.flatMap((file) => file.executeResult.filters));
  const engineDependencies = mergeEngineDependencies(files);
  const preserve = mergePreserve(files);

  return {
    context,
    recipe: await outputRecipe(context),
    executeResult: {
      markdown,
      supporting,
      filters,
      engineDependencies,
      preserve,
      postProcess: files.some((f) => f.executeResult.postProcess),
    },
    resourceFiles: ld.uniq(files.flatMap((file) => file.resourceFiles)),
  };
}
```

**Example merged markdown** for 3-chapter book:

```markdown
<!-- quarto-file-metadata: ... -->

# Introduction

This is the introduction chapter.

## Background

Some content.

<!-- quarto-file-metadata: ... -->

# Methods

This is the methods chapter.

## Approach

More content.

<!-- quarto-file-metadata: ... -->

::: {.quarto-book-part}
# Appendices
:::

<!-- quarto-file-metadata: ... -->

# Appendix A — Technical Details

Appendix content.
```

**Phase 3: Single Render**:

```typescript
async function renderSingleFileBook(
  project: ProjectContext,
  options: RenderOptions,
  files: ExecutedFile[],
  quiet: boolean,
): Promise<RenderedFile> {
  const executedFile = await mergeExecutedFiles(project, options, files);

  // Set book title metadata (title, author, date, etc.)
  executedFile.recipe.format = withBookTitleMetadata(
    executedFile.recipe.format,
    project.config,
  );

  // Call book extension pre-render hook
  executedFile.recipe.format = onSingleFileBookPreRender(
    executedFile.recipe.format,
    project.config,
  );

  // Single Pandoc render of merged markdown
  const renderCompletion = await renderPandoc(executedFile, quiet);
  const renderedFile = await renderCompletion.complete([]);

  // Cleanup individual chapter files
  files.forEach((file) => {
    cleanupExecutedFile(file, renderedFile.file);
  });

  // Call book extension post-render hook
  onSingleFileBookPostRender(project, renderedFile);

  return renderedFile;
}
```

**Result**: Single PDF/EPUB/DOCX with all chapters, unified table of contents, and cross-references resolved by Pandoc.

---

## Stage 11: Post-Render Hook

Book projects extend website post-render with **cross-reference and bibliography fixups** for multi-file HTML books.

```typescript
export async function bookPostRender(
  context: ProjectContext,
  incremental: boolean,
  outputFiles: ProjectOutputFile[],
) {
  // Get HTML output files
  const websiteFiles = websiteOutputFiles(outputFiles);
  if (websiteFiles.length > 0) {
    // BOOK-SPECIFIC: Fix cross-references across chapters
    await bookCrossrefsPostRender(context, websiteFiles);

    // BOOK-SPECIFIC: Unify bibliography across chapters
    await bookBibliographyPostRender(context, incremental, websiteFiles);

    // INHERITED: Run website post-render (sitemap, search, listings, aliases)
    await websitePostRender(context, incremental, outputFiles);
  }

  // Call format-specific post-render hooks (e.g., EPUB post-processing)
  const outputFormats: Record<string, Format> = {};
  outputFiles.forEach((file) => {
    if (file.format.pandoc.to) {
      outputFormats[file.format.pandoc.to] = file.format;
    }
  });
  for (const outputFormat of Object.values(outputFormats)) {
    const bookExt = outputFormat.extensions?.book as BookExtension;
    if (bookExt.bookPostRender) {
      await bookExt.bookPostRender(
        outputFormat,
        context,
        incremental,
        outputFiles,
      );
    }
  }
}
```

### Cross-Reference Fixup (Multi-File HTML Only)

**Problem**: In multi-file HTML books, cross-references like `@fig-plot` or `@sec-intro` must link to anchors in *other* HTML files, but Pandoc only knows about the current file.

**Solution**: Parse all HTML files post-render and fix up cross-reference links.

```typescript
export async function bookCrossrefsPostRender(
  context: ProjectContext,
  websiteFiles: WebsiteProjectOutputFile[],
) {
  // Build a map of all cross-reference IDs to their file locations
  const xrefMap: Map<string, { file: string; href: string }> = new Map();

  for (const outputFile of websiteFiles) {
    const doc = parseHtml(outputFile.file);

    // Find all IDs in this file
    const elementsWithIds = doc.querySelectorAll("[id]");
    for (const element of elementsWithIds) {
      const id = element.getAttribute("id");
      xrefMap.set(id, {
        file: outputFile.file,
        href: `${outputFile.href}#${id}`,
      });
    }
  }

  // Fix up all cross-reference links
  for (const outputFile of websiteFiles) {
    const doc = parseHtml(outputFile.file);

    // Find all internal links
    const links = doc.querySelectorAll("a[href^='#']");
    for (const link of links) {
      const href = link.getAttribute("href");
      const id = href.substring(1);  // Remove '#'

      const xref = xrefMap.get(id);
      if (xref && xref.file !== outputFile.file) {
        // This link points to another file - update it
        link.setAttribute("href", xref.href);
      }
    }

    // Write modified HTML
    writeHtml(outputFile.file, doc);
  }
}
```

**Example**:
- Chapter 1 (`intro.html`) contains: `<a href="#fig-plot">Figure 1</a>`
- Figure 1 is actually in Chapter 2 (`methods.html`)
- Post-render fixup changes link to: `<a href="methods.html#fig-plot">Figure 1</a>`

### Bibliography Fixup (Multi-File HTML Only)

**Problem**: Each chapter renders with its own bibliography section, but we want a unified bibliography.

**Solution**: Collect all bibliography entries, deduplicate, and append to a designated references page.

```typescript
export async function bookBibliographyPostRender(
  context: ProjectContext,
  incremental: boolean,
  websiteFiles: WebsiteProjectOutputFile[],
) {
  const allBibEntries: BibEntry[] = [];

  // Collect bibliography entries from all chapters
  for (const outputFile of websiteFiles) {
    const doc = parseHtml(outputFile.file);

    // Find bibliography section
    const bibSection = doc.querySelector("#refs, .references");
    if (bibSection) {
      const entries = bibSection.querySelectorAll(".csl-entry");
      for (const entry of entries) {
        allBibEntries.push({
          id: entry.getAttribute("id"),
          html: entry.innerHTML,
        });
      }

      // Remove bibliography from this chapter
      bibSection.remove();
      writeHtml(outputFile.file, doc);
    }
  }

  // Deduplicate entries
  const uniqueEntries = ld.uniqBy(allBibEntries, (entry) => entry.id);

  // Find references page
  const referencesFile = websiteFiles.find((file) =>
    file.href.includes("references.html")
  );

  if (referencesFile) {
    const doc = parseHtml(referencesFile.file);

    // Create unified bibliography
    const bibSection = doc.createElement("div");
    bibSection.setAttribute("id", "refs");
    bibSection.setAttribute("class", "references");

    for (const entry of uniqueEntries) {
      const div = doc.createElement("div");
      div.setAttribute("id", entry.id);
      div.setAttribute("class", "csl-entry");
      div.innerHTML = entry.html;
      bibSection.appendChild(div);
    }

    // Append to references page
    doc.querySelector("main").appendChild(bibSection);
    writeHtml(referencesFile.file, doc);
  }
}
```

---

## Format Extras

Book projects **merge** book-specific extras with inherited website extras.

```typescript
formatExtras: async (
  context: ProjectContext,
  source: string,
  flags: PandocFlags,
  format: Format,
  services: RenderServices,
) => {
  // Book-specific defaults
  let extras: FormatExtras = {
    pandoc: {
      [kToc]: !isEpubOutput(format.pandoc),      // TOC for all except EPUB
      [kNumberSections]: true,                   // Numbered sections
    },
    metadata: {
      [kCrossref]: {
        [kCrossrefChapters]: true,  // Enable chapter-level crossrefs
      },
    },
  };

  if (isHtmlOutput(format.pandoc, true)) {
    // HTML-specific: Add book SCSS and postprocessor
    if (formatHasBootstrap(format)) {
      extras.html = {
        [kSassBundles]: [bookScssBundle()],
        [kHtmlPostprocessors]: [bookHtmlPostprocessor()],
      };
    }

    // INHERIT website extras (navigation, search, breadcrumbs, etc.)
    const websiteExtras = await websiteProjectType.formatExtras!(
      context,
      source,
      flags,
      format,
      services,
    );

    // Merge book extras with website extras
    extras = mergeConfigs(extras, websiteExtras);

  } else if (isLatexOutput(format.pandoc)) {
    // PDF-specific: Use scrreprt document class, chapter divisions
    extras = mergeConfigs(extras, {
      metadata: {
        [kDocumentClass]: "scrreprt",  // KOMA-Script report class
        [kPaperSize]: "letter",
      },
      pandoc: {
        [kTopLevelDivision]: "chapter",  // Use \chapter instead of \section
      },
    });
  }

  return extras;
},
```

**Result**:
- HTML books get all website features (navbar, sidebar, search, breadcrumbs) **plus** book-specific styling
- PDF books get chapter-level divisions and appropriate LaTeX classes
- All formats get chapter-aware cross-references

---

## Key Differences: Book vs Website

| Aspect | Website | Book |
|--------|---------|------|
| **Inheritance** | Base type | Inherits website |
| **Config Structure** | `website: {...}` | `book: {...}` → copied to `website: {...}` |
| **Chapter Management** | N/A | `book.chapters`, `book.appendices`, `book.references` |
| **Rendering Modes** | 1 (multi-file only) | 2 (multi-file OR single-file) |
| **Pandoc Renderer** | Default | **Custom** (`bookPandocRenderer`) |
| **Chapter Numbering** | N/A | Automatic (1, 2, 3... or A, B, C) |
| **Cross-References** | File-scoped | **Project-wide** (post-render fixup) |
| **Bibliography** | File-scoped | **Unified** (post-render fixup) |
| **Title Handling** | From front matter | **Chapter numbers prepended** |
| **Navigation** | Pages | **Chapters** (numbered in sidebar) |
| **Downloads** | N/A | **PDF/EPUB/DOCX download buttons** |
| **Output Directory** | `_site/` | `_book/` |

---

## Data Flow Diagram

### Multi-File Book (HTML):

```
Project Detection → Find _quarto.yml with project.type: book
    ↓
Config Translation → book.* → website.*
    ↓
Build Chapter List → Parse book.chapters, assign numbers
    ↓
Pre-Render Hook → Build navigation from chapter list (inherited)
    ↓
FOR EACH CHAPTER:
    Parse → Extract heading
    ↓
    Add Chapter Number → "2  Methods"
    ↓
    Execute Engine → Run knitr/jupyter/markdown
    ↓
    Handle Language Cells → OJS, diagrams, etc.
    ↓
    Inject Format Extras → Navigation, search (inherited)
    ↓
    Pandoc → Convert to HTML
    ↓
    Postprocess → HTML manipulation
    ↓
    Write → chapter.html
    ↓
END FOR EACH
    ↓
Post-Render:
    ↓
    Fix Cross-References → Update links across chapters
    ↓
    Unify Bibliography → Collect and deduplicate citations
    ↓
    Generate Sitemap → All chapters (inherited)
    ↓
    Generate Search Index → All chapters (inherited)
    ↓
    Generate Listings → If configured (inherited)
```

### Single-File Book (PDF/EPUB/DOCX):

```
Project Detection → Find _quarto.yml with project.type: book
    ↓
Config Translation → book.* → website.*
    ↓
Build Chapter List → Parse book.chapters, assign numbers
    ↓
Pre-Render Hook → (No navigation for single-file)
    ↓
FOR EACH CHAPTER:
    Parse → Extract heading
    ↓
    Execute Engine → Run knitr/jupyter/markdown
    ↓
    Handle Language Cells → OJS, diagrams, etc.
    ↓
    ACCUMULATE (don't render yet)
    ↓
END FOR EACH
    ↓
Merge All Chapters:
    ↓
    Concatenate Markdown → All chapters as one document
    ↓
    Add Book Metadata → title, author, date, etc.
    ↓
    Inject Format Extras → No navigation, but crossref settings
    ↓
    Pandoc → Convert to PDF/EPUB/DOCX (single run)
    ↓
    Recipe Complete → latexmk for PDF, etc.
    ↓
    Write → book.pdf / book.epub / book.docx
    ↓
Post-Render:
    ↓
    Format-Specific Hooks → EPUB postprocessing, etc.
```

---

## Implications for Rust Port

### 1. Inheritance via Trait Composition

```rust
#[async_trait]
pub trait ProjectType: Send + Sync {
    fn type_name(&self) -> &str;
    fn lib_dir(&self) -> &str;
    fn output_dir(&self) -> &str;

    // NEW: Support for type inheritance
    fn inherits_from(&self) -> Option<&'static dyn ProjectType> {
        None
    }

    async fn pre_render(&self, context: &ProjectContext) -> Result<()> {
        // Default: delegate to parent if inherited
        if let Some(parent) = self.inherits_from() {
            parent.pre_render(context).await
        } else {
            Ok(())
        }
    }

    async fn format_extras(
        &self,
        project: &ProjectContext,
        source: &Path,
        format: &Format,
    ) -> Result<FormatExtras>;

    async fn post_render(
        &self,
        context: &ProjectContext,
        incremental: bool,
        output_files: &[ProjectOutputFile],
    ) -> Result<()>;
}

pub struct BookProjectType {
    website: &'static WebsiteProjectType,
    chapter_manager: Arc<RwLock<ChapterManager>>,
}

#[async_trait]
impl ProjectType for BookProjectType {
    fn type_name(&self) -> &str { "book" }
    fn lib_dir(&self) -> &str { "site_libs" }  // Inherit from website
    fn output_dir(&self) -> &str { "_book" }   // Override

    fn inherits_from(&self) -> Option<&'static dyn ProjectType> {
        Some(self.website)
    }

    async fn pre_render(&self, context: &ProjectContext) -> Result<()> {
        // Resolve special dates
        resolve_book_dates(context)?;

        // Delegate to website pre-render
        self.website.pre_render(context).await
    }

    async fn format_extras(
        &self,
        project: &ProjectContext,
        source: &Path,
        format: &Format,
    ) -> Result<FormatExtras> {
        // Book-specific extras
        let mut extras = FormatExtras {
            pandoc: PandocExtras {
                toc: !format.is_epub(),
                number_sections: true,
            },
            metadata: Metadata::from([
                ("crossref".to_string(), json!({ "chapters": true })),
            ]),
            ..Default::default()
        };

        if format.is_html() {
            // Add book SCSS bundle
            extras.html.sass_bundles.push(book_scss_bundle());
            extras.html.postprocessors.push(Box::new(BookHtmlPostprocessor));

            // INHERIT website extras
            let website_extras = self.website.format_extras(project, source, format).await?;
            extras = merge_format_extras(extras, website_extras);

        } else if format.is_latex() {
            extras.metadata.insert("documentclass".to_string(), json!("scrreprt"));
            extras.metadata.insert("papersize".to_string(), json!("letter"));
            extras.pandoc.top_level_division = Some("chapter".to_string());
        }

        Ok(extras)
    }

    async fn post_render(
        &self,
        context: &ProjectContext,
        incremental: bool,
        output_files: &[ProjectOutputFile],
    ) -> Result<()> {
        // Book-specific post-render
        let html_files = output_files.iter()
            .filter(|f| f.format.is_html())
            .collect::<Vec<_>>();

        if !html_files.is_empty() {
            book_crossrefs_post_render(context, &html_files).await?;
            book_bibliography_post_render(context, incremental, &html_files).await?;
        }

        // Inherit website post-render
        self.website.post_render(context, incremental, output_files).await?;

        // Format-specific hooks
        for output_file in output_files {
            if let Some(book_ext) = output_file.format.extensions.book.as_ref() {
                if let Some(post_render) = book_ext.post_render {
                    post_render(context, incremental, output_files).await?;
                }
            }
        }

        Ok(())
    }
}
```

### 2. Chapter Manager

```rust
pub struct ChapterManager {
    render_items: Vec<BookRenderItem>,
    chapter_map: HashMap<PathBuf, ChapterInfo>,
}

#[derive(Clone)]
pub struct BookRenderItem {
    pub item_type: BookRenderItemType,
    pub depth: usize,
    pub text: Option<String>,
    pub file: Option<PathBuf>,
    pub number: Option<usize>,
}

#[derive(Clone, Copy)]
pub enum BookRenderItemType {
    Index,
    Chapter,
    Appendix,
    Part,
}

#[derive(Clone)]
pub struct ChapterInfo {
    pub number: usize,
    pub appendix: bool,
    pub label_prefix: String,  // "1", "2", "A", "B", etc.
}

impl ChapterManager {
    pub async fn from_config(project: &ProjectContext) -> Result<Self> {
        let config = &project.config;
        let book_config = config.book.as_ref()
            .ok_or_else(|| anyhow!("No book config found"))?;

        let mut render_items = Vec::new();
        let mut chapter_map = HashMap::new();
        let mut chapter_number = 1;

        // Parse chapters
        if let Some(chapters) = &book_config.chapters {
            for chapter in chapters {
                let item = Self::parse_chapter_item(
                    project,
                    chapter,
                    BookRenderItemType::Chapter,
                    &mut chapter_number,
                    0,
                ).await?;
                render_items.push(item.clone());

                if let Some(file) = &item.file {
                    if let Some(number) = item.number {
                        chapter_map.insert(file.clone(), ChapterInfo {
                            number,
                            appendix: false,
                            label_prefix: number.to_string(),
                        });
                    }
                }
            }
        }

        // Parse appendices
        if let Some(appendices) = &book_config.appendices {
            render_items.push(BookRenderItem {
                item_type: BookRenderItemType::Appendix,
                depth: 0,
                text: Some("Appendices".to_string()),
                file: None,
                number: None,
            });

            chapter_number = 1;
            for appendix in appendices {
                let item = Self::parse_chapter_item(
                    project,
                    appendix,
                    BookRenderItemType::Chapter,
                    &mut chapter_number,
                    1,
                ).await?;
                render_items.push(item.clone());

                if let Some(file) = &item.file {
                    if let Some(number) = item.number {
                        chapter_map.insert(file.clone(), ChapterInfo {
                            number,
                            appendix: true,
                            label_prefix: std::char::from_u32(64 + number as u32)
                                .unwrap()
                                .to_string(),  // 'A', 'B', 'C'
                        });
                    }
                }
            }
        }

        Ok(ChapterManager {
            render_items,
            chapter_map,
        })
    }

    pub fn chapter_info(&self, file: &Path) -> Option<&ChapterInfo> {
        self.chapter_map.get(file)
    }

    pub fn render_items(&self) -> &[BookRenderItem] {
        &self.render_items
    }
}
```

### 3. Custom Pandoc Renderer

```rust
pub struct BookPandocRenderer {
    mode: BookRenderMode,
    executed_files: Mutex<Vec<ExecutedFile>>,
    render_completions: Mutex<Vec<PandocRenderCompletion>>,
}

pub enum BookRenderMode {
    MultiFile,   // HTML - render each chapter separately
    SingleFile,  // PDF/EPUB/DOCX - accumulate and merge
}

#[async_trait]
impl PandocRenderer for BookPandocRenderer {
    async fn on_render(
        &self,
        format: &str,
        file: ExecutedFile,
        quiet: bool,
    ) -> Result<()> {
        match self.mode {
            BookRenderMode::MultiFile => {
                // Render immediately
                let partitioned = partition_markdown(&file.execute_result.markdown);
                let file_relative = file.context.target.source
                    .strip_prefix(&file.context.project.dir)?;

                let is_index = file_relative.starts_with("index.");

                if is_index {
                    // Add book-level metadata
                    file.recipe.format = with_book_title_metadata(
                        file.recipe.format,
                        &file.context.project.config,
                    )?;

                    // Add cover image
                    if let Some(cover) = file.context.project.config.book.cover_image {
                        file.execute_result.markdown = format!(
                            "![]({}){{{.quarto-cover-image}}}\n\n{}",
                            cover,
                            file.execute_result.markdown
                        );
                    }
                } else {
                    // Add chapter number
                    let chapter_info = file.context.project.book.chapter_manager
                        .read().await
                        .chapter_info(file_relative);

                    file.recipe.format = with_chapter_metadata(
                        file.recipe.format,
                        &partitioned.heading_text,
                        &partitioned.heading_attr,
                        chapter_info,
                        &file.context.project.config,
                    )?;

                    file.execute_result.markdown = partitioned.markdown;
                }

                let completion = render_pandoc(file, quiet).await?;
                self.render_completions.lock().await.push(completion);
            }

            BookRenderMode::SingleFile => {
                // Accumulate for later merge
                self.executed_files.lock().await.push(file);
            }
        }

        Ok(())
    }

    async fn on_complete(
        &self,
        error: bool,
    ) -> Result<Vec<RenderedFile>> {
        if error {
            return Ok(vec![]);
        }

        match self.mode {
            BookRenderMode::MultiFile => {
                // Complete all renders
                let mut rendered_files = Vec::new();
                let completions = std::mem::take(&mut *self.render_completions.lock().await);
                for completion in completions {
                    rendered_files.push(completion.complete().await?);
                }
                Ok(rendered_files)
            }

            BookRenderMode::SingleFile => {
                // Merge and render
                let files = std::mem::take(&mut *self.executed_files.lock().await);
                let rendered = render_single_file_book(files).await?;
                Ok(vec![rendered])
            }
        }
    }
}

async fn render_single_file_book(files: Vec<ExecutedFile>) -> Result<RenderedFile> {
    let project = &files[0].context.project;
    let format = &files[0].context.format;

    // Merge markdown
    let chapter_manager = project.book.chapter_manager.read().await;
    let mut merged_markdown = String::new();

    for item in chapter_manager.render_items() {
        if let Some(file_path) = &item.file {
            let file = files.iter()
                .find(|f| f.context.target.source.ends_with(file_path))
                .ok_or_else(|| anyhow!("Chapter file not found: {:?}", file_path))?;

            let partitioned = partition_markdown(&file.execute_result.markdown);

            // Add metadata comment
            merged_markdown.push_str(&book_item_metadata(project, item, Some(file)));

            // Add title as level-1 heading
            let title_markdown = format!("# {}\n\n", partitioned.heading_text);
            merged_markdown.push_str(&title_markdown);

            // Add body
            merged_markdown.push_str(&partitioned.markdown);
            merged_markdown.push_str("\n\n");

        } else if item.item_type == BookRenderItemType::Part {
            // Part divider
            merged_markdown.push_str(&format!(
                "\n\n::: {{.quarto-book-part}}\n# {}\n:::\n\n",
                item.text.as_deref().unwrap_or("")
            ));
        }
    }

    // Create merged executed file
    let mut merged_file = files[0].clone();
    merged_file.execute_result.markdown = merged_markdown;
    merged_file.recipe.format = with_book_title_metadata(
        merged_file.recipe.format,
        &project.config,
    )?;

    // Render
    let completion = render_pandoc(merged_file, false).await?;
    let rendered = completion.complete().await?;

    Ok(rendered)
}
```

### 4. Cross-Reference Fixup

```rust
pub async fn book_crossrefs_post_render(
    context: &ProjectContext,
    html_files: &[&ProjectOutputFile],
) -> Result<()> {
    use scraper::{Html, Selector};

    // Build cross-reference map
    let mut xref_map: HashMap<String, String> = HashMap::new();

    for output_file in html_files {
        let html = std::fs::read_to_string(&output_file.file)?;
        let document = Html::parse_document(&html);

        let id_selector = Selector::parse("[id]").unwrap();
        for element in document.select(&id_selector) {
            if let Some(id) = element.value().attr("id") {
                let href = format!("{}#{}", output_file.href, id);
                xref_map.insert(id.to_string(), href);
            }
        }
    }

    // Fix up links
    for output_file in html_files {
        let html = std::fs::read_to_string(&output_file.file)?;
        let mut document = Html::parse_document(&html);
        let mut modified = false;

        let link_selector = Selector::parse("a[href^='#']").unwrap();
        for element in document.select(&link_selector) {
            if let Some(href) = element.value().attr("href") {
                let id = href.trim_start_matches('#');

                if let Some(target_href) = xref_map.get(id) {
                    // Check if link points to different file
                    if !target_href.starts_with(&output_file.href) {
                        // Update href
                        // Note: This requires mutable document manipulation
                        // In practice, use a library like lol_html for mutations
                        modified = true;
                    }
                }
            }
        }

        if modified {
            // Write back modified HTML
            std::fs::write(&output_file.file, document.html())?;
        }
    }

    Ok(())
}
```

### 5. Bibliography Fixup

```rust
pub async fn book_bibliography_post_render(
    context: &ProjectContext,
    incremental: bool,
    html_files: &[&ProjectOutputFile],
) -> Result<()> {
    use scraper::{Html, Selector};

    let mut all_bib_entries: Vec<BibEntry> = Vec::new();

    // Collect and remove bibliography from each chapter
    for output_file in html_files {
        let html = std::fs::read_to_string(&output_file.file)?;
        let mut document = Html::parse_document(&html);

        let bib_selector = Selector::parse("#refs, .references").unwrap();
        if let Some(bib_section) = document.select(&bib_selector).next() {
            let entry_selector = Selector::parse(".csl-entry").unwrap();
            for entry in bib_section.select(&entry_selector) {
                all_bib_entries.push(BibEntry {
                    id: entry.value().attr("id").unwrap_or("").to_string(),
                    html: entry.inner_html(),
                });
            }

            // Remove bibliography section
            // (Requires mutable document manipulation)
            // Write back modified HTML
        }
    }

    // Deduplicate
    all_bib_entries.sort_by(|a, b| a.id.cmp(&b.id));
    all_bib_entries.dedup_by(|a, b| a.id == b.id);

    // Find references page and append unified bibliography
    if let Some(refs_file) = html_files.iter()
        .find(|f| f.href.contains("references.html"))
    {
        let html = std::fs::read_to_string(&refs_file.file)?;
        let mut document = Html::parse_document(&html);

        // Create unified bibliography HTML
        let mut bib_html = String::from(r#"<div id="refs" class="references">"#);
        for entry in all_bib_entries {
            bib_html.push_str(&format!(
                r#"<div id="{}" class="csl-entry">{}</div>"#,
                entry.id, entry.html
            ));
        }
        bib_html.push_str("</div>");

        // Append to main element
        // (Requires mutable document manipulation)
        // Write back modified HTML
    }

    Ok(())
}

struct BibEntry {
    id: String,
    html: String,
}
```

---

## Timing Estimates

### Multi-File Book (100 chapters, HTML):

- Project detection: 10-50ms
- Config translation: 50-100ms
- Build chapter list: 100-500ms
- Pre-render hook (navigation): 100-500ms
- Per-chapter rendering: 100 × (1-120s) = 100-12000s
- Cross-reference fixup: 2-10s (parse all HTML, update links)
- Bibliography fixup: 1-5s (collect entries, deduplicate)
- Sitemap generation: 500ms-2s
- Search indexing: 5-50s (100 chapters)
- **Total**: 2-200 minutes (dominated by chapter rendering)

### Single-File Book (100 chapters, PDF):

- Project detection: 10-50ms
- Config translation: 50-100ms
- Build chapter list: 100-500ms
- Pre-render hook: (skipped for single-file)
- Per-chapter execution: 100 × (100ms-60s) = 10-6000s
- Merge markdown: 100-500ms
- Pandoc render: 10-180s (large document)
- LaTeX compilation: 30-300s (latexmk, multiple runs)
- **Total**: 1-110 minutes (dominated by execution + LaTeX)

---

## Summary

Book projects are **website projects with extensions**:

1. **Inheritance**: Books delegate most functionality to website project type
2. **Dual Modes**: Multi-file (HTML, like websites) vs single-file (PDF/EPUB/DOCX, merged)
3. **Custom Renderer**: Handles chapter numbering, title formatting, and merging
4. **Post-Render Fixups**: Cross-references and bibliography unified across chapters
5. **Rich Metadata**: Chapter numbers, appendix labels, unified table of contents

**Rust port must support**:
- Trait-based inheritance (BookProjectType inherits WebsiteProjectType)
- Chapter manager with automatic numbering
- Custom Pandoc renderer with mode selection
- HTML post-processing for cross-reference and bibliography fixups
- Config translation (book.* → website.*)

Book projects demonstrate Quarto's extensibility: new project types can build on existing ones while adding specialized behavior.
