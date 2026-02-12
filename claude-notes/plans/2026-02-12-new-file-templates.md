# New File Templates Feature

**Beads Issue:** bd-1uky
**Status:** Implementation Complete - Awaiting Review

## Overview

Allow hub-client users to create new files from project-specific templates stored in a `_quarto-hub-templates` directory. Templates are `.qmd` files identified by their `template-name` metadata. The New File dialog shows a dropdown selector populated from available templates.

## Design Goals

1. **Per-project customization**: Each project can define its own templates
2. **Self-documenting**: Templates are regular `.qmd` files that can be previewed/edited
3. **Simple convention**: No configuration files needed, just drop `.qmd` files in the right folder
4. **Graceful degradation**: Works without templates (current behavior preserved)

## Design Decisions (Resolved)

1. **Sidebar visibility**: Templates folder IS visible in sidebar (users need to edit templates)
2. **Missing template-name**: Use filename as display name fallback (e.g., "article" from `article.qmd`)
3. **Nested directories**: Not supported - only top-level `.qmd` files in `_quarto-hub-templates/`
4. **Content manipulation**: Use AST manipulation (parse → modify ConfigValue → re-serialize), NOT text manipulation

## User Flow

1. User clicks "+ New" to create a new file
2. In the "New Text File" tab, a template dropdown appears (if templates exist)
3. Dropdown shows: "Blank file" (default) + list of templates by name
4. User selects a template and enters a filename
5. New file is created with the template's content (with `template-name` stripped)

## Conventions

### Template Directory

```
project-root/
  _quarto-hub-templates/
    article.qmd          # template-name: "Article"
    presentation.qmd     # template-name: "Presentation"
    report.qmd           # no template-name → displays as "report"
    subfolder/           # ignored (only top-level .qmd files)
      draft.qmd
```

- **Location**: `_quarto-hub-templates/` at the project root
- **Files**: Only top-level `.qmd` files are considered templates
- **Naming**: `template-name` metadata provides display name; filename is fallback
- **Visible**: Directory appears in sidebar for easy template editing

### Template Metadata

```yaml
---
template-name: "Article with References"
title: "Untitled Article"
author: ""
format: html
---

# Introduction

Start writing here...
```

- **Optional**: `template-name` field in YAML frontmatter (filename used as fallback)
- **Display**: The `template-name` value (or filename) appears in the dropdown
- **Stripped**: When creating a file from template, `template-name` is removed from the content

## Implementation Plan

### Phase 1: WASM - Template Processing via AST

Add Rust functions to process templates using proper AST manipulation.

**Key insight**: We already have `parse_qmd_content` (QMD → JSON AST) and `ast_to_qmd` (JSON AST → QMD). For template processing, we add a single function that:
1. Parses QMD to Pandoc AST
2. Extracts `template-name` from `meta` ConfigValue (if present)
3. Removes `template-name` from `meta`
4. Re-serializes to QMD
5. Returns both the template name and stripped content

**File:** `crates/wasm-quarto-hub-client/src/lib.rs`

```rust
/// Process a template file: extract template-name and produce stripped content.
///
/// Returns JSON:
/// {
///   "success": true,
///   "templateName": "Article" | null,  // null if not present
///   "strippedContent": "---\ntitle: ...\n---\n\n# Intro..."
/// }
/// or { "success": false, "error": "..." }
#[wasm_bindgen]
pub fn prepare_template(content: &str) -> String
```

**Implementation approach:**

```rust
fn prepare_template_impl(content: &str) -> Result<(Option<String>, String), String> {
    use pampa::wasm_entry_points::qmd_to_pandoc;
    use pampa::writers::qmd::write as qmd_write;

    // 1. Parse QMD to Pandoc AST
    let (mut pandoc, _context) = qmd_to_pandoc(content.as_bytes())
        .map_err(|errs| errs.join("; "))?;

    // 2. Extract template-name from meta (which is a ConfigValue::Map)
    let template_name = extract_and_remove_template_name(&mut pandoc.meta);

    // 3. Re-serialize to QMD
    let mut buf = Vec::new();
    qmd_write(&pandoc, &mut buf)
        .map_err(|e| format!("Failed to write QMD: {:?}", e))?;

    let stripped = String::from_utf8(buf)
        .map_err(|e| format!("Invalid UTF-8: {}", e))?;

    Ok((template_name, stripped))
}

fn extract_and_remove_template_name(meta: &mut ConfigValue) -> Option<String> {
    if let ConfigValueKind::Map(entries) = &mut meta.value {
        // Find and remove template-name entry
        let mut template_name = None;
        entries.retain(|entry| {
            if entry.key == "template-name" {
                // Extract as plain text (handles both Scalar and PandocInlines)
                template_name = entry.value.as_plain_text();
                false // remove this entry
            } else {
                true // keep
            }
        });
        template_name
    } else {
        None
    }
}
```

