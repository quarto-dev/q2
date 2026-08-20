# Repro: footer/nav config markdown drops Link attributes and unwraps attributed Spans

Strand: bd-footer-link-attrs-dropped-1axx82op.
Minimal in-repo version of the external repro cited in the strand
(`~/repos/github/cscheid/q2-connect-docs/llms-info/repros/footer-link-attrs-dropped/`).

## Run

```bash
cargo run --bin q2 -- render claude-notes/plans/footer-link-attrs-investigation/repro
```

Then inspect `_site/index.html` (generated output; not committed).

## Observed at main @ 87c0e21a (v0.25.0), 2026-08-19

Footer (region-level `page-footer.center` and item-level `right[0].text` alike):

```html
<div class="nav-footer-center"><a href="https://example.com/prefs">cookie prefs</a> and attributed span and <img src="images/logo.svg" alt="logo" class="footer-logo" style="height: 22px;"></div>
...
<li class="nav-item"><a href="https://example.com/item">item link</a></li>
```

- Link `{#open_preferences_center .footer-link title="..."}` → bare `<a href>`; id, class, kv title all gone.
- Attributed span `[attributed span]{#sp .sp-cls}` → unwrapped plain text.
- Image control keeps every attribute (class, style). ✓

Body control in the same page keeps everything:

```html
<a href="https://example.com/prefs" id="open_preferences_center_body" class="footer-link" title="Cookie Preferences">cookie prefs</a> and <span id="spb" class="sp-cls">attributed span</span>
```

## After the fix (branch braid/bd-footer-link-attrs-dropped-1axx82op, 2026-08-20)

```html
<div class="nav-footer-center"><a href="https://example.com/prefs" id="open_preferences_center" class="footer-link" title="Cookie Preferences">cookie prefs</a> and <span id="sp" class="sp-cls">attributed span</span> and <img src="images/logo.svg" alt="logo" class="footer-logo" style="height: 22px;"></div>
...
<li class="nav-item"><a href="https://example.com/item" id="item-id" class="item-cls">item link</a></li>
```

## Root cause

`crates/quarto-navigation/src/render_html.rs`, `push_inline` (the strand calls it
`inline_to_html`; the wrapper is `inlines_to_html`):

- `Inline::Link` arm (~line 1019): emits only `href` + *target* title; never reads `l.attr`.
- `Inline::Span` arm (~line 1033): `// Drop attributes for simplicity; render content.`
- `Inline::Image` arm (~line 1060): full id/class/kv treatment — the working control.

Parity reference: the body writer `crates/pampa/src/writers/html.rs:966-1003`
(Link: href, `write_attr`, target title; Span: real `<span>` + attrs, always).
`rewrite_config_inlines` (`crates/quarto-core/src/transforms/navigation_href.rs:726`)
already recurses into Span content, so the href rewriter is unaffected by the fix.
