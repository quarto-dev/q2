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
