# Debugging Missing data-loc Attributes

**Issue:** HTML preview in hub-client is missing `data-loc` attributes needed for scroll sync

**Date:** 2025-12-30

## Status: Investigation Complete

The WASM module works correctly. The test script (`hub-client/test-wasm.mjs`) passes:
```
Has data-loc attributes: true
Sample data-loc attributes: ['data-loc="0:5:1-6:1"', ...]
```

## Root Cause Analysis

The issue is likely one of:

1. **Scroll sync toggle is OFF** (most likely)
   - `scrollSyncEnabled` defaults to `false` in Editor.tsx:176
   - User must click the "Sync" button in the toolbar to enable

2. **Browser/Vite caching**
   - Old WASM module being served
   - Need to rebuild and clear cache

3. **State propagation issue**
   - Toggle might not be triggering re-render with correct option

## Debugging Steps

### Step 1: Verify Toggle State

Open browser DevTools console and look for these logs when typing in the editor:
```
[doRender] scrollSyncEnabled: ???
[doRender] calling renderToHtml with sourceLocation: ???
[renderToHtml] sourceLocation option: ???
[renderToHtml] HTML has data-loc: ???
```

If `scrollSyncEnabled: false`, click the "Sync" button in the toolbar.

### Step 2: Rebuild and Restart (if Step 1 doesn't work)

```bash
cd /Users/cscheid/repos/github/cscheid/kyoto/hub-client

# Rebuild WASM
npm run build:wasm

# Restart dev server
npm run dev
```

### Step 3: Clear Browser Cache

1. Open DevTools → Network → Disable cache (while DevTools open)
2. Or hard refresh: Cmd+Shift+R (Mac) / Ctrl+Shift+R (Windows/Linux)
3. Or clear site data: DevTools → Application → Clear site data

### Step 4: Verify WASM Module Directly

Run the test script to confirm WASM works:
```bash
cd hub-client
node test-wasm.mjs
```

Expected output should show `Has data-loc attributes: true`.

### Step 5: Add Enhanced Logging (if still failing)

If issues persist, add logging in `wasmRenderer.ts`:

```typescript
export function renderQmdContentWithOptions(
  content: string,
  templateBundle: string = '',
  options: WasmRenderOptions = {}
): RenderResponse {
  const wasm = getWasm();
  const optionsJson = JSON.stringify({
    source_location: options.sourceLocation ?? false,
  });

  // ADD THIS:
  console.log('[renderQmdContentWithOptions] optionsJson:', optionsJson);

  const rawResult = wasm.render_qmd_content_with_options(content, templateBundle, optionsJson);

  // ADD THIS:
  console.log('[renderQmdContentWithOptions] raw result length:', rawResult.length);
  console.log('[renderQmdContentWithOptions] has data-loc:', rawResult.includes('data-loc'));

  return JSON.parse(rawResult);
}
```

### Step 6: Inspect iframe Content

In browser DevTools console:
```javascript
// Get the iframe
const iframe = document.querySelector('iframe');

// Check for data-loc attributes in iframe content
const elements = iframe.contentDocument.querySelectorAll('[data-loc]');
console.log('Elements with data-loc:', elements.length);
```

## Code Flow Reference

```
Editor.tsx
  └─> doRender(content) with sourceLocation: scrollSyncEnabled
      └─> wasmRenderer.renderToHtml(content, { sourceLocation: true })
          └─> renderQmdContentWithOptions(content, template, { sourceLocation: true })
              └─> wasm.render_qmd_content_with_options(content, template, '{"source_location":true}')
                  └─> lib.rs: Creates ProjectConfig with format.html.source-location: full
                      └─> pipeline.rs: Merges into pandoc.meta
                          └─> html.rs: extract_config_from_metadata() reads it
                              └─> write_with_source_tracking() generates data-loc attributes
```

## Files Involved

- `hub-client/src/components/Editor.tsx` - Toggle state, calls renderToHtml
- `hub-client/src/services/wasmRenderer.ts` - TypeScript wrapper for WASM
- `hub-client/src/hooks/useScrollSync.ts` - Reads data-loc from iframe
- `crates/wasm-quarto-hub-client/src/lib.rs` - WASM entry point
- `crates/quarto-core/src/pipeline.rs` - Config merging
- `crates/pampa/src/writers/html.rs` - Generates data-loc attributes