**File:** `hub-client/src/types/wasm-quarto-hub-client.d.ts`

```typescript
export function prepare_template(content: string): string;

// Response type
interface PrepareTemplateResponse {
  success: true;
  templateName: string | null;
  strippedContent: string;
} | {
  success: false;
  error: string;
}
```

#### Work Items

- [x] Implement `prepare_template` in `wasm-quarto-hub-client`
- [x] Add helper function `extract_and_remove_template_name` for ConfigValue manipulation
- [x] Update TypeScript type declarations
- [ ] Write unit tests (Rust side) - deferred to Phase 4
- [x] Build WASM and verify with manual test

### Phase 2: Hub-Client - Template Discovery Service

Create a service to discover and cache available templates.

**File:** `hub-client/src/services/templateService.ts` (new file)

```typescript
import { vfsReadFile, vfsListFiles } from './wasmRenderer';
import { prepare_template } from 'wasm-quarto-hub-client';

interface ProjectTemplate {
  path: string;            // e.g., "/project/_quarto-hub-templates/article.qmd"
  displayName: string;     // template-name or filename fallback
  strippedContent: string; // Content with template-name removed
}

const TEMPLATES_DIR = '/project/_quarto-hub-templates/';

/**
 * Discover all templates in the project.
 * Scans VFS for .qmd files in _quarto-hub-templates/ and processes them.
 */
export function discoverTemplates(): ProjectTemplate[] {
  const allFiles = vfsListFiles();
  const templateFiles = allFiles.filter(f =>
    f.startsWith(TEMPLATES_DIR) &&
    f.endsWith('.qmd') &&
    !f.slice(TEMPLATES_DIR.length).includes('/') // top-level only
  );

  const templates: ProjectTemplate[] = [];

  for (const path of templateFiles) {
    const content = vfsReadFile(path);
    if (!content) continue;

    const result = JSON.parse(prepare_template(content));
    if (!result.success) continue;

    // Use template-name if present, otherwise derive from filename
    const filename = path.slice(TEMPLATES_DIR.length);
    const displayName = result.templateName ??
      filename.replace(/\.qmd$/, '');

    templates.push({
      path,
      displayName,
      strippedContent: result.strippedContent,
    });
  }

  // Sort alphabetically by display name
  templates.sort((a, b) => a.displayName.localeCompare(b.displayName));

  return templates;
}
```

#### Work Items

- [x] Create `templateService.ts`
- [x] Implement `discoverTemplates()` function
- [x] Handle VFS path prefix (`/project/` vs bare paths)
- [ ] Write tests - deferred to Phase 4

### Phase 3: Hub-Client - UI Integration

Modify NewFileDialog to show template selector.

**File:** `hub-client/src/components/NewFileDialog.tsx`

Changes to the "New Text File" mode:

```tsx
// New state
const [templates, setTemplates] = useState<ProjectTemplate[]>([]);
const [selectedTemplate, setSelectedTemplate] = useState<ProjectTemplate | null>(null);
const [loadingTemplates, setLoadingTemplates] = useState(false);

// Load templates when dialog opens
useEffect(() => {
  if (isOpen && mode === 'text') {
    setLoadingTemplates(true);
    try {
      const discovered = discoverTemplates();
      setTemplates(discovered);
    } finally {
      setLoadingTemplates(false);
    }
  }
}, [isOpen, mode]);

// Reset template selection when dialog closes
useEffect(() => {
  if (!isOpen) {
    setSelectedTemplate(null);
    setTemplates([]);
  }
}, [isOpen]);

// Updated handleCreateTextFile
const handleCreateTextFile = useCallback(() => {
  const validationError = validateFilename(filename);
  if (validationError) {
    setError(validationError);
    return;
  }

  const content = selectedTemplate?.strippedContent ?? '';
  onCreateTextFile(filename, content);
  onClose();
}, [filename, selectedTemplate, validateFilename, onCreateTextFile, onClose]);
```

