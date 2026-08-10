# Repro observations (2026-08-10, HEAD = 0c5d0abe)

Command:

```bash
REPRO_VERSION=2026.08.0 cargo run --bin q2 -- render claude-notes/plans/shortcodes-website-config-investigation/repro
```

Body text substitutes correctly; all four project-level contexts leak the literal shortcode.
From `_site/index.html`:

```
7:<title>Home – My Site <small>Version {{< env REPRO_VERSION >}}</small></title>
17:    <a class="navbar-brand" href="./">My Site &lt;small&gt;Version {{&lt; env REPRO_VERSION &gt;}}&lt;/small&gt;</a>
29:  You are viewing version <strong>{{< env REPRO_VERSION >}}</strong>.
46:<p>Body-text shortcode (works in q2): version is 2026.08.0.</p>
54:    <div class="nav-footer-center">My Product {{&lt; env REPRO_VERSION &gt;}}
```

Notes:

- `<title>` (line 7): shortcode literal AND raw `<small>` markup embedded inside the
  `<title>` element (markup inside `<title>` is not rendered by browsers; Q1 substitutes
  the shortcode — check what Q1 does with the tags themselves when emitting `<title>`).
- navbar brand (line 17): whole title HTML-escaped — both the shortcode and the
  `<small>` tags show as text. Suggests the title string is emitted as plain text,
  never parsed as markdown (Q1 parses it, so raw HTML passes through and shortcodes
  resolve).
- include-before-body (line 29): file contents injected verbatim.
- footer center (line 46): escaped literal — the `|` block scalar goes through some
  path that escapes rather than markdown-parses. (Note `{{&lt;` — escaped — vs. the
  banner's raw `{{<`.)
- No warning emitted in any of the four contexts.

Repro fixture: `repro/` (this directory’s sibling). Derived from the external repro at
`/Users/cscheid/repos/github/cscheid/q2-connect-docs/llms-info/repros/shortcodes-in-metadata-and-includes/`,
plus `website.navbar` (needed to exercise the navbar-brand path, which the external
repro description mentions but its fixture does not trigger).

## Scope correction: doc-metadata shortcodes don't resolve either

Adding `subtitle: "Subtitle version {{< env REPRO_VERSION >}}"` to `index.qmd`
frontmatter and re-rendering with q2 at HEAD:

```
39:<p class="subtitle lead">Subtitle version <span class="quarto-unresolved-shortcode">?env</span></p>
```

`ShortcodeResolveTransform` walks `ast.blocks` only; the `Inline::Shortcode` node in
the parsed subtitle metadata survives unresolved and the writer renders its fallback
marker — even though `REPRO_VERSION` was set. The strand's premise that metadata
shortcodes work in q2 is wrong at HEAD.

## Quarto 1 comparison render (2026-08-10)

Same fixture (paths unquoted with `!path` since that's q2 syntax; subtitle line
included), rendered with the system Q1 dev binary (`quarto --version` → 99.9.9):

```bash
REPRO_VERSION=2026.08.0 quarto render .
```

Output (`grep 'REPRO_VERSION\|2026\.08\.0' _site/index.html`):

```
10:<title>Home – My Site Version 2026.08.0</title>
88:    <span class="navbar-title">My Site <small>Version 2026.08.0</small></span>
116:  You are viewing version <strong>2026.08.0</strong>.
122:<p class="subtitle lead">Subtitle version 2026.08.0</p>
139:<p>Body-text shortcode: version is 2026.08.0.</p>
551:<p>My Product 2026.08.0</p>
```

All five contexts substitute. `<title>` strips the `<small>` tags (Q1 assigns the
rendered envelope element's `innerText` — `website-meta.ts`); the navbar keeps them
as markup.

Include files are substituted but **not** markdown-parsed: appending
`**md-test** \`code-test\`` to `_banner.html` and re-rendering leaves both literal
while the shortcode still substitutes. Mechanism: `quarto-init/includes.lua` reads
include files into metadata as raw blocks; the shortcode filter's jog traversal walks
meta and applies text-level `apply_code_shortcode` to raw-block text.

## q2 text-context probe (2026-08-10)

Q1 also substitutes shortcodes at text level in code blocks, element attributes,
image src, and link targets. q2 probe (`probe.qmd` with a code block, link target,
and span attribute each containing `{{< env REPRO_VERSION >}}`; rendered with
`REPRO_VERSION` set):

```
30:<pre class="code-with-copy"><code>code block {{&lt; env REPRO_VERSION &gt;}}</code></pre>
33:<p><a href="&quot;https://example.com/{{&lt; env REPRO_VERSION &gt;}}/&quot;">link text</a></p>
34:<p>Span attr: <span data-v="{{&lt; env REPRO_VERSION &gt;}}">text</span></p>
```

All literal in q2 → filed as bd-fz6gwfq0.
