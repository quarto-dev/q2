# Python Filter Integration for quarto-markdown-pandoc

**Beads Issue:** k-fgyv
**Status:** Design/Planning Phase
**Created:** 2025-12-06

## Executive Summary

This document explores the feasibility and design of native Python filter integration for quarto-markdown-pandoc, enabling Python developers to write filters with a comparable experience to the existing Lua filters. The goal is to provide in-process execution (avoiding JSON filter subprocess overhead) and a Pythonic API inspired by Panflute.

## Goals

1. **In-process Python execution** - Avoid subprocess overhead of JSON filters
2. **Pythonic API** - Inspired by Panflute, designed to be usable in similar application scenarios
3. **Leverage existing Python environments** - Use the host system's Python interpreter
4. **Bidirectional integration** - Both:
   - Python package callable from Python programs (`import quarto_markdown`)
   - Python filters callable from the Quarto pipeline

## Design Decisions (Resolved)

1. **Package name:** `quarto-markdown` (not `quarto-pandoc` to avoid implying direct Pandoc association)
2. **Panflute compatibility:** "Inspired by" and "designed for similar use cases" - not a drop-in replacement
3. **WASM support:** Not a priority for initial implementation; only relevant if someone tries to use the library in Pyodide settings
4. **Citeproc:** Will be needed for full implementation, not required for initial exploratory work

## Background Research

### Current Lua Filter Architecture

The Lua filter implementation in `quarto-markdown-pandoc` (see `src/lua/`) provides:

| Component | Location | Purpose |
|-----------|----------|---------|
| `filter.rs` | 3,364 LOC | Core filter execution, traversal modes |
| `types.rs` | 1,567 LOC | Userdata wrappers for AST types |
| `constructors.rs` | 1,660 LOC | `pandoc.*` namespace with element constructors |
| `list.rs` | 574 LOC | List/Inlines/Blocks metatables |
| `runtime/` | 3 modules | Abstraction layer for system operations |

**Key patterns to replicate:**
- Userdata wrappers with `__index` for named field access
- Traversal modes: typewise (default) and topdown
- Return value semantics: `nil`=unchanged, element=replace, list=splice, `{}`=delete
- Runtime abstraction for sandboxing/WASM portability

### Rust-Python FFI Ecosystem

**Primary Option: PyO3 + maturin**

