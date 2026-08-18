# Plan 9 research: `quarto call engine` → `q2 call engine` (bd-m1jeqhhz)

**Status:** research complete pending §5 (julia daemon deep-dive), design options in §9.
**Binding requirement (Gordon, 2026-07-03):** the goal is **identical CLI behavior to
Quarto 1**, not an inspired-by design. Q1's *observed* behavior is the specification.
Same subcommand tree, argument shapes, defaults, help-text structure, output
formatting, exit codes, and error messages — modulo only the binary name and version
strings. Anywhere identity is architecturally impossible is a **numbered deviation**
(§8) for explicit ratification, never a silent redesign. Part 2 (optional Rust-engine
trait) is additive plumbing beneath this surface and must not alter the user-facing
contract.

## 0. Sources and provenance

| Source | State | Role |
|---|---|---|
| `external-sources/quarto-cli` | fork `christopherkenny/quarto-cli`, branch `main`, HEAD `e768e5c2d` ("Add Kotlin to language comment-character maps (#14482)"), `version.txt` 1.10.3 | Q1 source of the extension-engine architecture; the tree q2's ts-engine work is modeled on |
| `~/src/quarto-cli` (the live `quarto` binary, reports version 99.9.9) | **same commit `e768e5c2d`** | ground-truth observed behavior — source and binary line up exactly |
| `~/src/quarto-julia-engine` | separate extension checkout (read-only; a prior session owns a branch there) | divergence check vs the in-cli subtree |
| `~/src/quarto-marimo` | branch `q2-bare-sql-interop` (read-only) | "engine without commands" case study |
| `claude-notes/research/2026-07-03-plan9-q1-observed/` | captured 2026-07-03 | **verbatim observed corpus**: all help texts (incl. per-subcommand), error paths with exit codes, live `status`/`log` output against a running daemon — this is the byte-parity acceptance oracle |

Note: this fork moved the julia engine out of `src/execute/julia.ts` into the bundled
extension subtree `src/resources/extension-subtrees/julia-engine/`. Upstream quarto-cli
still has it built in; the *fork* is our reference architecture.

## 1. Observed behavior corpus (the parity spec's ground truth)

Captured in `claude-notes/research/2026-07-03-plan9-q1-observed/` with exit codes. Highlights:

| Invocation | Output (abridged) | Exit |
|---|---|---|
| `quarto call` (bare) | full help for `call` (Commands: help, engine, build-ts-extension, typst-gather) | **1** |
| `quarto call --help` | same help | 0 |
| `quarto call engine --help` | static help; usage `quarto call engine <engine-name> [args...]`; **no engine names listed**; shows inherited `--log/--log-level/--log-format/--quiet` | 0 |
| `quarto call engine` (no name) | cliffy `ERROR: Missing argument(s): engine-name` **plus a Deno stack trace** (red ANSI) | 1 |
| `quarto call engine nonexistent foo` | `Unknown engine: nonexistent` ⏎ `Available engines: knitr, jupyter, markdown, julia` | 1 |
| `quarto call engine markdown` / `jupyter` / `knitr` | `Engine <name> does not support subcommands` | 1 |
| `quarto call engine julia` (bare) | **nothing at all** (silent no-op) | **0** |
| `quarto call engine julia --help` | ANSI-colored help, usage line literally `Usage: COMMAND`; Commands: status, kill, log, close `<file>`, stop | 0 |
| `quarto call engine julia frobnicate` | same help + `error: Unknown command "frobnicate". Did you mean command "close"?` | **2** |
| `quarto call engine julia close --help` | shows `-f, --force` `(Default: false)`; usage `COMMAND close <file>` | 0 |
| `quarto call engine julia status` (live daemon) | `QuartoNotebookRunner server status:` block — started-at + relative age, runner version, environment dir, pid, port, julia version, timeout, `workers active: N`, per-worker path/state/run started/run finished/timeout/pid/exe/exeflags/env | 0 |
| `quarto call engine julia log` | raw dump of the julia server log file (ANSI passes through) | 0 |

Explanations for the oddities (from source, §2–3):

- **`Usage: COMMAND`** and ANSI colors in the julia help: the dispatcher builds a fresh,
  *unnamed* cliffy `Command` for the engine to populate; it never gets `.name()` and
  never gets the root's `.help({colors:false})` treatment, so cliffy's default name
  placeholder `COMMAND` and default colorized help renderer show through.
- **Silent no-op, exit 0** for `call engine julia` bare: the temp command has no
  `.action()` and no default subcommand; cliffy rc.3 `parse([])` just returns.
