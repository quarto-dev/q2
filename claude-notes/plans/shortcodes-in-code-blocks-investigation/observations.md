# Reproduction at HEAD (2026-08-10)

Checkout: `braid/bd-environment-files-372u9qbs-load-environment-files` (contains
main @ 3ef77da8 via PR #486 merge). Pre-flight `cargo xtask verify --skip-hub-build`
passed at this HEAD before the repro.

Invocation (from `repro/`):

```
cargo run --quiet --bin q2 -- render repro.qmd
```

Observed output (`repro.html`, inspected):

```html
<p>Body: vendor is LDAP.</p>                                                <!-- body: substitutes -->
<pre class="ini code-with-copy" data-filename="example.gcfg"><code>[Section &quot;corporate {{&lt; meta vendor &gt;}}&quot;]
```

Body-text shortcode resolves; inside the fence the shortcode stays literal
(HTML-escaped), no warning — exactly as the strand describes. Q1 renders
`[Section "corporate LDAP"]` (verified in the origin repro against Q1 dev).

Generated render output was deleted after inspection; only the fixture is committed.
