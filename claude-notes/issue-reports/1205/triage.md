# Issue #1205 — `io.popen` in a Lua extension hangs a parallel website render (macOS)

- **GitHub**: https://github.com/quarto-dev/q2/issues/1205
- **Reporter**: @rich-iannone (Richard Iannone), 2026-11-13
- **Triage date**: 2026-11-13
- **Worktree**: `.worktrees/issue-1205` (branch `issue-1205`, based on `main` @ `637508a6b`)
- **Braid strand**: bd-hf93wq9r
- **Scope**: the whole issue. It is a single report: stock `io.popen` deadlocks
  pass-2 workers. Out of scope: Linux behaviour (not reproducible on this
  machine, and it does not change the fix; see Open questions).

## Summary

A Lua shortcode that calls `io.popen` hangs `q2 render` of a 61-page website
forever at 0% CPU. The same command through `pandoc.pipe` renders in about 1 s.
Reproduced 3/3 here. The cause is a **macOS libc deadlock between
`fflush(NULL)` and a concurrent `fclose`/`pclose` on another thread**. Lua's
`l_popen` is `(fflush(NULL), popen(c, m))`, and q2 runs one Lua state per
pass-2 worker thread in a single process. A standalone C program with no Lua
and no q2 reproduces the deadlock. **Real bug.** The fix is to stop calling
`fflush(NULL)` from Lua, which means replacing the stock `io.popen`. A global
lock around `popen`/`pclose` is not enough (see Open questions, Q3).

## Reproduction

`repro.sh <outdir> [pages=60] [calls=10] [popen|pipe]` generates the
reporter's project.

```sh
R=claude-notes/issue-reports/1205
$R/repro.sh /tmp/r-popen 60 10 popen && (cd /tmp/r-popen && q2 render)   # hangs, 0% CPU
$R/repro.sh /tmp/r-pipe  60 10 pipe  && (cd /tmp/r-pipe  && q2 render)   # 61/61 in ~1 s
```

Observed with `q2 0.45.0`, the installed binary, and with a `target/debug/q2`
built from `main` @ `637508a6b` (see Verification commands).

`sample` of the hung process: all 15 `quarto-pass2-N` threads sleep in
`flockfile → __psynch_mutexwait`. 11 of them come from
`_fwalk` (`fflush(NULL)`), and 2 come from `pclose → fclose → flockfile`. So
even `pclose`\'s own `fclose` cannot take the lock of the `FILE` it is closing.
This is consistent with the reporter's debugger observation: the `FILE` being
waited on is already closed (`_flags=0`, `_file=-1`) and its lock is still
held. The `FILE` lock has been left held or corrupted inside libc. This is not
a slow flush.

### Standalone C repros (no Lua, no q2)

- `popen_race.c`: 12 threads loop Lua's `l_popen` sequence
  (`fflush(NULL); popen; read; pclose`).
  - Unmodified: **STALLED 3/3**.
  - With one global mutex around `fflush+popen` and around `pclose`: OK 3/3.
  - Without `fflush(NULL)` (`NO_FFLUSH=1` variant): OK 3/3.
- `fflush_fclose_race.c`: 6 threads loop `fflush(NULL)` while 6 threads loop
  `fopen("/dev/null")+fputs+fclose`, with no `popen` at all: **STALLED 3/3**.

Build both with `cc -O2 -pthread <file>.c -o <name>`.

## Localization

- `crates/pampa/src/lua/process_view.rs:161` `install`, and `:183`
  `guard_stock_spawn`: `io.popen` and `os.execute` are already replaced for the
  life of each state (wrapped to raise inside `with_environment` or
  `with_working_directory`), but the wrapper still calls the stock function.
  This is the natural place to put a replacement.
- `crates/pampa/src/lua/system.rs:107` (`command`) and `:142` (`pipe`): the
  Rust spawn path that does not hang. A replacement `io.popen` would spawn
  through the same `spawn(lua, runtime, …)` and would also honour the
  per-state env and cwd view. The `with_environment` restriction on `io.popen`
  could then be lifted.
- Vendored Lua 5.4 (`mlua` `vendored` feature, `crates/pampa/Cargo.toml:82`)
  defines `l_popen` in `liolib.c` as `(fflush(NULL), popen(c,m))` under
  `LUA_USE_POSIX`.
- In-tree callers of `io.popen`/`os.execute` are only the profiling and
  coverage paths (`resources/pandoc-filters/filters/ast/runemulation.lua`,
  `.../datadir/luacov/runner.lua`), so q2's own filters do not trigger the hang.
  Third-party extensions do.

## Open questions — resolved during triage

1. **Is this q2/mlua-specific, or libc?** It is libc. `popen_race.c` deadlocks
   with no Lua involved.