- **Stack trace** on missing `engine-name`: root command uses `.throwErrors()`; the
  `CommandError` is caught in `quarto.ts` and logged via `logError(e, false)`, which
  includes the stack.
- **Exit 2** for unknown julia subcommand: cliffy's default unknown-command handler on
  the parentless temp command (contrast exit 1 everywhere else on this surface).

## 2. Q1 command tree and dispatcher (spec)

### 2.1 Tree

```
quarto call                       src/command/call/cmd.ts (17 lines)
├── engine <engine-name> [args...]      src/command/call/engine-cmd.ts (50 lines)
│   └── <dynamic; engine-contributed via populateCommand>
│       └── julia: status | kill | log | close <file> [-f/--force] | stop
├── build-ts-extension [entry-point]    (out of Plan 9 scope; q2 already has build-ts-extension)
└── typst-gather                        (out of Plan 9 scope)
```

`call` bare → `showHelp(); Deno.exit(1)`. The `dev-call` sibling command is hidden and
out of scope. Top-level command wrapping (`appendLogOptions`, `appendProfileArg`) is
what puts `--log`/`--log-level`/`--log-format`/`--quiet`/`--profile` on `call` and
`call engine` — these are NOT re-applied to the engine's temp command.

### 2.2 Dispatcher — `src/command/call/engine-cmd.ts`, exact flow

```ts
export const engineCommand = new Command()
  .name("engine")
  .description(`Access functionality specific to quarto's different rendering engines.`)
  .stopEarly()
  .arguments("<engine-name:string> [args...:string]")
  .action(async (options, engineName: string, ...args: string[]) => {
    await initializeProjectContextAndEngines();
    const engine = executionEngine(engineName);
    if (!engine) {
      console.error(`Unknown engine: ${engineName}`);
      console.error(`Available engines: ${executionEngines().map((e) => e.name).join(", ")}`);
      Deno.exit(1);
    }
    if (!engine.populateCommand) {
      console.error(`Engine ${engineName} does not support subcommands`);
      Deno.exit(1);
    }
    const engineSubcommand = new Command()
      .description(`Access functionality specific to the ${engineName} rendering engine.`);
    engine.populateCommand(engineSubcommand);
    await engineSubcommand.parse(args);
  });
