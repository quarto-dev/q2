# HTML Postprocessor Analysis and Rust DOM API Design

**Date**: 2025-12-20
**Status**: Research Complete - API Design Pending
**Issue**: k-xol0
**Parent Epic**: k-xlko

## Executive Summary

This document analyzes the HTML postprocessors in quarto-cli to understand DOM manipulation patterns and inform the design of a Rust DOM API for the Quarto Rust port. The TypeScript version uses browser-style DOM manipulation via `linkedom` or `deno_dom`. The Rust port will need equivalent capabilities.

## Postprocessor Inventory

Based on analysis of `external-sources/quarto-cli/src/format/html/`, the following postprocessors are declared in `htmlFormatExtras()`:

| Postprocessor | Source File | Purpose |
|---------------|-------------|---------|
| `htmlFormatPostprocessor` | `format-html.ts` | Core HTML formatting (code tools, anchors, annotations) |
| `metadataPostProcessor` | `format-html-meta.ts` | SEO metadata (Google Scholar, Open Graph) |
| `notebookViewPostProcessor` | `format-html-notebook.ts` | Notebook cell styling and counters |
| `bootstrapHtmlPostprocessor` | `format-html-bootstrap.ts` | Bootstrap-specific styling and TOC |
| `codeToolsPostprocessor` | `codetools.ts` | Code tools UI (source toggle) |
| `overflowXPostprocessor` | `layout.ts` | Prevent horizontal scrolling for widgets |
| `katexPostProcessor` | `format-html-math.ts` | KaTeX module system fixes |
| `discoverResourceRefs` | `html.ts` | Resource dependency discovery |
| `fixEmptyHrefs` | `html.ts` | Ensure anchors have href for CSS |
| `metadataHtmlPostProcessor` | `website-meta.ts` | Social media metadata |
| `dashboardHtmlPostProcessor` | `format-dashboard.ts` | Dashboard layout transformation |

## Detailed DOM Operations Analysis

This section catalogs every DOM operation found in the postprocessors, organized by operation type.

### Query Operations

| Operation | Example | Frequency |
|-----------|---------|-----------|
| `querySelectorAll(selector)` | `doc.querySelectorAll("pre.sourceCode")` | **Very High** |
| `querySelector(selector)` | `doc.querySelector("header > .title")` | **Very High** |
| `getElementById(id)` | `doc.getElementById("quarto-header")` | Medium |

**Selector patterns used:**
- Tag: `table`, `blockquote`, `figure`, `a`, `style`
- Class: `.sourceCode`, `.cell`, `.cell-output-display`
- ID: `#TOC`, `#quarto-toc-target`, `#title-block-header`
- Attribute: `[data-quarto-postprocess="true"]`, `[data-execution_count]`
- Attribute suffix: `[src$="katex.min.js"]`, `[href$="katex.min.css"]`
- Child: `header > .title`, `nav#TOC > ul a`
- Descendant: `.column-margin .cell-output-display img`
- Pseudo: `tbody > tr:first-child.odd`
- Union: `h2, h3, h4, h5, h6`
- Negation: `img:not(.img-fluid)`

### Element Creation

| Operation | Example | Frequency |
|-----------|---------|-----------|
| `createElement(tag)` | `doc.createElement("div")` | **Very High** |
| `createTextNode(text)` | `doc.createTextNode("Draft")` | Medium |

### Class Manipulation

| Operation | Example | Frequency |
|-----------|---------|-----------|
| `classList.add(class)` | `el.classList.add("anchored")` | **Very High** |
| `classList.remove(class)` | `el.classList.remove("hidden")` | Medium |
| `classList.contains(class)` | `el.classList.contains("no-anchor")` | High |

### Attribute Operations

| Operation | Example | Frequency |
|-----------|---------|-----------|
| `getAttribute(name)` | `el.getAttribute("data-code-preview")` | **Very High** |
| `setAttribute(name, value)` | `el.setAttribute("data-scroll-target", href)` | **Very High** |
| `removeAttribute(name)` | `el.removeAttribute("style")` | High |
| `hasAttribute(name)` | `el.hasAttribute("data-scroll-target")` | Low |
| `attributes` (collection) | `for (const attr of el.attributes)` | Low |

### Tree Navigation

| Operation | Example | Frequency |
|-----------|---------|-----------|
| `parentElement` | `code.parentElement?.classList.add(clz)` | **Very High** |
| `parentNode` | `child.parentNode?.replaceChild(...)` | High |
| `children` | `for (const child of el.children)` | Medium |
| `firstChild` | `while (child.firstChild)` | Medium |
| `previousElementSibling` | `secNumber.previousElementSibling` | Medium |
| `nextSibling` | `katexScript.nextSibling` | Low |

### Tree Modification

