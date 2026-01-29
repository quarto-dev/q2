# Editorial Marks SCSS Styling

## Overview

Add SCSS styling support for editorial marks (`[++ content]`, `[-- content]`, `[!! content]`, `[>> content]`) in Rust Quarto. The styling should be defined in the Quarto SCSS layer system so that:

1. Default styling is provided out of the box
2. Users can easily override the default styles via custom SCSS
3. Styling respects light/dark themes

## Background

### Editorial Mark Syntax and HTML Output

| Syntax | Element | Purpose |
|--------|---------|---------|
| `[++ text]` | `<span class="quarto-insert">` | Insertions |
| `[-- text]` | `<span class="quarto-delete">` | Deletions |
| `[!! text]` | `<span class="quarto-highlight">` | Highlights |
| `[>> text]` | `<span class="quarto-edit-comment">` | Comments |

Note: These are converted from AST types (`Insert`, `Delete`, `Highlight`, `EditComment`) to Spans with Quarto-specific classes during postprocessing in `pampa/src/pandoc/treesitter_utils/postprocess.rs`.

### Current State

- `<mark>` has minimal styling (`padding: 0em`) in `_bootstrap-rules.scss`
- `<ins>`, `<del>`, `<span class="comment">` use browser defaults (underline, strikethrough, none)
- No theme-aware colors or customizable variables

### Layer System

The SCSS layer system assembles styles in this order:
1. **USES**: framework → quarto → user
2. **FUNCTIONS**: framework → quarto → user
3. **DEFAULTS**: user → quarto → framework (reversed for `!default`)
4. **MIXINS**: framework → quarto → user
5. **RULES**: framework → quarto → user

This means:
- Variables defined with `!default` in the Quarto layer can be overridden by users
- Rules in the Quarto layer provide defaults that can be overridden by user rules

## Design Decisions

### 1. Variable Location

Define editorial mark variables in `_bootstrap-variables.scss` with `!default` so users can override:

```scss
// Editorial marks
$editorial-ins-color: null !default;         // null = inherit
$editorial-ins-bg: rgba(0, 255, 0, 0.1) !default;
$editorial-ins-decoration: none !default;

$editorial-del-color: null !default;         // null = inherit
$editorial-del-bg: rgba(255, 0, 0, 0.1) !default;
$editorial-del-decoration: line-through !default;

$editorial-mark-bg: rgba(255, 255, 0, 0.3) !default;
$editorial-mark-padding: 0.1em 0.2em !default;

$editorial-comment-color: $text-muted !default;
$editorial-comment-style: italic !default;
$editorial-comment-bg: null !default;        // null = transparent
```

### 2. Rules Location

Define editorial mark rules in `_bootstrap-rules.scss`:

```scss
// Editorial marks
.quarto-insert {
  @if $editorial-ins-bg {
    background-color: $editorial-ins-bg;
  }
  @if $editorial-ins-color {
    color: $editorial-ins-color;
  }
  text-decoration: $editorial-ins-decoration;
}

.quarto-delete {
  @if $editorial-del-bg {
    background-color: $editorial-del-bg;
  }
  @if $editorial-del-color {
    color: $editorial-del-color;
  }
  text-decoration: $editorial-del-decoration;
}

.quarto-highlight {
  background-color: $editorial-mark-bg;
  padding: $editorial-mark-padding;
}

.quarto-edit-comment {
  color: $editorial-comment-color;
  font-style: $editorial-comment-style;
  @if $editorial-comment-bg {
    background-color: $editorial-comment-bg;
  }
}
```

### 3. Theme Awareness

The default colors should work well in both light and dark themes:
- Use semi-transparent backgrounds that adapt to the underlying color
- Use `$text-muted` for comment color (already theme-aware)
- Optional: Add dark-mode specific overrides using the existing `$code-block-theme-dark-threshhold` pattern

### 4. User Override Example

Users can override in their custom SCSS:

```scss
/*-- scss:defaults --*/
$editorial-ins-bg: #d4edda;
$editorial-del-bg: #f8d7da;
$editorial-comment-color: #6c757d;

/*-- scss:rules --*/
// Additional custom rules if needed
```

## Work Items

- [x] Add editorial mark variables to `_bootstrap-variables.scss`
- [x] Update/replace the `mark` rule and add `ins`, `del`, `.comment` rules in `_bootstrap-rules.scss`
- [x] Add tests to verify SCSS compiles correctly with the new rules
- [x] Test styling in both light and dark themes (all 25 Bootswatch themes compile)
- [ ] Document the new variables in docs/ (if we have styling documentation)
- [ ] Test in hub-client preview (requires WASM rebuild)

## Testing Strategy

1. **Unit tests**: Verify SCSS compiles without errors
2. **Visual tests**: Create a test document with all editorial mark types and verify rendering
3. **Theme tests**: Test with different Bootswatch themes (light: cosmo, dark: darkly)
4. **Override tests**: Verify user can override default variables

## Files to Modify

1. `resources/scss/bootstrap/_bootstrap-variables.scss` - Add new variables
2. `resources/scss/bootstrap/_bootstrap-rules.scss` - Add/update CSS rules
3. Possibly new test fixtures in `crates/quarto-sass/test-fixtures/`

## Out of Scope

- Separate SCSS file (keeping it simple by adding to existing Quarto layer files)
- Per-document editorial mark customization (can be done via existing custom SCSS support)
