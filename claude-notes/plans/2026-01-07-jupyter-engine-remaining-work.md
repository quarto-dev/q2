# Jupyter Engine: Remaining Work

**Date**: 2026-01-07
**Related**: [Jupyter Engine Implementation Plan](2026-01-07-jupyter-engine-implementation.md)

## Overview

The Jupyter engine MVP is complete. This document describes remaining work to bring it to full production readiness.

## Completed (MVP)

- Kernelspec discovery via runtimelib
- Kernel process spawning with proper lifecycle management
- Execute request/reply via ZeroMQ
- stdout (StreamContent) collection
- display_data (images) collection
- ExecuteResult collection
- Error output collection
- Clean kernel shutdown
- JupyterTransform as AstTransform (CodeBlock → output blocks)
- Daemon kernel persistence across transforms
- Inline expression evaluation (`{python} expr` syntax)
- 8 integration tests (all passing)

## Outstanding Work

### 1. Chunk Options Support

**Priority**: High
**Effort**: Medium

The current implementation ignores chunk options. Need to implement:

```yaml
#| echo: false
#| eval: true
#| output: asis
#| warning: false
#| error: true
#| include: true
#| fig-width: 8
#| fig-height: 6
#| fig-cap: "My figure"
```

**Implementation approach**:
- Parse chunk options from CodeBlock attributes or leading YAML comments
- Pass options to execution context
- Apply `echo` (show/hide code), `eval` (execute or skip)
- Apply `output` processing (asis, hide, etc.)
- Apply figure options to matplotlib/plotly setup code

**Files to modify**:
- `transform.rs`: Extract and parse chunk options
- `execute.rs`: Respect `eval` option
- `output.rs`: Respect `output`, `warning`, `error` options
- New `options.rs`: Chunk option parsing and defaults

### 2. Language-Specific Setup/Cleanup Code

**Priority**: Medium
**Effort**: Low

Currently, user code executes without any setup. Need to add:

**Python setup** (before first cell):
```python
# Configure matplotlib for inline display
import matplotlib
matplotlib.use('agg')
import matplotlib.pyplot as plt
plt.rcParams['figure.figsize'] = (fig_width, fig_height)
plt.rcParams['figure.dpi'] = fig_dpi

# Configure Plotly
import plotly.io as pio
pio.renderers.default = 'png'
```

**Python cleanup** (after last cell):
```python
# Close matplotlib figures
plt.close('all')
```

**Implementation approach**:
- Store setup/cleanup code as Rust string constants in `setup.rs`
- Execute setup code after kernel starts (before first user cell)
- Execute cleanup code after last user cell (before shutdown)
- Make setup configurable via document metadata

**Files to modify**:
- New `setup.rs`: Setup/cleanup code constants per language
- `transform.rs`: Execute setup before cells, cleanup after

### 3. Kernel Timeout and Interrupt Handling

**Priority**: Medium
**Effort**: Medium

Currently, execution can hang indefinitely if a cell never completes.

**Needed features**:
- Per-cell execution timeout (configurable, default 5 minutes)
- Kernel interrupt on timeout (send SIGINT to kernel process)
- Graceful recovery after interrupt
- User-configurable timeout in metadata

**Implementation approach**:
- Add timeout parameter to `execute()` method
- Use `tokio::time::timeout()` around iopub collection loop
- On timeout, send interrupt signal to kernel process
- Wait for kernel to return to idle state
- Report timeout error to user

**Files to modify**:
- `session.rs`: Add `execute_with_timeout()`, interrupt handling
- `transform.rs`: Pass timeout from options

### 4. Error Recovery / Continue-on-Error

**Priority**: Medium
**Effort**: Low

Currently, errors halt execution. Need to implement `error: true` option.

**Behavior when `error: true`**:
- Capture error output but continue to next cell
- Include error traceback in output blocks
- Mark cell output as error (for styling)

**Implementation approach**:
- Check `error` option before returning early on ExecuteStatus::Error
- Format error output as styled CodeBlock
- Continue execution loop

**Files to modify**:
- `transform.rs`: Check error option, don't short-circuit
- `output.rs`: Format error output blocks with styling

### 5. Figure Output Improvements

**Priority**: Low
**Effort**: Medium

Current figure handling is basic. Improvements needed:

- **Figure captions**: Apply `fig-cap` option to image alt text
- **Figure sizing**: Apply `fig-width`/`fig-height` to output
- **Multiple figures**: Handle cells that produce multiple figures
- **SVG support**: Prefer SVG for HTML, PDF for LaTeX
- **Figure linking**: Support `fig-link` option
- **Figure layout**: Support `layout-ncol`, `layout-nrow`

**Files to modify**:
- `output.rs`: Enhanced figure handling
- `transform.rs`: Pass figure options to output conversion

### 6. Widget Support (ipywidgets)

**Priority**: Low
**Effort**: High

Jupyter widgets require special handling:
- Comm messages for widget state
- JavaScript embedding for HTML output
- Static rendering fallback for non-interactive outputs

**This is complex and may be deferred to a future phase.**

### 7. Integration with Render Pipeline

**Priority**: High
**Effort**: Low

Currently JupyterTransform is standalone. Need to:
- Register JupyterTransform in the pipeline
- Ensure it runs at the correct stage (after includes, before writers)
- Pass proper RenderContext with format info
- Handle multiple formats (HTML vs PDF figure preferences)

**Files to modify**:
- `crates/quarto-core/src/transform/mod.rs`: Register JupyterTransform
- `crates/quarto-core/src/render/context.rs`: Ensure format info available

### 8. Cache/Freeze Support

**Priority**: Low
**Effort**: High

For incremental rendering, need to:
- Hash cell inputs (code + options)
- Store outputs in cache directory
- Skip execution if cache hit
- Invalidate cache when dependencies change

**This is a significant feature and may be deferred.**

## Testing Gaps

Current tests cover happy paths. Additional tests needed:

- [ ] Chunk options parsing
- [ ] `echo: false` hides code in output
- [ ] `eval: false` skips execution
- [ ] `error: true` continues after error
- [ ] Execution timeout handling
- [ ] Kernel interrupt recovery
- [ ] Multiple figure outputs from single cell
- [ ] Julia kernel execution
- [ ] R kernel execution (via IRkernel)

## Module Structure (Current)

```
crates/quarto-core/src/engine/jupyter/
├── mod.rs          # JupyterEngine (ExecutionEngine trait)
├── daemon.rs       # JupyterDaemon (in-process kernel manager)
├── session.rs      # KernelSession (single kernel connection)
├── execute.rs      # ExecuteResult, CellOutput types
├── kernelspec.rs   # Kernel discovery and resolution
├── output.rs       # Output → AST conversion
├── error.rs        # JupyterError type
└── transform.rs    # JupyterTransform (AstTransform)
```

## Recommended Priority Order

1. **Integration with render pipeline** - Required for the engine to actually be used
2. **Chunk options support** - Core Quarto functionality
3. **Kernel timeout/interrupt** - Robustness requirement
4. **Error recovery** - User experience
5. **Language setup code** - Proper figure generation
6. **Figure improvements** - Polish
7. **Cache support** - Performance optimization
8. **Widget support** - Advanced feature
