# Investigation notes — bd-named-entities-w6xbfftj

Investigated 2026-08-10 at main @ `0cb8abce`. Pre-flight
`cargo xtask verify --skip-hub-build` passed before any changes.

## Reproduction at HEAD

```
$ printf 'A &gt; B &nbsp; C &#62; D\n' | cargo run -q -p pampa --bin pampa -- -t native
[ Para [Str "A", Space, Space, Str "B", Space, Space, Str "C", Space, Str ">", Space, Str "D"] ]
```

- `&gt;` and `&nbsp;` are gone entirely (note the doubled `Space` where each
  entity used to sit).
- Numeric `&#62;` correctly becomes `Str ">"`.

`repro.qmd` in this directory is copied from the strand's external repro
(`~/repos/github/cscheid/q2-connect-docs/llms-info/repros/named-entities-dropped/`),
which carries the Q1-verified expected output.

## Where the node dies

The grammar produces `entity_reference` (regex over
`crates/tree-sitter-qmd/common/html_entities.json`, built in
`common/common.js` `html_entity_regex()`); the pampa inline converter match in
`crates/pampa/src/pandoc/treesitter.rs` has an arm for
`numeric_character_reference` (line ~845) but none for `entity_reference`, so
it hits the default arm (line ~1695), which only writes
`[TOP-LEVEL MISSING NODE] Warning: Unhandled node kind: entity_reference`
to the verbose buffer and returns `IntermediateUnknown` — dropped downstream.
That's why the loss is silent in normal renders.

## Entity table facts (for the fix)

- `html_entities.json` is the WHATWG table: 2,231 keys of the form `"&name;"`
  (and 106 legacy `"&name"` without semicolon), each with `codepoints` and a
  pre-composed `characters` string — multi-codepoint entities
  (`"&NotEqualTilde;"` → `"≂̸"`, U+2242 U+0338) need no special handling if we
  emit `characters` directly.
- Node text is the full `&name;`, which is exactly the JSON key — direct map
  lookup, no trimming needed (for the semicolon-terminated forms the grammar
  matches).
- `tree-sitter-qmd/bindings/rust/lib.rs` already exports `include_str!`
  constants (`NODE_TYPES` etc.) — precedent for exporting the JSON to pampa.

## Discovered grammar bug (filed separately)

`html_entity_regex()` maps every key through
`name.substring(1, name.length - 1)`. For the 106 legacy no-semicolon keys
this strips the final *letter* instead of a `;`: `&AMP` contributes the
alternative `AM`, `&AElig` contributes `AEli`, etc. Consequences:

- Bogus references like `&AM;` / `&AEli;` parse as `entity_reference`
  (converter must fall back to literal text on lookup miss).
- Bare legacy forms (`&AMP` without `;`) do **not** match — which is actually
  correct for markdown (CommonMark only recognizes semicolon-terminated
  entities), so the fix is to *filter* those keys out of the regex, not to
  support them.

Fix belongs in the grammar (filter `Object.keys(...).filter(n => n.endsWith(';'))`),
requires `tree-sitter generate; tree-sitter build` + grammar tests — tracked as
its own strand linked `discovered-from` this one.
