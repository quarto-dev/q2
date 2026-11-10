---
title: 'Findings: color-scheme toggle at HEAD (bd-kwgtllvn)'
---

Rendered on `main` @ `8f17e950d` (2026-11-08) with `cargo run --bin q2 -- render`.

## `repro-site/`: website navbar, `theme: {light: cosmo, dark: darkly}`

```html
<div class="quarto-navbar-tools">
  <a href="" class="quarto-color-scheme-toggle quarto-navigation-tool px-1"
     onclick="window.quartoToggleColorScheme(); return false;"
     title="Toggle dark mode"><i class="bi"></i></a>
</div>
```

- Link role on a button-like control, so Space does not activate it.
- The state (light or dark) is conveyed only by the `.alternate` icon
  swap, and there is no `aria-checked`.
- The `title` is hard-coded English. q2 renders it from
  `crates/quarto-navigation/src/render_html.rs` and does not read the
  `toggle-dark-mode` language term.

## `repro-plain/`: a document with no navbar, same theme pair

The page ships the inline `quarto-color-mode` runtime
(`crates/quarto-core/resources/js/quarto-color-mode.js`). Its
`DOMContentLoaded` fallback does this:

```js
toggle = window.document.createElement("a");
toggle.href = "";
// ...
toggle.title = "Toggle dark mode";
// ...
window.document.body.appendChild(toggle);
```

- It has the same link-role and missing-state problems as the navbar
  toggle.
- It is appended straight to `<body>`, outside any landmark. Quarto 1
  now wraps it in a `header`, or in a named `region` when the page
  already has a banner.
- Its label is hard-coded English. The script gets its configuration
  from `data-*` attributes on its own `<script>` tag (`template.rs`,
  `color-mode-script`) and has no label attribute today.

## Already at parity

- The dashboard navbar toggle (`template.rs`, Dashboard Phase 7) is a
  `<button type="button" role="switch" aria-checked="false">` with a
  localized `aria-label`. However, `aria-checked` is never updated,
  because `setColorSchemeToggle()` only toggles `.alternate`. A dark
  dashboard therefore announces the switch as "off". The runtime fix
  covers this case too.
- The SCSS rule `button.quarto-color-scheme-toggle { background:
  transparent; border: none; padding: 0; cursor: pointer }` is already
  ported (`_bootstrap-rules.scss`). Its comment still says that site
  navbars use an `<a>`.
- There is no toggle without a dark variant. The navbar flag and the
  runtime script are both gated on the theme pair.

## Not applicable

- Sidebar `tools:` dark toggle: q2 does not render sidebar tools yet
  (bd-fod3).
- The preview path (hub-client and q2-preview) does not ship the
  color-mode runtime.