```

Mechanics that the parity port must reproduce:

1. **`.stopEarly()`** — everything after `<engine-name>`, including `--flags`, passes
   raw into the second-stage parse. `quarto call engine julia close nb.qmd --force`
   reaches the engine's own parser untouched.
2. **Lazy engine resolution** inside the action: `initializeProjectContextAndEngines()`
   registers builtins (knitr, jupyter, markdown — Map insertion order) plus
   project-config and bundled-extension engines (julia comes from the bundled subtree
   via `zeroFileProjectContext` → `resolveEngineExtensions`, so it works from **any
   directory**, no project needed). The `Available engines:` list is exactly this
   registration order: `knitr, jupyter, markdown, julia`.
3. **Two error gates**, in order: unknown name (exit 1), then no `populateCommand`
   (exit 1). Engines without commands ARE listed in `Available engines:`.
4. **Fresh unnamed temp `Command`** with only a description; engine mutates it;
   `parse(args)` re-enters the CLI parser as a fresh universe (no inherited options,
   no parent, default error handling → exit 2 + help on unknown subcommand).
5. There is **no** "list engines with commands" path and engine subcommands never
   appear in any static `--help`.

History note: the original 2025-03 implementation built the tree eagerly (engines DID
show in `--help`); the current lazy shape came from a circular-dependency fix
(`a8d9e76c1`). The silent no-op and `Usage: COMMAND` warts date from that refactor.

## 3. The populateCommand contract (spec)

`src/execute/types.ts:70`, on `ExecutionEngineDiscovery` (the *static* discovery
interface — no project/file context needed):

```ts
populateCommand?: (command: Command) => void;   // optional; mutates in place
```

The identical declaration is **already vendored in q2** at
`ts-packages/quarto-types/src/execution-engine.ts:198`, alongside a minimal
cliffy-shaped `Command` builder interface in `ts-packages/quarto-types/src/cli.ts`:
`command(name, desc?) / description(desc) / action(fn) / arguments(args) /
option(flags, desc, opts?)` — all returning `Command`. It is currently never called by
the q2 host and has no wire verb.

Cliffy chaining subtlety the builder interface must honor: `.command(name, desc)`
returns the **newly created subcommand**; subsequent `.description()/.arguments()/
.option()/.action()` apply to that subcommand; the next `.command()` attaches a
*sibling* (cliffy re-roots to the parent chain).

## 4. Engine survey: who implements populateCommand

Exactly **one** engine in the surveyed universe: **julia** (the bundled extension
subtree; `populateCommand` at `julia-engine.ts:113`, impl `populateJuliaEngineCommand`
at lines 976–1010). The registered subcommands, verbatim registrations:

| Subcommand | cliffy registration | Args/options | Action |
|---|---|---|---|
| `status` | `.command("status", "Status")` + `.description("Get status information on the currently running Julia server process.")` | — | `logStatus` |
| `kill` | `.command("kill", "Kill server")` + `.description("Kill the control server if it is currently running. This will also kill all notebook worker processes.")` | — | `killJuliaServer` |
| `log` | `.command("log", "Print julia server log")` + `.description("Print the content of the julia server log file if it exists which can be used to diagnose problems.")` | — | `printJuliaServerLog` |
| `close` | `.command("close", "Close the worker for a given notebook. If it is currently running, it will not be interrupted.")` | `<file:string>`; `-f, --force` "Force closing. This will terminate the worker if it is running." (default false) | `closeWorker(file, force)` |
| `stop` | `.command("stop", "Stop the server")` + `.description("Send a message to the server that it should close all notebooks and exit. This will fail if any notebooks are not idle.")` | — | `stopServer` |

- **knitr, jupyter, markdown**: the field is entirely absent (no noop impls exist).
- **marimo** (`~/src/quarto-marimo`): no populateCommand, no CLI contribution; its
  execution is one-shot subprocess (`uv run --with marimo extract.py`), no daemon —
  the canonical "engine has no commands" case. Packaged via `contributes: engines:`
  in `_extension.yml` like julia.
- Engine-module load validation checks `name`/`launch`/`claimsLanguage` only —
  `populateCommand` is never validated.

## 5. Julia daemon machinery per subcommand

Verified: **the entire populateCommand surface — the five registrations, all five
handlers, `connectAndWriteJuliaCommandToRunningServer`, transport-file reading, the
HMAC protocol, message shapes, and `quartonotebookrunner.jl` — is byte-identical
between the in-cli subtree (A) and `~/src/quarto-julia-engine` (B).** All A/B
divergence is confined to the render/oneShot-close path (B's `worker-close.ts`
busy-recovery work on branch `q2-close-busy-fix`, plus keep-ipynb and detached-stdio
tweaks) — irrelevant to Plan 9's CLI surface, though B's busy semantics document what
the daemon does on `close` of a running worker.

### 5.1 Discovery: runtime dir + transport file

- Runtime dir: `quarto.path.runtime("julia")` → platform table: macOS
  `$HOME/Library/Caches/quarto/julia`; Windows `%LOCALAPPDATA%\quarto\julia`; Linux
  `$XDG_RUNTIME_DIR/julia` if set (note: no `quarto` segment in that case), else
  `$XDG_DATA_HOME/quarto/julia` (default `~/.local/share/quarto/julia`). Created on
  demand; on failure, a 4-line error chain ending with the GitHub issue URL
  `https://github.com/quarto-dev/quarto-cli/issues/4594#issuecomment-1619177667`.
- `julia_transport.txt`: single JSON line + `\n`, written by the server:
  `{"port": N, "pid": N, "key": "..."}`. The TS interface declares
  `juliaVersion`/`environment`/`runnerVersion` too but **they are never written nor
  read** — dead fields; only `port`/`pid`/`key` are real.
- Read with a completeness retry: up to 20 re-reads at 100 ms until the content ends
  with `\n`, else throws the raw string `Read invalid transport file that did not end
  with a newline`.
- `julia_server_log.txt` in the same dir. **One global daemon per user** (no project
  component in the path). The server's Julia `atexit` removes the transport file;
  stale files after a crash are expected, and no CLI subcommand ever deletes one
  (only the render path does, on reused-connection handshake failure).

### 5.2 Wire protocol to the daemon

- TCP to `127.0.0.1:<port>`, newline-delimited JSON.
- Request line: `{"hmac": "<base64(HMAC-SHA256(key=transport.key, msg=payload))>",
  "payload": "<JSON.stringify(command)>"}` + `\n` — payload double-encoded as a
  string field. Responses are plain JSON (not HMAC-wrapped), one per line.
- Handshake: send `{"type":"isready","content":{}}`, expect literal `true`, raced
  against a **10 000 ms** timeout. String return = failure (and note a Q1 bug: the
  "expected isready to return true" message interpolates the Promise object,
  rendering `[object Promise]`). **No timeout after the handshake** — status/close/
  stop wait indefinitely.
- Command/response map (CLI-relevant subset):
  `status → string` (server-formatted report), `close → {status:true}`,
  `forceclose → {status:true}`, `stop → {message:"Server stopped."}`,
  `isready → true`. (`run` and `isopen` exist but are render-path only.)
