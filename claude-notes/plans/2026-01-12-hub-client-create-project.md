# Hub-Client Project Management Refactor and Create New Project Implementation

**Epic ID**: k-1omt
**Created**: 2026-01-12
**Status**: Planning

## Executive Summary

This plan covers refactoring the hub-client "Select a Project" view to properly distinguish between connecting to existing projects vs creating new ones, and implementing genuine project creation functionality by porting `quarto create project` from TypeScript Quarto to Rust Quarto with support for both native and WASM environments.

## Background

### Current State

1. **Hub-client ProjectSelector (`hub-client/src/components/ProjectSelector.tsx`)**
   - Shows "Add New Project" button that requires an existing Automerge indexDocId
   - Actually "connects to" an existing project rather than creating one
   - Projects stored in IndexedDB with: indexDocId, syncServer, description

2. **WASM module (`crates/wasm-quarto-hub-client`)**
   - Provides VFS operations and QMD rendering
   - No project creation functionality

3. **System Runtime (`crates/quarto-system-runtime`)**
   - Has `SystemRuntime` trait with `NativeRuntime` and `WasmRuntime` implementations
   - Good foundation for adding JavaScript execution abstraction
   - No JavaScript execution capability currently

4. **TypeScript Quarto project creation**
   - `src/command/create/artifacts/project.ts` - artifact creation interface
   - `src/project/project-create.ts` - actual project creation logic
   - Uses `renderEjs()` for template rendering
   - Project types: default, website, blog, manuscript, book, confluence
   - Creates `_quarto.yml` and scaffold files using EJS templates

5. **rusty_v8_experiments (`external-sources/rusty_v8_experiments`)**
   - Working `ejs-runner` crate using deno_core
   - Browserified EJS bundle (50KB)
   - Pattern established for pre-bundled JavaScript with `include_str!()`

### Goals

1. Rename "Add New Project" to "Connect to Project" to accurately describe current functionality
2. Add "Create New Project" that creates a skeleton Quarto project structure
3. Port `quarto create project` functionality to Rust
4. Implement JavaScript execution abstraction in SystemRuntime for EJS template rendering
5. Support both native (deno_core/V8) and WASM (JS interop) environments

## Architecture

### JavaScript Execution Abstraction

We need to add JavaScript execution capability to the `SystemRuntime` trait.

#### Design Principles

**IMPORTANT: These principles are intentional and should be preserved in future designs.**

1. **Application-specific entry points, NOT generic `eval()`**
   - The trait exposes purpose-specific methods like `render_ejs()`, not `eval_js()`
   - This is intentional for safety, testability, and abstraction
   - Each method documents what it does and what inputs it accepts
   - No arbitrary code execution is exposed through the public API

2. **Implementation-agnostic trait API**
   - The trait is defined purely in terms of Rust types (`String`, `serde_json::Value`, etc.)
   - No deno_core, rusty_v8, or wasm-bindgen types leak into the trait definition
   - This allows swapping the underlying JS engine without changing the public API
   - While we currently plan to use deno_core for native, this could change

3. **Asymmetric implementations are acceptable**
   - Native: embeds V8 via deno_core (Rust calls into embedded JS)
   - WASM: calls out to browser JS via wasm-bindgen (Rust calls external JS)
   - The trait hides this architectural difference from consumers

```rust
// In quarto-system-runtime/src/traits.rs
// Uses async_trait crate for async methods in trait

use async_trait::async_trait;

#[async_trait]
pub trait SystemRuntime: Send + Sync {
    // ... existing sync methods unchanged ...

    // ═══════════════════════════════════════════════════════════════════════
    // JAVASCRIPT EXECUTION
    // ═══════════════════════════════════════════════════════════════════════

    /// Check if JavaScript execution is available on this runtime.
    fn js_available(&self) -> bool {
        false // Default: not available
    }

    /// Render a simple string template using JavaScript.
    ///
    /// Template format: "Hello, ${name}!" with data {"name": "World"} → "Hello, World!"
    /// Uses simple ${key} replacement, NOT full JavaScript template literals.
    ///
    /// This is scaffolding for validating JS execution architecture.
    /// May be removed or refactored once render_ejs is working.
    async fn js_render_simple_template(
        &self,
        template: &str,
        data: &serde_json::Value
    ) -> RuntimeResult<String> {
        let _ = (template, data);
        Err(RuntimeError::NotSupported(
            "JavaScript execution is not available on this runtime".to_string(),
        ))
    }

    /// Render an EJS template with the given data.
    ///
    /// # Arguments
    /// * `template` - EJS template string
    /// * `data` - JSON data to pass to the template
    ///
    /// # Returns
    /// Rendered string on success, RuntimeError on failure
    async fn render_ejs(
        &self,
        template: &str,
        data: &serde_json::Value
    ) -> RuntimeResult<String> {
        let _ = (template, data);
        Err(RuntimeError::NotSupported(
            "EJS rendering is not available on this runtime".to_string(),
        ))
    }
}
```

