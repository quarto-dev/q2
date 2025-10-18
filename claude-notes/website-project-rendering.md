# Website Project Rendering in Quarto-CLI

**Date:** 2025-10-11
**Purpose:** Analysis of rendering pipeline for `project: type: website` in `_quarto.yml`
**Status:** Complete
**Related:** [single-document-render-pipeline.md](single-document-render-pipeline.md)

## Executive Summary

This document analyzes what happens when `quarto render` is called from a directory containing a `_quarto.yml` file with `project: type: website`. The pipeline shares stages 1-10 with single document rendering (see [single-document-render-pipeline.md](single-document-render-pipeline.md)), but adds **project-level orchestration** with pre-render, multi-file coordination, and post-render hooks.

**Key Insight:** Website projects transform single-file rendering into a **coordinated multi-file system** with shared navigation, search indexing, sitemap generation, and cross-document features.

```
Individual Files (Stages 1-10 from single-document pipeline)
           +
Project Orchestration (Pre-render → Multi-file → Post-render)
           ↓
Complete Website (with navigation, search, sitemap, listings)
```

## Pipeline Comparison: Single File vs Website Project

| Aspect | Single File | Website Project |
|--------|-------------|-----------------|
| **Project Context** | Minimal single-file context | Full project context with type-specific behavior |
| **Input Files** | One file | All files matching render globs |
| **Pre-render** | None | `initWebsiteNavigation()` - builds navbar/sidebar structure |
| **Format Extras** | Format-specific only | Format + Website-specific (navigation, search, breadcrumbs) |
| **File Rendering** | Single iteration | Loop over all input files |
| **Post-render** | None | Sitemap, search index, listings, aliases generation |
| **Output** | Single HTML/PDF | Complete website with cross-references |

## Complete Website Rendering Pipeline

```
User runs: quarto render (in directory with _quarto.yml type: website)
```

### Stage 0: Project Detection

**File:** `src/project/project-context.ts`
**Function:** `projectContext(path, notebookContext, renderOptions)`

#### What Happens

1. **Search for `_quarto.yml`**
   ```typescript
   async function projectContext(path, notebookContext, renderOptions) {
     let dir = Deno.statSync(path).isDirectory ? path : dirname(path);

     while (true) {
       const configFile = projectConfigFile(dir);  // Looks for _quarto.yml
       if (configFile) {
         // Found project config
         break;
       }

       // Walk up directory tree
       const nextDir = dirname(dir);
       if (nextDir === dir) {
         return undefined;  // No project found
       }
       dir = nextDir;
     }
   }
   ```

2. **Read and Validate Project Config**
   ```typescript
   const config = await readAndValidateYamlFromFile(
     configFile,
     configSchema,
     "Project validation failed"
   );
   ```

3. **Detect Project Type**
   ```typescript
   const projType = config.project[kProjectType];  // "website"
   const type = projectType(projType);  // Returns websiteProjectType object
   ```

4. **Create Project Context**
   ```typescript
   const result: ProjectContext = {
     dir,                    // Project root directory
     config: projectConfig,  // Merged configuration
     files: {
       input: files,         // All input files (computed next)
       resources: [],
       config: [configFile],
     },
     engines: engines,       // Engines used by files
     formatExtras: type.formatExtras,  // Website-specific extras
     isSingleFile: false,    // NOT a single file!
   };
   ```

5. **Find Input Files**
   ```typescript
   const { files, engines } = await projectInputFiles(result, projectConfig);

   // Walks directory tree, finding all renderable files
   // Respects:
   // - project.render globs (if specified)
   // - Ignores output directory (_site for websites)
   // - Ignores hidden files (_*), dot files (.*), README.md
   ```

**Key Difference:** Instead of a minimal single-file context, we have a **full project context** with:
- Multiple input files
- Shared configuration
- Type-specific behavior (websiteProjectType)
- Project-wide resources

### Stage 1: Pre-Render Hook (Website-Specific)

**File:** `src/project/types/website/website.ts`
**Function:** `websiteProjectType.preRender(context)`

This stage happens **before any files are rendered**.

```typescript
export const websiteProjectType: ProjectType = {
  preRender: async (context: ProjectContext) => {
    await initWebsiteNavigation(context);
  },
  // ...
};
```

#### Website Navigation Initialization

**File:** `src/project/types/website/website-navigation.ts`
**Function:** `initWebsiteNavigation(project)`

**What happens:**

```typescript
export async function initWebsiteNavigation(project: ProjectContext) {
  // Reset unique menu IDs
  resetMenuIds();

  // Read navigation config from _quarto.yml
  const {
    navbar,      // Top navigation bar
    sidebars,    // Left/right sidebars
    pageNavigation,  // Prev/Next page links
    footer,      // Footer content
    pageMargin,  // Margin content
    bodyDecorators,  // Additional HTML
    announcement,    // Site-wide announcement banner
  } = await websiteNavigationConfig(project);

  if (!navbar && !sidebars && !pageNavigation && !footer && !pageMargin && !bodyDecorators) {
    return;  // No navigation configured
  }

  // Build sidebars data
  if (sidebars) {
    navigation.sidebars = await sidebarsEjsData(project, sidebars);
    navigation.sidebars = resolveNavReferences(navigation.sidebars) as Sidebar[];
  } else {
    navigation.sidebars = [];
  }

  // Build navbar data
  if (navbar) {
    navigation.navbar = await navbarEjsData(project, navbar);
    navigation.navbar = resolveNavReferences(navigation.navbar) as Navbar;
  } else {
    navigation.navbar = undefined;
  }

  // Store in global navigation object
  navigation.pageNavigation = pageNavigation;
  navigation.footer = await resolveFooter(project, footer);
  navigation.bodyDecorators = bodyDecorators;
  navigation.pageMargin = pageMargin;
  navigation.announcement = announcement;
}
```

