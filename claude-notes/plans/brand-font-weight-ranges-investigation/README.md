# bd-5fseopxy investigation fixtures

Four single-page `type: default` projects, each isolating one surface of the
bug. Render any of them from the repo root with

    cargo run -q --bin q2 -- render claude-notes/plans/brand-font-weight-ranges-investigation/<dir>

and inspect `<dir>/index_files/quarto/quarto-theme-*.css`. Rendered output
(`index.html`, `index_files/`) is gitignored here.

| dir                 | `_brand.yml` range on          | observed at `b7e7c96a`                                   |
| ------------------- | ------------------------------ | -------------------------------------------------------- |
| `repro/`            | google + file + headings slot  | 6996-byte DEFAULT_CSS fallback (file `@font-face` fails first) |
| `repro-google-only/`| `fonts[].weight` (google)      | bundle compiles; `@import` requests `wght@0,400;1,400` only |
| `repro-fonts-only/` | google + file                  | DEFAULT_CSS; grass: `font-weight: 300..800;` expected ";" |
| `repro-slot-only/`  | `headings.weight`              | DEFAULT_CSS; grass: `$headings-font-weight: 500..700 !default;` expected ";" |

`trace: summary` is set in the `_quarto.yml` of the three failing variants so
the swallowed `[trace] [warn] theme CSS compilation failed` line is visible
(bd-jsvetdea explains why nothing shows otherwise).