**Native Implementation (NativeRuntime)**:
- Use `deno_core` with browserified EJS bundle (from rusty_v8_experiments)
- V8 runtime embedded in Rust binary
- Include EJS bundle via `include_str!()`
- Implementation detail: could be swapped for another JS engine if needed

**WASM Implementation (WasmRuntime)**:
- Use `wasm-bindgen` to call JavaScript EJS library in browser context
- Import EJS via npm and expose via JavaScript interop
- Browser handles sandboxing automatically

**WASM JS Bridge Design (Decision Record):**

We use `#[wasm_bindgen(module = "...")]` to import JS functions, NOT `inline_js`:

```rust
// In crates/wasm-quarto-hub-client (or quarto-system-runtime wasm.rs)
#[wasm_bindgen(module = "/src/wasm-js-bridge/template.js")]
extern "C" {
    #[wasm_bindgen(js_name = renderSimpleTemplate)]
    fn render_simple_template_js(template: &str, data_json: &str) -> js_sys::Promise;
}
```

```javascript
// hub-client/src/wasm-js-bridge/template.js
export function renderSimpleTemplate(template, dataJson) {
    return new Promise((resolve, reject) => {
        try {
            const data = JSON.parse(dataJson);
            const result = template.replace(/\$\{(\w+)\}/g, (_, key) => {
                return key in data ? String(data[key]) : '';
            });
            resolve(result);
        } catch (e) {
            reject(e);
        }
    });
}
```