| Operation | Example | Frequency |
|-----------|---------|-----------|
| `appendChild(child)` | `container.appendChild(element)` | **Very High** |
| `insertBefore(new, ref)` | `parent.insertBefore(scaffold, el)` | High |
| `replaceChild(new, old)` | `parent.replaceChild(outerScaffold, sourceCodeDiv)` | High |
| `remove()` | `element.remove()` | High |
| `replaceWith(el)` | `sidebarEl.replaceWith(sidebarContainerEl)` | Medium |
| `prepend(el)` | `header.prepend(titleDiv)` | Low |
| `append(...nodes)` | `container.append(...sidebarToggle(id, doc))` | Low |
| `cloneNode(deep)` | `toc.cloneNode(true)` | Low |

### Content Manipulation

| Operation | Example | Frequency |
|-----------|---------|-----------|
| `innerHTML` (get) | `if (style.innerHTML)` | Medium |
| `innerHTML` (set) | `container.innerHTML = html` | Medium |
| `innerText` (get) | `line.innerText` | Medium |
| `innerText` (set) | `el.innerText = text` | Medium |
| `textContent` (set) | `codeBlockLine.textContent = ""` | Low |

### Property Access

| Operation | Example | Frequency |
|-----------|---------|-----------|
| `id` (get) | `if (heading.id !== "toc-title")` | High |
| `id` (set) | `clonedToc.id = "TOC-body"` | Low |
| `tagName` | `if (child.tagName === "TH")` | Medium |

### Document-Level

| Operation | Example | Frequency |
|-----------|---------|-----------|
| `doc.body` | `doc.body.classList.add(kDashboardClz)` | High |
| `doc.head` (via querySelector) | `doc.querySelector("head")?.appendChild(m)` | Medium |

---

## Postprocessor-by-Postprocessor Analysis

### 1. htmlFormatPostprocessor (format-html.ts:697-896)

**Operations used:**
```
classList.add, classList.remove, classList.contains
querySelectorAll, querySelector, getElementById
createElement, createTextNode
getAttribute, setAttribute, removeAttribute
parentElement, previousElementSibling, firstChild
appendChild, insertBefore, replaceChild, remove
attributes (iteration), tagName, id
```

**Key patterns:**
- Class hoisting: `code.classList.remove(clz); code.parentElement?.classList.add(clz)`
- Scaffold insertion: `parent.replaceChild(scaffold, el); scaffold.appendChild(el)`
- Element replacement: Create new element, move children, copy attributes, replace

### 2. bootstrapHtmlPostprocessor (format-html-bootstrap.ts:277-545)

**Operations used:**
```
querySelector, querySelectorAll, getElementById
classList.add, classList.contains
getAttribute, setAttribute, removeAttribute, hasAttribute
cloneNode, remove, replaceWith, insertBefore, appendChild
parentElement, parentNode, children, firstChild
```

**Key patterns:**
- TOC manipulation: `toc.remove(); tocTarget.replaceWith(toc)`
- TOC cloning: `const clonedToc = toc.cloneNode(true)`
- Attribute copying: `link.getAttribute("href")?.replaceAll(":", "\\:")`

### 3. notebookViewPostProcessor (format-html-notebook.ts:36-97)

**Operations used:**
```
querySelectorAll
classList.add, classList.contains
getAttribute
createElement, createTextNode
appendChild, insertBefore, cloneNode, remove
parentElement, previousElementSibling, tagName
```

**Key patterns:**
- Wrapper creation: Create container, decorator, content elements
- Sibling processing: Check previous sibling, clone and remove

### 4. codeToolsPostprocessor (codetools.ts:92-250)

**Operations used:**
```
querySelectorAll, querySelector
classList.add
getAttribute, setAttribute
createElement, createTextNode
appendChild, replaceChild, prepend
parentElement, firstChild
innerHTML (set), innerText (get/set), textContent (set)
```

**Key patterns:**
- Dropdown menu construction: Create nested ul/li/a structure
- Text manipulation: `line.innerText`, `span.innerText = text.replace(...)`

### 5. metadataPostProcessor (format-html-meta.ts)

**Operations used:**
```
createElement
setAttribute
querySelector (for head)
appendChild
createTextNode (for newlines)
```

**Very simple pattern:** Create META/LINK element, set attributes, append to head.

### 6. katexPostProcessor (format-html-math.ts)

**Operations used:**
```
querySelector (attribute suffix selector)
removeAttribute
createElement
insertBefore
parentNode, nextSibling (implicit via insertBefore)
innerText (set)
```

**Simple pattern:** Find script, insert before/after it.

### 7. discoverResourceRefs (html.ts)

**Operations used:**
```
querySelectorAll
getAttribute
innerHTML (get)
```

**Read-only pattern:** Only queries and reads, no modifications.

### 8. fixEmptyHrefs (html.ts)