2. **Is `fflush(NULL)` the necessary ingredient?** Yes, in practice.
   `popen`/`pclose` without `fflush(NULL)` completed 3/3 at 12×300.
3. **Would the reporter's option 2 (serialize `popen`, `pclose` and
   `fflush(NULL)` with one global lock) be enough?** No.
   `fflush_fclose_race.c` shows that `fflush(NULL)` also deadlocks against a
   plain `fclose` on another thread. Every Lua `fclose` path would also need
   the lock: `file:close`, `io.close`, `io.lines` exhaustion, `__gc`/`__close`
   on file handles, and `io.input`/`io.output` replacement. Some of those run
   from the GC at arbitrary points. That is fragile. Removing the
   `fflush(NULL)` call (option 1) removes the only trigger.
4. **Does `os.execute` have the same problem?** Lua's `os_execute` calls
   `system()` directly, with no `fflush(NULL)` in `loslib.c`. It is not
   implicated by this evidence, and the repro uses only `io.popen`.
5. **Linux?** Not tested: no Docker or Linux box is available in this session.
   glibc's `_IO_flush_all` takes `list_all_lock` and unlinks under it in
   `fclose`, so glibc is probably immune. The answer does not change the fix.
   The Rust-spawn replacement is platform-independent and also gives `io.popen`
   the per-state env and cwd, so it is the right fix on every platform.