**Sidebar Resolution:**
```typescript
async function sidebarsEjsData(project: ProjectContext, sidebars: Sidebar[]) {
  for (const sidebar of sidebars) {
    // Expand "auto" items (automatically discover files in directory)
    sidebar.contents = await expandAutoSidebarItems(project, sidebar.contents);

    // Resolve each sidebar item
    for (const item of sidebar.contents) {
      if (item.href) {
        // Resolve input file href to output href
        const resolved = await resolveInputTarget(project, item.href);
        item.href = resolved.outputHref;
        item.text = item.text || resolved.title;
      }

      // Recursively resolve nested items
      if (item.contents) {
        await resolveSidebarItems(project, item);
      }
    }
  }
}
```

**Navbar Resolution:**
```typescript
async function navbarEjsData(project: ProjectContext, navbar: Navbar) {
  const data: Navbar = {
    background: navbar.background || "primary",
    search: navbar.search !== undefined ? navbar.search : "overlay",
    collapse: navbar.collapse !== undefined ? navbar.collapse : true,
    title: navbar.title || websiteTitle(project.config),
  };

  // Resolve left nav items
  if (navbar.left) {
    data.left = [];
    for (const item of navbar.left) {
      data.left.push(await navigationItem(project, item));
    }
  }

  // Resolve right nav items
  if (navbar.right) {
    data.right = [];
    for (const item of navbar.right) {
      data.right.push(await navigationItem(project, item));
    }
  }

  return data;
}
```

**Key Result:** After pre-render, there's a **global navigation state** containing:
- Fully resolved sidebar structure (with output hrefs)
- Fully resolved navbar structure
- Footer, page navigation, announcements

This state is used by **every file** during rendering.

### Stages 2-10: Per-File Rendering (Same as Single Document)

Each file goes through the same 10-stage pipeline documented in [single-document-render-pipeline.md](single-document-render-pipeline.md), but with **website-specific format extras** injected.

**File:** `src/command/render/render-files.ts`
**Function:** `renderFiles(files, options, notebookContext, ..., project)`

```typescript
export async function renderFiles(
  files: RenderFile[],
  options: RenderOptions,
  notebookContext: NotebookContext,
  alwaysExecuteFiles: string[],
  pandocRenderer: PandocRenderer,
  project: ProjectContext,
) {
  const tempContext = createTempContext();

  // Render each file
  for (const file of files) {
    const result = await renderFileInternal(
      file,
      options,
      notebookContext,
      project,        // Project context passed through!
      tempContext,
    );

    await pandocRenderer(result);  // Render with Pandoc
  }

  tempContext.cleanup();
}
```

Each file's **Stage 9 (Pandoc Conversion)** includes website-specific format extras.

#### Website Format Extras (Injected Per File)

**File:** `src/project/types/website/website.ts`
**Function:** `websiteProjectType.formatExtras(project, source, flags, format, services)`

```typescript
formatExtras: async (project, source, flags, format, services) => {
  if (!isHtmlFileOutput(format.pandoc)) {
    return {};  // Only for HTML output
  }

  // Get navigation extras (navbar, sidebar, breadcrumbs)
  const extras = formatHasBootstrap(format)
    ? await websiteNavigationExtras(project, source, flags, format, services.temp)
    : await websiteNoThemeExtras(project, source, flags, format, services.temp);

  // Add title prefix (website title appears before page title)
  const title = websiteTitle(project.config);
  if (title) {
    extras.pandoc = {
      [kTitlePrefix]: title,
    };
  }

  // Add favicon dependency
  const favicon = websiteConfigString(kSiteFavicon, project.config);
  if (favicon) {
    extras.html = extras.html || {};
    extras.html.dependencies = extras.html.dependencies || [];
    extras.html.dependencies.push({
      name: "site-favicon",
      links: [{
        rel: "icon",
        href: projectOffset(project, source) + "/" + favicon,
        type: contentType(favicon),
      }],
    });
  }

  // Add postprocessors
  extras.html[kHtmlPostprocessors] = extras.html[kHtmlPostprocessors] || [];
  extras.html[kHtmlPostprocessors].push(...[
    websiteDraftPostProcessor,
    canonicalizeTitlePostprocessor,
    htmlResourceResolverPostprocessor(source, project),
  ]);

  // Add listing postprocessors (for blog pages)
  const listingDeps = await listingHtmlDependencies(source, project, format, services.temp, extras);
  if (listingDeps) {
    extras.html[kHtmlPostprocessors].unshift(listingDeps[kHtmlPostprocessors]);
    extras.html[kDependencies].push(...listingDeps[kDependencies]);
  }

  // Add about page postprocessors
  const aboutDeps = await aboutHtmlDependencies(source, project, format, services.temp, extras);
  if (aboutDeps) {
    extras.html[kHtmlPostprocessors].push(aboutDeps[kHtmlPostprocessors]);
  }

  // Add metadata postprocessors
  const metadataDeps = metadataHtmlDependencies(source, project, format, extras);
  extras.html[kHtmlPostprocessors].push(metadataDeps[kHtmlPostprocessors]);

  // Add analytics dependencies
  const analyticsDep = websiteAnalyticsScriptFile(project, services.temp);
  if (analyticsDep) {
    extras[kIncludeInHeader].push(analyticsDep);
  }

  return extras;
},
```

#### Website Navigation Extras (Most Important)

**File:** `src/project/types/website/website-navigation.ts`
**Function:** `websiteNavigationExtras(project, source, flags, format, temp)`

