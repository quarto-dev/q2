# HTML Attribute Handling Fix Plan

## Problem Statement

The pampa HTML writer incorrectly prefixes ALL key-value attributes with `data-`. This causes several issues:

1. **Standard HTML5 attributes** like `style`, `title`, `dir`, `lang`, `width`, `height` are incorrectly written as `data-style`, `data-title`, etc.
2. **Already-prefixed `data-*` attributes** are doubled: `data-foo` becomes `data-data-foo`
3. **ARIA attributes** like `aria-label`, `aria-hidden` are incorrectly written as `data-aria-label`, etc.

### Example

Input: `[text]{style="color:red"}`

- **Current (incorrect)**: `<span data-style="color:red">text</span>`
- **Expected (Pandoc-compatible)**: `<span style="color:red">text</span>`

## Root Cause

In `crates/pampa/src/writers/html.rs`, lines 176-179:

```rust
// Pandoc prefixes custom attributes with "data-"
for (k, v) in attrs {
    write!(ctx, " data-{}=\"{}\"", escape_html(k), escape_html(v))?;
}
```

This unconditionally prefixes all attributes with `data-`.

## Pandoc's Behavior

Pandoc uses sophisticated logic (from `src/Text/Pandoc/Writers/HTML.hs`):

1. Check if attribute is in `html5Attributes` set (~140 standard attributes)
2. Check if attribute is in `rdfaAttributes` set
3. Check if attribute already starts with `data-` or `aria-`
4. Check if attribute contains `:` (namespace prefix like `epub:type`)

Only if NONE of these conditions are met does Pandoc add the `data-` prefix.

## Implementation Plan

### Phase 1: Create HTML5 Attributes Set

Create a new module or const in `crates/pampa/src/writers/html.rs` with the standard HTML5 attributes:

```rust
/// Standard HTML5 attributes that should NOT be prefixed with data-
/// Based on https://html.spec.whatwg.org/multipage/indices.html#attributes-3
const HTML5_ATTRIBUTES: &[&str] = &[
    "abbr", "accept", "accept-charset", "accesskey", "action",
    "allow", "alt", "async", "autocapitalize", "autocomplete",
    "autofocus", "autoplay", "charset", "checked", "cite",
    "class", "color", "cols", "colspan", "content", "contenteditable",
    "controls", "coords", "crossorigin", "data", "datetime",
    "decoding", "default", "defer", "dir", "dirname", "disabled",
    "download", "draggable", "enctype", "enterkeyhint", "for",
    "form", "formaction", "formenctype", "formmethod", "formnovalidate",
    "formtarget", "headers", "height", "hidden", "high", "href",
    "hreflang", "http-equiv", "id", "imagesizes", "imagesrcset",
    "inputmode", "integrity", "is", "ismap", "itemid", "itemprop",
    "itemref", "itemscope", "itemtype", "kind", "label", "lang",
    "list", "loading", "loop", "low", "manifest", "max", "maxlength",
    "media", "method", "min", "minlength", "multiple", "muted",
    "name", "nomodule", "nonce", "novalidate", "open", "optimum",
    "pattern", "ping", "placeholder", "playsinline", "poster",
    "preload", "readonly", "referrerpolicy", "rel", "required",
    "reversed", "role", "rows", "rowspan", "sandbox", "scope",
    "selected", "shape", "size", "sizes", "slot", "span",
    "spellcheck", "src", "srcdoc", "srclang", "srcset", "start",
    "step", "style", "tabindex", "target", "title", "translate",
    "type", "typemustmatch", "updateviacache", "usemap", "value",
    "width", "workertype", "wrap",
    // Event handlers
    "onabort", "onauxclick", "onbeforematch", "onblur", "oncancel",
    "oncanplay", "oncanplaythrough", "onchange", "onclick", "onclose",
    "oncontextmenu", "oncopy", "oncuechange", "oncut", "ondblclick",
    "ondrag", "ondragend", "ondragenter", "ondragleave", "ondragover",
    "ondragstart", "ondrop", "ondurationchange", "onemptied", "onended",
    "onerror", "onfocus", "onformdata", "oninput", "oninvalid",
    "onkeydown", "onkeypress", "onkeyup", "onload", "onloadeddata",
    "onloadedmetadata", "onloadstart", "onmousedown", "onmouseenter",
    "onmouseleave", "onmousemove", "onmouseout", "onmouseover",
    "onmouseup", "onpaste", "onpause", "onplay", "onplaying",
    "onprogress", "onratechange", "onreset", "onresize", "onscroll",
    "onsecuritypolicyviolation", "onseeked", "onseeking", "onselect",
    "onslotchange", "onstalled", "onsubmit", "onsuspend", "ontimeupdate",
    "ontoggle", "onvolumechange", "onwaiting", "onwheel",
];

/// RDFa attributes
const RDFA_ATTRIBUTES: &[&str] = &[
    "about", "content", "datatype", "href", "prefix",
    "property", "rel", "resource", "rev", "src", "typeof", "vocab",
];
```

