# Monaco Editor Diagnostics Integration Plan

**Issue:** k-i5nw
**Date:** 2025-12-28
**Status:** Approved - ready for implementation

## Design Decisions

The following decisions were made during review:

1. **Warnings scope**: Restructure warnings to preserve locations (requires pipeline changes). The current `.to_text(None)` pattern is a code smell that discards valuable structured information.

2. **Errors without locations**: Show as a separate notification/banner in the UI, not as inline squiggles. These are rare but shouldn't be lost.

3. **Detail markers**: Merge detail info into the main marker's message rather than creating separate markers. This reduces visual noise. Future enhancement: hovering over the main marker could highlight related detail locations.

## Overview

This plan outlines the implementation for surfacing structured `DiagnosticMessage` errors from the QMD rendering pipeline to Monaco editor as inline squiggles/markers. Currently, parse errors are displayed only in the preview pane as plain text; this feature will show them directly in the editor at their source locations.

## Current State Analysis

### Error Reporting Infrastructure (Rust)

**`quarto-error-reporting::DiagnosticMessage`** provides rich structured errors:
```rust
pub struct DiagnosticMessage {
    pub code: Option<String>,           // e.g., "Q-1-1"
    pub title: String,                   // Brief error title
    pub kind: DiagnosticKind,            // Error/Warning/Info/Note
    pub problem: Option<MessageContent>, // What went wrong
    pub details: Vec<DetailItem>,        // Bulleted details with locations
    pub hints: Vec<MessageContent>,      // Fix suggestions
    pub location: Option<SourceInfo>,    // Where error occurred
}
```

The `to_json()` method produces machine-readable output with all fields including source locations.

### Pipeline Flow

```
parse_qmd() in quarto-core/src/pipeline.rs
    ↓
Returns: (Pandoc, ASTContext, Vec<DiagnosticMessage>)
    ↓
On success: DiagnosticMessages become warnings (converted to strings!)
On error: DiagnosticMessages joined as text for QuartoError::Parse
```

**Problem #1:** Source location information is discarded - only `.to_text(None)` is used. This is a code smell that we will fix.

### WASM Layer (wasm-quarto-hub-client)

```rust
// Current RenderResponse
struct RenderResponse {
    success: bool,
    error: Option<String>,           // Plain text error
    html: Option<String>,
    diagnostics: Option<Vec<String>>, // Never populated!
    warnings: Option<Vec<String>>,    // Just message strings
}
```

**Problem #2:** The `diagnostics` field exists but is never used. Warnings lose their locations.

### Hub Client (TypeScript)

```typescript
// Current RenderResponse interface
interface RenderResponse {
  success?: boolean;
  error?: string;
  html?: string;
  diagnostics?: string[];  // Just strings, no locations
}
```

Errors are shown in `renderError()` as HTML in the preview iframe - no Monaco integration.

**Problem #3:** No code to convert diagnostics to Monaco markers or call `setModelMarkers()`.

## Proposed Solution

### Phase 1: WASM Layer - Return Structured Diagnostics

Modify `wasm-quarto-hub-client/src/lib.rs`:

1. **Define a serializable diagnostic struct for JSON transport:**

```rust
#[derive(Serialize)]
struct JsonDiagnostic {
    kind: String,           // "error" | "warning" | "info" | "note"
    title: String,
    code: Option<String>,
    problem: Option<String>,
    hints: Vec<String>,
    // Primary location
    start_line: Option<u32>,     // 1-based
    start_column: Option<u32>,   // 1-based
    end_line: Option<u32>,
    end_column: Option<u32>,
    // Details with their own locations
    details: Vec<JsonDiagnosticDetail>,
}

#[derive(Serialize)]
struct JsonDiagnosticDetail {
    kind: String,
    content: String,
    start_line: Option<u32>,
    start_column: Option<u32>,
    end_line: Option<u32>,
    end_column: Option<u32>,
}
```

2. **Update `RenderResponse` to use structured diagnostics:**

```rust
struct RenderResponse {
    success: bool,
    error: Option<String>,
    html: Option<String>,
    diagnostics: Option<Vec<JsonDiagnostic>>,  // Now structured!
    warnings: Option<Vec<JsonDiagnostic>>,     // Now structured!
}
```

3. **Implement conversion from `DiagnosticMessage` to `JsonDiagnostic`:**

- Use `SourceInfo::map_offset()` to convert byte offsets to line/column
- Create a minimal `SourceContext` with the input content for the mapping
- Handle all `SourceInfo` variants (Original, Substring, Concat, FilterProvenance)

4. **Populate diagnostics in both success and error paths:**

- Success: warnings field with all parse warnings
- Error: diagnostics field with parse errors

### Phase 2: TypeScript Types and Converter

Create `hub-client/src/types/diagnostic.ts`:

```typescript
export interface Diagnostic {
  kind: 'error' | 'warning' | 'info' | 'note';
  title: string;
  code?: string;
  problem?: string;
  hints: string[];
  startLine?: number;
  startColumn?: number;
  endLine?: number;
  endColumn?: number;
  details: DiagnosticDetail[];
}

export interface DiagnosticDetail {
  kind: 'error' | 'info' | 'note';
  content: string;
  startLine?: number;
  startColumn?: number;
  endLine?: number;
  endColumn?: number;
}
```

Create `hub-client/src/utils/diagnosticToMonaco.ts`:

```typescript
import type * as Monaco from 'monaco-editor';
import type { Diagnostic } from '../types/diagnostic';

export interface DiagnosticsResult {
  markers: Monaco.editor.IMarkerData[];
  unlocatedDiagnostics: Diagnostic[];  // For banner/notification display
}

export function diagnosticsToMarkers(
  diagnostics: Diagnostic[]
): DiagnosticsResult {
  const markers: Monaco.editor.IMarkerData[] = [];
  const unlocatedDiagnostics: Diagnostic[] = [];

  for (const diag of diagnostics) {
    // Main diagnostic location
    if (diag.startLine != null) {
      markers.push({
        severity: kindToSeverity(diag.kind),
        message: formatMessage(diag),  // Details merged into message
        startLineNumber: diag.startLine,
        startColumn: diag.startColumn ?? 1,
        endLineNumber: diag.endLine ?? diag.startLine,
        endColumn: diag.endColumn ?? 1000,
        code: diag.code,
        source: 'quarto',
      });
    } else {
      // No location - collect for banner display
      unlocatedDiagnostics.push(diag);
    }
  }

  return { markers, unlocatedDiagnostics };
}

function kindToSeverity(kind: string): Monaco.MarkerSeverity {
  switch (kind) {
    case 'error': return 8;   // MarkerSeverity.Error
    case 'warning': return 4; // MarkerSeverity.Warning
    case 'info': return 2;    // MarkerSeverity.Info
    case 'note': return 1;    // MarkerSeverity.Hint
    default: return 2;
  }
}

function formatMessage(diag: Diagnostic): string {
  let msg = diag.title;
  if (diag.problem) {
    msg += '\n' + diag.problem;
  }
  // Merge details into the message (per design decision #3)
  if (diag.details.length > 0) {
    msg += '\n\nDetails:\n' + diag.details.map(d => '  • ' + d.content).join('\n');
  }
  if (diag.hints.length > 0) {
    msg += '\n\nSuggestions:\n' + diag.hints.map(h => '  → ' + h).join('\n');
  }
  return msg;
}
```

### Phase 3: Editor Integration

Modify `hub-client/src/components/Editor.tsx`:

1. **Add state for diagnostics and unlocated errors:**

```typescript
const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([]);
const [unlocatedErrors, setUnlocatedErrors] = useState<Diagnostic[]>([]);
```

2. **Update the render result handling:**

```typescript
const doRender = useCallback(async (qmdContent: string) => {
  // ... existing code ...

  const result = await renderToHtml(qmdContent);

  // Collect diagnostics from both success and error paths
  const allDiagnostics: Diagnostic[] = [
    ...(result.diagnostics ?? []),
    ...(result.warnings ?? []),
  ];
  setDiagnostics(allDiagnostics);

  // ... rest of existing render logic ...
}, []);
```

3. **Apply markers when diagnostics change, separating located from unlocated:**

```typescript
useEffect(() => {
  if (!editorRef.current) return;

  const model = editorRef.current.getModel();
  if (!model) return;

  const { markers, unlocatedDiagnostics } = diagnosticsToMarkers(diagnostics);
  monaco.editor.setModelMarkers(model, 'quarto', markers);
  setUnlocatedErrors(unlocatedDiagnostics);
}, [diagnostics]);
```

4. **Render banner for unlocated diagnostics:**

```typescript
// In the JSX, above or below the editor
{unlocatedErrors.length > 0 && (
  <div className="diagnostics-banner">
    {unlocatedErrors.map((diag, i) => (
      <div key={i} className={`diagnostic-${diag.kind}`}>
        {diag.code && <span className="diag-code">[{diag.code}]</span>}
        <span className="diag-title">{diag.title}</span>
        {diag.problem && <span className="diag-problem">{diag.problem}</span>}
      </div>
    ))}
  </div>
)}
```

5. **Clear markers on file change:**

```typescript
// In handleFileChange
const handleFileChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
  // ... existing code ...
  setDiagnostics([]); // Clear diagnostics for new file
  setUnlocatedErrors([]);
};
```

### Phase 4: Update wasmRenderer.ts Types

Update `hub-client/src/services/wasmRenderer.ts`:

```typescript
import type { Diagnostic } from '../types/diagnostic';

interface RenderResponse {
  success?: boolean;
  error?: string;
  html?: string;
  diagnostics?: Diagnostic[];
  warnings?: Diagnostic[];
}

export async function renderToHtml(qmdContent: string): Promise<{
  html: string;
  success: boolean;
  error?: string;
  diagnostics?: Diagnostic[];
  warnings?: Diagnostic[];
}> {
  // ... implementation with diagnostics passthrough ...
}
```

### Phase 5: Pipeline Changes for Structured Warnings

Modify `quarto-core/src/pipeline.rs` to preserve structured warnings:

1. **Update `RenderOutput` to include structured warnings:**

```rust
pub struct RenderOutput {
    pub html: String,
    pub warnings: Vec<DiagnosticMessage>,  // Keep structured, not ParseWarning
    pub source_context: SourceContext,     // Needed for line/column mapping
}
```

2. **Update `render_qmd_to_html` to preserve diagnostics:**

```rust
// Instead of converting to strings:
// let warnings = parse_warnings.iter().map(|w| ParseWarning::new(w.to_text(None))).collect();

// Keep structured:
Ok(RenderOutput {
    html,
    warnings: parse_warnings,
    source_context,  // Pass through for WASM to use
})
```

3. **Remove `ParseWarning` struct** (or deprecate it) since we now use `DiagnosticMessage` directly.

## Implementation Order

1. **Pipeline changes (Phase 5)** - Fix the code smell first
   - Update `RenderOutput` to hold `Vec<DiagnosticMessage>` + `SourceContext`
   - Remove the `.to_text(None)` conversion
   - Update callers to handle the new type

2. **WASM layer changes (Phase 1)** - Core infrastructure
   - Define `JsonDiagnostic` structs
   - Implement `DiagnosticMessage` → `JsonDiagnostic` conversion using `SourceContext`
   - Update `RenderResponse` and rendering functions
   - Rebuild WASM module

3. **TypeScript types (Phase 2)** - Interface definitions
   - Create `diagnostic.ts` type definitions
   - Create `diagnosticToMonaco.ts` converter

4. **Editor integration (Phase 3 & 4)** - UI hookup
   - Update `wasmRenderer.ts` types
   - Add diagnostic state to Editor
   - Apply Monaco markers
   - Add banner for unlocated diagnostics

## Testing Strategy

1. **Unit tests for Rust conversion:**
   - Test `DiagnosticMessage` with various `SourceInfo` types
   - Verify JSON output structure
   - Test edge cases (no location, Concat sources, etc.)
   - Test that line/column are correctly 1-indexed for Monaco

2. **Unit tests for pipeline changes:**
   - Verify `RenderOutput.warnings` are `Vec<DiagnosticMessage>` (not strings)
   - Verify `source_context` is populated correctly
   - Test that existing CLI rendering still works (may need to call `.to_text()` at CLI level)

3. **Manual testing:**
   - Create QMD files with known parse errors
   - Verify squiggles appear at correct positions
   - Test multiple errors in same file
   - Test error + warning combination
   - Test error clearing when fixed
   - Test banner display for errors without locations
   - Verify details appear in marker hover message

4. **Test cases to verify:**
   - Unclosed fenced code block
   - Invalid YAML frontmatter
   - Unclosed inline formatting
   - Multiple errors at different locations
   - Errors without source locations (banner display)

## Resolved Questions

These questions were resolved during the design review:

1. **Detail locations** → Decided: Merge details into the main marker's message (no separate markers). Future enhancement may add hover-to-highlight functionality.

2. **Errors without locations** → Decided: Show as a banner/notification, not inline squiggles.

3. **Warnings scope** → Decided: Also restructure warnings to preserve locations. The `.to_text(None)` pattern is a code smell.

## Remaining Notes

- **Preview pane error display**: Keep as fallback, but make it less prominent once Monaco markers are working.

- **Performance**: The render itself is debounced (300ms), so markers will update at the same rate. No additional debouncing needed.

## Dependencies

- Monaco Editor already imported via `@monaco-editor/react`
- No new npm dependencies required
- WASM rebuild required after Rust changes

## Risks

1. **Source location mapping complexity:**
   - The `SourceInfo` transformation chain can be complex
   - Mitigation: Comprehensive unit tests

2. **WASM module size increase:**
   - Adding `SourceContext` to WASM could increase size
   - Mitigation: Measure before/after, optimize if needed

3. **Performance with many errors:**
   - Monaco can handle many markers, but extreme cases could lag
   - Mitigation: Consider limiting displayed markers if needed

4. **CLI compatibility:**
   - Changing `RenderOutput` affects both WASM and CLI code paths
   - The CLI currently expects string warnings for display
   - Mitigation: CLI code will need to call `.to_text()` at its output layer instead of receiving pre-formatted strings. This is cleaner architecture anyway.

## Future Enhancements

- Quick-fix suggestions from hints
- Error hover tooltips with full diagnostic info
- Error navigation (F8 style)
- Problems panel integration
- Link to error documentation (via error codes)