```typescript
export async function websiteNavigationExtras(
  project: ProjectContext,
  source: string,
  flags: PandocFlags,
  format: Format,
  temp: TempContext,
): Promise<FormatExtras> {
  // Determine which sidebar applies to this file
  const inputRelative = relative(project.dir, source);
  const target = await resolveInputTarget(project, inputRelative);
  const href = target?.outputHref || inputFileHref(inputRelative);
  const sidebar = sidebarForHref(href, format);

  // Build navigation data for this page
  const nav: Record<string, unknown> = {
    hasToc: hasToc(),
    tocLocation: "right",
    layout: formatPageLayout(format),
    navbar: disableNavbar ? undefined : navigation.navbar,  // From global state
    sidebar: disableSidebar ? undefined : expandedSidebar(href, sidebar),
    footer: navigation.footer,  // From global state
    language: format.language,
    showBreadCrumbs: websiteConfigBoolean(kBreadCrumbNavigation, true, project.config),
    announcement: navigation.announcement,
  };

  // Add prev/next page links based on sidebar position
  const pageNavigation = nextAndPrevious(href, sidebar);
  if (navigation.pageNavigation || format.metadata[kSitePageNavigation] === true) {
    nav.prevPage = pageNavigation.prevPage;
    nav.nextPage = pageNavigation.nextPage;
  }

  // Compute breadcrumbs for this page
  const crumbs = breadCrumbs(href, sidebar);
  navigation.breadCrumbs = crumbs;

  // Generate HTML envelope using EJS templates
  const bodyEnvelope = {
    before: renderEjs("nav-before-body.ejs", { nav }),
    afterPreamble: renderEjs("nav-after-body-preamble.ejs", { nav }),
    afterPostamble: renderEjs("nav-after-body-postamble.ejs", { nav }),
  };

  // Dependencies
  const dependencies = [
    await websiteNavigationDependency(project),  // quarto-nav.js
  ];

  const sassBundles = [
    websiteNavigationSassBundle(),  // quarto-nav.scss
  ];

  // Add search dependencies
  const searchDep = await websiteSearchDependency(project, source);
  if (searchDep) {
    dependencies.push(...searchDep);
    sassBundles.push(websiteSearchSassBundle());
  }

  return {
    [kIncludeInHeader]: includeInHeader,
    html: {
      [kSassBundles]: sassBundles,
      [kDependencies]: dependencies,
      [kBodyEnvelope]: bodyEnvelope,  // Wraps page content in nav structure!
      [kHtmlPostprocessors]: [
        navigationHtmlPostprocessor(project, format, source),
      ],
    },
  };
}
```

**Key Features:**

1. **Body Envelope**: Wraps every page in consistent navigation HTML
2. **Sidebar Expansion**: Marks current page and expands parent sections
3. **Breadcrumbs**: Computed from sidebar hierarchy
4. **Prev/Next Links**: Based on position in sidebar
5. **Search Integration**: Adds search UI and dependencies

#### Example Body Envelope

```html
<!-- before -->
<header id="quarto-header" class="headroom fixed-top">
  <nav class="navbar navbar-expand-lg">
    <div class="navbar-brand-container">
      <a class="navbar-brand" href="./index.html">My Website</a>
    </div>
    <div class="navbar-collapse">
      <ul class="navbar-nav">
        <li><a href="./about.html">About</a></li>
        <li><a href="./posts.html">Blog</a></li>
      </ul>
    </div>
    <div class="quarto-navbar-tools">
      <div id="quarto-search-container"></div>
    </div>
  </nav>
</header>

<!-- afterPreamble -->
<div id="quarto-sidebar" class="sidebar collapse sidebar-navigation">
  <nav id="quarto-sidebar-nav">
    <ul>
      <li><a href="./intro.html">Introduction</a></li>
      <li><a href="./guide.html" class="active">User Guide</a></li>
      <li><a href="./api.html">API Reference</a></li>
    </ul>
  </nav>
</div>

<!-- [Page content goes here] -->

<!-- afterPostamble -->
<footer class="footer">
  <div class="nav-footer">
    <div class="nav-footer-left">© 2025 My Website</div>
    <div class="nav-footer-right">
      Built with <a href="https://quarto.org/">Quarto</a>
    </div>
  </div>
</footer>
```

**Navigation HTML Postprocessor** then:
- Resolves cross-document links
- Adds breadcrumbs to title block
- Marks active navigation items
- Handles repository links (edit, source, issue)
- Removes section numbers if disabled

### Stage 11: Post-Render Hooks (Website-Specific)

**File:** `src/project/types/website/website.ts`
**Function:** `websiteProjectType.postRender(context, incremental, outputFiles)`

This stage happens **after all files are rendered**.

```typescript
export const websiteProjectType: ProjectType = {
  postRender: async (context, incremental, outputFiles) => {
    await websitePostRender(context, incremental, websiteOutputFiles(outputFiles));
  },
  // ...
};

export async function websitePostRender(
  context: ProjectContext,
  incremental: boolean,
  outputFiles: ProjectOutputFile[],
) {
  // Filter out 404.html from indexing
  const doc404 = join(projectOutputDir(context), "404.html");
  outputFiles = outputFiles.filter((file) => file.file !== doc404);

  // 1. Update sitemap.xml
  await updateSitemap(context, outputFiles, incremental);

  // 2. Update search.json index
  await updateSearchIndex(context, outputFiles, incremental);

  // 3. Complete listing generation (RSS feeds for blogs)
  completeListingGeneration(context, outputFiles, incremental);

  // 4. Generate page aliases (redirects)
  await updateAliases(context, outputFiles, incremental);

  // 5. Ensure index.html exists (create redirect if not)
  await ensureIndexPage(context);
}
```

#### 11.1 Sitemap Generation

**File:** `src/project/types/website/website-sitemap.ts`
**Function:** `updateSitemap(context, outputFiles, incremental)`

