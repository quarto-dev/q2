# Repro for bd-x30aq7ae

`bad-frontmatter.qmd`: the flow sequence on line 3 (`author: [a, b`) is never
closed, so the scanner trips on the `:` of `format: html` (file line 4, column 7).

Captured on `main` @ 3d3360ab6 (debug build), from the fixture's directory:

    q2 render bad-frontmatter.qmd --json-errors 2> q2-render-json-errors.out.json

(The absolute scratch path in `source_file` was stripped.) Observed:

- `"code":"Q-0-99"`
- title contains `(at crates/pampa/src/utils/diagnostic_collector.rs:45)`
- span 2:1–5:1 (the whole YAML body), not 4:7
- the message's own "line 3 column 7" is relative to the YAML body, so it points
  one line too high in file terms. The "byte 35" is really a char index
  (yaml-rust2 `Marker::index`).
