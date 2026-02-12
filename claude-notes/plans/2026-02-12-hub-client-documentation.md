# Hub-Client Documentation

**Beads Issue:** bd-3n80
**Status:** Implementation Complete

## Overview

Create user-facing documentation for Quarto Hub (hub-client) in the `docs/quarto-hub/` directory. The documentation should introduce users to the collaborative editing prototype and explain its key features.

## Goals

1. **Accessible to users**: Focus on how to use features, not technical implementation
2. **Consistent style**: Match the existing docs structure and tone
3. **Prototype disclaimer**: Make clear this is experimental/prototype software
4. **Feature coverage**: Document the main user-facing capabilities

## Documentation Structure

```
docs/
  _quarto.yml          # Update navbar to include Quarto Hub
  quarto-hub/
    index.qmd          # Overview and getting started
    files.qmd          # File management (sidebar, new files, upload)
    preview.qmd        # Live preview and rendering
    themes.qmd         # Theme customization (Bootswatch, custom SCSS)
    templates.qmd      # Project-specific file templates
    projects.qmd       # Creating and managing projects
    collaboration.qmd  # Sharing and real-time sync
```

## Page Outlines

### index.qmd - Overview

- What is Quarto Hub?
- Prototype status disclaimer
- Key capabilities (browser-based, collaborative, live preview)
- Quick start guide
- Links to detailed feature pages

### files.qmd - File Management

- File sidebar navigation
- Creating new text files
- Uploading images and binary files
- File organization (folders)
- Supported file types

### preview.qmd - Live Preview

- Real-time rendering
- Side-by-side editing and preview
- Scroll synchronization
- Error display and diagnostics
- Document outline navigation

### themes.qmd - Themes

- Default Bootstrap theme
- Bootswatch theme selection
- Custom SCSS files
- Theme frontmatter syntax

### templates.qmd - File Templates

- What are templates?
- Creating a templates directory (`_quarto-hub-templates/`)
- Template file structure (`.qmd` with `template-name` metadata)
- Using templates to create new files
- Example templates

### projects.qmd - Projects

- Creating a new project
- Project types (default, website)
- Project configuration (`_quarto.yml`)
- Opening existing projects

### collaboration.qmd - Collaboration

- How real-time sync works (high-level)
- Sharing projects via URL
- Security considerations
- Multiple users editing simultaneously

## Work Items

### Phase 1: Setup

- [x] Create `docs/quarto-hub/` directory
- [x] Update `docs/_quarto.yml` to add Quarto Hub to navbar
- [x] Create `docs/quarto-hub/index.qmd` with overview

### Phase 2: Core Feature Pages

- [x] Create `files.qmd` - File management documentation
- [x] Create `preview.qmd` - Live preview documentation
- [x] Create `themes.qmd` - Theme customization documentation

### Phase 3: Advanced Feature Pages

- [x] Create `templates.qmd` - File templates documentation
- [x] Create `projects.qmd` - Project management documentation
- [x] Create `collaboration.qmd` - Collaboration documentation

### Phase 4: Polish

- [x] Review all pages for consistency
- [x] Add cross-links between pages
- [ ] Test documentation renders correctly (requires Quarto CLI)
- [x] Verify all features described are accurate

## Style Guidelines

Based on existing docs:

1. **Frontmatter**: Simple title, no extra metadata
2. **Structure**: Start with brief intro, use `##` for main sections
3. **Tone**: Clear, professional, helpful
4. **Code examples**: Use fenced code blocks with language hints
5. **Links**: Use relative links to other docs pages
6. **Lists**: Use bullet points for feature lists
7. **Work in Progress**: Include disclaimers where features are experimental

## Example Page Structure

```markdown
---
title: "Page Title"
---

Brief introduction paragraph explaining what this page covers.

## Main Section

Content with clear explanations.

### Subsection

More detailed information.

## Another Section

- Bullet point feature
- Another feature
- Third feature

## See Also

- [Related Page](related.qmd)
```

## Notes

- Hub-client is a prototype; documentation should set appropriate expectations
- Focus on what users can do today, not future plans
- Keep technical details minimal (link to library-docs for internals)
- Screenshots could be valuable but are out of scope for initial docs