```typescript
export async function updateSitemap(
  context: ProjectContext,
  outputFiles: ProjectOutputFile[],
  incremental: boolean,
) {
  const outputDir = projectOutputDir(context);
  const sitemapPath = join(outputDir, "sitemap.xml");
  const baseUrl = websiteBaseurl(context.config);  // From website.site-url

  if (typeof baseUrl === "string") {
    // Normalize baseUrl (ensure trailing slash)
    let normalizedBaseUrl = baseUrl.endsWith("/") ? baseUrl : baseUrl + "/";

    const urlsetEntry = async (outputFile: ProjectOutputFile) => {
      const file = outputFile.file;
      return {
        loc: normalizedBaseUrl + pathWithForwardSlashes(relative(outputDir, file)),
        lastmod: await inputModified(file, context),  // Modification date
        draft: await isDraft(outputFile),
      };
    };

    // Full render or incremental update
    if (!incremental || !existsSync(sitemapPath)) {
      // Create fresh sitemap
      const urlset: Urlset = [];
      for (const file of outputFiles) {
        urlset.push(await urlsetEntry(file));
      }
      writeSitemap(sitemapPath, urlset, draftMode);
    } else {
      // Parse existing sitemap, update entries
      const urlset = await readSitemap(sitemapPath);
      for (const outputFile of outputFiles) {
        const loc = fileLoc(outputFile.file);
        const url = urlset.find((url) => url.loc === loc);
        if (url) {
          url.lastmod = await inputModified(outputFile.file, context);
          url.draft = await isDraft(outputFile);
        } else {
          urlset.push(await urlsetEntry(outputFile));
        }
      }
      writeSitemap(sitemapPath, urlset, draftMode);
    }

    // Create robots.txt
    if (!existsSync(join(outputDir, "robots.txt"))) {
      const robotsTxt = `Sitemap: ${normalizedBaseUrl}sitemap.xml\n`;
      Deno.writeTextFileSync(join(outputDir, "robots.txt"), robotsTxt);
    }
  }
}
```

**Generated sitemap.xml:**
```xml
<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url>
    <loc>https://example.com/index.html</loc>
    <lastmod>2025-10-11T10:30:00.000Z</lastmod>
  </url>
  <url>
    <loc>https://example.com/about.html</loc>
    <lastmod>2025-10-11T09:15:00.000Z</lastmod>
  </url>
  <url>
    <loc>https://example.com/posts/welcome.html</loc>
    <lastmod>2025-10-10T14:22:00.000Z</lastmod>
  </url>
</urlset>
```

#### 11.2 Search Index Generation

**File:** `src/project/types/website/website-search.ts`
**Function:** `updateSearchIndex(context, outputFiles, incremental)`

```typescript
export async function updateSearchIndex(
  context: ProjectContext,
  outputFiles: ProjectOutputFile[],
  incremental: boolean,
) {
  const outputDir = projectOutputDir(context);
  const searchJsonPath = join(outputDir, "search.json");

  // Load existing index if incremental
  const searchDocs = new Array<SearchDoc>();
  if (incremental && existsSync(searchJsonPath)) {
    const existingSearchJson = JSON.parse(Deno.readTextFileSync(searchJsonPath));
    searchDocs.push(...existingSearchJson);
  }

  let updatedSearchDocs = [...searchDocs];

  for (const outputFile of outputFiles) {
    const file = outputFile.file;
    const href = pathWithForwardSlashes(relative(outputDir, file));

    // Skip if search disabled for this file
    if (outputFile.format.metadata[kSearch] === false) {
      updatedSearchDocs = updatedSearchDocs.filter((doc) => !doc.href.startsWith(href));
      continue;
    }

    // Skip if not HTML
    if (!isHtmlFileOutput(outputFile.format.pandoc)) {
      continue;
    }

    // Parse HTML document
    const contents = Deno.readTextFileSync(file);
    const doc = new DOMParser().parseFromString(contents, "text/html")!;

    // Extract title
    const titleEl = doc.querySelector("h1.title");
    const title = titleEl ? titleEl.textContent : websiteTitle(context.config);

    // Compute breadcrumbs for search results
    const sidebar = sidebarForHref(`/${href}`, outputFile.format);
    const bc = breadCrumbs(`/${href}`, sidebar);
    const crumbs = bc.filter((crumb) => crumb.text).map((crumb) => crumb.text);

    // Remove non-searchable content
    doc.getElementById("title-block-header")?.remove();
    doc.querySelector(`nav[role="doc-toc"]`)?.remove();
    doc.querySelectorAll("script, style").forEach((el) => el.remove());

    // Index page + sections separately
    const sections = doc.querySelectorAll("section.level2, section.footnotes");
    if (sections.length > 0) {
      // Main page entry (text before first section)
      const mainEl = doc.querySelector("main.content");
      const firstEl = mainEl?.firstElementChild;
      const pageText: string[] = [];
      if (firstEl) {
        firstEl.querySelectorAll("h1, h2, h3, h4, h5, h6").forEach((h) => h.remove());
        const trimmed = firstEl.textContent.trim();
        if (trimmed) pageText.push(trimmed);
        firstEl.remove();
      }

      // Add unsectioned paragraphs
      doc.querySelectorAll("main.content > p, main.content > div.cell").forEach((p) => {
        const text = p.textContent.trim();
        if (text) pageText.push(text);
        p.remove();
      });

      if (pageText.length > 0) {
        updateDoc({
          objectID: href,
          href: href,
          title,
          section: "",
          text: encodeHtml(pageText.join("\n")),
          crumbs,
        });
      }

      // Section entries (with #anchors)
      for (const section of sections) {
        const h2 = section.querySelector("h2");
        if (section.id) {
          const sectionTitle = h2 ? h2.textContent : "";
          const hrefWithAnchor = `${href}#${section.id}`;
          h2?.remove();
          const sectionText = section.textContent.trim();
          if (sectionText) {
            updateDoc({
              objectID: hrefWithAnchor,
              href: hrefWithAnchor,
              title,
              section: sectionTitle,
              text: encodeHtml(sectionText),
              crumbs,
            });
          }
        }
      }
    } else {
      // Single entry for whole page
      const main = doc.querySelector("main");
      if (main) {
        const mainText = main.textContent.trim();
        if (mainText) {
          updateDoc({
            objectID: href,
            href,
            title,
            section: "",
            text: encodeHtml(mainText),
            crumbs,
          });
        }
      }
    }
  }

  // Write updated search.json
  if (updatedSearchDocs.length > 0) {
    const updatedSearchJson = JSON.stringify(updatedSearchDocs, undefined, 2);
    Deno.writeTextFileSync(searchJsonPath, updatedSearchJson);
  }
}
```

**Generated search.json:**
```json
[
  {
    "objectID": "index.html",
    "href": "index.html",
    "title": "My Website",
    "section": "",
    "text": "Welcome to my website. This is the homepage content...",
    "crumbs": []
  },
  {
    "objectID": "guide.html",
    "href": "guide.html",
    "title": "User Guide",
    "section": "",
    "text": "This guide will help you get started...",
    "crumbs": ["Documentation", "User Guide"]
  },
  {
    "objectID": "guide.html#installation",
    "href": "guide.html#installation",
    "title": "User Guide",
    "section": "Installation",
    "text": "To install the software, run the following command...",
    "crumbs": ["Documentation", "User Guide"]
  }
]
```

**Search UI** (injected via navigation extras):
- Uses Fuse.js for client-side search (or Algolia for external indexing)
- Autocomplete UI with keyboard shortcuts (f, /, s)
- Shows breadcrumbs and section context in results
- Collapses multiple sections from same document

#### 11.3 Listing Generation (Blogs)

**File:** `src/project/types/website/listing/website-listing.ts`
**Function:** `completeListingGeneration(context, outputFiles, incremental)`

For pages with `listing:` in YAML (blog index pages):
- Generates RSS/Atom feeds
- Updates listing contents with latest posts
- Sorts by date/title
- Filters by category

#### 11.4 Aliases (Redirects)

**File:** `src/project/types/website/website-aliases.ts`
**Function:** `updateAliases(context, outputFiles, incremental)`

For pages with `aliases:` in YAML:
```yaml
---
title: My Page
aliases:
  - /old-url.html
  - /another-old-url.html