[PyO3](https://github.com/PyO3/pyo3) is the de facto standard for Rust-Python FFI, with:
- Active development (latest talk: Feb 2025 "Techniques learned from five years finding the way for Rust in Python")
- Mature ecosystem including [maturin](https://github.com/PyO3/maturin) for packaging
- Used by major projects: Polars, pydantic-core, cryptography, orjson

**Alternative: RustPython**

[RustPython](https://github.com/RustPython/RustPython) is a pure-Rust Python interpreter:
- No CPython dependency
- Better WASM support
- **Not production-ready** (stated in their README)
- Limited stdlib and C extension compatibility

**Recommendation:** PyO3 is the clear choice for production use.

### Notable Projects Using PyO3

| Project | Architecture | Key Insight |
|---------|--------------|-------------|
| [Polars](https://github.com/pola-rs/polars) | `py-polars` wraps Rust `polars` crate | Clean separation, `pyo3-polars` for type conversion |
| [pydantic-core](https://github.com/pydantic/pydantic-core) | Core validation in Rust | 17x speedup over pure Python; shows recursive traversal works well |
| [orjson](https://github.com/ijl/orjson) | JSON ser/de in Rust | 10x faster than stdlib; uses `pyo3-ffi` for maximum performance |
| [cryptography](https://github.com/pyca/cryptography) | ASN.1 parsing in Rust | Memory-safe parsing with abi3 wheels for broad compatibility |

### Deep Dive: pydantic-core Architecture

pydantic-core is the most architecturally similar project to our use case. Both involve:
- Recursive tree traversal (validators calling validators vs. filter functions calling walk)
- Python callbacks during Rust traversal (validator functions vs. filter actions)
- Type-based dispatch (CombinedValidator enum vs. Inline/Block enums)

**Key architectural patterns from pydantic-core:**

#### 1. Schema Compilation Pattern

pydantic-core "compiles" Python schema dicts into a Rust validator tree at initialization time:

```rust
#[pyclass(module = "pydantic_core._pydantic_core", frozen)]
pub struct SchemaValidator {
    validator: Arc<CombinedValidator>,     // Compiled Rust validator tree
    definitions: Definitions<Arc<CombinedValidator>>,
    py_schema: Py<PyAny>,                  // Cached for pickling
}
```

**Parallel for quarto-markdown:** We could compile filter specifications at load time, building a Rust representation of the filter chain.

#### 2. Trait-Based Validator Composition

Validators implement a common trait and are composed via an enum:

```rust
#[enum_dispatch(CombinedValidator)]
pub trait Validator: Send + Sync + Debug {
    fn validate<'py>(
        &self,
        py: Python<'py>,
        input: &(impl Input<'py> + ?Sized),
        state: &mut ValidationState<'_, 'py>,
    ) -> ValResult<Py<PyAny>>;
}

pub enum CombinedValidator {
    TypedDict(TypedDictValidator),
    Union(UnionValidator),
    Model(ModelValidator),
    // ... 40+ variants
}
```

**Parallel for quarto-markdown:** Our existing `Inline`/`Block` enums serve a similar role. The `Validator` trait is analogous to our filter action functions.

#### 3. Context Threading via ValidationState

Rather than passing many parameters through the call stack, pydantic-core threads context through a single state object:

```rust
pub struct ValidationState<'a, 'py> {
    pub recursion_guard: &'a mut RecursionState,
    pub exactness: Option<Exactness>,
    pub fields_set_count: Option<usize>,
    pub allow_partial: PartialMode,
    pub has_field_error: bool,
    extra: Extra<'a, 'py>,  // User context, config, etc.
}
```

**Parallel for quarto-markdown:** This is similar to the `doc` parameter in Panflute filters. We should create a `FilterState` struct that carries:
- Document metadata
- User-provided context
- Recursion guard for cycle detection
- Diagnostic accumulator

#### 4. Python Function Callbacks (FunctionBeforeValidator/FunctionAfterValidator)

pydantic-core wraps Python callables and invokes them at the appropriate time:

```rust
struct FunctionInfo {
    pub function: Py<PyAny>,           // Stored Python callable
    pub field_name: Option<Py<PyString>>,
    pub info_arg: bool,
}

impl FunctionBeforeValidator {
    fn _validate<'s, 'py>(&'s self, ...) -> ValResult<Py<PyAny>> {
        let r = if self.info_arg {
            let info = ValidationInfo::new(py, state.extra(), &self.config, field_name);
            self.func.call1(py, (input.to_object(py)?, info))
        } else {
            self.func.call1(py, (input.to_object(py)?,))
        };
        let value = r.map_err(|e| convert_err(py, e, input))?;
        call(value.into_bound(py), state)  // Thread result to next validator
    }
}
```

**Key insight:** Python functions are stored as `Py<PyAny>` and called with `.call1()`. Errors are caught and converted. The result is threaded to the next operation.

**Parallel for quarto-markdown:** Our filter actions work exactly the same way:
- Store the Python action function
- Call it with the current element and document
- Handle the return value (None=unchanged, element=replace, list=splice)
- Thread to child traversal

#### 5. Definitions System for Recursive Schemas

pydantic-core handles self-referential types via a definitions system:

```rust
pub struct DefinitionRef<T> {
    reference: Arc<String>,
    value: Weak<OnceLock<T>>,  // Weak to avoid reference cycles
    name: Arc<LazyName>,
}
```

The `Weak` reference prevents Rust reference cycles when a validator refers to itself.

**Parallel for quarto-markdown:** Less critical for us since AST nodes don't have the same self-referential structure, but the pattern could be useful if we ever support recursive filter specifications.

#### 6. Recursion Guard for Cycle Detection in Data

```rust
pub struct RecursionGuard<'a, S: ContainsRecursionState> {
    state: &'a mut S,
    obj_id: usize,   // Python object identity
    node_id: usize,  // Validator node identity
}

impl RecursionGuard {
    pub fn new(state, obj_id, node_id) -> Result<Self, RecursionError> {
        if !state.insert(obj_id, node_id) {
            return Err(RecursionError::Cyclic);
        }
        // ... depth check
    }
}

impl Drop for RecursionGuard {
    fn drop(&mut self) {
        state.remove(self.obj_id, self.node_id);
    }
}
```

**Parallel for quarto-markdown:** Important for our `walk()` implementation to prevent infinite loops if users create circular structures.

#### 7. Input Abstraction (Generic Over Input Source)

```rust
pub trait Input<'py>: fmt::Debug {
    fn validate_str(&self, strict: bool) -> ValMatch<EitherString<'_, 'py>>;
    fn validate_int(&self, strict: bool) -> ValMatch<EitherInt<'_>>;
    // ...
}
```

Three implementations: `PythonInput`, `JsonInput`, `StringInput`.

**Parallel for quarto-markdown:** We have `qmd`, `json`, and potentially other input formats. The Input trait pattern would let validators work generically.

#### 8. GC Integration

pydantic-core carefully integrates with Python's garbage collector:

```rust
impl_py_gc_traverse!(SchemaValidator {
    validator, definitions, py_schema, py_config,
});

#[pymethods]
impl SchemaValidator {
    fn __traverse__(&self, visit: PyVisit) -> Result<(), PyTraverseError> {
        self.py_gc_traverse(&visit)
    }
}
```

**Parallel for quarto-markdown:** Any Rust struct holding `Py<PyAny>` references must implement `__traverse__` for Python's cyclic GC.

### Summary: Applicable Patterns

| pydantic-core Pattern | quarto-markdown Application |
|----------------------|----------------------------|
| Schema compilation | Compile filter specs to Rust filter chain at load time |
| Trait-based composition | Filter actions implement common interface |
| ValidationState context | FilterState carries doc, metadata, diagnostics |
| Python callback storage | Store action functions as `Py<PyAny>` |
| Weak refs for definitions | Not immediately needed, but good for future |
| RecursionGuard | Implement in `walk()` to prevent infinite loops |
| Input trait abstraction | Could generalize over input formats |
| GC traverse macro | Essential for any struct holding Py refs |

### Target Developer Experience (Panflute)

[Panflute](https://scorreia.com/software/panflute/) provides:

```python
def action(elem, doc):
    if isinstance(elem, pf.Emph):
        return pf.Strikeout(*elem.content)

def main(doc=None):
    return pf.run_filter(action, doc=doc)
```

**Key API elements:**
- `run_filter(action, prepare=None, finalize=None, doc=None)`
- `elem.walk(action)` for recursive traversal
- Element constructors: `pf.Str()`, `pf.Para()`, etc.
- `doc.get_metadata()`, `stringify(elem)`
- Return `None` (unchanged), element (replace), `[]` (delete)

## Proposed Architecture

### Option A: Python-First with Embedded Rust (Recommended)

Create a Python package `quarto-markdown` that wraps the Rust library:

```
quarto-markdown-python/     # Crate directory
├── pyproject.toml         # maturin-based build
├── Cargo.toml             # Rust crate configuration
├── src/
│   ├── lib.rs             # PyO3 module entry point
│   ├── types.rs           # Python wrappers for AST types
│   ├── constructors.rs    # Element constructors
│   ├── filter.rs          # Filter execution
│   └── io.rs              # read/write functions
├── python/
│   └── quarto_markdown/   # Python package (pip install quarto-markdown)
│       ├── __init__.py    # High-level API
│       ├── types.py       # Type stubs + pure Python helpers
│       └── py.typed       # PEP 561 marker
└── tests/
```

**Crate dependency:**
```toml
[dependencies]
pyo3 = { version = "0.23", features = ["extension-module", "abi3-py39"] }
quarto-pandoc-types = { path = "../quarto-pandoc-types" }
quarto-markdown-pandoc = { path = "../quarto-markdown-pandoc", default-features = false }
```

### Option B: Embedded Python in Rust

Alternatively, extend `quarto-markdown-pandoc` to embed Python:

```rust
// In quarto-markdown-pandoc/src/python/filter.rs
use pyo3::prelude::*;

pub fn apply_python_filter(
    pandoc: &Pandoc,
    filter_path: &Path,
    target_format: &str,
) -> FilterResult<Pandoc> {
    pyo3::prepare_freethreaded_python();
    Python::with_gil(|py| {
        // Load filter module
        // Register pandoc namespace
        // Execute filter
    })
}
```

**Trade-offs:**

| Aspect | Option A | Option B |
|--------|----------|----------|
| Primary use case | Python developers calling Quarto | Quarto calling Python filters |
| Python dependency | At runtime | At build time + runtime |
| WASM support | No | Possible with RustPython fallback |
| Distribution | pip installable | Part of quarto binary |
| Developer experience | Excellent | Good |

**Recommendation:** Start with Option A for better Python ecosystem integration, then optionally add Option B for the embedded filter use case.

## Detailed Design

### 1. AST Type Wrappers

Following the Lua pattern in `types.rs`, create Python classes for each AST type:

```python
# What users see (type stubs + runtime)
class Inline:
    @property
    def tag(self) -> str: ...
    def clone(self) -> "Inline": ...
    def walk(self, action: Callable) -> "Inline": ...

class Str(Inline):
    text: str
    def __init__(self, text: str) -> None: ...

class Emph(Inline):
    content: list[Inline]
    def __init__(self, *content: Inline) -> None: ...
```

```rust
// Rust implementation
#[pyclass]
pub struct PyInline(pub Inline);

#[pymethods]
impl PyInline {
    #[getter]
    fn tag(&self) -> &'static str {
        match &self.0 {
            Inline::Str(_) => "Str",
            Inline::Emph(_) => "Emph",
            // ...
        }
    }

    fn __getattr__(&self, py: Python, name: &str) -> PyResult<PyObject> {
        match (&self.0, name) {
            (Inline::Str(s), "text") => Ok(s.text.clone().into_py(py)),
            (Inline::Emph(e), "content") => {
                Ok(inlines_to_list(py, &e.content)?.into())
            }
            // ...
        }
    }
}
```

### 2. Element Constructors

Provide a `pandoc` module with element constructors:

```python
import quarto_markdown as qm

# Element construction
s = qm.Str("hello")
e = qm.Emph(qm.Str("world"))
p = qm.Para(s, qm.Space(), e)

# Attribute construction
attr = qm.Attr(id="myid", classes=["cls1", "cls2"], attributes={"key": "value"})
```

### 3. Document I/O

```python
import quarto_markdown as qm

# Read from QMD
doc = qm.read("document.qmd")
doc = qm.read(qmd_string, format="qmd")

# Read from JSON (Pandoc format)
doc = qm.read("document.json", format="json")

# Write to various formats
qm.write(doc, "output.json", format="json")
qm.write(doc, "output.md", format="markdown")
native_repr = qm.dumps(doc, format="native")
```

### 4. Filter Execution

```python
import quarto_markdown as qm

def action(elem, doc):
    if isinstance(elem, qm.Emph):
        return qm.Strong(*elem.content)
    return None  # unchanged

def main(doc=None):
    return qm.run_filter(action, doc=doc)

if __name__ == "__main__":
    main()
```

### 5. Traversal Modes

Support both typewise (default) and topdown traversal:

```python
# Typewise (default) - process by element type
@qm.filter
def process_emphasis(elem: qm.Emph, doc) -> qm.Inline | None:
    return qm.Strong(*elem.content)

# Topdown - explicit traversal control
@qm.filter(traverse="topdown")
def process_document(elem, doc):
    if isinstance(elem, qm.Div):
        # Process div, stop descent
        return elem, False
    return None, True  # continue descent
```

### 6. Runtime Abstraction

Port the Lua runtime abstraction for sandboxing:

```python
# Default: full system access
runtime = qm.NativeRuntime()

# Sandboxed: restricted access (for untrusted filters)
runtime = qm.SandboxedRuntime(
    allow_read=["/safe/path"],
    allow_write=[],
    allow_net=False
)

doc = qm.run_filter(action, doc=doc, runtime=runtime)
```

## Implementation Plan

### Phase 1: Foundation (MVP)

1. Create `quarto-markdown-python` crate with PyO3 setup
2. Implement core AST type wrappers (Inline, Block, Document)
3. Implement basic element constructors
4. Implement `read()` and `write()` for QMD/JSON
5. Basic filter execution with `run_filter()`

**Deliverable:** Python package that can read QMD, run simple filters, write output

### Phase 2: Full API

1. Complete all AST type wrappers (including Table, Citation, etc.)
2. Implement `walk()` on elements
3. Add typewise and topdown traversal modes
4. Implement `doc.get_metadata()`
5. Add diagnostic collection (`quarto.warn()`, `quarto.error()`)

**Deliverable:** Feature-complete Python filter API

### Phase 3: Polish and Ecosystem

1. Type stubs for IDE support (`py.typed`, `.pyi` files)
2. ABI3 wheels for Python 3.9+
3. CI/CD with maturin for cross-platform builds
4. Documentation and examples
5. Panflute compatibility layer (optional)

**Deliverable:** Production-ready package on PyPI

### Phase 4: Embedded Python Filters (Optional)

1. Add Python embedding to `quarto-markdown-pandoc`
2. Extend `FilterSpec` to include Python filters
3. Unified filter chain (Lua, Python, JSON)
4. Consider RustPython for WASM support

**Deliverable:** `-F filter.py` support in the Quarto CLI

## Technical Considerations

### GIL and Threading

PyO3 requires careful handling of Python's GIL:
- Use `Python::with_gil()` for all Python operations
- Consider `Python::allow_threads()` for CPU-bound Rust work
- Filter execution is single-threaded (matches Lua behavior)

### Memory Management

- Rust owns the AST; Python gets wrapped references
- Use `Py<T>` for Python-owned data that outlives `with_gil()`
- Clone AST nodes when returning from filters (immutable by default)

### Error Handling

- Rust `Result` maps to Python exceptions
- Filter errors should include source locations when available
- Integrate with `quarto-error-reporting` for consistent diagnostics

### Platform Support

- Linux: x86_64, aarch64 (manylinux wheels)
- macOS: x86_64, aarch64
- Windows: x86_64
- No WASM support initially (requires RustPython or Pyodide research)

## Open Questions (Remaining)

1. **JSON filter compatibility:** Support Panflute JSON filter protocol for interop?
2. **Async support:** Should we support async filter functions for I/O-bound operations?
3. **Thread safety:** Should Document be `Send + Sync` for multi-threaded Python code?
4. **Pickling:** Should we support pickle/cloudpickle for distributed computing scenarios (like pydantic-core does)?

## References

### PyO3 and maturin
- [PyO3 User Guide](https://pyo3.rs/)
- [maturin User Guide](https://www.maturin.rs/)
- [PyO3 GitHub](https://github.com/PyO3/pyo3)
- [maturin GitHub](https://github.com/PyO3/maturin)

### Notable Projects
- [Polars](https://github.com/pola-rs/polars) - DataFrame library
- [pydantic-core](https://github.com/pydantic/pydantic-core) - Validation core
- [orjson](https://github.com/ijl/orjson) - Fast JSON library
- [cryptography](https://github.com/pyca/cryptography) - Crypto library

### Alternative Approaches
- [RustPython](https://github.com/RustPython/RustPython) - Pure Rust Python
- [PyOxidizer](https://pyoxidizer.readthedocs.io/) - Python embedding

### Pandoc Ecosystem
- [Panflute](https://scorreia.com/software/panflute/) - Python Pandoc filters
- [pandocfilters](https://github.com/jgm/pandocfilters) - Original Python filters
- [Pandoc Lua Filters](https://pandoc.org/lua-filters.html) - Official docs
