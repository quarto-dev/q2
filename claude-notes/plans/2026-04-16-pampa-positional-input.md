# Pampa: Accept input file as a positional argument

## Overview

Pandoc accepts its input file(s) as positional arguments (e.g. `pandoc input.md`).
Pampa currently requires `-i/--input <FILE>`. To bring the CLI closer to
Pandoc's, accept a single positional input-file argument while keeping `-i`
working for backward compatibility.

## Findings

### Current state

`crates/pampa/src/main.rs:49-50` defines:

```rust
#[arg(short = 'i', long = "input", default_value = "-")]
input: String,
```

The value `-` means "read from stdin". Downstream usage at
`crates/pampa/src/main.rs:184-196` branches on `args.input == "-"` to either
read stdin or open the file.

### Where `-i` is used today

- `crates/pampa/CLAUDE.md` — documents the `-i` flag in the "Binary usage"
  section and in example commands.
- `crates/pampa/tools/pandoc-diff/server.ts:59` — invokes pampa with
  `-i "${tmpFile.name}"`. Would continue to work unchanged.
- Various `claude-notes/` files reference `-i` in example commands.

### Desired semantics (matches Pandoc for the single-file case)

| Invocation                           | Input source           |
| ------------------------------------ | ---------------------- |
| `pampa`                              | stdin                  |
| `pampa file.qmd`                     | `file.qmd` (positional)|
| `pampa -i file.qmd`                  | `file.qmd` (flag, back-compat) |
| `pampa -`                            | stdin (explicit)       |
| `pampa -i - file.qmd`                | error (conflict)       |
| `pampa a.qmd b.qmd`                  | error (unsupported — single-file only for now) |

Rationale for rejecting the conflict case: silently picking one is a footgun.
Pandoc itself accepts multiple positional files and concatenates them; we are
deliberately *not* implementing that here (out of scope — open a follow-up
issue if someone needs it).

## Work Items

- [x] **Test (TDD, first)**: add a CLI test that runs `pampa` with a positional
      input file and asserts it produces the same output as `-i <file>`. Put it
      in `crates/pampa/tests/test_cli_input_arg.rs` using `std::process::Command`
      on the built binary via `env!("CARGO_BIN_EXE_pampa")`.
- [x] **Test**: `pampa -i file.qmd` still works (regression guard for
      back-compat).
- [x] **Test**: passing both `-i` and a positional argument produces a non-zero
      exit and a readable error message mentioning "input".
- [x] **Test**: passing two positional arguments produces a non-zero exit.
- [x] Run the new tests and confirm they fail in the expected way before
      touching `main.rs`. (Three of five fail for the intended reasons; two
      pass already because clap rejects unknown positional args and because
      `-i` already works — these remain valid regression guards.)
- [x] **Implement**: in `crates/pampa/src/main.rs`, added a positional field
      `input_positional: Option<String>` and changed `input` to `Option<String>`
      (dropping the `default_value`). Effective input resolved at the top of
      `main()` with an explicit conflict error (exit code 2) when both are set;
      multiple positionals are naturally rejected by clap (unknown arg).
- [x] Run the new tests and confirm they pass. (5/5 passing.)
- [x] Run `cargo nextest run -p pampa` to catch regressions in the pampa crate.
      (3705 passed, 4 skipped.)
- [x] Run `cargo nextest run --workspace` to catch regressions in downstream
      crates (per CLAUDE.md's monorepo rule). (7268 passed, 197 skipped.)
- [x] Run `cargo fmt` on `main.rs`.
- [x] Update `crates/pampa/CLAUDE.md` "Binary usage" section to document the
      positional form alongside `-i`. Also fixed a stale reference
      (`quarto-markdown-pandoc` → `pampa`) caught while editing.

## Non-goals

- Multi-file concatenation (Pandoc-style `pampa a.qmd b.qmd`). Out of scope.
- Changing the `-o/--output` interface. It already matches Pandoc.
- Touching `tools/pandoc-diff/server.ts` — `-i` keeps working, no need.

## Risks

- **Low.** The change is additive: `-i` semantics are preserved. The only new
  failure mode is the conflict error, which is the intended safeguard.
- Clap will reject an unknown positional by default; making it `Option<String>`
  avoids breaking existing invocations that pass no positional.