---
```

Generates redirect HTML pages at old URLs pointing to new location.

#### 11.5 Ensure Index Page

**File:** `src/project/types/website/website-navigation.ts`
**Function:** `ensureIndexPage(context)`

```typescript
export async function ensureIndexPage(project: ProjectContext) {
  const outputDir = projectOutputDir(project);
  const indexPage = join(outputDir, "index.html");

  if (!safeExistsSync(indexPage)) {
    // No index.html - create redirect to first input file
    const firstInput = project.files.input[0];
    if (firstInput) {
      const firstInputHref = relative(project.dir, firstInput);
      const resolved = await resolveInputTarget(project, firstInputHref);
      if (resolved) {
        writeRedirectPage(indexPage, resolved.outputHref);
      }
    }
  }
}

export function writeRedirectPage(path: string, href: string) {
  const redirectHtml = `
<!DOCTYPE html>
<html>
  <head>
    <meta http-equiv="refresh" content="0; url=${href}">
    <script>window.location.replace("${href}");</script>
  </head>
  <body>
    <p>Redirecting to <a href="${href}">${href}</a>...</p>
  </body>
</html>
  `;
  Deno.writeTextFileSync(path, redirectHtml);
}
```

## Multi-File Rendering Strategy

**File:** `src/command/render/render-files.ts`

Files are rendered **sequentially** (not in parallel):

```typescript
export async function renderFiles(
  files: RenderFile[],
  options: RenderOptions,
  notebookContext: NotebookContext,
  alwaysExecuteFiles: string[],
  pandocRenderer: PandocRenderer,
  project: ProjectContext,
) {
  const tempContext = createTempContext();

  // Sequential rendering
  for (const file of files) {
    const fileLifetime = createNamedLifetime("render-file");
    try {
      const result = await renderFileInternal(
        file,
        options,
        notebookContext,
        project,
        tempContext,
      );

      // Render immediately (defaultPandocRenderer)
      await pandocRenderer(result);
    } finally {
      fileLifetime.cleanup();
    }
  }

  tempContext.cleanup();
}
```

**Why sequential?**
1. **Shared state**: Navigation state built in pre-render used by all files
2. **Cross-references**: Later files may reference earlier files
3. **Predictable ordering**: Listings and feeds need consistent ordering
4. **Memory management**: Large projects would exhaust memory if all parallel

**Optimization opportunity for Rust port:** Parallelize within constraints:
- Group independent files (no cross-references)
- Render groups in parallel
- Maintain ordering within groups

## Complete Data Flow

```
User runs: quarto render (in website project directory)

Stage 0: Project Detection
├─ Search for _quarto.yml
├─ Read and validate config
├─ Detect project type: website
├─ Find all input files
└─ Create ProjectContext
    ↓
Stage 1: Pre-Render Hook
├─ Read website.navbar from config
├─ Read website.sidebar from config
├─ Resolve all navigation hrefs
├─ Build global navigation state
└─ Store in navigation singleton
    ↓
Stages 2-10: For Each File (sequential)
├─ [Same as single-document-render-pipeline.md]
├─ EXCEPT: formatExtras includes website-specific
│   ├─ Navigation HTML envelope
│   ├─ Search dependencies
│   ├─ Breadcrumbs
│   ├─ Prev/Next page links
│   └─ Website postprocessors
└─ Output: HTML file in _site/
    ↓
