# Investigation notes — bd-email-autolink-dropped-2jj38iiv

Date: 2026-08-10, at main @ 46cacc88.

## Reproduction at HEAD

```
$ echo 'Contact <sales@example.com> now.' | cargo run -q --bin pampa -- -t native
Warning: [Q-2-9] HTML element converted to raw HTML
[ Para [Str "Contact", Space, RawInline (Format "html") "<sales@example.com>", Space, Str "now."] ]

$ echo 'Contact <mailto:sales@example.com> now.' | cargo run -q --bin pampa -- -t native
[ Para [Str "Contact", Space, Link ( "" , ["uri"] , [] ) [Str "mailto:sales@example.com"] ("mailto:sales@example.com" , ""), Space, Str "now."] ]
```

Confirms the strand: the bare email form becomes `RawInline html` (invisible in
browsers), the `mailto:` URI form works but keeps the prefix as visible text.

## Pandoc reference behavior (pandoc @ /opt/homebrew/bin/pandoc)

```
$ echo 'Contact <sales@example.com> now.' | pandoc -f markdown -t native
Link ( "" , [ "email" ] , [] ) [ Str "sales@example.com" ] ( "mailto:sales@example.com" , "" )

$ echo 'Contact <sales@example.com> now.' | pandoc -f commonmark -t native
Link ( "" , [] , [] ) [ Str "sales@example.com" ] ( "mailto:sales@example.com" , "" )
```

Q1 uses the `markdown` reader, so Q1 parity = `class="email"`. Note q2 already
adds `class="uri"` to URI autolinks (matching the markdown reader), so adding
`class="email"` is the consistent choice.

## Code spot-check (all paths from the strand still accurate)

- Scanner: `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`,
  `parse_open_angle_brace` (line 1821). The autolink emission at line 1885
  requires `had_url_like_character` (a `:` or `%` seen before `>`, line 1878).
  Bare emails have neither → falls through to `HTML_ELEMENT` (line 1889).
  `could_be_autolink` is falsified only by leading `/` or embedded space/tab.
- pampa: `crates/pampa/src/pandoc/treesitter.rs` line 850 dispatches
  `"autolink"` → `process_uri_autolink` in
  `crates/pampa/src/pandoc/treesitter_utils/uri_autolink.rs`, which emits
  `Link` with class `uri`, target = the literal content, text = the literal
  content. No email awareness anywhere (grep for `email` in
  treesitter_utils comes back empty).
- The raw-HTML fallback (Q-2-9 emission) is the `"html_element"` arm at
  `crates/pampa/src/pandoc/treesitter.rs:1537` (warning built at line 1634).
- Grammar: `_autolink` is an external token aliased to `$.autolink`
  (grammar.js lines 624, 1098). No grammar.js change should be needed —
  the disambiguation is entirely in the external scanner.

## Over-approximation safety argument (scanner side)

Any *real* HTML open tag with attributes contains whitespace → already
disqualified by `could_be_autolink = false`. A no-attribute tag `<name>` cannot
contain `@` (not valid in tag names). So gating AUTOLINK on "saw `@`" only
diverts strings that were never valid HTML elements anyway (e.g. `<foo@@bar>`),
and those can be given precise treatment in Rust.

## CommonMark email autolink production (spec §Autolinks)

```
<[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*>
```

Notable consequences: `<a@b>` is a valid email autolink (single-label domain);
backslash escapes are not allowed inside; no scheme, no whitespace.

## Existing test surface

- Corpus has URI autolink tests (`test/corpus/link.txt`, "autolinks" section)
  but zero email-shaped cases anywhere in `test/corpus/`.
- `bd-ly83qewg` (closed, related area) recently touched the same function —
  its plan `claude-notes/plans/2026-08-07-angle-bracket-inner-whitespace.md`
  is the model for how to change `parse_open_angle_brace` safely
  (corpus tests + pampa integration tests + full rebuild via
  `tree-sitter generate; tree-sitter build`).
