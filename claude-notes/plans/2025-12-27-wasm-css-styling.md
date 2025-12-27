# WASM Artifact and Styling System

**Issue:** k-giyy
**Date:** 2025-12-27
**Status:** Ready for implementation

## Problem

The WASM client produces correct DOM structure but lacks styling. The CLI writes CSS to disk and links to it; the WASM path has no mechanism to provide resources to the rendered HTML.

## Solution Architecture

A two-piece system:

1. **Render pipeline produces artifacts** → stored in VFS at well-known paths
2. **React post-processor** → walks iframe DOM, replaces file references with data URIs

This keeps the render pipeline "pure" (normal HTML with file paths) while the presentation layer handles browser-specific transformations.

## Design Decisions

### 1. Artifact Path Convention

```
/.quarto/project-artifacts/
```

Rationale: Aligns with CLI's `.quarto` project directory. As more project information is shared between CLI and collaborative editor, paths will work automatically.

### 2. Artifact Exposure

**Simplest option**: Auto-populate VFS after render.

After `render_qmd_to_html()` completes, artifacts are written to VFS at their designated paths. The React SPA reads them via existing VFS functions.

### 3. Post-Processing Timing

**State-driven via custom hook**.

The Editor component uses a persistent iframe with `srcDoc`. Post-processing is triggered through a React-idiomatic pattern:

1. iframe `onLoad` event updates state (doesn't do work directly)
2. `useEffect` reacts to state change and runs post-processor
3. All logic encapsulated in `useIframePostProcessor` hook

This separates "something happened" (state update) from "react to change" (useEffect), following React conventions and making the code more resilient to refactoring.

No `MutationObserver` needed - future incremental rendering will address dynamic content.

### 4. Post-Processor Scope

**Editor-specific utility** in `hub-client/src/utils/` or similar.

Not a generic library - tailored to collaborative editor needs.

### 5. Edge Cases

| Case | Approach |
|------|----------|
| Large images | Accept for now; incremental renderer will address |
| Recursive .qmd links | Handle relative paths using project path context |
| Content updates | Re-run post-processor; DOM is small for prototypes |

## Implementation Plan

### Phase 1: Pipeline Artifact Collection

**Files:** `crates/quarto-core/src/pipeline.rs`

1. After rendering, store CSS in ArtifactStore:
   ```rust
   ctx.artifacts.store(
       "css:default",
       Artifact::from_string(DEFAULT_CSS, "text/css")
           .with_path(PathBuf::from("/.quarto/project-artifacts/styles.css"))
   );
   ```

2. Modify template to reference artifact path:
   ```html
   <link rel="stylesheet" href="/.quarto/project-artifacts/styles.css">
   ```

### Phase 2: WASM Artifact Exposure

**Files:** `crates/wasm-quarto-hub-client/src/lib.rs`

After successful render, populate VFS with artifacts:

```rust
// In render_qmd_content, after successful render
for (key, artifact) in ctx.artifacts.iter() {
    if let Some(path) = &artifact.path {
        get_runtime().add_file(path, artifact.content.clone());
    }
}
```

Return render result as before - artifacts are now in VFS.

### Phase 3: React Post-Processor

**Files:**
- `hub-client/src/hooks/useIframePostProcessor.ts` (new)
- `hub-client/src/utils/iframePostProcessor.ts` (new)
- `hub-client/src/components/Editor.tsx` (modify)

#### useIframePostProcessor.ts (Custom Hook)

```typescript
import { RefObject, useCallback, useEffect, useState } from 'react';
import { postProcessIframe, PostProcessOptions } from '../utils/iframePostProcessor';

/**
 * React hook for post-processing iframe content after render.
 *
 * Follows React-idiomatic patterns:
 * - onLoad handler just updates state (doesn't do work)
 * - useEffect reacts to state and runs post-processor
 * - Encapsulates all post-processing logic
 */
export function useIframePostProcessor(
  iframeRef: RefObject<HTMLIFrameElement>,
  options: PostProcessOptions
) {
  // Track load events as state changes
  const [loadCount, setLoadCount] = useState(0);

  // Handler just signals "iframe loaded" - no work here
  const handleLoad = useCallback(() => {
    setLoadCount(n => n + 1);
  }, []);

  // Work happens in useEffect, reacting to state
  useEffect(() => {
    if (loadCount > 0 && iframeRef.current?.contentDocument) {
      postProcessIframe(iframeRef.current, options);
    }
  }, [loadCount, iframeRef, options]);

  return { handleLoad };
}
```

#### iframePostProcessor.ts (Processing Logic)

```typescript
import { vfsReadFile } from '../services/wasmRenderer';

export interface PostProcessOptions {
  /** Current file path for resolving relative links */
  currentFilePath: string;
  /** Callback when user clicks a .qmd link */
  onQmdLinkClick?: (targetPath: string) => void;
}

/**
 * Post-process iframe content after render.
 * - Replaces /.quarto/ resource links with data URIs
 * - Converts .qmd links to click handlers
 */
export function postProcessIframe(
  iframe: HTMLIFrameElement,
  options: PostProcessOptions
): void {
  const doc = iframe.contentDocument;
  if (!doc) return;

  // Replace CSS links with data URIs
  doc.querySelectorAll('link[rel="stylesheet"]').forEach(link => {
    const href = link.getAttribute('href');
    if (href?.startsWith('/.quarto/')) {
      const result = vfsReadFile(href);
      if (result.success && result.content) {
        const dataUri = `data:text/css;base64,${btoa(result.content)}`;
        link.setAttribute('href', dataUri);
      }
    }
  });

  // Replace image sources with data URIs
  doc.querySelectorAll('img').forEach(img => {
    const src = img.getAttribute('src');
    if (src?.startsWith('/.quarto/')) {
      const result = vfsReadFile(src);
      if (result.success && result.content) {
        const mimeType = guessMimeType(src);
        // For binary files, content should already be base64 encoded
        const dataUri = `data:${mimeType};base64,${result.content}`;
        img.setAttribute('src', dataUri);
      }
    }
  });

  // Convert .qmd links to click handlers
  if (options.onQmdLinkClick) {
    doc.querySelectorAll('a[href$=".qmd"]').forEach(anchor => {
      const href = anchor.getAttribute('href');
      if (href) {
        const targetPath = resolveRelativePath(
          options.currentFilePath,
          href
        );
        anchor.addEventListener('click', (e) => {
          e.preventDefault();
          options.onQmdLinkClick!(targetPath);
        });
        // Visual hint that it's an internal link
        anchor.setAttribute('data-internal-link', 'true');
      }
    });
  }
}

/** Resolve a relative path against the current file's directory */
function resolveRelativePath(currentFile: string, relativePath: string): string {
  if (relativePath.startsWith('/')) {
    return relativePath; // Already absolute
  }
  // Get directory of current file
  const lastSlash = currentFile.lastIndexOf('/');
  const currentDir = lastSlash >= 0 ? currentFile.substring(0, lastSlash + 1) : '/';
  return normalizePath(currentDir + relativePath);
}

function normalizePath(path: string): string {
  const parts = path.split('/').filter(p => p !== '.');
  const result: string[] = [];
  for (const part of parts) {
    if (part === '..') {
      result.pop();
    } else if (part) {
      result.push(part);
    }
  }
  return '/' + result.join('/');
}

function guessMimeType(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase();
  const mimeTypes: Record<string, string> = {
    'png': 'image/png',
    'jpg': 'image/jpeg',
    'jpeg': 'image/jpeg',
    'gif': 'image/gif',
    'svg': 'image/svg+xml',
    'webp': 'image/webp',
    'css': 'text/css',
    'js': 'text/javascript',
  };
  return mimeTypes[ext || ''] || 'application/octet-stream';
}
```

#### Editor.tsx Changes

```tsx
// Add imports
import { useIframePostProcessor } from '../hooks/useIframePostProcessor';

// Add handler for .qmd link clicks
const handleQmdLinkClick = useCallback((targetPath: string) => {
  const file = files.find(f => f.path === targetPath || '/' + f.path === targetPath);
  if (file) {
    setCurrentFile(file);
  }
}, [files]);

// Use the custom hook
const { handleLoad } = useIframePostProcessor(iframeRef, {
  currentFilePath: currentFile?.path ?? '',
  onQmdLinkClick: handleQmdLinkClick,
});

// Update iframe element
<iframe
  ref={iframeRef}
  srcDoc={previewHtml}
  title="Preview"
  sandbox="allow-same-origin"
  onLoad={handleLoad}
/>
```

### Phase 4: Update Template

**Files:** `crates/quarto-core/src/template.rs`

Ensure the default template references the artifact path for CSS:

```rust
const DEFAULT_HTML_TEMPLATE: &str = r#"<!DOCTYPE html>
<html$if(lang)$ lang="$lang$"$endif$>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
$if(pagetitle)$
<title>$pagetitle$</title>
$endif$
$for(css)$
<link rel="stylesheet" href="$css$">
$endfor$
$if(header-includes)$
$header-includes$
$endif$
</head>
<body>
$body$
</body>
</html>
"#;
```

The `css` variable will contain `/.quarto/project-artifacts/styles.css`.

## Files to Modify

| File | Changes |
|------|---------|
| `crates/quarto-core/src/pipeline.rs` | Store CSS artifact after render |
| `crates/quarto-core/src/resources.rs` | Export `DEFAULT_CSS` (already public) |
| `crates/wasm-quarto-hub-client/src/lib.rs` | Populate VFS with artifacts after render |
| `hub-client/src/hooks/useIframePostProcessor.ts` | New file - custom React hook |
| `hub-client/src/utils/iframePostProcessor.ts` | New file - post-processor logic |
| `hub-client/src/components/Editor.tsx` | Use hook, remove hardcoded CSS wrapper |
| `hub-client/src/services/wasmRenderer.ts` | Expose `vfsReadFile` if not already |

## Testing

1. **Unit test**: Pipeline stores CSS artifact
2. **Unit test**: Path resolution for relative .qmd links
3. **Manual test**: Render document with callout, verify styling appears
4. **Manual test**: Click .qmd link, verify file switches

## Future Extensions

This architecture supports:
- Image resources (same pattern)
- JavaScript resources
- Font files
- Cross-reference popups
- Citation hover previews
- Incremental rendering (post-processor runs per-component)
