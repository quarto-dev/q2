# Lua "current path": Q1's mechanism, Q2's divergence, and design avenues (GH #588)

**Strand:** bd-sr0nipl7
**Issue:** https://github.com/quarto-dev/q2/issues/588 (`resolve_path` returns a
different directory inside a loaded file)
**Sibling issue:** https://github.com/quarto-dev/q2/issues/587 (`require`
unavailable in Lua filters) — same subsystem, and any fix here should be
designed with #587's fix in mind.

## The bug, in one paragraph

`quarto.utils.resolve_path("_modules/greet.lua")` called from an extension's
top-level script returns `<ext-root>/_modules/greet.lua` in both engines. The
same call made at load time *inside* a file the script `require`d returns
`<ext-root>/_modules/_modules/greet.lua` in Q2 — the module's own directory
leaked into "current path". Quarto 1 returns the extension root from both call
sites. Nothing errors; the wrong path surfaces later, elsewhere. The `dofile`
path in Q2 is already correct (contract settled in #112); `require` broke it
because `register_scoped_require` (added by #450, commit `6ff4221e`) pushes the
loaded module's directory onto the *same* stack `resolve_path` reads.

## How Quarto 1 represents "current path"

All references are to `external-sources/quarto-cli` at the checkout in this
repo. The machinery lives in `src/resources/pandoc/datadir/init.lua` plus two
filter-infra files.

Q1 has **three distinct layers**, and the key to its behavior is that they are
*separate*:

### 1. Process cwd (ground truth fallback)

Pandoc runs with the OS working directory set for the render.
`resolvePath` (`init.lua:313`) joins a still-relative path onto
`pandoc.system.get_working_directory()` as the last step. This is a real OS
cwd; nothing in Lua mutates it during filter execution.

### 2. `PANDOC_SCRIPT_FILE` (per-state constant, shadowed per extension)

Pandoc sets `PANDOC_SCRIPT_FILE` once per Lua state to the filter script it
loaded (for Q1's single emulated state, that is quarto's own `main.lua`).
Extension scripts are loaded into a sandbox env whose `PANDOC_SCRIPT_FILE` is
the extension script (`wrapped-filter.lua:48`), but `init.lua`'s own machinery
reads the state-global one — it serves only as the *bottom fallback* of
`scriptDir()`.

### 3. The `scriptFile` stack (the actual "current script" notion)

`init.lua:167-186`:

```lua
local scriptFile = {}                     -- stack of file paths
local function scriptDir()
   if #scriptFile > 0 then
      return pandoc.path.directory(scriptFile[#scriptFile])
   else
      return pandoc.path.directory(PANDOC_SCRIPT_FILE)   -- fallback
   end
end
```

`_quarto.withScriptFile(file, callback)` (`init.lua:776`) push/pops around a
callback. **The complete inventory of push sites** — every one is a
*top-level-script boundary*, never a module load:

| Push site | Event |
| --- | --- |
| `wrapped-filter.lua:157` (`makeWrappedLuaFilter`) | extension filter script **load** |
| `ast/runemulation.lua:98` | each **filter pass** whose filter carries a `scriptFile` |
| `quarto-pre/shortcodes-handlers.lua:70` | shortcode script **load** |
| `customnodes/shortcodes.lua:417` | shortcode handler **call** |

`require` and `dofile` **never touch the stack**. That is the invariant #588
asks Q2 to honor: *"current path" = the extension's top-level script
directory, stable across module loads.*

### Consumers of `scriptDir()` in Q1

- `quarto.utils.resolve_path` = `resolvePathExt` (`init.lua:322`):
  relative → `join(scriptDir(), path)`, then cwd-join if somehow still
  relative.
- All `quarto.doc.add_html_dependency`/`attach_to_dependency`/
  `add_format_resource`-style path normalization (`init.lua:898-934`).

### Q1's `require` patch — calling-file resolution WITHOUT stack movement

`init.lua:259-305`. Two cases:

1. **`./`-relative modname** (`require("./sibling")`): resolved against the
   *calling chunk's* file, obtained via `debug.getinfo(2, "S").source` — Lua
   debug introspection, not mutable state. Falls back to `scriptDir()`, then
   cwd. Re-enters `require` with the fully-qualified path so caching keys are
   canonical.
2. **Bare modname** (`require("_modules/greet")`): temporarily appends every
   dir in `scriptDirs()` (the `PANDOC_SCRIPT_FILE` dir plus each stack entry,
   **bottom-up** — outermost first) to `package.path`, calls the original
   `require`, restores `package.path`.

Plus an `absolute_searcher` (`init.lua:212`) so `require("/abs/path/mod")`
works via `dofile`.

Note what this design achieves: the "which file is executing right now"
question is answered *inside `require` itself, at call time, via
introspection* — it never leaks into the shared "current script" state that
`resolve_path` reads. Q1 therefore has **two notions** that Q2 currently
conflates:

- **script root** — mutable stack, moves at script boundaries only;
- **calling file** — derived on demand from `debug.getinfo`, no state.

## How Q2 represents "current path"

One stack per Lua state: `_quarto_script_dir_stack`
(`crates/pampa/src/lua/quarto_api.rs:179-335`), holding *directory strings*
(Q1 holds file paths and derives dirs).

**Pushers:**

| Site | Event | Popped? |
| --- | --- | --- |
| `filter.rs:181` | filter script load (fresh Lua state per filter) | never (state dies with the filter) |
| `shortcode.rs:165` | shortcode script load (shared state, one push per extension script) | **never — leaks** (see below) |
| `shortcode.rs:249/255` | shortcode handler call | yes |
| `quarto_api.rs:265/271` (`register_scoped_require`) | **required module execution** | yes |

The first three mirror Q1's inventory (Q2 has no per-pass filter push because
each filter owns its state; the load-time push is still in effect at call
time — equivalent outcome). The fourth is the divergence: Q1 has no analogue.

**Consumers of `current_script_dir` (stack top):**

| Consumer | Q1 analogue |
| --- | --- |
| `quarto.utils.resolve_path` (`quarto_api.rs:490-511`) | `resolvePathExt` |
| `quarto.doc` dependency path resolution (`quarto_doc.rs:137`) | `init.lua:898-934` |
| WASM `dofile`/`loadfile` relative resolution (`dofile_wasm.rs:39`) | native cwd semantics |
| the scoped `require`'s candidate walk (`quarto_api.rs:222-240`) | Q1 `scriptDirs()` in `package.path` |

The scoped `require` (only installed in the **shortcode** state,
`shortcode.rs:117` — filters lack it entirely, which is #587) resolves a
modname by walking the stack **top-down** (innermost first; Q1 searches
outermost first — a second, minor divergence), trying `<dir>/<name>.lua`,
dot-to-slash, and `<dir>/<name>/init.lua`; executes the module sandboxed
**with the module's own dir pushed**, "so nested requires resolve relative to
the module"; caches by resolved absolute path.

### Why the push exists, and what it actually buys

#450's intent: a loaded module can `require` a sibling **by bare name**
(`require("greet")` from inside `_modules/probe.lua`). Q1 does *not* support
that — in Q1 a nested module must write `require("_modules/greet")`
(root-relative, since `scriptDirs()` only contains script dirs) or
`require("./greet")` (calling-file-relative via `debug.getinfo`). So the push
gives Q2 a *superset* of Q1's require behavior — paid for by breaking the
`resolve_path` invariant, because the "calling file" notion was implemented by
mutating the "script root" state.

### The shortcode load-time push leak (discovered)

`shortcode.rs:165` pushes at script load and never pops, so the shared
shortcode state accumulates one stack entry per loaded extension script.
Masked in practice by the call-time push/pop, but it means "stack bottom"
isn't a meaningful fallback, and any future consumer walking the whole stack
(as `require` does) sees stale dirs from unrelated extensions. Worth fixing
alongside, or at least deciding deliberately. Filed as a discovered strand
from bd-sr0nipl7.

## WASM constraints on any design

This is where Q2 genuinely cannot copy Q1:

1. **No process cwd.** In the browser/VFS world there is no OS working
   directory; "absolute" means a rooted VFS path under `/project/`. Q1's
   final cwd-join fallback has no equivalent; Q2's existing answer (return
   the path unchanged when the stack is empty, treat rooted paths as final —
   `quarto_util::is_rooted`, bd-picv) is the right shape. Any design must be
   expressible purely as explicit state + rooted-path checks.
2. **No `debug` stdlib on WASM.** The restricted lib set is
   `COROUTINE | TABLE | STRING | UTF8 | MATH` (`filter.rs:145`,
   `shortcode.rs:94`). Q1's `debug.getinfo(2).source` trick for
   calling-file-relative requires is unavailable; a "calling file" notion
   must be tracked on the Rust side (which is fine — Q2's `require` is a Rust
   closure and already knows exactly which file it is executing).
3. **All file access goes through `SystemRuntime`** — the scoped require and
   `dofile_wasm` already do this, so candidate probing works identically on
   native and VFS. Any fix stays inside this abstraction.
4. **One shared Lua state for all shortcode extensions** (unlike Q1-pandoc's
   state-per-filter for top-level filters, and unlike Q2 filters). The stack
   discipline is what isolates extensions from each other; sloppy pushes are
   cross-extension contamination, not just wrong paths.

None of these block Q1-compatible *observable* behavior. The stack-of-dirs
design is already the WASM-compatible replacement for "current directory";
the bug is only about *which events* move it.

## Design avenues

### A. Split the two notions into two stacks (recommended)

Keep `_quarto_script_dir_stack` as the exact analogue of Q1's `scriptFile`
stack: pushed **only** at top-level script boundaries (filter load, shortcode
load, shortcode call). Give `require` a private module-dir stack (Lua registry
table or Rust-side state in the closure) that only `require` itself consults:

- `require` push/pops the *module* stack around module execution;
- candidate walk = module stack top-down, then script stack top-down —
  **byte-for-byte the same candidate order as today**, since today's module
  pushes sit on top of the same stack;
- `resolve_path`, `quarto.doc` deps, and WASM `dofile`/`loadfile` read only
  the script stack.

Outcome: all three rows of #588's table return `<ext-root>/_modules/greet.lua`;
`require` behavior is completely unchanged (no risk to #450's shipped corpus);
the #112 `dofile` contract generalizes to `require`. Small, local to
`quarto_api.rs`.

One knowingly-retained superset: bare-name sibling require from a nested
module keeps working (Q1 would error). And one knowingly-retained shadowing
divergence: if `<root>/util.lua` and `<root>/_modules/util.lua` both exist,
a `require("util")` from inside `_modules/probe.lua` loads the `_modules` one
in Q2 (module dir searched first) and the root one in Q1. If we care, search
the script stack first and the module stack as fallback — that maximizes Q1
agreement at the cost of changing current Q2 resolution order. Recommendation:
keep current order (module-first) unless a real extension surfaces the
shadowing case; document the divergence.

### B. Q1-literal: no module-dir state at all

Remove the push entirely; support `./`-relative requires by tracking the
currently-executing chunk on the Rust side (the `require` closure knows the
file it is loading — no `debug` lib needed); bare names search script dirs
only. This is maximal compatibility — including *failing* where Q1 fails
(bare sibling name from a nested module) — but it is strictly less capable
than A, would break any extension that started relying on #450's behavior,
and buys nothing #588 asks for. Not recommended; noted for completeness.

### C. Tag stack entries and skip them in `current_script_dir`

Keep one stack but mark require-pushed entries so `resolve_path` (and the
other non-require consumers) skip them. Observationally identical to A;
strictly messier (every consumer needs to know about the tag, and the
"one stack, two meanings" confusion that caused the bug survives in the data
structure). Mentioned only because it is the smallest diff; A is the smallest
*honest* diff.

### Cross-cutting work, same contract

1. **#587 (require missing in filters):** install `register_scoped_require`
   in `create_filter_environment` (`filter.rs`) — the runtime handle is
   already in scope. Doing #588 first (or together) matters: if #587 lands on
   today's require, filters inherit the same `resolve_path` breakage.
2. **Shortcode load-push leak** (`shortcode.rs:165`): pop after script
   evaluation. Load-time `resolve_path`/`require` still see the loading
   script's dir; call-time is covered by the existing call push. This makes
   the script stack's invariant crisp: *at rest, the stack holds exactly the
   scripts currently executing.*
3. **Search-order divergence** (Q2 innermost-first vs Q1 outermost-first when
   multiple script dirs are on the stack): only observable with nested
   shortcode invocation across extensions with colliding module names.
   Document; don't chase.

## Proposed contract statement

Generalizing the #112 `dofile` decision (recorded in `dofile_wasm.rs:1-14`):

> The script-dir stack moves only at top-level script boundaries: filter
> script load, shortcode script load, and shortcode handler invocation.
> **Loaders — `require`, `dofile`, `loadfile` — never move it.** A loader
> that needs the location of the file it is currently executing tracks that
> privately; it must not publish it through the script-dir stack, because
> `quarto.utils.resolve_path`, `quarto.doc` dependency resolution, and WASM
> `dofile` all define "current path" as *the innermost running script's
> directory*, exactly as Quarto 1's `scriptFile` stack does.

This should land as the header comment of the script-dir-stack section in
`quarto_api.rs` (and `dofile_wasm.rs`'s header can point at it).

## Test plan sketch (TDD, per repo policy)

1. **Unit (fails first):** in `quarto_api.rs` tests — module loaded via the
   scoped require calls `resolve_path("_modules/greet.lua")` at load time;
   assert extension root, not doubled segment. Model on
   `test_script_dir_stack_resolve_path_uses_top`.
2. **The three-row table from #588 as an end-to-end fixture:** a smoke-all
   extension mirroring mcanouil's repro (`top`/`via_require`/`via_dofile`
   emitted into the document); assert all three equal. This pins the contract
   through the real `q2 render` path per the end-to-end verification policy.
3. **`require`-preservation tests:** existing #450 contract corpus must stay
   green (bare sibling require from nested module still resolves).
4. **WASM smoke:** the scoped require is shared native/WASM via
   `SystemRuntime`, and `.claude/rules/wasm.md` requires `wasm_lua.rs`
   coverage when this area moves — add a case exercising require-then-
   resolve_path under the VFS `/project/` prefix.
