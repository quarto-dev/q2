---
title: 'Fix Windows test failures: lua system command tests use Unix-only programs'
date: 2026-06-25
---

Strand: bd-c5bdf948
Date: 2026-06-25

## Verdict

**Tests problem, not tools problem.** The `exec_command` runtime is correct and
Pandoc-compatible; the three tests are non-hermetic and assume Unix coreutils on
`PATH`.

## Overview

Three tests in `crates/pampa/src/lua/system.rs` call programs that are not
standalone executables on a stock Windows install:

- `test_command_success` → `echo` (a `cmd`/PowerShell builtin, not a binary)
- `test_command_failure` → `false` (Unix utility)
- `test_command_with_input` → `cat` (Unix utility)

All three fail with `RuntimeError("I/O error: program not found")`.

## Investigation findings (evidence)

The failure is **PATH-dependent**, which is why it looked intermittent:

| Run environment | `echo`/`false`/`cat` on PATH? | Result |
|---|---|---|
| Bash tool (Git Bash) | Yes — `C:\Program Files\Git\usr\bin\{echo,false,cat}.exe` | **PASS** (8/8) |
| PowerShell tool (no Git usr/bin) | No | **FAIL** (3/3 command tests) |

`where.exe echo` → `C:\Program Files\Git\usr\bin\echo.exe`; the same for `false`
and `cat`. Git Bash puts `usr/bin` (MSYS2 coreutils) on PATH, so any process
spawned from a Git Bash shell inherits real `echo.exe`/`cat.exe`/`false.exe`.
PowerShell, `cmd`, and a clean CI runner do **not** — so `Command::new("echo")`
returns "program not found". That is the environment the strand was filed from.

### Why this is NOT a tools bug

`SystemRuntime::exec_command` (`crates/quarto-system-runtime/src/native.rs:216`)
is a direct `Command::new(command).args(args).spawn()` — no shell. This matches
Pandoc's `pandoc.system.command`, which also spawns the program directly with no
shell. On a stock Windows box a Lua filter author calling
`pandoc.system.command('echo', …)` would hit the same "program not found" — that
is correct, Pandoc-compatible behavior. Making the *tool* paper over missing
Unix programs would diverge from Pandoc and is the wrong fix.

### What the tests actually exercise

The `command` binding wiring, not the programs themselves:
- `test_command_success` — exit-code-0 maps to `Value::Boolean(false)`, stdout captured.
- `test_command_failure` — non-zero exit maps to `Value::Integer(code)`.
- `test_command_with_input` — stdin piping + stdout capture round-trips.

Any platform-appropriate program that produces the same observable behavior
satisfies these contracts.

## Scope: 8 tests, not 3 (confirmed)

The same disease lives in a second crate. All confirmed failing from PowerShell
(coreutils-free PATH) with `program not found`:

`crates/pampa/src/lua/system.rs` (Lua binding layer):
- `test_command_success` — `echo` — assert `contains("hello")` *(tolerant)*
- `test_command_failure` — `false` — assert integer exit *(tolerant)*
- `test_command_with_input` — `cat` — assert `contains(...)` *(tolerant)*

`crates/quarto-system-runtime/src/native.rs` (runtime layer):
- `test_exec_command_success` — `echo` — `contains` *(tolerant)*
- `test_exec_command_failure` — `false` — `!success()` *(tolerant)*
- `test_exec_pipe_failure` — `false` — expects `ProcessFailed` *(tolerant)*
- `test_exec_command_with_stdin` — `cat` — **`assert_eq!(stdout, "input data")`** *(strict)*
- `test_exec_pipe_success` — `cat` — **`assert_eq!(output, b"pipe input")`** *(strict)*

The two **strict** stdin tests are the design wrinkle: no stock Windows program
echoes stdin **verbatim** — `findstr`/`sort`/`more` all append `\r\n`. The
runtime *does* pass bytes through unchanged (`exec_command` is byte-clean); the
trailing CRLF is an artifact of the chosen Windows echo program, not the
runtime. So the strict assertions must compare `trim_end()` (or `contains`) on
Windows, otherwise the program's CRLF breaks an otherwise-correct round-trip.

## Verified program choices (Windows)

Tested from a coreutils-free shell — all exit 0, all round-trip the input:

| Concept | Unix | Windows |
|---|---|---|
| print arg, exit 0 | `echo hello` | `cmd /C echo hello` |
| nonzero exit | `false` | `cmd /C exit 1` |
| echo stdin | `cat` | `findstr "^"` (matches every line, preserves order) |

`cmd.exe`, `findstr.exe`, `sort.exe` are all in `C:\Windows\System32` — present
on every Windows install. Chose `findstr "^"` over `sort` (sort reorders
multi-line input; findstr preserves it — safer if a fixture grows).

## Design (option A — cross-platform, keep Windows coverage)

Per-crate, test-only `#[cfg]` helpers (the two crates can't share a helper
without a new test-util dependency; duplication is two tiny functions):

```rust
// returns the program + args for "print hello, exit 0"
#[cfg(not(windows))]
fn echo_cmd() -> (&'static str, &'static [&'static str]) { ("echo", &["hello"]) }
#[cfg(windows)]
fn echo_cmd() -> (&'static str, &'static [&'static str]) { ("cmd", &["/C", "echo", "hello"]) }
```

(pampa's tests build a Lua string, so its helpers return the `command(...)`
snippet instead; same cfg shape.)

For the two **strict** stdin tests, swap the program via the helper AND relax
the assertion to line-ending-tolerant:

```rust
assert_eq!(output.stdout_string().trim_end(), "input data");
```

A one-line doc comment on each helper explains why (Pandoc-compatible
`exec_command` spawns directly with no shell; Unix coreutils are absent on a
stock Windows box; the strict tests trim because the Windows echo program adds
CRLF).

## Phases

- [ ] Phase 1 (TDD): confirm red, then fix
  - [ ] Confirm the 8 tests fail from PowerShell (done — evidence above).
  - [ ] pampa: cfg-helpers for the 3 `command` tests.
  - [ ] native: cfg-helpers for the 5 exec tests; trim-compare the 2 strict ones.
- [ ] Phase 2: verify
  - [ ] All 8 pass from **PowerShell** (coreutils-free).
  - [ ] All 8 still pass from **Git Bash**.
  - [ ] `cargo xtask verify --skip-hub-build`.

## Open question for Chris

**Scope confirmation.** Fix all 8 here (one mechanical change, one root cause),
or keep this strand to pampa's 3 and file the 5 `native.rs` ones as a linked
sibling strand? I lean fix-all-8 — splitting leaves the workspace red on Windows
for no benefit, and the fix is identical in shape.