Stage 11: Post-Render Hooks
├─ 11.1: Generate sitemap.xml
│   ├─ List all HTML files
│   ├─ Add lastmod dates
│   └─ Write sitemap.xml + robots.txt
├─ 11.2: Generate search.json
│   ├─ Parse each HTML file
│   ├─ Extract title, sections, text
│   ├─ Add breadcrumbs from navigation
│   └─ Write search.json
├─ 11.3: Complete listings
│   ├─ Generate RSS feeds
│   └─ Update blog indexes
├─ 11.4: Generate aliases
│   └─ Create redirect HTML files
└─ 11.5: Ensure index.html
    └─ Create redirect if missing

Final output: Complete website in _site/
├─ index.html
├─ about.html
├─ guide.html
├─ sitemap.xml
├─ robots.txt
├─ search.json
└─ site_libs/ (CSS, JS, fonts)
```

## Key Website Features

### 1. Shared Navigation State

**Pattern:** Singleton navigation object

```typescript
// Global state (in website-shared.ts)
export const navigation: {
  navbar?: Navbar;
  sidebars: Sidebar[];
  pageNavigation: boolean;
  footer?: NavigationFooter;
  breadCrumbs?: Breadcrumb[];
  bodyDecorators?: BodyDecorator[];
  announcement?: Announcement;
} = {
  sidebars: [],
};
```

**Why:** All files need consistent navigation structure. Building it once in pre-render and reusing is much faster than rebuilding for each file.

### 2. Input Target Resolution

**Pattern:** Map input file paths to output hrefs

```typescript
interface InputTarget {
  input: string;           // Input file path (relative to project)
  outputHref: string;      // Output href ("/guide.html")
  title: string;           // Page title from metadata
  draft: boolean;          // Is this a draft?
}

// Built during pre-render from all project files
const inputTargets: Map<string, InputTarget> = new Map();

// Used by navigation, search, sitemaps
async function resolveInputTarget(project: ProjectContext, inputPath: string) {
  return inputTargets.get(inputPath);
}
```

**Why:** Navigation, breadcrumbs, and search need to reference other pages. This provides a central index.

### 3. Incremental Rendering

**Pattern:** Diff-based updates

```typescript
async function updateSearchIndex(context, outputFiles, incremental) {
  if (incremental && existsSync("search.json")) {
    // Load existing index
    const existing = JSON.parse(readFileSync("search.json"));

    // Update only changed files
    for (const file of outputFiles) {
      const existingDoc = existing.find((doc) => doc.href === file.href);
      if (existingDoc) {
        existingDoc.text = extractText(file);  // Update
      } else {
        existing.push(createSearchDoc(file));  // Insert
      }
    }

    writeFileSync("search.json", JSON.stringify(existing));
  } else {
    // Full rebuild
    const docs = outputFiles.map(createSearchDoc);
    writeFileSync("search.json", JSON.stringify(docs));
  }
}
```

**Why:** Re-rendering only changed files is much faster. Sitemap and search index support incremental updates.

### 4. Cross-Document References

**Pattern:** Link resolution postprocessor

```typescript
async function resolveProjectInputLinks(source, project, doc) {
  // Find all internal links
  const links = doc.querySelectorAll("a[href]");

  for (const link of links) {
    const href = link.getAttribute("href");

    // If href points to an input file (e.g., "guide.qmd")
    if (isInputFile(href)) {
      // Resolve to output href
      const target = await resolveInputTarget(project, href);
      if (target) {
        link.setAttribute("href", target.outputHref);  // "guide.html"
      }
    }
  }
}
```

**Why:** Authors write `[link](other-page.qmd)` but output needs `href="other-page.html"`.

## Timing Estimates

Assume 100-file website project:

| Stage | Time | Notes |
|-------|------|-------|
| 0. Project Detection | 50-200ms | Find config, parse YAML, discover files |
| 1. Pre-Render (Navigation) | 200-500ms | Resolve all sidebar/navbar hrefs |
| 2-10. File Rendering (×100) | **100-2000s** | 1-20s per file × 100 files |
| 11.1 Sitemap | 100-500ms | Generate XML for 100 files |
| 11.2 Search Index | 2-10s | Parse HTML, extract text for 100 files |
| 11.3 Listings | 100ms-2s | Generate RSS feeds |
| 11.4 Aliases | 10-100ms | Create redirect files |
| 11.5 Index Page | 10ms | Check/create redirect |
| **Total** | **100-2000s** | Dominated by file rendering |

**Bottleneck:** Individual file rendering (stages 2-10). For 100 files at 10s each = ~17 minutes.

**Optimization opportunities:**
1. **Parallelize file rendering** (within constraints)
2. **Cache engine execution** (freeze system already does this)
3. **Incremental rendering** (only changed files)

## Implications for Rust Port

### 1. Project Type Trait

```rust
#[async_trait]
pub trait ProjectType {
  fn type_name(&self) -> &str;
  fn lib_dir(&self) -> &str;
  fn output_dir(&self) -> &str;

  async fn pre_render(&self, context: &ProjectContext) -> Result<()> {
    Ok(())  // Default: no-op
  }

  async fn format_extras(
    &self,
    project: &ProjectContext,
    source: &Path,
    format: &Format,
  ) -> Result<FormatExtras> {
    Ok(FormatExtras::default())  // Default: no extras
  }

  async fn post_render(
    &self,
    context: &ProjectContext,
    incremental: bool,
    output_files: &[ProjectOutputFile],
  ) -> Result<()> {
    Ok(())  // Default: no-op
  }
}

pub struct WebsiteProjectType {
  navigation: Arc<RwLock<NavigationState>>,
}