- Server error shape: `{error: string, juliaError?: string}` → thrown as
  `Julia server returned error after receiving "<type>" command:\n\n<error>` +
  optionally `\n\nThe underlying Julia error was:\n\n<juliaError>`.

### 5.3 Per-subcommand semantics

| Subcommand | Steps | No-server behavior | Output sink |
|---|---|---|---|
| `status` | transport file → handshake → `{"type":"status"}` → write returned string **raw to stdout** (no added newline) → close conn | info `Julia control server is not running.`, **exit 0**; handshake fail → info `Found transport file but can't connect to control server.`, **still exit 0** | stdout (raw) |
| `kill` | transport file → `Deno.kill(pid, "SIGTERM")` → info `Sent SIGTERM to server process` — **no socket, no transport-file cleanup** (server atexit handles it) | info `Julia control server is not running.`, exit 0 | logger |
| `log` | dump raw bytes of `julia_server_log.txt` to stdout (ANSI passes through) | info `Server log file doesn't exist`, exit 0 | stdout (raw) |
| `close <file>` | absolutize path (cwd-join + lexical normalize, **no symlink/case canonicalization** — worker identity is that exact string) → `{"type":"close"/"forceclose","content":{"file":<abs>}}` → info `Worker closed successfully.` / `Worker force-closed successfully.` | **throws** `Julia control server is not running.` (nonzero exit) — asymmetric vs status/kill; handshake fail also **throws** | logger |
| `stop` | `{"type":"stop","content":{}}` → info `<result.message>` (`Server stopped.`) | same throw paths as `close` | logger |

Semantics notes: plain `close` of a *running* worker fails server-side with a "worker
is busy" error (surfaces via the thrown server-error format); `forceclose` terminates
it; `stop` fails unless all workers are idle; `kill` is the out-of-band SIGTERM
hammer (workers are children of the control server and die with it). Server idle
timeout is 300 s (`QuartoNotebookRunner.serve(; timeout = 300)`).

The **no-server asymmetry** (`status`/`kill` exit 0 with info; `close`/`stop` throw)
is observed Q1 behavior and part of the parity spec.

### 5.4 Daemon startup (render-path only — context, not Plan 9 surface)

No CLI subcommand ever starts the daemon. Startup happens in
`startOrReuseJuliaServer` during render: transport file exists → reuse; else spawn
detached (`start_quartonotebookrunner_detached.jl` indirection on Unix; PowerShell
`Start-Process -WindowStyle Hidden` on Windows), julia binary from
`QUARTO_JULIA` env (default `julia`), project from `QUARTO_JULIA_PROJECT` or the
runtime-dir environment maintained by `ensure_environment.jl`.

## 6. q2 substrate (what the port builds on)

### 6.1 CLI seam — already Q1-shaped

`crates/quarto/src/main.rs:415–423`:

```rust
/// Access functions of Quarto subsystems such as its rendering engines
Call {
    function: Option<String>,
    #[arg(trailing_var_arg = true)]
    args: Vec<String>,
},
```

dispatching to `crates/quarto/src/commands/call/mod.rs` (string dispatch; currently
`Some("test")` only, with hand-written usage text). Adding `Some("engine")` receiving
`args` verbatim reproduces Q1's `.stopEarly()` pass-through exactly. Binary is `q2`
(`#[command(name = "q2")]`, version string `q2 (quarto 2) <ver>`).

### 6.2 Wire protocol

`crates/quarto-core/src/engine/ts_protocol.rs`: internally-tagged (`type`) serde enums
`ToEngine` (11 variants: init, loadEngine, launchEngine, shutdown, claimsLanguage,
claimsFile, markdownForFile, execute, intermediateFiles, dependencies, cancel) and
`FromEngine` (10 variants) inside a `{ id, msg }` correlation envelope. Adding a verb
= enum variant + serde rename + wire-shape test, mirror in
`ts-packages/quarto-engine-host-deno/src/types.ts`, `case` in `host.ts` `dispatch`,
rebuild bundle (`cargo xtask build-engine-host-bundle`), re-embed. No version
handshake; the bundle is embedded content-hashed so Rust and harness ship in lockstep
(third-party engines are the compat surface, `quartoRequired` declared but unenforced).

Relevant dispatch facts: `loadEngine` is the cheap discovery tier (populateCommand
lives on the *discovery* interface, so a call-engine surface needs only LoadEngine,
not LaunchEngine); `Init { global: HostGlobalConfig }` carries
`is_interactive_session`/`running_in_ci` (the julia daemon-default inputs).