### Phase 2: Update write_attr Function

Modify the `write_attr` function to check attributes before prefixing:

```rust
fn write_attr<W: Write>(attr: &Attr, ctx: &mut HtmlWriterContext<'_, W>) -> std::io::Result<()> {
    let (id, classes, attrs) = attr;

    if !id.is_empty() {
        write!(ctx, " id=\"{}\"", escape_html(id))?;
    }

    if !classes.is_empty() {
        write!(ctx, " class=\"{}\"", escape_html(&classes.join(" ")))?;
    }

    for (k, v) in attrs {
        if should_prefix_attribute(k) {
            write!(ctx, " data-{}=\"{}\"", escape_html(k), escape_html(v))?;
        } else {
            write!(ctx, " {}=\"{}\"", escape_html(k), escape_html(v))?;
        }
    }

    Ok(())
}

/// Determine if an attribute should be prefixed with data-
fn should_prefix_attribute(attr: &str) -> bool {
    // Never prefix if already has data- or aria- prefix
    if attr.starts_with("data-") || attr.starts_with("aria-") {
        return false;
    }

    // Never prefix if contains colon (namespace prefix)
    if attr.contains(':') {
        return false;
    }

    // Never prefix standard HTML5 or RDFa attributes
    if is_html5_attribute(attr) || is_rdfa_attribute(attr) {
        return false;
    }

    // Everything else gets prefixed
    true
}

fn is_html5_attribute(attr: &str) -> bool {
    HTML5_ATTRIBUTES.binary_search(&attr).is_ok()
}

fn is_rdfa_attribute(attr: &str) -> bool {
    RDFA_ATTRIBUTES.binary_search(&attr).is_ok()
}
```

### Phase 3: Performance Optimization

For efficient lookup, we have options:

1. **Sorted array with binary search** (simplest, ~O(log n))
2. **HashSet** (O(1) but more memory)
3. **phf crate** for perfect hash at compile time (O(1), zero runtime cost)

Recommendation: Start with sorted array + binary search. If profiling shows this is a bottleneck (unlikely), upgrade to phf.

### Phase 4: Enable Tests

Remove `#[ignore]` annotations from tests in `crates/pampa/tests/test_html_attr_handling.rs`.

## Testing

Tests are already written in `crates/pampa/tests/test_html_attr_handling.rs`:

- `test_style_attribute_not_prefixed`
- `test_title_attribute_not_prefixed`
- `test_dir_attribute_not_prefixed`
- `test_lang_attribute_not_prefixed`
- `test_width_attribute_not_prefixed`
- `test_height_attribute_not_prefixed`
- `test_data_attribute_not_doubled`
- `test_data_cites_attribute_preserved`
- `test_aria_label_not_prefixed`
- `test_aria_hidden_not_prefixed`
- `test_custom_attribute_prefixed` (already passes)
- `test_unknown_attribute_prefixed` (already passes)
- `test_mixed_attributes`
- `test_div_style_attribute`

## Files to Modify

1. `crates/pampa/src/writers/html.rs` - Main fix
2. `crates/pampa/tests/test_html_attr_handling.rs` - Remove `#[ignore]` when fix is complete

## Verification

After implementation:

```bash
# Run all HTML attribute tests
cargo nextest run -p pampa test_html_attr_handling

# Compare with Pandoc
echo '[text]{style="color:red"}' | pandoc -f markdown -t html
printf '---\ntitle: Test\n---\n\n[text]{style="color:red"}\n' | cargo run --bin pampa -- -f qmd -t html
```

Both should output: `<span style="color:red">text</span>`

## Impact

This fix is required for:
- Proper CSS styling in WASM preview (the original bug report)
- Correct ARIA accessibility attributes
- Proper handling of standard HTML attributes like `title`, `dir`, `lang`
- Pandoc compatibility