#[async_trait]
impl ProjectType for WebsiteProjectType {
  fn type_name(&self) -> &str { "website" }
  fn lib_dir(&self) -> &str { "site_libs" }
  fn output_dir(&self) -> &str { "_site" }

  async fn pre_render(&self, context: &ProjectContext) -> Result<()> {
    let nav = init_website_navigation(context).await?;
    *self.navigation.write().unwrap() = nav;
    Ok(())
  }

  async fn format_extras(
    &self,
    project: &ProjectContext,
    source: &Path,
    format: &Format,
  ) -> Result<FormatExtras> {
    let nav = self.navigation.read().unwrap();
    website_navigation_extras(project, source, format, &nav).await
  }

  async fn post_render(
    &self,
    context: &ProjectContext,
    incremental: bool,
    output_files: &[ProjectOutputFile],
  ) -> Result<()> {
    website_post_render(context, incremental, output_files).await
  }
}
```

### 2. Navigation State Management

```rust
pub struct NavigationState {
  pub navbar: Option<Navbar>,
  pub sidebars: Vec<Sidebar>,
  pub page_navigation: bool,
  pub footer: Option<NavigationFooter>,
  pub breadcrumbs: Vec<Breadcrumb>,
}

// Thread-safe singleton
pub struct WebsiteProjectType {
  navigation: Arc<RwLock<NavigationState>>,
}

// Pre-render: Write navigation state
async fn init_website_navigation(context: &ProjectContext) -> Result<NavigationState> {
  let config = &context.config.website;

  let navbar = if let Some(navbar_config) = &config.navbar {
    Some(resolve_navbar(context, navbar_config).await?)
  } else {
    None
  };

  let mut sidebars = Vec::new();
  if let Some(sidebar_configs) = &config.sidebar {
    for sidebar_config in sidebar_configs {
      sidebars.push(resolve_sidebar(context, sidebar_config).await?);
    }
  }

  Ok(NavigationState {
    navbar,
    sidebars,
    page_navigation: config.page_navigation.unwrap_or(false),
    footer: config.footer.clone(),
    breadcrumbs: Vec::new(),
  })
}

// Per-file rendering: Read navigation state
async fn website_navigation_extras(
  project: &ProjectContext,
  source: &Path,
  format: &Format,
  nav: &NavigationState,
) -> Result<FormatExtras> {
  let href = compute_output_href(project, source)?;
  let sidebar = sidebar_for_href(&href, &nav.sidebars);
  let breadcrumbs = compute_breadcrumbs(&href, sidebar);

  let nav_data = NavData {
    navbar: nav.navbar.clone(),
    sidebar: sidebar.map(|s| expand_sidebar(&href, s)),
    prev_page: prev_page(&href, sidebar),
    next_page: next_page(&href, sidebar),
    breadcrumbs,
  };

  let body_envelope = BodyEnvelope {
    before: render_template("nav-before-body.ejs", &nav_data)?,
    after_preamble: render_template("nav-after-body-preamble.ejs", &nav_data)?,
    after_postamble: render_template("nav-after-body-postamble.ejs", &nav_data)?,
  };

  Ok(FormatExtras {
    html: HtmlExtras {
      body_envelope: Some(body_envelope),
      postprocessors: vec![
        Box::new(NavigationHtmlPostprocessor::new(project, nav_data)),
      ],
      dependencies: vec![navigation_dependency()],
      sass_bundles: vec![navigation_sass_bundle()],
    },
  })
}
```

### 3. Search Index Generation

```rust
pub struct SearchDoc {
  pub object_id: String,
  pub href: String,
  pub title: String,
  pub section: String,
  pub text: String,
  pub crumbs: Vec<String>,
}

pub async fn update_search_index(
  context: &ProjectContext,
  output_files: &[ProjectOutputFile],
  incremental: bool,
) -> Result<()> {
  let output_dir = project_output_dir(context);
  let search_json_path = output_dir.join("search.json");

  // Load existing if incremental
  let mut search_docs = if incremental && search_json_path.exists() {
    let json = tokio::fs::read_to_string(&search_json_path).await?;
    serde_json::from_str(&json)?
  } else {
    Vec::new()
  };

  for output_file in output_files {
    // Skip if search disabled
    if output_file.format.metadata.get("search") == Some(&Value::Bool(false)) {
      // Remove existing entries
      search_docs.retain(|doc: &SearchDoc| !doc.href.starts_with(&output_file.href));
      continue;
    }

    // Parse HTML
    let html = tokio::fs::read_to_string(&output_file.file).await?;
    let document = Html::parse_document(&html);

    // Extract title
    let selector = Selector::parse("h1.title").unwrap();
    let title = document.select(&selector).next()
      .and_then(|el| el.text().collect::<Vec<_>>().join(""))
      .unwrap_or_else(|| website_title(&context.config));

    // Compute breadcrumbs
    let crumbs = compute_breadcrumbs_for_search(&output_file);

    // Extract sections
    let sections = extract_sections(&document)?;

    // Main page entry
    let main_text = extract_main_text(&document)?;
    if !main_text.is_empty() {
      search_docs.push(SearchDoc {
        object_id: output_file.href.clone(),
        href: output_file.href.clone(),
        title: title.clone(),
        section: String::new(),
        text: html_escape(&main_text),
        crumbs: crumbs.clone(),
      });
    }

    // Section entries
    for section in sections {
      search_docs.push(SearchDoc {
        object_id: format!("{}#{}", output_file.href, section.id),
        href: format!("{}#{}", output_file.href, section.id),
        title: title.clone(),
        section: section.title,
        text: html_escape(&section.text),
        crumbs: crumbs.clone(),
      });
    }
  }

  // Write search.json
  let json = serde_json::to_string_pretty(&search_docs)?;
  tokio::fs::write(&search_json_path, json).await?;

  Ok(())
}
```

### 4. Parallelization Strategy

```rust
pub async fn render_files(
  files: &[RenderFile],
  project: &ProjectContext,
  options: &RenderOptions,
) -> Result<Vec<ProjectOutputFile>> {
  // Analyze dependencies between files
  let dep_graph = build_dependency_graph(files, project)?;

  // Topologically sort into groups
  let file_groups = topological_group_sort(&dep_graph)?;

  let mut all_outputs = Vec::new();

  for group in file_groups {
    // Render files within group in parallel
    let tasks: Vec<_> = group.iter().map(|file| {
      render_file_internal(file, project, options)
    }).collect();

    let outputs = futures::future::try_join_all(tasks).await?;
    all_outputs.extend(outputs);
  }

  Ok(all_outputs)
}

