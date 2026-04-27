# Grammar crate scout (task #15)

Scouted 2026-04-19 for wiring into `quarto-highlight`. Versions/commits verified at that date; re-verify if re-vendoring.

| Language | Crate | Version | Language symbol | Repo | Commit SHA | Query path | License |
|---|---|---|---|---|---|---|---|
| Python | `tree-sitter-python` | `0.25.0` | `tree_sitter_python::LANGUAGE` | tree-sitter/tree-sitter-python | `26855eabccb19c6abf499fbc5b8dc7cc9ab8bc64` | `queries/highlights.scm` | MIT |
| R | `tree-sitter-r` | `1.2.0` | `tree_sitter_r::LANGUAGE` | r-lib/tree-sitter-r | `0e6ef7741712c09dc3ee6e81c42e919820cc65ef` | `queries/highlights.scm` | MIT |
| JavaScript | `tree-sitter-javascript` | `0.25.0` | `tree_sitter_javascript::LANGUAGE` | tree-sitter/tree-sitter-javascript | `58404d8cf191d69f2674a8fd507bd5776f46cb11` | `queries/highlights.scm` | MIT |
| TypeScript | `tree-sitter-typescript` | `0.23.2` | `tree_sitter_typescript::LANGUAGE_TYPESCRIPT` **and** `LANGUAGE_TSX` | tree-sitter/tree-sitter-typescript | `75b3874edb2dc714fb1fd77a32013d0f8699989f` | `queries/highlights.scm` (shared between TS + TSX) | MIT |
| Bash | `tree-sitter-bash` | `0.25.1` | `tree_sitter_bash::LANGUAGE` | tree-sitter/tree-sitter-bash | `a06c2e4415e9bc0346c6b86d401879ffb44058f7` | `queries/highlights.scm` | MIT |
| SQL | `tree-sitter-sequel` | `0.3.11` | `tree_sitter_sequel::LANGUAGE` (not `sql`) | DerekStride/tree-sitter-sql | `c2e1e08db1ea20dc23bdb8d228a81a8756e9c450` | `queries/highlights.scm` | MIT |
| HTML | `tree-sitter-html` | `0.23.2` | `tree_sitter_html::LANGUAGE` | tree-sitter/tree-sitter-html | `73a3947324f6efddf9e17c0ea58d454843590cc0` | `queries/highlights.scm` | MIT |
| CSS | `tree-sitter-css` | `0.25.0` | `tree_sitter_css::LANGUAGE` | tree-sitter/tree-sitter-css | `dda5cfc5722c429eaba1c910ca32c2c0c5bb1a3f` | `queries/highlights.scm` | MIT |
| JSON | `tree-sitter-json` | `0.24.8` | `tree_sitter_json::LANGUAGE` | tree-sitter/tree-sitter-json | `001c28d7a29832b06b0e831ec77845553c89b56d` | `queries/highlights.scm` | MIT |
| YAML | `tree-sitter-yaml` | `0.7.2` | `tree_sitter_yaml::LANGUAGE` | tree-sitter-grammars/tree-sitter-yaml | `4463985dfccc640f3d6991e3396a2047610cf5f8` | `queries/highlights.scm` | MIT |
| Julia | `tree-sitter-julia` | `0.23.1` | `tree_sitter_julia::LANGUAGE` | tree-sitter/tree-sitter-julia | `e0f9dcd180fdcfcfa8d79a3531e11d99e79321d3` | `queries/highlights.scm` | MIT |
| Lua | `tree-sitter-lua` | `0.5.0` | `tree_sitter_lua::LANGUAGE` | MunifTanjim/tree-sitter-lua | `4fbec840c34149b7d5fe10097c93a320ee4af053` | `queries/highlights.scm` | MIT |

## Caveats to verify at build time

- `tree-sitter-typescript` crate exposes two separate language constants (`LANGUAGE_TYPESCRIPT`, `LANGUAGE_TSX`). The upstream ships distinct `typescript/queries/highlights.scm` and `tsx/queries/highlights.scm` — not a single shared file.
- SQL crate name is `tree-sitter-sequel`, Rust module name verified by build (scout said `tree_sitter_sequel`, not `sql`).
- Some crates (TypeScript 0.23.x, HTML 0.23.x, Julia 0.23.x, JSON 0.24.x) may internally pin tree-sitter at <0.25. Cargo will error on the resolver if they do; in that case we either upgrade the grammar crate to a 0.25-compatible version if one exists, or accept duplicate tree-sitter versions (not viable — Language types would not be interchangeable).