**UI additions in text mode:**

```tsx
<div className="text-file-form">
  {templates.length > 0 && (
    <div className="template-selector">
      <label htmlFor="template">Template:</label>
      <select
        id="template"
        value={selectedTemplate?.path ?? ''}
        onChange={(e) => {
          const template = templates.find(t => t.path === e.target.value);
          setSelectedTemplate(template ?? null);
        }}
      >
        <option value="">Blank file</option>
        {templates.map((t) => (
          <option key={t.path} value={t.path}>
            {t.displayName}
          </option>
        ))}
      </select>
    </div>
  )}

  <label htmlFor="filename">Filename:</label>
  <input ... />
</div>
```

**File:** `hub-client/src/components/NewFileDialog.css`

```css
.template-selector {
  margin-bottom: 1rem;
}

.template-selector label {
  display: block;
  margin-bottom: 0.25rem;
  font-weight: 500;
}

.template-selector select {
  width: 100%;
  padding: 0.5rem;
  border: 1px solid var(--border-color);
  border-radius: 4px;
  background: var(--bg-color);
  font-size: 0.9rem;
}
```

#### Work Items

- [x] Add template state management
- [x] Load templates when dialog opens (text mode)
- [x] Add template dropdown UI
- [x] Update `handleCreateTextFile` to use template content
- [x] Style the dropdown to match existing dialog design
- [x] Handle loading state (templates might take a moment to load)
- [ ] Write component tests - deferred to Phase 4

### Phase 4: Polish and Edge Cases

- [x] Handle template discovery errors gracefully (log, continue without templates)
- [ ] Consider caching templates and invalidating on file changes (future)
- [x] Add keyboard navigation for dropdown (already native with `<select>`)
- [ ] Consider showing template preview on hover/selection (future enhancement)
- [ ] Documentation: add a brief note in hub-client docs about template support
- [x] Write unit tests for Rust `prepare_template` function (WASM tests: `prepareTemplate.wasm.test.ts`)
- [x] Write tests for `templateService.ts` (`templateService.test.ts`)
- [x] Write component tests for NewFileDialog template feature (`NewFileDialog.integration.test.tsx`)

## Technical Considerations

### VFS Path Handling

The VFS normalizes all paths to use `/project/` as the root:
- `vfsAddFile("index.qmd", ...)` stores as `/project/index.qmd`
- `vfsListFiles()` returns paths WITH the `/project/` prefix
- `vfsReadFile()` accepts paths with OR without the prefix (it normalizes internally)

Therefore, template discovery must use:
```typescript
const TEMPLATES_DIR = '/project/_quarto-hub-templates/';
```

### AST Manipulation Details

The `prepare_template` function modifies the Pandoc AST:

1. **Input**: QMD source text
2. **Parse**: `qmd_to_pandoc()` → `Pandoc { meta: ConfigValue, blocks: Blocks }`
3. **Extract**: Find `template-name` in `meta.value` (a `ConfigValueKind::Map`)
4. **Mutate**: Remove the `template-name` entry from the map
5. **Serialize**: `qmd_write(&pandoc)` → QMD text without `template-name`

The `as_plain_text()` method on ConfigValue handles both:
- `Scalar(Yaml::String(s))` → returns `s`
- `PandocInlines(inlines)` → converts inlines to plain text

### Performance

Template discovery happens when the dialog opens:
- VFS is already in memory (synced via Automerge)
- `vfsListFiles()` is O(n) where n = number of files
- `prepare_template()` parses each template file once
- For typical projects (< 10 templates), this is instant

### Future Enhancements

1. **Template preview**: Show a rendered preview when template is selected
2. **Template categories**: Support `template-category` for grouping
3. **Template assets**: Allow templates to reference images in the templates folder
4. **Smart filename**: Suggest filename based on template (e.g., `chapter-1.qmd`)

## Success Criteria

1. Users can create project-specific templates by adding `.qmd` files to `_quarto-hub-templates`
2. Templates appear in the New File dialog dropdown
3. `template-name` metadata provides display name (filename as fallback)
4. Creating a file from a template pre-populates it correctly (without `template-name`)
5. Templates folder is visible and editable in the sidebar
6. System handles edge cases gracefully (no templates, parse errors, etc.)