### 6.3 ExecutionEngine trait

`crates/quarto-core/src/engine/traits.rs`: required `name()` + `execute()`; three
optionality precedents — default no-op impls (`claims_*`, `shutdown`, `is_alive`),
Option-returning capability (`quarto_required()`), and default
`Err(ExecutionError::NotSupported)` (`markdown_for_file`). Established migration rule
(plan1a-engine): **q2-native types only on the trait**; `Ts*` DTOs stay confined to
`ts_engine.rs`/`ts_protocol.rs`; TsEngine overrides trait methods by translating at
that seam.

### 6.4 Engine registration/lookup

`EngineRegistry` (`registry.rs`): builtins markdown/knitr/jupyter + TS engines from
`_extensions/` contributions (`EngineContribution::External` with static claims;
bundle-missing error points at `q2 build-ts-extension`). Reached via
`ProjectContext::discover`/`single_file` → `build_engine_registry` (private to
`project::mod`). Host spawn is lazy — registry construction alone spawns no Deno.
Alias map handles unnamed extensions whose runtime name is only known post-LoadEngine.
`engines_in_order()`: contribution order → builtins `markdown, knitr, jupyter` → rest
alphabetical. **Note the order difference vs Q1's `knitr, jupyter, markdown, julia`
for the `Available engines:` message — see deviation D-? (§8).**

### 6.5 Native engines' daemon-like state (Part 2 substrate)

- **jupyter**: real in-process daemon — `JupyterDaemon` global singleton
  (`daemon.rs`), sessions keyed `(kernel_name, working_dir)`, ZeroMQ + connection file
  under the runtimelib runtime dir, idle timeout 300 s, **`kill_on_drop(true)`** —
  kernels die with the q2 process. Nothing persists across q2 invocations, so a
  `q2 call engine jupyter status` would have nothing durable to report today.
- **knitr**: zero persistent state; one-shot `Rscript` per execute.
- **markdown**: passthrough.
- The only *detached* daemon in the whole system is julia's QNR control server,
  spawned by the TS engine's own JS inside Deno, discovered via its transport file.

### 6.6 Established Q1→q2 migration pattern (plan1a/1b/1c)