**Why `module` over `inline_js`:**
- `inline_js` would be simpler for the interstitial test alone
- But EJS MUST use `module` (can't inline 50KB library)
- Using `module` for interstitial test exercises the same bundler integration path
- If Vite has module resolution issues, we find them before investing in EJS
- This is the purpose of the interstitial gate: validate architecture early

**Considered alternative:** `inline_js` is cleaner for self-contained JS, but doesn't test
the module resolution that EJS requires. We chose consistency over local simplicity.

### JavaScript Bundle Build Mechanism

The crate includes pre-bundled JavaScript files that are embedded via `include_str!()`.
These bundles are committed to git and don't rebuild on every `cargo build`.

**Directory Structure:**
```
crates/quarto-system-runtime/
├── Cargo.toml
├── build.rs                    # Handles optional bundle rebuild
├── js/
│   ├── package.json            # npm dependencies (ejs, esbuild)
│   ├── package-lock.json
│   ├── esbuild.config.mjs      # Bundle configuration
│   ├── src/
│   │   ├── simple-template.js  # For interstitial test (no deps)
│   │   └── ejs-entry.js        # EJS wrapper
│   └── dist/                   # Generated bundles (committed to git)
│       ├── simple-template-bundle.js
│       └── ejs-bundle.js
└── src/
    └── js_bundles.rs           # include_str! the bundles
```

**Rebuilding Bundles:**
```bash
# Normal build - uses committed bundles
cargo build -p quarto-system-runtime

# Rebuild bundles (when updating JS dependencies or code)
QUARTO_REBUILD_JS_BUNDLES=1 cargo build -p quarto-system-runtime

# Or manually
cd crates/quarto-system-runtime/js && npm install && npm run build
```

**build.rs behavior:**
- Always watches `js/dist/*.js` for changes (recompile if bundles change)
- Only runs npm when `QUARTO_REBUILD_JS_BUNDLES` env var is set
- CI can set this env var to ensure bundles are up-to-date

### Interstitial Test Design

The interstitial test validates JS execution without requiring EJS:

**Simple Template JS (no dependencies):**
```javascript
// js/src/simple-template.js
// Simple ${key} replacement - NOT JavaScript template literals (no eval)
function renderSimpleTemplate(templateStr, dataJson) {
    const data = JSON.parse(dataJson);
    return templateStr.replace(/\$\{(\w+)\}/g, (_, key) => {
        return key in data ? String(data[key]) : '';
    });
}
```

**Test case:**
- Input: template=`"Hello, ${name}!"`, data=`{"name": "World"}`
- Expected output: `"Hello, World!"`

This tests:
- Rust → JS data passing (JSON serialization)
- JS execution (string manipulation)
- JS → Rust result passing (string return)
- Async flow works correctly

### Project Creation Flow

```
                    ┌─────────────────────────────────────────────┐
                    │           hub-client (React/TS)             │
                    │                                             │
                    │  ProjectSelector.tsx                        │
                    │  ┌───────────────┐ ┌──────────────────────┐│
                    │  │ Connect to    │ │ Create New Project   ││
                    │  │ Project       │ │                      ││
                    │  │ (existing     │ │ - Select type        ││
                    │  │  indexDocId)  │ │ - Enter title        ││
                    │  └───────────────┘ │ - Call WASM          ││
                    │                    └──────────────────────┘│
                    └──────────────────────────┬──────────────────┘
                                               │
                                               ▼
                    ┌─────────────────────────────────────────────┐
                    │    wasm-quarto-hub-client (WASM)            │
                    │                                             │
                    │  create_project(type, title) → files JSON   │
                    │     │                                       │
                    │     ▼                                       │
                    │  WasmRuntime::render_ejs()                  │
                    │     │                                       │
                    │     ▼ (via wasm-bindgen)                    │
                    │  JavaScript EJS.render()                    │
                    └──────────────────────────┬──────────────────┘
                                               │
                                               ▼
                    ┌─────────────────────────────────────────────┐
                    │    Automerge Document Creation              │
                    │                                             │
                    │  - Create IndexDocument with files mapping  │
                    │  - Create individual file documents         │
                    │  - Connect to sync server                   │
                    └─────────────────────────────────────────────┘
```

## Implementation Plan

### Phase 1: Hub-client UI Changes (Simple, Low Risk)

**Subtask 1.1: Rename "Add New Project" to "Connect to Project"**
- File: `hub-client/src/components/ProjectSelector.tsx`
- Change button text and update any related strings
- Update form header/description to clarify this connects to existing project

**Subtask 1.2: Add "Create New Project" Button Placeholder**
- Add second button for project creation
- Initially show "Coming Soon" or disabled state
- Design UI for project type/title input

### Phase 2: JavaScript Execution in SystemRuntime (Foundation)

**Subtask 2.1: Design JsExecution trait**
- File: `crates/quarto-system-runtime/src/traits.rs`
- Add `render_ejs()` method to SystemRuntime trait or create separate trait
- Define error types for JS execution failures
- Document design principles (application-specific entry points, implementation-agnostic API)

**Subtask 2.2: Implement NativeRuntime JS execution**
- New file: `crates/quarto-system-runtime/src/js_native.rs`
- Add deno_core dependency (conditional on `not(target_arch = "wasm32")`)
- Port ejs-runner pattern from rusty_v8_experiments
- Include browserified EJS bundle via `include_str!()`

**Subtask 2.3: Implement WasmRuntime JS execution**
- File: `crates/quarto-system-runtime/src/wasm.rs`
- Add `#[wasm_bindgen]` imports for JavaScript EJS
- Define JavaScript interop functions

**Subtask 2.4: Hub-client JavaScript EJS setup**
- Add EJS npm dependency to hub-client
- Create JavaScript wrapper for WASM to call
- Wire up wasm-bindgen imports

**Subtask 2.5: Interstitial JS runtime validation (GATE)**
- Create a minimal test entry point to verify JS execution works before implementing EJS
- Must pass on BOTH native and WASM targets
- Purpose: validate the architecture before investing in EJS integration
- Test should be "obviously correct" - e.g., simple JSON transformation or string operation
- This is a gate: Phase 3 should not start until this test passes on both targets
- Can be removed or refactored once EJS is working (it's scaffolding, not permanent API)

### Phase 3: Project Creation Logic (Core Functionality)

**Subtask 3.1: Port project templates to Rust**
- New crate or module: `quarto-project-create` or extend `quarto-core`
- Define project types enum: Default, Website, Blog, Manuscript, Book
- Embed EJS templates as Rust strings (from quarto-cli resources)
- Create template data structures

**Subtask 3.2: Implement project scaffolding logic**
- Port `projectCreate()` from TypeScript
- Generate `_quarto.yml` using EJS
- Generate scaffold files (index.qmd, etc.)
- Return file structure as JSON

**Subtask 3.3: Add WASM entry point for project creation**
- File: `crates/wasm-quarto-hub-client/src/lib.rs`
- New function: `create_project(project_type: &str, title: &str) -> String`
- Returns JSON with file paths and contents

### Phase 4: Hub-client Integration

**Subtask 4.1: Implement Create Project dialog**
- Project type selector (dropdown)
- Title input field
- Create button that calls WASM

**Subtask 4.2: Automerge document creation from WASM result**
- Create new IndexDocument with unique docId
- Create file documents from WASM result
- Store project metadata in IndexedDB
- Connect to sync server

**Subtask 4.3: Error handling and UX polish**
- Loading states
- Error messages
- Success navigation to new project

### Phase 5: Security Review and Testing

**Subtask 5.1: Security analysis**
- EJS template injection risks
- Browser sandbox considerations
- Template validation requirements

**Subtask 5.2: Testing**
- Unit tests for EJS rendering
- Integration tests for project creation
- E2E tests for full flow

## Technical Notes

### EJS in Browser - Security Considerations

EJS executes JavaScript code within templates (`<% code %>` and `<%= expression %>`). In the browser context:

1. **Trusted templates only**: We control all templates (embedded in WASM/JS bundle)
2. **No user-provided templates**: Project creation uses only built-in templates
3. **Browser sandbox**: Even if code runs, it's sandboxed by browser
4. **CSP headers**: Can add Content-Security-Policy if needed

For this use case (project scaffolding with built-in templates), the risk is minimal since:
- Templates are hardcoded/bundled
- Data is simple (title, type strings)
- No external template loading

### deno_core Dependencies (Native Only)

From rusty_v8_experiments:
```toml
# These are ONLY for native target, NOT for WASM
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
deno_core = "0.376"
deno_web = "0.257"        # Only needed for EJS, not for simple template test
deno_webidl = "0.226"     # Only needed for EJS
serde_v8 = "0.285"
tokio = { version = "1", features = ["rt-multi-thread"] }
async-trait = "0.1"
```

**Critical constraints:**
1. All Deno crates must be compatible versions of deno_core
2. These types MUST NOT leak into the public trait API
3. The trait uses only `String`, `serde_json::Value`, `RuntimeResult`
4. If we need to swap deno_core for another engine, only internal code changes

### Project Types from TypeScript Quarto

```typescript
const kProjectCreateTypes = [
  "default",    // Simple project with _quarto.yml
  "website",    // Website with index.qmd, default template
  "blog",       // Website with blog template
  "manuscript", // Academic manuscript
  "book",       // Multi-chapter book
  "confluence", // Confluence wiki export
];
```

For hub-client MVP, we can start with:
- `default` - minimal project
- `website` - basic website

### File Structure for Default Project

```
project/
├── _quarto.yml    # Project configuration
└── (empty or with single document)
```

### File Structure for Website Project

```
project/
├── _quarto.yml    # Project configuration (type: website)
├── index.qmd      # Home page
├── about.qmd      # About page (optional)
└── styles.css     # Custom styles (optional)
```

## Dependencies

- deno_core (native only, for V8 runtime)
- ejs npm package (hub-client, for browser JS execution)
- wasm-bindgen (WASM ↔ JS interop)

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| deno_core compile time | Build slowdown | Feature-flag, compile only when needed |
| deno_core doesn't work out | Rework needed | Implementation-agnostic trait API allows swapping; don't leak deno_core types |
| WASM binary size increase | Slower initial load | EJS is small (~50KB), acceptable |
| EJS security in browser | Code injection | Use only built-in templates, no user input |
| Complex async flow | Implementation bugs | Thorough testing, simple MVP first |
| Architecture issues found late | Wasted effort | Interstitial test gate validates design before EJS work |

## Success Criteria

1. [x] "Connect to Project" button clearly indicates connecting to existing project (done: 1c52e8e)
2. [ ] Interstitial JS test passes on both native and WASM targets (Phase 2 gate)
3. [ ] "Create New Project" creates a valid Quarto project structure
4. [ ] New project appears in Automerge and syncs correctly
5. [ ] At least "default" and "website" project types work
6. [ ] EJS templates render correctly in both native and WASM
7. [ ] No security vulnerabilities introduced

## Future Work (Out of Scope for This Epic)

- Additional project types (book, manuscript, blog)
- Extension creation (`quarto create extension`)
- Custom template support
- Project import from local filesystem
- Project export to local filesystem
