# Investigation findings — ellipsis not smart (bd-ellipsis-not-smart-48bv2pe6)

Raw evidence gathered 2026-08-12 at HEAD `7bcddf61`.

## Repro (`repro.qmd`)

```
$ cargo run -q -p pampa --bin pampa -- -t json -i repro.qmd | jq -c '.blocks[].c'
[{"c":"the"},{Space},{"c":"..."},{Space},{"c":"menu"}]     <- wrong
[{"c":"a…b"}]                                              <- correct
[{"c":"see"},{Space},{"c":"(...)"},{Space},{"c":"here"}]   <- wrong
[{"c":"x"},{Space},{"c":"–"},{Space},{"c":"y"}]            <- dash control, correct
```

## CST comparison — the actual mechanism

`the ... menu` (`pampa -v`):

```
pandoc_str (0,0)-(0,3)      "the"
pandoc_space
pandoc_str (0,4)-(0,5)      "."     <- three separate
pandoc_str (0,5)-(0,6)      "."        single-char
pandoc_str (0,6)-(0,7)      "."        nodes
pandoc_space
pandoc_str (0,8)-(0,12)     "menu"
```

`the -- menu`:

```
pandoc_str (0,0)-(0,3)      "the"
pandoc_space
pandoc_str (0,4)-(0,6)      "--"    <- ONE node
pandoc_space
pandoc_str (0,7)-(0,11)     "menu"
```

`apply_smart_typography` is applied **per node**. Each single-dot node is a run
of length 1, which correctly stays literal. `merge_strs` then concatenates them
into `...` *after* conversion. The dash run arrives as one node, so it converts.

Escaped `a\.\.\.b` yields 2-byte nodes `(0,1)-(0,3)` etc. — backslash + dot —
so escaped and unescaped dot runs *are* distinguishable by node content.

## Root cause — `grammar.js:99`

```js
const startStrRegex = regexOr(
    "[" + PANDOC_NON_ASCII_WHITESPACE + PANDOC_ALPHA_NUM + PANDOC_SMART_QUOTES + "-]");
```

`-` is in the token-start class; `.` is not. The continuation class
(`grammar.js:130`) contains both `.` and `-`.

Consequences:

- token starting with `-` → `--` lexes as one node (start `-`, continuation `-`)
- token starting with alnum → `a...b` lexes as one node
- token starting with `.` → falls through to the single-char alternative
  `"[>.,;!?]"` (`grammar.js:127`) → one node per dot

## Confirming probes (`positions.qmd`)

```
$ cargo run -q -p pampa --bin pampa -- -t json -i positions.qmd \
    | jq -c '[.blocks[].c[] | select(.t=="Str") | .c] | join(" ")'
"x -… y   x ... y   x a… y   x ‘… y   x .... y   x .. y"
```

| input      | output | predicted by the startStrRegex theory |
|------------|--------|---------------------------------------|
| `x -... y` | `-…`   | yes — `-` starts a token, dots absorbed |
| `x ‘... y` | `‘…`   | yes — smart quote starts a token        |
| `x a... y` | `a…`   | yes — alnum starts a token              |
| `x ... y`  | `...`  | yes — `.` cannot start a token          |
| `x .... y` | `....` | yes — should be `….` per Pandoc         |
| `x .. y`   | `..`   | correct in both (run of 2 stays literal) |

`x -... y` → `-…` is the decisive one: prefixing a hyphen *repairs* the
ellipsis. This rules out the strand's stated hypothesis (that a dot-leading
token is routed down an attribute-class path that never reaches
`apply_smart_typography`) — the function is reached in every case; it just
never sees more than one dot at a time.

## Existing test coverage (the gap)

`crates/pampa/tests/snapshots/native/smart-typography.qmd`:

```
Mid-word em-dashes---convert and en--dashes too, plus ellipsis...
Escaped a\-\-b and a\-\-\-b and a\.\.\.b stay literal.
Code `a---b` and `x...y` are untouched.
```

Every dot run is word-adjacent (`ellipsis...`).

`crates/pampa/tests/roundtrip_tests/qmd-json-qmd/dashes_spaced.qmd`:

```
spaced — dash, en – dash, and ellipsis… done.
```

This one *looks* like it covers the space-preceded position, but the source
already contains U+2026 — it tests that a converted ellipsis round-trips, not
that `...` converts. Neither fixture exercises the failing positions.