6. **Can Lua reach libc `exit()`, which runs the same `_fwalk`?** No.
   `os.exit` is replaced in every production filter state
   (`crates/pampa/src/lua/filter.rs:196`) and shortcode state
   (`shortcode.rs:125`) by `os_exit.rs` (GH #941). The replacement raises an
   error and never calls `exit`. The only other non-test Lua states,
   `lib.rs:48` and `:81`, are WASM self-tests that load no `os` library.

## Prior art and mechanism

This is a known class of libc bug, with prior reports on two other libcs and
on hosts that embed Lua.

- **musl, 2018.** A multithreaded server embedding Lua (skynet) deadlocked
  with one thread in `io_popen → fflush(NULL)` and another in
  `io_pclose → pclose → fclose`, both on the same `FILE`
  ([report](https://openwall.com/lists/musl/2018/11/02/1)). Rich Felker
  confirmed it was a libc bug: `fflush(NULL)` took the open-file-list lock
  before each stream lock, and `fclose` took them in the opposite order. He
  also called `fflush(NULL)` valid but "a really bad idea" in threaded code
  ([reply](https://openwall.com/lists/musl/2018/11/02/2)).
- **Apple darwin-dev, 2014.** "Hangs in flockfile() during fread() or
  fclose()" shows the stack we see:
  `_fwalk → sflush_locked → flockfile → __psynch_mutexwait`, reached from
  `fflush(NULL)` ([thread](https://lists.apple.com/archives/darwin-dev/2014/Jul/msg00010.html),
  [mirror](https://groups.google.com/g/darwin-dev/c/e5qPebpfvYM)). According
  to search summaries, Greg Parker (Apple) attributed it to `_fwalk` walking
  the FILE list without a lock, and the poster worked around it by dropping
  `fflush(NULL)`. The archive pages themselves were unreachable during triage
  (HTTP 502 and a bot wall), so that attribution is second-hand.
- **Lua mailing list (lua_cn).** Threads name `l_popen`\'s `fflush(NULL)` as
  the source of crashes and deadlocks in multithreaded hosts
  ([1](https://groups.google.com/g/lua_cn/c/eGxWR92KYT8),
  [2](https://groups.google.com/g/lua_cn/c/ILsAntdlw6s)).

**macOS mechanism, from Apple's published Libc source**
([fwalk.c](https://raw.githubusercontent.com/apple-oss-distributions/Libc/main/stdio/FreeBSD/fwalk.c),
[fflush.c](https://raw.githubusercontent.com/apple-oss-distributions/Libc/main/stdio/FreeBSD/fflush.c),
[fclose.c](https://raw.githubusercontent.com/apple-oss-distributions/Libc/main/stdio/FreeBSD/fclose.c),
[findfp.c](https://raw.githubusercontent.com/apple-oss-distributions/Libc/main/stdio/FreeBSD/findfp.c),
[popen.c](https://raw.githubusercontent.com/apple-oss-distributions/Libc/main/gen/FreeBSD/popen.c)).
This is our reading of the code; Apple has not confirmed it.

- `fflush(NULL)` is `_fwalk(sflush_locked)`. `_fwalk` takes **no lock**: a
  comment says the walk is safe because entries are only appended. It visits
  every slot whose `_flags` is nonzero, and `sflush_locked` calls `FLOCKFILE`
  on that slot.
- `fclose` does `FLOCKFILE … _file = -1 … FUNLOCKFILE` and then
  `__sfprelease`. `__sfprelease` calls
  `pthread_mutex_destroy(&fp->_extra->fl_mutex)` and only afterwards sets
  `_flags = 0` under `FILELIST_LOCK`. `__sfp` later reuses the slot and
  re-initializes its extra data (`INITEXTRA`).
- So a walker can see a closing slot as still open and wait on its mutex.
  The closer then destroys that mutex, which may be re-initialized for a
  different `FILE`. Destroying a mutex that has a waiter is undefined
  behaviour in POSIX, and a waiter that never wakes fits all the evidence:
  the reporter's debugger state (`_flags=0`, `_file=-1`, lock held), our
  `sample` with `pclose → fclose` also stuck in `flockfile`, and
  `fflush_fclose_race.c` hanging with no `popen` at all.
- **`popen`/`pclose` do not walk the FILE list.** `popen` iterates only its
  own `pidlist` under `pidlist_mutex`. `pclose` unlinks its entry under that
  mutex, releases it, and then calls `fclose` and `wait4`. Without
  `fflush(NULL)`, nothing on the `io.popen` path calls `_fwalk`.

**What this means for the fix.** The trigger is `_fwalk`, and the only way
Lua code reaches it is `fflush(NULL)` in `l_popen`, since `os.exit` is
already replaced (Q6). Removing that call removes the hazard. A global lock
is ruled out by mechanism, not only by experiment, because any `fclose` can
be the other side of the race. Option (b) below rests on Apple's source and
not only on the 3/3 `NO_FFLUSH` runs.

## Outcome / recommended next step

Filed bd-hf93wq9r. Fix scope:

- Replace the stock `io.popen` (native only; WASM has none) with an
  implementation that does not call `fflush(NULL)`. Two candidate approaches,
  to be decided in the fix:
  - (a) Spawn through the Rust `spawn` path used by `pandoc.pipe`, and return a
    handle that supports `read` (all formats), `lines`, `write` for `"w"`
    mode, `close` returning `true|nil, "exit"|"signal", code`, and
    `__close`/`__gc`. `io.type(h)` should still return `"file"` if feasible.
    The env/cwd plan notes this as the hard part (`2026-10-31-with-environment-process-env.md:43`).
    Bonus: `io.popen` then respects `with_environment`/`with_working_directory`
    instead of raising.
  - (b) Call libc `popen` from Rust **without** `fflush(NULL)`, and wrap the
    result in a real `luaL_Stream` (via `mlua::ffi`, `LUA_FILEHANDLE`
    metatable, `closef` → `pclose`). This keeps exact stock semantics, including
    `io.type`. It relies on libc `popen`/`pclose` being thread-safe without
    `fflush`. Apple's source supports that: neither walks the FILE list (see
    Prior art and mechanism), and Q2's 3/3 `NO_FFLUSH` runs agree.
- Regression test: an integration test that runs `io.popen` from several Lua
  states on parallel threads, alongside `io.open`/`close`, under a timeout.
  On `main` this should deadlock on macOS.
- Docs: none required. Optionally mention in the extension-authoring docs that
  `pandoc.pipe` is preferred.

## Verification commands used

```sh
gh issue view 1205 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
cargo xtask verify --skip-hub-build        # pre-flight on main @ 637508a6b: all 19971 Rust tests
                                           # passed; 27 hub WASM tests failed on a stale local pkg
                                           # (built Nov 11, older than src); after `npm run build:wasm`,
                                           # `npm run test:wasm` passed 480/480
claude-notes/issue-reports/1205/repro.sh <dir> 60 10 popen|pipe
perl -e 'alarm shift; exec @ARGV' 60 q2 render   # macOS has no `timeout`
sample <pid> 2 -file sample.txt                  # on the hung render
cc -O2 -pthread popen_race.c -o popen_race && ./popen_race 12 300 [lock]
cc -O2 -pthread fflush_fclose_race.c -o ffr && ./ffr 12 20000
braid search popen / "io.open" / "Lua io" / "os.execute"   # no existing strand
```

## Cross-references

- bd-with-environment-process-env-5folaygy, bd-9hzg79zn: per-state process
  view that introduced `guard_stock_spawn`.
- `claude-notes/plans/2026-10-31-with-environment-process-env.md`: notes on
  why `io.popen` was guarded rather than replaced (`LUA_FILEHANDLE`/`io.type`).
- `.claude/rules/wasm.md`: WASM has no `io.popen`; keep the replacement
  `cfg(not(target_arch = "wasm32"))`.