fn build_dependency_graph(
  files: &[RenderFile],
  project: &ProjectContext,
) -> Result<DependencyGraph> {
  let mut graph = DependencyGraph::new();

  for file in files {
    // Parse file to find cross-document links
    let links = extract_cross_document_links(file, project)?;

    for link in links {
      // Add edge: file -> linked_file
      graph.add_edge(file.path.clone(), link);
    }
  }

  Ok(graph)
}

fn topological_group_sort(graph: &DependencyGraph) -> Result<Vec<Vec<RenderFile>>> {
  // Kahn's algorithm variant that groups independent nodes
  let mut groups = Vec::new();
  let mut remaining = graph.clone();

  while !remaining.is_empty() {
    // Find all nodes with no incoming edges
    let independent: Vec<_> = remaining.nodes()
      .filter(|node| remaining.incoming_edges(node).is_empty())
      .cloned()
      .collect();

    if independent.is_empty() {
      return Err(Error::CyclicDependency);
    }

    // This group can be rendered in parallel
    groups.push(independent.clone());

    // Remove these nodes from graph
    for node in independent {
      remaining.remove_node(&node);
    }
  }

  Ok(groups)
}
```

### 5. Sitemap Generation

```rust
pub async fn update_sitemap(
  context: &ProjectContext,
  output_files: &[ProjectOutputFile],
  incremental: bool,
) -> Result<()> {
  let output_dir = project_output_dir(context);
  let sitemap_path = output_dir.join("sitemap.xml");
  let base_url = website_baseurl(&context.config)?;

  let mut entries = if incremental && sitemap_path.exists() {
    read_sitemap(&sitemap_path).await?
  } else {
    Vec::new()
  };

  for output_file in output_files {
    let href = pathdiff::diff_paths(&output_file.file, &output_dir)
      .unwrap()
      .to_string_lossy()
      .replace('\\', "/");

    let loc = format!("{}{}", base_url, href);
    let lastmod = file_modified_iso8601(&output_file.file).await?;

    if let Some(entry) = entries.iter_mut().find(|e| e.loc == loc) {
      entry.lastmod = lastmod;
    } else {
      entries.push(UrlsetEntry { loc, lastmod });
    }
  }

  write_sitemap(&sitemap_path, &entries).await?;

  // Create robots.txt
  let robots_path = output_dir.join("robots.txt");
  if !robots_path.exists() {
    tokio::fs::write(&robots_path, format!("Sitemap: {}sitemap.xml\n", base_url)).await?;
  }

  Ok(())
}

struct UrlsetEntry {
  loc: String,
  lastmod: String,
}

async fn write_sitemap(path: &Path, entries: &[UrlsetEntry]) -> Result<()> {
  use quick_xml::events::{Event, BytesStart, BytesEnd, BytesText};
  use quick_xml::Writer;

  let mut writer = Writer::new(Vec::new());

  writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

  let mut urlset = BytesStart::new("urlset");
  urlset.push_attribute(("xmlns", "http://www.sitemaps.org/schemas/sitemap/0.9"));
  writer.write_event(Event::Start(urlset))?;

  for entry in entries {
    writer.write_event(Event::Start(BytesStart::new("url")))?;

    writer.write_event(Event::Start(BytesStart::new("loc")))?;
    writer.write_event(Event::Text(BytesText::new(&entry.loc)))?;
    writer.write_event(Event::End(BytesEnd::new("loc")))?;

    writer.write_event(Event::Start(BytesStart::new("lastmod")))?;
    writer.write_event(Event::Text(BytesText::new(&entry.lastmod)))?;
    writer.write_event(Event::End(BytesEnd::new("lastmod")))?;

    writer.write_event(Event::End(BytesEnd::new("url")))?;
  }

  writer.write_event(Event::End(BytesEnd::new("urlset")))?;

  tokio::fs::write(path, writer.into_inner()).await?;
  Ok(())
}
```

## Key Source Locations

- Project context: `src/project/project-context.ts`
- Website project type: `src/project/types/website/website.ts`
- Navigation: `src/project/types/website/website-navigation.ts`
- Search: `src/project/types/website/website-search.ts`
- Sitemap: `src/project/types/website/website-sitemap.ts`
- Listings: `src/project/types/website/listing/website-listing.ts`
- Aliases: `src/project/types/website/website-aliases.ts`

## Conclusion

Website project rendering extends the single-document pipeline with:

1. **Pre-render hooks**: Build shared navigation state once
2. **Format extras injection**: Add navigation/search/breadcrumbs per file
3. **Multi-file coordination**: Sequential rendering with shared state
4. **Post-render hooks**: Generate sitemap, search index, listings, aliases

The Rust port needs:
- **ProjectType trait** with pre/post-render hooks
- **Thread-safe navigation state** (Arc<RwLock<>>)
- **Dependency graph analysis** for safe parallelization
- **Incremental update support** for sitemap/search
- **HTML parsing** for search indexing (scraper crate)
- **XML generation** for sitemaps (quick-xml crate)