protocol (typed serde verbs + wire tests) → host (dispatch case, normalize Q1's loose
returns at the boundary) → engine (q2-native trait method with safe default; TsEngine
override) → integration (real caller + E2E through the real binary). Cardinal rule:
**no wire/trait surface without a real q2 caller** ("adding q2-native equivalents
without a real second implementer would calcify the design prematurely").

## 7. Q1-parity spec summary (normative)

The port must reproduce, byte-for-byte where feasible:

1. `q2 call` bare → help + exit 1; `q2 call engine --help` → static help, no engine
   names.
2. `q2 call engine <name> [args...]`: raw pass-through of everything after `<name>`.
3. Unknown engine → `Unknown engine: <name>` ⏎ `Available engines: <list>`, exit 1.
4. Engine without commands → `Engine <name> does not support subcommands`, exit 1.
5. Engine WITH commands: julia's five subcommands with the exact names, descriptions,
   `<file>` argument, `-f/--force` default-false option, help layout, and output
   formats captured in §1 / `claude-notes/research/2026-07-03-plan9-q1-observed/`.
6. Bare `q2 call engine julia` → silent no-op, exit 0 (Q1 quirk — candidate D-3).
7. Unknown julia subcommand → help + `error: Unknown command "x". Did you mean
   command "y"?`, exit 2.
8. `status`/`log`/`kill`/`close`/`stop` output strings per §5 verbatim.

## 8. Numbered deviation candidates (for Gordon to ratify)

> Default per the binding clarification is **replicate Q1 exactly**, including warts.
> Each row is only a *candidate*: "replicate" is always the zero-decision option.

| # | Q1 behavior | Faithful replication? | Candidate deviation |
|---|---|---|---|
| D-1 | `Usage: COMMAND` (unnamed temp command) in engine help | trivially replicable | name the synthetic command (`q2 call engine julia`) — cosmetic improvement, breaks byte parity |
| D-2 | Missing `engine-name` → cliffy ERROR **with Deno stack trace** | stack trace is Deno-specific; a Rust port cannot produce *that* trace | emit `ERROR: Missing argument(s): engine-name` without the stack (partial parity — first line identical) |
| D-3 | Bare `call engine julia` → silent success (exit 0) | trivially replicable | show engine help + exit 1 (what the pre-refactor Q1 did) |
| D-4 | ANSI-colored help for engine subcommands vs plain help elsewhere | replicable | uniform plain help |
| D-5 | Unknown subcommand exit code **2** (vs 1 everywhere else) | replicable | normalize to 1 |
| D-6 | `Available engines: knitr, jupyter, markdown, julia` (Q1 registration order) | q2's registry order differs (`markdown, knitr, jupyter`, TS engines by contribution order) | either hard-match Q1's builtin order in this message, or accept q2's order |
| D-7 | Q1 loads engines from bundled extension subtrees, so `julia` exists from any cwd | q2 has no bundled julia engine — TS engines come from the project's `_extensions/` | `q2 call engine julia` only works where the julia engine extension is installed; message when absent = `Unknown engine: julia` + q2's available list |
| D-8 | `--log`/`--log-level`/`--log-format`/`--quiet`/`--profile` inherited on `call engine` | q2's global flag set differs (`-v` verbosity) | document; do not fake Q1's logging flags |
| D-9 | `quarto call` help lists `help, engine, build-ts-extension, typst-gather`; bare → help + exit 1 | q2's `call` already exists with its own `test` function and hand-written usage; `typst-gather` doesn't exist in q2 | Plan 9 owns only the `engine` arm; `q2 call`-level help stays q2-flavored (Q1 text adopted where q2 has the same subcommand) |
| D-10 | D-2's stack trace aside, Q1 errors print via Deno/cliffy machinery (e.g. red ANSI `error:` prefix on unknown subcommand) | replicable under Option 1 (§9.1) since cliffy itself renders them | — |

## 9. Design options

### 9.0 The constraint that shapes everything

Q1's engine CLI actions produce **raw terminal output**: `status` and `log` write raw
bytes to stdout (`Deno.stdout.writeSync`), cliffy renders ANSI-colored help and
"Did you mean" errors, and exit codes are 0/1/2. But q2's shared `TsEngineHost`
subprocess uses **stdout as the JSONL protocol channel** — an engine action writing
raw bytes there corrupts framing. Any design routing actions through the existing
host must intercept stdout host-side and relay captured output over the wire, plus
re-emulate cliffy's rendering in Rust. That is the main axis separating the options.

### 9.1 Option 1 — one-shot "call mode" in the engine host, real cliffy vendored (RECOMMENDED)

`q2 call engine <name> [args...]` spawns a **dedicated, short-lived Deno process**
running the same embedded engine-host bundle in a new entry mode
(`engine-host.js call ...`), with **stdout/stderr inherited** from q2 and the exit
code propagated. The call mode replicates Q1's `engine-cmd.ts` dispatcher verbatim in
TS: import the engine module, check `populateCommand` (absent → print
`Engine <name> does not support subcommands`, exit 1), build a real **cliffy
v1.0.0-rc.3 `Command`** (vendored into the bundle — MIT; TS structural typing means
the engine's `populateCommand(command)` accepts it directly) with the description
`Access functionality specific to the <name> rendering engine.`, call
`populateCommand`, then `parse(args)`.

Rust side: `commands/call/mod.rs` gains a `Some("engine")` arm → project/registry
discovery (no Deno spawn; registry construction is static) → gate 1 Rust-side
(`Unknown engine: <name>` + `Available engines: <list>`, exit 1) → dispatch through
the Part-2 trait hook (§9.4). `TsEngine`'s override spawns the call-mode process,
passing the engine bundle path, engine name, `HostGlobalConfig` (the actions need
`quarto.path.runtime("julia")`), and the raw args.

- **Parity:** byte-identical by construction — the *same engine code* runs under the
  *same CLI library* Q1 uses, including the warts (`Usage: COMMAND`, ANSI colors,
  exit 2 + did-you-mean, silent bare no-op). Zero cliffy emulation in Rust.
- **No new wire verbs**; the ToEngine/FromEngine protocol is untouched.
- Costs: a second host entry mode (a new lifecycle to test); cliffy vendored into the
  bundle (size; esbuild-from-deno-URL feasibility must be verified early — fallback
  is a minimal builder/renderer byte-matched to the observed corpus in §1).
- Bonus: no daemon or render machinery involved; `status` with no daemon is a
  CI-safe e2e test.

### 9.2 Option 2 — descriptor + invoke wire verbs over the shared host

Add `ToEngine::DescribeCommands { engine }` (host runs `populateCommand` against a
*collector* implementing the minimal builder, returns a serializable command-tree
descriptor) and `ToEngine::InvokeCommand { engine, path, options, positionals }`
(host runs the registered action). Rust parses args against the descriptor, renders
help, and prints relayed output.

- Pros: single host lifecycle; everything on the existing protocol; the descriptor
  doubles as a native-engine command model (deep Part-2 unification).
- Cons: **high parity risk** — Rust must re-emulate cliffy's help layout, colors,
  did-you-mean (Levenshtein), and exit-code map; the host must monkey-patch
  `Deno.stdout`/`quarto.console` to capture action output (fragile, wrong for raw
  byte dumps and interleaving); two new wire verb pairs; violates the epic's
  "no wire surface without a real second caller" instinct (the descriptor model has
  exactly one consumer).

### 9.3 Option 3 — native Rust implementation of the julia commands

Skip `populateCommand` entirely; q2 implements `status/kill/log/close/stop` in Rust
speaking the QNR transport-file + HMAC protocol directly (fully specced in §5).

- Pros: no Deno required for daemon management; gives q2 a native daemon-management
  library it may eventually want anyway (render teardown, `q2 check`).
- Cons: **breaks the extension contract** — a third-party TS engine's
  `populateCommand` would be ignored; hardcodes julia's command set in q2 and drifts
  when the julia engine updates; parity is per-string hand-maintenance. It is
  behavior parity for julia-today but not mechanism parity — the wrong trade under
  the binding clarification.

### 9.4 Part 2 — the Rust-engine trait hook (common to all options)

Q1 parity fact: **no built-in engine has commands** — knitr/jupyter/markdown must
print `Engine <name> does not support subcommands` (exit 1). The epic's calcification
rule ("no trait surface without a real second implementer") argues for the minimal
hook, not a descriptor framework:

```rust
/// Q1 populateCommand equivalent. Default: the engine offers no subcommands,
/// and `q2 call engine <name> ...` reports "does not support subcommands".
fn run_cli_command(
    &self,
    args: &[String],
    ctx: &EngineCallContext,   // runtime dirs / global config the actions need
) -> Result<EngineCallOutcome, ExecutionError> {
    Err(ExecutionError::not_supported("run_cli_command"))
}
```

(`EngineCallOutcome` carries the child's exit code so q2 can propagate it exactly.)
`TsEngine` is the sole override (spawns call-mode under Option 1). Native engines
keep the default, so the CLI layer prints Q1's exact message. A future native engine
with real commands implements the method and owns its own parsing/help — acceptable
because Q1 has no native-with-commands precedent to be parity-bound to. Jupyter's
in-process daemon (`kill_on_drop`, dies with q2) has nothing durable to manage
cross-process today, so wiring commands for it now would be invented surface.

### 9.5 Recommendation

**Option 1 + the minimal trait hook (§9.4).** It is the only option where byte-level
parity is a *structural consequence* (same engine code, same CLI library, real
stdout) rather than an emulation target, it adds zero wire surface, and it respects
both the extension contract and the epic's minimal-surface rule. Verify
cliffy-vendoring feasibility as the plan's first spike; the observed corpus (§1) is
the acceptance oracle either way.

**Approved by Gordon 2026-07-03**, with the naming family `call_engine_command` /
`CallEngineOutcome` / host entry mode `call-engine` ("call-engine" as the feature's
compound noun, mirroring the CLI path).

## 10. Implementation research (2026-07-03, post-approval)

### 10.1 Cliffy vendoring spike — SUCCEEDED

- `deno info` on `cliffy@v1.0.0-rc.3/command/mod.ts`: 66-module graph, 285 KB source,
  the only non-cliffy deps are 6 files from `std@0.196.0` (`fmt/colors.ts`,
  `console/unicode_width.ts` + `_data.json` + `_rle.ts`, 2 `assert` files).
- Mirrored the graph into a local `vendor/` tree preserving paths. Exactly **three
  mechanical patches** needed: rewrite absolute std URLs → relative paths in
  `command/deps.ts` and `table/deps.ts`; change `assert { type: "json" }` →
  `with { type: "json" }` in std's `console/unicode_width.ts` (the old syntax is a
  hard error in Deno 2.x — this is also why `deno bundle` of the raw URLs fails).
- Bundled the vendored tree with the repo's pinned esbuild (0.28.0) under the exact
  `esbuild.config.mjs` options (`platform:'neutral'`, `format:'esm'`,
  `external:['jsr:*','node:*']`, minify): **56 KB minified** added.
- Ran the bundle under Deno 2.9: help output reproduces the corpus byte pattern —
  same `Usage: COMMAND`, same ANSI sequences. Spike artifacts:
  scratchpad `cliffy-spike/` (session-local).

### 10.2 Corpus addenda (stream separation + edges)

- `Unknown engine:` / `Available engines:` / `Engine X does not support subcommands`
  → **stderr**, exit 1. `Julia control server is not running.` (status, no daemon,
  isolated HOME) → **stderr**, exit 0, stdout empty.
- `quarto call engine help` → the engine command's own static help on stdout, exit 0
  (cliffy's global HelpCommand reaches it; "help" is not treated as an engine name).
- New corpus files: `engine-help-subcmd.*`, `unknown.{out,err}.txt`,
  `nosupport.{out,err}.txt`, `status-nodaemon.{out,err,exit}`.

### 10.3 q2-side seams (file:line, from implementation survey)

- **Host entry**: `ts-packages/quarto-engine-host-deno/src/main.ts` is the only
  Deno-touching file; `Deno.args` is consumed nowhere today — branch on
  `Deno.args[0] === "call-engine"` before `runHost`. Bundle rebuilt by
  `cargo xtask build-engine-host-bundle` → `npm run bundle` → `esbuild.config.mjs`;
  CI freshness gate re-bundles and `git diff --exit-code`s the artifact
  (`.github/workflows/ts-test-suite.yml:181–189`).
- **Engine init requirement**: julia's handlers use the module-level `quarto` API
  captured in `discovery.init(quartoAPI)` — the call-engine mode must call
  `init(buildQuartoAPI(global, denoHost))` after import, mirroring host.ts
  `loadEngine`.
- **Config**: `quarto.path.runtime(name)` = `global.runtimeDir + "/" + name`
  (ensureDir). Rust constructs `HostGlobalConfig` in `build_engine_registry`
  (`crates/quarto-core/src/project/mod.rs:615–649`) from
  `quarto_util::quarto_runtime_dir()` etc. — the call-engine spawn must pass the
  identical values so it addresses the same transport files a render used.
- **Spawn helpers** (`crates/quarto-core/src/engine/ts_process.rs`):
  `is_available()` pub (line 151, with the "Install Deno from https://deno.land/"
  message pattern at 513–517); `extracted_bundle_path()` **private** (line 136) —
  needs a visibility bump; one-shot spawn should be a fresh `Command` with inherited
  stdio + `.status()`, not `spawn_into` (which pipes).
- **CLI**: `crates/quarto/src/commands/call/mod.rs` string dispatch — add
  `Some("engine") => engine::execute(args)` + `mod engine;`; update the usage/help
  strings. ProjectContext pattern per `get_config.rs:69–73`
  (`ProjectContext::discover(&input, runtime.as_ref())`); registry is the public
  field `project.registry`; a bare directory with no `_quarto.yml`/`_extensions`
  discovers fine and yields builtins-only with **no host spawn** (`needs_host`
  gate).
- **Name lookup**: `EngineRegistry::get` is a plain key match; the julia fixture
  declares `name: julia` so `get("julia")` works. The alias map is populated only
  *after* a LoadEngine, so undeclared-name extensions are addressable pre-load only
  by extension id — acceptable; note in plan.
- **Fixtures**: `crates/quarto-core/tests/fixtures/extensions/julia-engine/` — the
  committed `julia-engine.js` **contains `populateJuliaEngineCommand`** (e2e-ready,
  no daemon needed for `--help`/gate tests); `echo-engine/` (and `echo-legacy/`)
  have **no populateCommand** — natural gate-2 fixtures.
- **e2e precedent**: `crates/quarto/tests/integration/build_ts_extension_e2e.rs`
  drives the real binary via `env!("CARGO_BIN_EXE_q2")`, deno-gated by
  skip-early-return; julia daemon tests isolate via temp `HOME` +
  `SharedTransportSentinel` (`julia_engine_e2e.rs:150–195`).
- **Deno-native tests**: `src/<name>.deno-test.ts` files are vitest-excluded and run
  as raw `deno test --allow-all` steps in `ts-test-suite.yml` — a call-engine-mode
  test adds one more step there.
- **Trait default precedent**: `markdown_for_file` returns
  `Err(ExecutionError::not_supported("..."))` (traits.rs:170–176); `NotSupported`
  variant at error.rs:124–127.

### 10.4 Design deltas settled during implementation research

- Trait signature drops the speculative `ctx` parameter:
  `fn call_engine_command(&self, args: &[String]) -> Result<CallEngineOutcome, ExecutionError>`
  — TsEngine already owns `engine_path` and (via its host handle) the
  `HostGlobalConfig`; no other implementer exists. Add a context struct only when a
  real second implementer needs one (epic minimal-surface rule).
- `CallEngineOutcome { exit_code: i32 }` — carries the child's exit status for exact
  propagation (Q1 exit codes 0/1/2 are part of the parity contract).
- D-6 resolution: emit `Available engines:` with builtins in Q1's order
  (`knitr, jupyter, markdown`) followed by external engines in contribution order —
  byte parity for the common case at zero cost.