**Operations used:**
```
querySelectorAll
getAttribute
setAttribute
```

**Simple pattern:** Query anchors, set empty href if missing.

### 9. dashboardHtmlPostProcessor (format-dashboard.ts)

**Operations used:**
```
querySelectorAll, querySelector
classList.add, classList.remove, classList.contains
getAttribute, setAttribute, removeAttribute
createElement
appendChild, replaceWith
parentElement, children, tagName
innerHTML (set)
```

**Key patterns:**
- Recursive child processing
- Element replacement with complex new structure
- Delegate to sub-functions (processCards, processSidebars, etc.)

### 10. Helper: makeEl (format-dashboard-shared.ts)

```typescript
export function makeEl(name: string, attr: Attr, doc: Document) {
  const el = doc.createElement(name);
  if (attr.id) el.id = attr.id;
  for (const cls of attr.classes || []) el.classList.add(cls);
  for (const key of Object.keys(attr.attributes || {})) {
    el.setAttribute(key, attr.attributes[key]);
  }
  return el;
}
```

This is a builder pattern for element creation - useful for Rust API design.

---

## Summary: Minimum Viable DOM API

Based on this analysis, here are the **required operations** for Rust:

### Tier 1: Essential (used by almost every postprocessor)

```rust
// Document
doc.query_selector(selector) -> Option<Element>
doc.query_selector_all(selector) -> Vec<Element>
doc.create_element(tag) -> Element
doc.body() -> Element

// Element - Query
el.query_selector(selector) -> Option<Element>
el.query_selector_all(selector) -> Vec<Element>

// Element - Classes
el.class_list().add(class)
el.class_list().remove(class)
el.class_list().contains(class) -> bool

// Element - Attributes
el.get_attribute(name) -> Option<String>
el.set_attribute(name, value)
el.remove_attribute(name)

// Element - Tree Navigation
el.parent_element() -> Option<Element>

// Element - Tree Modification
el.append_child(child)
el.insert_before(new_child, reference)
el.remove()
```

### Tier 2: Commonly Used

```rust
// Document
doc.create_text_node(text) -> TextNode
doc.get_element_by_id(id) -> Option<Element>

// Element - Tree Navigation
el.children() -> Vec<Element>
el.first_child() -> Option<Node>
el.previous_element_sibling() -> Option<Element>

// Element - Tree Modification
el.replace_child(new_child, old_child)
el.replace_with(new_element)
el.clone_node(deep: bool) -> Element

// Element - Properties
el.id() -> String
el.set_id(id)
el.tag_name() -> String
```

### Tier 3: Less Common but Needed

```rust
// Document
doc.head() -> Element  // or via query_selector

// Element - Attributes
el.has_attribute(name) -> bool
el.attributes() -> Vec<(String, String)>

// Element - Tree Navigation
el.parent_node() -> Option<Node>
el.next_sibling() -> Option<Node>

// Element - Tree Modification
el.prepend(child)
el.append(children...)

// Element - Content
el.inner_html() -> String
el.set_inner_html(html)
el.inner_text() -> String
el.set_inner_text(text)
el.text_content() -> Option<String>
el.set_text_content(text)
```

### CSS Selector Requirements

Must support:
- Tag: `table`
- Class: `.sourceCode`
- ID: `#TOC`
- Attribute presence: `[data-quarto-postprocess]`
- Attribute value: `[data-quarto-postprocess="true"]`
- Attribute suffix: `[src$="katex.min.js"]`
- Child combinator: `header > .title`
- Descendant combinator: `.column-margin img`
- Union: `h2, h3, h4, h5, h6`
- Negation: `:not(.img-fluid)`
- Pseudo-class: `:first-child`

## Rust Implementation Strategy

Given the analysis above, here's the recommended approach:

### Use scraper for Parsing + Custom Mutable Wrapper

Since `scraper` uses an immutable tree (ego-tree), we need a strategy for mutations:

**Option A: Convert to mutable tree after parsing**
1. Parse HTML with scraper (get query/selector support)
2. Convert to our own mutable tree structure
3. Postprocessors work on mutable tree
4. Serialize back to HTML

**Option B: Use html5ever + rcdom directly**
1. Parse with html5ever into rcdom (already mutable via Rc<RefCell>)
2. Implement `selectors::Element` trait for rcdom nodes
3. Build our DOM wrapper API on top
4. Serialize with html5ever's serializer

**Recommendation**: Option B is cleaner since rcdom is already mutable. We just need to:
1. Add CSS selector support (implement `selectors::Element`)
2. Build ergonomic wrapper API matching the operations above

### The selectors::Element Trait

The `selectors` crate is a generic CSS selector engine extracted from Firefox/Servo. It doesn't know about any specific tree structure - you teach it how to navigate and inspect your tree by implementing its `Element` trait:

```rust
pub trait Element: Sized {
    type Impl: SelectorImpl;

    // Navigation
    fn parent_element(&self) -> Option<Self>;
    fn prev_sibling_element(&self) -> Option<Self>;
    fn next_sibling_element(&self) -> Option<Self>;

    // Identity
    fn is_html_element_in_html_document(&self) -> bool;
    fn local_name(&self) -> &LocalName;
    fn namespace(&self) -> &Namespace;

    // Matching
    fn has_id(&self, id: &Atom, case_sensitivity: CaseSensitivity) -> bool;
    fn has_class(&self, name: &Atom, case_sensitivity: CaseSensitivity) -> bool;
    fn attr_matches(&self, ns: &NamespaceConstraint, local_name: &LocalName, operation: &AttrSelectorOperation) -> bool;

    // ... ~15 more methods for pseudo-classes, etc.
}
```

Once implemented for rcdom's `Handle` type, selector matching is free:

```rust
let selector = Selector::parse("div.foo > span[data-x]").unwrap();
let matches: Vec<_> = elements.iter()
    .filter(|el| selector.matches(el))
    .collect();
```

This is exactly what `scraper` does - it implements `selectors::Element` for ego-tree nodes. Surprisingly, no existing crate provides this adapter for rcdom, so we'll need to write it ourselves. The implementation is straightforward (~100-200 lines) since it's just delegation to rcdom's existing node inspection methods.

### Proposed Rust Traits

```rust
pub trait HtmlDocument {
    fn query_selector(&self, selector: &str) -> Option<ElementRef>;
    fn query_selector_all(&self, selector: &str) -> Vec<ElementRef>;
    fn create_element(&self, tag: &str) -> ElementRef;
    fn create_text_node(&self, text: &str) -> NodeRef;
    fn body(&self) -> ElementRef;
    fn head(&self) -> ElementRef;
    fn get_element_by_id(&self, id: &str) -> Option<ElementRef>;
    fn serialize(&self) -> String;
}

pub trait Element {
    // Query
    fn query_selector(&self, selector: &str) -> Option<ElementRef>;
    fn query_selector_all(&self, selector: &str) -> Vec<ElementRef>;

    // Classes
    fn class_list(&self) -> ClassList;

    // Attributes
    fn get_attribute(&self, name: &str) -> Option<String>;
    fn set_attribute(&self, name: &str, value: &str);
    fn remove_attribute(&self, name: &str);
    fn has_attribute(&self, name: &str) -> bool;

    // Properties
    fn id(&self) -> Option<String>;
    fn set_id(&self, id: &str);
    fn tag_name(&self) -> &str;

    // Tree navigation
    fn parent_element(&self) -> Option<ElementRef>;
    fn children(&self) -> Vec<ElementRef>;
    fn first_child(&self) -> Option<NodeRef>;
    fn previous_element_sibling(&self) -> Option<ElementRef>;

    // Tree modification
    fn append_child(&self, child: NodeRef);
    fn insert_before(&self, new_child: NodeRef, reference: &NodeRef);
    fn remove(&self);
    fn replace_with(&self, new_element: ElementRef);
    fn clone_node(&self, deep: bool) -> ElementRef;

    // Content
    fn inner_html(&self) -> String;
    fn set_inner_html(&self, html: &str);
    fn text_content(&self) -> Option<String>;
    fn set_text_content(&self, text: &str);
}

pub struct ClassList { /* ... */ }

impl ClassList {
    fn add(&self, class: &str);
    fn remove(&self, class: &str);
    fn contains(&self, class: &str) -> bool;
    fn toggle(&self, class: &str) -> bool;
}
```

---

## Next Steps

1. **Prototype DOM wrapper** - Build wrapper around html5ever + rcdom with selector support
2. **Implement core operations** - Focus on Tier 1 operations first
3. **Port htmlFormatPostprocessor** - First test case, covers most patterns
4. **Add selector patterns** - Implement required CSS selector features
5. **Port remaining postprocessors** - In order of complexity

---

## References

- **quarto-cli source**: `external-sources/quarto-cli/src/format/html/`
  - `format-html.ts` - main postprocessor declarations (line 697+)
  - `format-html-bootstrap.ts` - Bootstrap styling (line 277+)
  - `format-html-meta.ts` - SEO metadata
  - `format-html-notebook.ts` - notebook view
  - `codetools.ts` - code tools UI
  - `format-dashboard.ts` - dashboard layout
- **Rust crates**:
  - [html5ever](https://crates.io/crates/html5ever) - HTML5 parser
  - [markup5ever_rcdom](https://crates.io/crates/markup5ever_rcdom) - mutable DOM tree
  - [selectors](https://crates.io/crates/selectors) - CSS selector engine (used by Firefox)
