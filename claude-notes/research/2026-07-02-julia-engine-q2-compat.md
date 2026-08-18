# Julia engine extension — q2 compatibility log (Plan 4 Phase 4A)

**Plan:** `claude-notes/plans/2026-04-16-julia-validation.md`, Phase 4A
**Upstream source:** `~/src/quarto-julia-engine` (machine-local checkout;
same content as Q1's `src/resources/extension-subtrees/julia-engine/` git
subtree)
**Fixture destination:** `crates/quarto-core/tests/fixtures/extensions/julia-engine/`
**Commits:** `e56da9c29` (import-map parity), `317bc930c` (fixture copy +
static-claiming keys + rebundle)
**Deno version used for rebundle:** `deno 2.9.0 (stable, release,
aarch64-apple-darwin)`, `v8 14.9.207.2-rusty`, `typescript 6.0.3`

This log documents every difference between the committed fixture and
upstream `~/src/quarto-julia-engine`, as input for (a) eventually merging
q2-specific changes back upstream (Gordon's call, deferred) and (b) the 4B
render debugger.

## 1. Import-map parity (prerequisite, not a fixture change)

`resources/extension-build/deno.json` and
`resources/extension-build/deno.workspace.json` were missing the Q1
bare-specifier aliases that `julia-engine.ts` relies on for its bare
imports (`"path"`, `"fs/exists"`, `"encoding/base64"`). Added, matching
Q1's `src/resources/extension-build/import-map.json` and plan1c's config
spec (`claude-notes/plans/2026-04-16-plan1c-extension-integration.md`
~L421-446) exactly:

```jsonc
"path":       "jsr:@std/path@1.0.8",
"path/posix": "jsr:@std/path@1.0.8/posix",
"log":        "jsr:/@std/log@0.224.0",
"log/":       "jsr:/@std/log@0.224.0/",
"fs/":        "jsr:/@std/fs@1.0.16/",
"encoding/":  "jsr:/@std/encoding@1.0.9/"
```

Same additions in both files (published-template `deno.json` and
in-repo-workspace `deno.workspace.json` — only the `@quarto/api`
/`@quarto/types` resolution differs between those two; the `@std` aliases
are identical). `log`/`log/` are added for parity even though
`julia-engine.ts` doesn't currently import `log` (its logging goes through
`quarto.console.*`, not Deno's `@std/log`) — Q1's map declares it and
nothing depends on its *absence*, so parity was kept exact rather than
minimized.

`cargo build -p quarto` and `cargo nextest run -p quarto build_ts_extension`
both green after this change (15/15 tests pass) — the embedded
`SHIPPED_DENO_JSON` (`include_str!` in
`crates/quarto/src/commands/build_ts_extension.rs`) picks up the edit
automatically since it's the same file.

## 2. Fixture copy — included / excluded

Copied with `rsync -av` from `~/src/quarto-julia-engine/` to
`crates/quarto-core/tests/fixtures/extensions/julia-engine/`.

**Included** (preserves upstream's own layout — the plan explicitly
forbids inventing a new one):
- `_quarto.yml` (root project file — makes the fixture a renderable
  project dir for 4B–4E test documents)
- `_extensions/julia-engine/` — the co-located runtime package:
  `_extension.yml`, `julia-engine.js` (pre-built bundle, later
  regenerated — see §4), `Project.toml`, `ensure_environment.jl`,
  `quartonotebookrunner.jl`, `start_quartonotebookrunner_detached.jl`
- `src/` — TS dev source: `julia-engine.ts` (1115 lines), `constants.ts`
  (15 lines)

**Excluded:**
- `.git/` — VCS metadata, not fixture content
- `.github/` — upstream repo's own CI workflows, not relevant to q2
- `.quarto/` — upstream's local render cache (xref index, freeze cache,
  project-cache) — build artifact, not source
- `tests/` — upstream's own Deno test suite (`tests/smoke/`,
  `tests/docs/`, `run-tests.sh`/`.ps1`) — repo-only, not needed for the q2
  fixture (q2's own Plan-4 test documents live directly under the fixture
  root per the plan)
- `example.html`, `example_files/` — upstream's committed example render
  output (148 KB HTML + bundled libs `bootstrap`/`quarto-html`/`clipboard`)
  — repo-only, matched by `example*` exclude
- `README.md`, `AGENTS.md`, `CLAUDE.md` (a symlink to `AGENTS.md`
  upstream) — repo-only docs
- `.DS_Store`, `.gitignore` — machine/VCS noise

No secrets found. `grep -ril "token|secret|api_key|apikey|password"`
across the upstream tree hit only: bundled third-party JS
(`bootstrap.min.js`, `clipboard.min.js` — excluded anyway via
`example_files/`) and `src/julia-engine.ts`'s `secret: string` HMAC
parameter name (crypto-signing API, not an embedded credential) — no
exclusion needed beyond the above.

## 3. `_extension.yml` — q2 static-claiming keys added

Upstream `_extension.yml`:

```yaml
title: Quarto Julia Engine Extension
version: 0.1.0
quarto-required: ">=1.9.0"
contributes:
  engines:
    - path: julia-engine.js
```

Fixture `_extension.yml` (added lines only — `title`/`version`/
`quarto-required` untouched):

```yaml
contributes:
  engines:
    - path: julia-engine.js
      name: julia
      claims:
        julia:
          kind: primary
          priority: 1
      file-extensions:
        - .jl
```

Exact text from the plan (Phase 4A, ~L79-87). `claims-files` deliberately
**not** declared — Julia's dynamic `claimsFile` is content-inspecting
(`# %%` percent scripts), so leaving it undeclared keeps Pass-1 zero-spawn
resolution intact (`file-extensions` is only a can-handle pre-filter).

## 4. Rebundle — provenance and byte-identity result

**The literal invocation in the task brief
(`q2 build-ts-extension src/julia-engine.ts` from the fixture root) does
NOT work** — `resolve_extension_dir` only special-cases a path literally
named `_extension.yml`; any other path (including a `.ts` file) is treated
as a candidate *directory* and probed for `<path>/_extension.yml`, which
of course doesn't exist for a file path. Confirmed empirically:

```
$ q2 build-ts-extension src/julia-engine.ts
Error: No _extension.yml found in src/julia-engine.ts. Pass the extension
directory or _extension.yml path explicitly.
```

The correct extension-directory argument, `_extensions/julia-engine`, then
hits a **second**, structural mismatch: `find_entry_ts` hardcodes the
convention `<ext_dir>/src/<ext_dir_basename>.ts` (this is what the
echo-engine fixture satisfies — echo is flat, ext_dir == fixture root, with
`src/` and `_extension.yml` co-located). Julia's upstream layout
deliberately does **not** co-locate TS source inside the shipped
`_extensions/<name>/` package (that's the whole point of the plan's
"preserve upstream layout, don't invent a new one" directive — `src/` sits
at the repo/fixture root, sibling to `_extensions/`, matching every real
Quarto-1 extension repo). Confirmed empirically:

```
$ q2 build-ts-extension _extensions/julia-engine
Error: No TypeScript entry point found. Expected src/julia-engine.ts
inside _extensions/julia-engine.
```

Neither failure is a `deno` error and neither is fixed by editing
`julia-engine.ts` — both are `q2 build-ts-extension`'s own directory-
resolution assumptions failing to match a real Q1 extension repo's shape.
Per the brief, `crates/` source is out of scope for this task, so the
build was made to work via a **local, non-committed, one-time symlink**:

```
_extensions/julia-engine/src -> ../../src     # created only for the build, removed after
```

This satisfies `find_entry_ts`'s convention without duplicating or moving
any committed file. `deno bundle` canonicalizes the symlinked entry path
before resolving relative imports, so the emitted module-path banner
comments read `src/julia-engine.ts` / `src/constants.ts` (the *real*
path), not the symlink path — i.e. the output is indistinguishable from a
bundle built directly against the true source tree. The symlink was
removed immediately after the build; it is **not** part of the commit (
verified: `find crates/.../julia-engine -type l` returns nothing after
cleanup, and `git status` before staging showed no symlink entry).

Exact invocation used (from the fixture root, after creating the temp
symlink):

```
$ q2 build-ts-extension _extensions/julia-engine -v
⚠️  deno bundle is experimental and subject to changes
Bundled 77 modules in 15ms
  _extensions/julia-engine/julia-engine.js 43.47KB

Built: _extensions/julia-engine/src/julia-engine.ts → _extensions/julia-engine/julia-engine.js
```

Config resolved via workspace auto-detection (tier 3 —
`find_workspace_root` walked up from `_extensions/julia-engine/` to the
repo root, which contains `ts-packages/quarto-api`), i.e.
`resources/extension-build/deno.workspace.json` — `@quarto/api`/
`@quarto/types` from local workspace source, `@std/*` from `jsr:` (network
fetch, as expected/accepted by the plan; Deno's local jsr cache serviced
it without incident).

**Result: the rebuilt `julia-engine.js` is byte-identical to the upstream
Q1-built bundle** (`diff` empty; both MD5 `d9d5120eb94b187903a43fb500e65eea`,
44512 bytes). This is the strongest possible build-compatibility signal —
q2's bundler config (post-parity-fix) reproduces Q1's own build output
exactly, not just "close enough."

Spot-check evidence (bundle content):
- `grep -c encodeBase64` → 3 hits (the `encoding/base64` alias resolved
  and inlined)
- Module-path banners show `jsr.io/@std/path/1.0.8`,
  `jsr.io/@std/path/1.0.8/posix`, `jsr.io/@std/path/1.0.8/windows`,
  `jsr.io/@std/fs/1.0.16`, `jsr.io/@std/encoding/1.0.9` — all four pinned
  `@std` packages actually got fetched and inlined, confirming the §1
  import-map edit is load-bearing (not merely present-but-unused)
- No `@quarto/api`/`quarto-api` string markers in the bundle — expected:
  `julia-engine.ts` imports only **types** from `@quarto/types` (erased at
  bundle time) and references the `quarto` global at runtime (injected by
  engine-host, never imported) — so there is nothing from `@quarto/api` to
  inline. `quarto.jupyter.toMarkdown`/`quarto.console.withSpinner` etc.
  remain as literal unresolved identifiers in the bundle, as expected for
  a host-injected global.

## 5. `julia-engine.ts` source modifications

**None.** No changes were made to `julia-engine.ts` or `constants.ts`. The
byte-identical rebundle result (§4) is itself the proof that no source
edit was necessary.

## 6. Regression / build verification

- `cargo build -p quarto`: green (after §1 edit)
- `cargo nextest run -p quarto build_ts_extension`: 15/15 passed
- `cargo nextest run -p quarto-core -E 'test(echo)'`: 10/10 passed (no
  regression in the echo fixture's shared extension-build config path)
- `cargo nextest run -p quarto-core -E 'test(engine_registry_build)'`:
  9/9 passed

## 7. Audit pass for the 4B debugger — `quarto.*` API surface + resource resolution

**Namespaces actually called** (via `grep -oE 'quarto\.[a-zA-Z]+\.[a-zA-Z]+'`,
deduplicated): `quarto.console`, `quarto.format`, `quarto.jupyter`,
`quarto.mappedString`, `quarto.markdownRegex`, `quarto.path`,
`quarto.system` — **7 namespaces**, not the "8" the plan's 2026-07-01 audit
mentions (25 distinct calls across 8 namespaces). Could not find an 8th
`quarto.*` namespace anywhere in `julia-engine.ts` by any grep pattern
tried (`quarto\.[a-zA-Z]+` alone lists the same 7). `crypto.subtle.*` is
used (lines 814, 822) but that's **native Deno `crypto`**, not a `quarto`
namespace — possibly what inflated the plan's count to 8, or the audit
counted a namespace this pass didn't trigger on (e.g. one gated behind a
code path not statically greppable). **Flag for 4B**: re-run the 2026-07-01
audit's own methodology to reconcile the count; this pass trusts the
static grep over the plan's prose count but doesn't have the original
audit's methodology to compare against.

Individual `quarto.*` call sites actually present (30 call sites across
those 7 namespaces, not 25 — plan's number may have been a distinct-method
count rather than a call-site count):
`console.{error,info,withSpinner}`,
`format.{isHtmlCompatible,isIpynbOutput,isLatexOutput,isMarkdownOutput,isPresentationOutput}`,
`jupyter.{assets,isPercentScript,percentScriptToMarkdown,resultEngineDependencies,resultIncludes,toMarkdown}`,
`mappedString.{fromFile,fromString,indexToLineCol,splitLines}`,
`markdownRegex.{extractYaml,partition}`,
`path.{absolute,runtime,toForwardSlashes}`,
`system.{isInteractiveSession,runningInCI}`.

**Resource resolution (`import.meta.url`):** single use, line 46:
`const extensionDir = dirname(fromFileUrl(import.meta.url));`. This
resolves to the directory of the *loaded bundle* at runtime — i.e.
`_extensions/julia-engine/` once installed/claimed, which is exactly where
the `.jl` scripts (`quartonotebookrunner.jl`,
`start_quartonotebookrunner_detached.jl`, `ensure_environment.jl`) and
`Project.toml` live in the committed fixture. **Nothing suspicious here**
provided q2's TS-engine host loads the bundle from its co-located
`_extensions/julia-engine/julia-engine.js` path unchanged (no copy-to-temp
step that would break the `extensionDir` co-location assumption) — flag
for 4B to confirm the loader doesn't relocate the bundle before `import()`.

**Native Deno APIs used** (all should work unmodified under real Deno per
the plan's expectation): `Deno.Command`, `Deno.connect`,
`Deno.{Conn,TcpConn}` (types), `Deno.build`, `Deno.consoleSize`,
`Deno.env`, `Deno.kill`, `Deno.{readFileSync,writeFileSync,removeSync}`,
`Deno.readTextFileSync`, `Deno.RemoveOptions` (type), `Deno.stdout`,
plus `crypto.subtle.{importKey,sign}` (Web Crypto, ambient — not a `Deno.*`
call). No 4A-level red flags; flag for 4B: `Deno.Command`/`Deno.connect`
are exactly the daemon-spawning / transport-connecting calls the plan's
4B daemon warning is about (`execute: daemon: false` must actually gate
these call sites at runtime — 4A did not verify that at the call-site
level, only that P1.1b landed the wiring).

## 8. Open items / concerns carried to 4B

1. **`build-ts-extension`'s directory-resolution convention doesn't fit
   real Q1 extension repos.** Both failure modes in §4 are structural, not
   config: (a) the CLI can't take a `.ts` file as `PATH` (only a directory
   or an `_extension.yml` path), and (b) it hardcodes TS source living
   *inside* the same directory as `_extension.yml`, which is true for the
   echo fixture's synthetic flat layout but false for every real upstream
   Quarto-1 extension repo (source at repo root, shipped package in
   `_extensions/<name>/`). This task worked around it with a throwaway,
   uncommitted symlink because touching `crates/` source was out of scope
   — but that means **the documented Phase-4A rebundle command in the plan
   itself doesn't work as written**, and any future re-rebundle (e.g. after
   a real source edit in 4B+) will hit the same two errors and need the
   same workaround. Worth a small follow-up to either extend
   `find_entry_ts`/`resolve_extension_dir` to accept a `--entry <ts-path>`
   override, or to document the symlink workaround directly in the
   `build-ts-extension --help` text / plan. Not fixed here (out of this
   task's scope; scope is documented, not code changes).
2. **Namespace-count discrepancy** (§7) — the plan's 2026-07-01 audit says
   8 namespaces / 25 calls; this pass found 7 namespaces / 30 call sites.
   Not necessarily a bug (different counting methodology is plausible),
   but worth reconciling before 4B assumes API-surface completeness from
   the plan's number alone.
3. **Daemon call-site verification deferred to 4B** — confirmed
   `Deno.Command`/`Deno.connect` are present and where the daemon logic
   lives, but did not trace whether `execute: daemon: false` actually
   short-circuits them at the call-site level (that requires an actual
   render, which is explicitly 4B's job, not 4A's).

## 9. Phase 4B — first real Julia render: failures found + fixes

The 4B minimal doc (`crates/quarto-core/tests/fixtures/extensions/julia-engine/minimal.qmd`:
`engine: julia`, `execute: daemon: false`, one `{julia}` cell `1 + 1`) was
rendered through the real path (`q2 render` → Deno engine-host → real Julia
QuartoNotebookRunner 0.17.4, julia 1.11.7, deno 2.9.0). Three failures, in
order, each fixed minimally. J1 (`crates/quarto-core/tests/integration/julia_engine_e2e.rs`)
is the frozen end-to-end seam that now binds the whole chain.

### API pre-flight (before rendering)
Reconciled §7's flag: all **25 distinct `quarto.*` members** julia-engine.ts
calls exist in the assembled global. Crucially, the 6 `quarto.jupyter.*` members
used (`assets`, `isPercentScript`, `percentScriptToMarkdown`,
`resultEngineDependencies`, `resultIncludes`, `toMarkdown`) are ALL among
`makeJupyter`'s 7 *implemented* methods (`ts-packages/quarto-api/src/jupyter/index.ts`)
— none hit the 15 `NotImplemented` throwers. `console.withSpinner` exists
(`console/index.ts`). No missing-member risk. (Namespace count: the "8th" in
the plan's prose is `crypto.subtle` — native Web Crypto, not a `quarto.*`
namespace; §7's 7-namespace count stands.)

### Failure 1 — extension not discovered (`author` required)
**Symptom:** render exits 0 but the `{julia}` cell renders as an *unexecuted*
code listing (`<pre class="{julia}"><code>1 + 1</code></pre>`); a WARN
`missing required 'author' field` repeats.
**Diagnosis:** q2's extension reader (`crates/quarto-core/src/extension/discover.rs`)
makes `author` a **required** field; upstream julia `_extension.yml` (and Q1)
have none, so discovery drops the extension → no engine claims `julia`.
q2-vs-Q1 divergence.
**Fix (fixture):** add `author: Quarto Julia Engine` to the fixture
`_extensions/julia-engine/_extension.yml` (echo's fixture already has one).

### Failure 2 — QuartoNotebookRunner crash on empty `sourceRanges`
**Symptom:** `Execution failed in julia: … ArgumentError: reducing over an empty
collection is not allowed` at QNR `server.jl:628`
(`maximum(r -> r.lines.stop, source_ranges)`).
**Diagnosis:** julia-engine.ts `buildSourceRanges` maps every input line back to
its origin via the `MappedString.map`. q2's Rust side sent the execute request
with an **empty** wire source map (`ts_engine.rs:399` was
`let source_map = Vec::new();` — a stub that only worked for engines like echo
that never inspect markdown provenance). The host's `rehydrateMappedString`
then built an opaque MappedString whose `.map` returns null for every line, so
`buildSourceRanges` produced `[]`, and julia-engine.ts sent `sourceRanges: []`.
QNR has a `::Nothing` overload for "no ranges" but not for an empty vector →
`maximum` over `[]` throws. Q1 never triggers this because its execute
MappedString is always fully mapped.
**Fix (Rust, q2-side):** `crates/quarto-core/src/engine/ts_engine.rs` — new
`build_source_map(input, ctx)` walks `input` line-by-line and resolves each
line-start offset through the *existing* provenance already on
`ExecutionContext` (`source_info.map_offset` + `source_context`), emitting one
`TsSourceMapEntry { start, length, source: {file, fileOffset} }` per line
(unmappable/`Generated` lines → `source: None`). No protocol/type change — the
wire field, `TsSourceMapEntry`, and the host-side rehydrate already existed and
round-trip in the echo tests. The upstream engine is byte-for-byte unchanged.
After the fix the progress line reads `Running [1/1] at line 7: 1 + 1` —
correct file-line attribution, confirming the map is real, not identity.

### Failure 3 — executed cells vanish (missing execute-visibility defaults)
**Symptom:** render exits 0, no crash, but the HTML `<body>` is empty — the
execute-result markdown is only the 205-char frontmatter; the `{julia}` cell
and its `2` output are gone. Instrumentation confirmed QNR returned a correct
notebook (`cells=3`; the code cell has `src=["1 + 1"]`,
`outputs=[{execute_result, data:{"text/plain":"2"}}]`), yet `jupyterToMarkdown`
emitted zero `cellOutputs`.
**Diagnosis:** `jupyterToMarkdown` gates each cell on
`includeCell`/`includeCode`/`includeOutput` → `shouldInclude` (`tags.ts`),
which for an absent cell-level option falls back to `options.execute[kInclude]`
etc. q2's host `metadataAsFormat` is a faithful port of Q1's *partition-only*
`metadataAsFormat` — but Q1 merges the **writer format's execute defaults**
(`include/eval/output/echo/warning = true`, `formats-shared.ts:210-217`) into
the metadata during *format resolution*, BEFORE that partition. q2 has no
writer-format-defaults layer, so `format.execute` reaches the engine with only
the frontmatter keys (`daemon:false`), leaving `include`/`output`/`eval`
undefined → every cell dropped. Julia is q2's first real `jupyterToMarkdown`
consumer (echo emits its own markdown), so this hole never surfaced before.
**Fix (engine-host TS, q2-side):** new `applyExecuteDefaults(format)` in
`ts-packages/quarto-engine-host-deno/src/metadata-as-format.ts` fills absent
execute-visibility keys with Q1's base defaults; called after `metadataAsFormat`
at the execute and dependencies Format-construction sites in `host.ts`. The
faithful `metadataAsFormat` port is left pure. Unit tests added
(`metadata-as-format.test.ts` T1.11). **Bundle rebuilt + committed**
(`dist/engine-host-deno.js`).
**KNOWN DIVERGENCE (for 4F/4G) — CORRECTED IN §14 (2026-07-02):** only Q1's
*base* execute defaults are applied — the per-writer overrides are NOT. The
original claim here ("Q1's HTML would hide cell source") was **overstated**:
the 4F Q1 comparison showed Q1's plain HTML shows source by default too; the
real gap is the *presentation-family* per-writer overrides (revealjs/pptx/
dashboard etc.). See §14 for the source-cited correction; tracked as
**bd-cymkcyaf**. Closing it needs a real writer-format-defaults layer, not a
host shim; deliberately deferred.

### Result (end-to-end, inspected)
`q2 render minimal.qmd` → `minimal.html` `<body>` contains:
`<div id="cell-1" class="cell">` with the echoed source `1 + 1` and
`<div class="cell-output cell-output-display">…<code>2</code>…</div>`.
Cold start: QNR's Julia project was already instantiated on this machine (no
multi-minute install observed); the control server started in ~4s per render
(`daemon: false` → oneShot, server closed after each render — no detached
server escaped; no transport-file cleanup needed).

### Resolved from §8
- §8.3 (daemon call-site): `daemon: false` renders spawn a oneShot control
  server that is closed at end of render; no lingering detached server observed.
  (Full daemon-mode *behavior* is 4E/V-1, not asserted here.)

## 10. Phase 4C/4D — figures, multi-cell state, error handling (seam J4)

Full evidence lives in `.superpowers/sdd/plan4-task-4cd-report.md`; this
section is the compat-log summary.

### 4C — Plots.jl figure render

**Notebook environment setup required (not a q2 bug).** `q2 render plot.qmd`
initially failed with a real Julia error (`ArgumentError: Package Plots not
found in current path`), not a q2/engine bug. Root cause: QuartoNotebookRunner
launches each notebook's Julia *worker* with CWD = the notebook's own
directory and `JULIA_PROJECT=@.` (search upward from CWD for the nearest
`Project.toml`) unless the frontmatter overrides it
(`QuartoNotebookRunner/src/server.jl:151-168,239-242`). The fixture root had
no `Project.toml` of its own — only `_extensions/julia-engine/Project.toml`,
which is the *engine's* runtime environment, not the *notebook's*. Fixed by
running `julia --project=. -e 'using Pkg; Pkg.add("Plots"); Pkg.precompile()'`
at the fixture root and committing the resulting `Project.toml`/`Manifest.toml`
(40K). This mirrors real-world Quarto+Julia practice — a project needs its own
Julia environment for any packages its documents `using`, independent of any
extension's own environment. Worth a line in 4G's migration docs.

**Figure-file vs. inline-base64 divergence (traced, not a q2 bug).** The plan
predicted a file-based figure (`<stem>_files/figure-html/*.png` +
`<img src="...">` referencing it). The actual render instead produced
`<img src="data:image/png;base64,...">` — a fully valid, displaying image, but
inline rather than file-based. Root cause: `displayDataMimeType`
(`ts-packages/quarto-api/src/jupyter/display-data.ts`) is a faithful,
documented port of Q1's own priority-list quirk — for HTML-family targets,
`text/html` is unconditionally unshifted to the FRONT of the MIME priority
list, ahead of `image/png`/`image/svg+xml`. `Plots.jl`'s default GR-backend
plot object IS `showable` as `text/html` (confirmed empirically:
`showable(MIME("text/html"), plot(1:10, rand(10)))` → `true`), and its
`text/html` show method itself emits a self-contained
`<img src="data:image/png;base64,...">` snippet. So `mdOutputDisplayData`
picks the HTML branch (`mdHtmlOutput`, which just echoes the raw HTML
verbatim) instead of the image branch (`mdImageOutput`, which writes a file
under `assets.figures_dir`) — the file-writing code path is simply never
reached for this document. This is the SAME code Quarto 1 uses (the quirk is
explicitly documented as an intentional faithful port), so a real Q1 render
of this exact document would plausibly produce the same result. **Flag for
4H/J5:** if J5's website-render row wants to bind the `supporting`-forwarding
hunk in `map_execute_result`, it needs a document/cell whose only showable
MIME is an image type — `plot.qmd` as committed would make that specific
J5 assertion vacuous (green even with the forward reverted), for the same
reason it's vacuous here.

**`supporting` field — confirmed non-empty (verified, not assumed).**
Temporary instrumentation (`eprintln!` on `map_execute_result`, added,
observed, removed — final `git diff` on `ts_engine.rs` is empty) showed
`supporting=["…/plot_files"]` — ONE entry, the whole `<stem>_files` directory,
not scoped to `figure-html`. `julia-engine.ts` sends
`supporting: [join(assets.base_dir, assets.supporting_dir)]`
UNCONDITIONALLY (`src/julia-engine.ts:287`); `assets.figures_dir` is created
eagerly by `host.fs.ensureDir` (`assets.ts:91`) regardless of whether any
figure ever lands inside it, and `assets.ts`'s walk check resolved
`supporting_dir` to the whole `files_dir` (not `figures_dir`) because nothing
else existed under it at execute time. An initial guess of `supporting=[]`
(reasoning from "no file was written") was WRONG and corrected only after
actually running the instrumentation — a good reminder to verify rather than
infer for this kind of check.

### 4D — multi-cell state (V-5) + error handling (J4)

**V-5 (manual, confirmed):** `multi-cell.qmd` (`x = 42` / `println("x is
$x")`, `daemon: false`) renders with cell 2's output `x is 42` — state
persists across cells within one QuartoNotebookRunner `run` request. No q2
hunk involved; recorded per the plan, no frozen test added.

**J4 (frozen, GREEN, RED-proven):** landed in
`crates/quarto-core/tests/integration/julia_engine_e2e.rs` as
`j4_error_handling_does_not_wedge_host`. "Same process" binding: discovers
`ProjectContext` exactly ONCE, then renders both the error doc and the J1
minimal doc via `render_document_to_file(..., Some(&project), ...)` — per
`pipeline.rs`, a render given `Some(&project)` (no registry override) uses
`project.registry`, i.e. the SAME `Arc<EngineRegistry>`/`Arc<TsEngineHost>`
for both renders. This is the same sharing mechanism a real multi-page
project render uses (4H's J6/J8), so it's the faithful analog of "the same
process."

**Assertion-strength finding.** The first draft of the error-message
assertion (`message.contains("this should fail gracefully")` alone) did NOT
redden against the named revert (the `FromEngine::Error` arm in
`TsEngineHost::request`, `ts_process.rs:~693`) — it stayed falsely GREEN.
Cause: after the revert, `request()` returns `Ok(FromEngine::Error{..})`
instead of `Err`; `TsEngine::execute`'s OWN fallback arm
(`other => Err(ExecutionError::other(format!("unexpected response to
Execute: {other:?}")))`) then produces an error whose `{:?}` Debug dump of
the `FromEngine::Error` struct still happens to embed the original message
text — so the bare `contains(...)` check couldn't discriminate. Fixed by
adding two more assertions: the message must contain `"Execution failed in
julia:"` (the properly-typed `ExecutionFailed` Display) and must NOT contain
`"unexpected response to Execute"` (the fallback's signature). Verified the
full RED→GREEN cycle TWICE against the strengthened assertions (both times:
revert in place → RED with the expected panic message; revert removed,
`git diff` on `ts_process.rs` confirmed empty → GREEN,
`cargo nextest run -p quarto-core -E 'test(j4)'` → `1 passed`). Not a q2
production bug — the production code was correct throughout; only the FIRST
DRAFT of the test assertion was too weak, and it was strengthened (never
weakened) before any GREEN was declared final. Full trail:
`.superpowers/sdd/plan4-task-4cd-report.md`.

**Daemon hygiene:** found and killed one pre-existing escaped Julia daemon
under this worktree's fixture path (control server + worker, running since
before this session, presumably an earlier debugging leftover) via its
transport file. A separate daemon under a different directory
(`/Users/gordon/docs/julia/...`) belongs to the concurrent docs agent and was
left untouched. All of this task's own `daemon: false` renders spawned and
cleanly closed their own oneShot control servers.

## 11. Phase 4E — cell options (J2), exeflags/env (J3), daemon (V-1)

Full evidence lives in `.superpowers/sdd/plan4-task-4e-report.md`; this
section is the compat-log summary.

### J2 — document-level `execute: echo: false` (frozen, GREEN, RED-proven)

Landed as `julia_engine_e2e::j2_document_level_echo_false_hides_source_keeps_output`.
Doc: `execute: {daemon: false, echo: false}` + a cell whose SOURCE contains
`j2_hidden_source_variable` (never in output) and whose OUTPUT contains
`j2 output present` (never in source). Three assertions: `cell-output`
present, output token present, source token ABSENT. RED→GREEN proven
verbatim against the spec'd T14 revert (`metadata: HashMap::new()` at
`ts_engine.rs:394`): under the revert the host's `applyExecuteDefaults`
fills the now-absent `echo` with base default `true` → source listing
rendered → RED at the source-absent assertion. Restore verified via empty
`git diff` → GREEN.

### J3 — `julia: exeflags`/`env` (frozen, GREEN, RED-proven — with two
### evidence-backed deviations from the seam-spec row)

Landed as `julia_engine_e2e::j3_exeflags_and_env_through_julia_block`.
`env` was FOLDED into J3 (no separate V-2): same doc, cell prints both
`nthreads: <n>` and `FOO=<val>`; frozen assertions `nthreads: 2` and
`FOO=BAR`.

**Schema stop-point (resolved).** Confirmed against the INSTALLED
QuartoNotebookRunner 0.17.4 source (`~/.julia/packages/QuartoNotebookRunner/
evCNi/src/server.jl`, exactly the version the fixture's `Project.toml` pins):
`_exeflags_and_env(options)` reads `options["format"]["metadata"]["julia"]
["exeflags"]` and `["env"]` (`server.jl:151-168`); `env` entries are
`KEY=VALUE` strings `addenv`-merged into the worker process (`Malt.jl:390-396`);
absent values come from `default_frontmatter()`
(`julia => D("env" => [], "exeflags" => [])`, `server.jl:888-896`). So the
frontmatter shape is a top-level `julia:` mapping with `exeflags`/`env`
string arrays — matching the plan.

**Deviation 1 — fixture placement (real q2 bug found, bd-uf4epv4w).** The
spec'd document-level `julia: exeflags: ["--threads=2"]` is IMPOSSIBLE in q2
today: DocumentMetadata-context strings are parsed as markdown
(`pampa/src/pandoc/meta.rs`) and `apply_smart_typography`
(`pampa/src/pandoc/treesitter.rs:566`, always-on) converts the leading `--`
to an en dash. QNR then treats `–threads=2` as a FILE argument for the
worker command and the render fails
(`SystemError: opening file ".../–threads=2"`). Verified at the pampa level:
`exeflags: ["-t2"]` survives as `Str "-t2"` while `other: "--threads=2"`
becomes `Str "–threads=2"`. This mangles EVERY machine-facing metadata
string (`--`/`---`/`...`/quotes), not just exeflags; Q1 is immune because it
partitions frontmatter with a plain YAML parser. Filed as **bd-uf4epv4w**
(architectural — raw-string preservation). J3's fixture moved the `julia:`
block to the temp project's `_quarto.yml`, where ProjectConfig-context
strings stay literal and the spec's exact `--threads=2` survives.

**Deviation 2 — named revert re-anchored (spec'd revert empirically
undiscriminating).** The seam spec's named revert (shared T14 hunk) left J3
GREEN — twice (document-level AND project-level fixtures). Two stacked
reasons, both verified in QNR 0.17.4 source:
1. QNR parses the notebook FILE's own frontmatter and recursive-merges it
   UNDER the wire options (`_extract_relevant_options`, `server.jl:366`).
2. Deeper: julia-engine.ts sends `target.markdown` in the run command and
   QNR's socket layer uses it as a file-content OVERRIDE (`_get_markdown`,
   `socket.jl:497-498`); q2's engine input is the AST serialized AFTER
   `MetadataMergeStage` (`serialize_ast_to_qmd`, engine_execution.rs), so
   the override's frontmatter carries the FULL merged metadata — project
   layers included. Anything in merged metadata reaches QNR through the
   markdown override even with the T14 wire path reverted.
The T14 hunk remains revert-bound by J2 (host-side `format.execute`
consumption has no QNR fallback). J3's revert was re-anchored to the
PROJECT metadata layer in `MetadataMergeStage::run`
(`metadata_merge.rs:~214`, `let project_layer = …map(…)` →
`let project_layer = None;`): under it the `julia:` block never enters
merged metadata (absent from both the serialized frontmatter and the wire)
→ QNR defaults → RED at `nthreads: 2` (worker spawned with 1 thread).
Proven RED→GREEN; restore verified via empty `git diff`.

**Interpretation note for 4F/4G:** in q2's current design the
QNR-consumed julia options are delivered redundantly (serialized
frontmatter override AND wire `format.metadata`); the wire path is the one
Q1-shaped engines that do NOT send `target.markdown` would rely on. J3 now
binds the end-to-end property "project-config `julia:` options reach the
QNR worker", which is also the piece Q1 compatibility actually needs
(document-level options reach QNR through the file itself in both Q1 and
q2).

### 4E manual greps — cell-level `#| output: false` / `#| warning: false`

Rendered through the real binary (`target/debug/q2 render <doc>.qmd`, docs
with `execute: daemon: false`), output inspected:

- `#| output: false` + `println("output-false-marker-should-be-absent")`:
  the rendered HTML contains ZERO `cell-output` blocks; the marker appears
  ONLY in the echoed source listing (`.cell-code`, echo defaults true per
  the §9 known divergence). Cell OUTPUT correctly suppressed.
- `#| warning: false` + `@warn "warning-false-marker-should-be-absent"` +
  `println("normal-output-should-remain")`: the ONLY output block is
  `cell-output cell-output-stdout` containing `normal-output-should-remain`;
  the warning text appears ONLY in the echoed source. Warning suppressed,
  normal output kept.

These bind `jupyterToMarkdown`'s cell-option path (`#|` options travel
inside cell source through QNR back into `cell.options`), complementing
J2's document-level binding.

### V-1 — daemon mode (manual, deliberately not frozen)

The julia transport file is GLOBAL per user
(`<quarto_runtime_dir>/julia/julia_transport.txt`, macOS:
`~/Library/Caches/quarto/julia/`), and a concurrent docs-agent daemon owned
it for the whole session (control server PID 9828 + its workers — left
untouched throughout, verified alive after teardown). V-1 was therefore run
in an ISOLATED world: `HOME=/tmp/julia-v1-home` (redirects
`quarto_runtime_dir` via dirs::cache_dir) with `JULIA_DEPOT_PATH` pinned to
the real depot and `QUARTO_JULIA` pinned to the real julia binary.

Observations (full invocations + timings in the 4E task report):

1. **Cold `daemon: false` render**: q2 starts the DETACHED control server
   anyway (transport file written, server PID 96341, port 8001). First
   attempt failed with `Execution failed in julia: undefined` — the fresh
   runtime env ran `Pkg.update()` (~2.5 min) and the server needed ~12 s
   more to write the transport file, exceeding julia-engine.ts's ~10.5 s
   15-try poll (`pollTransportFile` rejects with no value → "undefined").
   Environment-induced cold-start flake, exactly the masquerade the plan's
   CI-gating note warns about; retry succeeded (49.6 s — first worker pays
   QNR worker-package precompile).
2. **oneShot semantics answered** (V-1's open question): `daemon: false`
   closes only the per-file WORKER (render printed worker pid 97525; no
   worker children remained afterward) — the control server and transport
   file PERSIST after the render.
3. **`daemon: true` render #1**: 5.73 s; worker 98497 spawned and REMAINED
   OPEN after the render (the daemon observable).
4. **`daemon: true` render #2**: 0.33 s (vs 5.73 s — markedly faster), and
   the cell's `getpid()` output printed the SAME worker pid 98497 — direct
   in-band proof of worker reuse. Same control server (96341), no new
   server start.
5. **Teardown (out-of-band)**: read `{port, pid}` from the transport file →
   `kill -TERM 96341` → server exited, its Malt atexit STOPPED the worker
   (98497 gone) and REMOVED the transport file itself (quartonotebookrunner
   .jl's `atexit` + Malt `stop`; server log records the SIGTERM). Final
   `ps` sweep matched the pre-session baseline EXACTLY — zero orphan julia
   processes from this task's renders; docs-agent daemon 9828 + transport
   intact.

**Promotable to a J-row**: the observables were stable and in-band
(worker pid printed by the cell discriminates reuse without log
scraping) — a frozen row could assert "same worker pid across two
daemon:true renders; different/absent worker after daemon:false" behind a
HOME-isolated harness. Needs controller sign-off (frozen spec, additions
only) plus a decision on the isolated-HOME harness cost. NOT added here.

**§9 correction**: §9's "no detached server escaped" for 4B is now
explained — those renders REUSED the pre-existing docs-agent control
server (global transport file), so no server escaped *from those renders*;
oneShot does NOT prevent a detached server in a cold world (see obs. 1-2).

### 4E addendum — J4 error path leaks its QNR worker (bd-l9jhy5u0)

Discovered while triple-checking process hygiene after the final 4E test
runs: `executeJulia` only sends the oneShot `close` AFTER a successful
`run` (`julia-engine.ts:742-749`), so when the run errors (J4's
`error(...)` doc) the throw skips the close and the notebook's worker
stays open on the global control server. Verified by `lsof`: the two
lingering workers' CWDs were the (deleted) nextest TempDirs of this
session's J4 executions. Same code shape upstream in Q1 — candidate
upstream report. Filed as **bd-l9jhy5u0** (fix sketch: try/finally). This
also explains the pool of mystery workers observed hanging off the shared
control server at session start (earlier agents' J4 runs), and why that
server never reaches its 300 s idle timeout.

### 4H addendum — bd-677297ca fixed (supporting DIRECTORY → files at add_engine_files)

The J5/J6 blocker is resolved: `DocumentResourceReport::add_engine_files`
(`crates/quarto-core/src/project_resources.rs`) now expands an
engine-reported supporting entry that resolves to an on-disk DIRECTORY
(julia-engine's faithful-Q1 `supporting: [<stem>_files]` shape) into its
contained files recursively via `SystemRuntime::is_dir`/`dir_list` —
controller-adjudicated option (c); the report and everything downstream
(`resolve_reported_resources`, `copy_resources_to_output_dir`,
`SystemRuntime`) stay file-only and unchanged. Website julia-figure
renders now return Ok; J5 + the J6 project-render row are un-`#[ignore]`d
and GREEN (J6 pinned to `QUARTO_JOBS=1` so the tracing capture's
thread-local subscriber sees the spawn event — rayon Pass-2 workers don't
inherit `with_default` scopes). No change to julia-engine.ts.

## 12. Phase 4I — Pass-1/resolution cost audit (J9, V-4)

### Hardening pass (4H review minors, landed ahead of 4I)

Three Minor findings from the 4H code review, fixed in
`crates/quarto-core/src/project_resources.rs`'s `add_engine_files` (the
directory-expansion walk added for bd-677297ca):

1. **Symlink-cycle guard.** `NativeRuntime::is_dir` follows symlinks and the
   explicit-stack walk had no visited-set. RED (unguarded): a self-referencing
   symlink (`plot_files/cycle -> plot_files`) did not hang outright — macOS's
   symlink ELOOP limit bounded it — but produced 32 duplicate `cell-1.png`
   entries at increasing `cycle/cycle/...` depth instead of the single real
   file, proving pathological over-recursion. Fixed by canonicalizing each
   directory before pushing it onto the walk stack and skipping anything
   already visited. GREEN after.
2. **`tracing::warn!` on the unreadable-subdirectory fail-soft**, logging the
   skipped directory path. Code-only (existing tests green) — a
   permission-based dedicated test would be unix-only and unreliable when run
   as root.
3. Reworded a stale comment in `julia_engine_e2e.rs` (J5) that described the
   figure mechanism as `display(MIME("image/png"), p)` / `InlineDisplay`; the
   actual `plot.qmd` fixture uses `savefig` + a `PngFigure` wrapper struct.
   Comment-only.

Commit `0786cab25`. Verified: `cargo nextest run -p quarto-core` (2623
passed, including the frozen J1-J6 rows).

### J9 — zero-load resolution ordering (test-only, echo-based)

Both consumed tracing events (the `engine_host` spawn event, `engine_resolution`
resolution-complete event) landed in 4H, so 4I's J9 row was test-only. New test
`j9_resolution_before_spawn_zero_load` in `crates/quarto-core/tests/integration/echo_engine_e2e.rs`
(deno-gated, no Julia needed — echo's static `Primary(echo)` claim is the
zero-load surface). Renders a single-doc echo fixture through the real
`render_to_file` path under an `OrderedCapture` tracing layer (records event
targets IN ORDER, so ordering — not just count — is assertable) and checks:

- exactly one `engine_resolution` event (a MISSING event fails outright,
  rather than letting the ordering comparison vacuously pass);
- exactly one `engine_host` spawn event;
- the spawn's index is strictly greater than the resolution event's index.

**Named-revert RED/GREEN (verbatim, proof-only — no diff left in
`ts_engine.rs` after):** temporarily replaced the static early-answer branch
in `claims_language` (`if let Some(map) = &self.claims`, `ts_engine.rs:~601`)
with an always-`None` match, forcing every `claims_language` call through the
dynamic wire path (which requires `ensure_loaded`, i.e. a spawn, before it can
answer). RED:

```
engine_host spawn (index 1) must order AFTER engine_resolution
resolution-complete (index 2) ... all captured targets in order:
["quarto_core::render_to_file", "engine_host", "engine_resolution", ...]
```

— the spawn fired at index 1, resolution-complete at index 2: exactly the
predicted seam break (a dynamic-claims engine spawns during resolution, not
after it). Reverted the temp edit (`git diff` on `ts_engine.rs` came back
empty); re-ran GREEN:

```
PASS [ 0.387s] (1/1) quarto-core::integration echo_engine_e2e::j9_resolution_before_spawn_zero_load
```

Commit `62d7dadf6`. Verified: `cargo nextest run -p quarto-core` (2624
passed, including the frozen J1-J6 rows and the new J9 row).

### V-4 — Julia multi-page Pass-1/ordering run (manual evidence)

Built `q2` (`cargo build --bin q2`) and rendered a temp copy of the committed
`julia-website` fixture (the two `.qmd` pages + `_quarto.yml`, with the
`julia-engine` extension and its notebook `Project.toml`/`Manifest.toml`
copied in from the sibling `julia-engine` fixture, mirroring
`setup_julia_website_project`'s runtime assembly):

```
$ RUST_LOG=engine_host=info,engine_resolution=info \
    ./target/debug/q2 render /tmp/v4-julia-website
Rendering project: /private/tmp/v4-julia-website (type: website)
Rendered 2 of 2 files to /private/tmp/v4-julia-website/_site
```

stdout (tracing):

```
INFO engine_resolution: engine resolution complete engine_count=0   # index.qmd (no engine)
INFO engine_resolution: engine resolution complete engine_count=1   # plot.qmd (julia)
INFO engine_host: engine-host spawned pid=17206                     # exactly ONE spawn…
INFO engine_host: Running [1/1] at line 27:  ENV["GKSwstype"] = "100"  # …at the first Julia execute
```

Confirms: exactly one `engine-host spawned` line (`grep -c "engine-host
spawned"` → 1); it orders strictly after BOTH `engine resolution complete`
lines; and the very next `engine_host`-target line is the child's own
execute-time stderr forward (`Running [1/1] at line 27...`, the first line of
`plot.qmd`'s cell), i.e. the spawn happens at first execute, not during
resolution. Output inspected: `_site/index.html`, `_site/plot.html`, and
`_site/plot_files/figure-html/cell-2-output-1.png` all present on disk.

No new orphan processes attributable to this render were left behind beyond
the pre-existing pool of leaked `startup.jl` QNR workers already tracked
under **bd-l9jhy5u0** (§11 4E addendum — the J4 error path's missing
try/finally close; this render's cell did not error, so it isn't a new
instance of that leak, but the pre-existing pool from earlier sessions'
error-path runs was not touched/cleaned as part of 4I, which is test-only
per the plan).

## 13. Phase 4J — Julia through `q2 preview` (V-7, manual evidence only)

First real-engine validation of 1c-R5's native capture → splice `q2 preview`
path. No frozen test is added — the registry-read hunks stay bound by the
echo/P2-14 seam; this is recorded evidence per the plan's V-7 row.

**Build.** `cargo build --bin q2` at HEAD (`d3fc71291`). The task's staleness
note applies: the embedded SPA/WASM bundle was **not** rebuilt (no
`npm run build:wasm` / `build-q2-preview-spa` run), so the browser-rendered
DOM was not independently verified. That only affects the browser-side
rendering of non-engine chrome — the engine capture path is native Rust and
was exercised and inspected directly (see below), which is a stronger, more
direct check than a screenshot of stale-SPA-rendered HTML would have been.

**Setup.** Temp copy of the committed fixture (`_extensions/`, `_quarto.yml`,
`Project.toml`, `Manifest.toml`, `minimal.qmd` — `engine: julia`,
`execute: daemon: false`, one `{julia}` cell `1 + 1`) to
`/tmp/q2-preview-julia-4j.Fl0jKx/`, **not** the committed fixture tree (which
`q2 preview` would otherwise pollute with a samod store / capture cache).

**Baseline process count (before starting).** `ps aux | grep juliaup/julia-1.11.7`
→ 25 processes, all pre-existing: 24 leaked `QuartoNotebookRunner/…/startup.jl`
pool workers (bd-l9jhy5u0) with start times ranging 06:30–10:31, plus one
long-running `quartonotebookrunner.jl` daemon server (pid 9828, unrelated
project `~/docs/julia`, started 06:21). None attributable to this session.

### Invocation 1 — start preview, observe eager capture

```
$ RUST_LOG=info,engine_host=trace,engine_resolution=trace \
    cargo run --bin q2 -- preview /tmp/q2-preview-julia-4j.Fl0jKx/minimal.qmd \
    --data-dir /tmp/q2-preview-julia-4j-data.bIuNn0 --no-browser --port 0
```

Relevant stdout (tracing):

```
INFO q2::commands::preview: starting q2 preview server url=http://127.0.0.1:63651/?page=minimal.qmd
INFO q2::commands::preview: resolved preview engine policy engine_policy=Manual
INFO engine_resolution: engine resolution complete engine_count=1
INFO quarto_hub::watch: Started filesystem watcher path=/private/tmp/q2-preview-julia-4j.Fl0jKx …
INFO engine_host: engine-host spawned pid=28215
INFO engine_host: Running [1/1] at line 7:  1 + 1
INFO quarto_preview::capture_driver: recorded engine capture(s) rel_path=minimal.qmd engines=julia
INFO quarto_preview::capture_driver: recorded engine captures count=1
```

This is the **eager capture** call site (`record_eager_captures` /
`capture_driver.rs:57`) firing a real Julia execute (`Running [1/1] at line
7:  1 + 1`) — the spawn (`engine_host`) orders after `engine resolution
complete`, matching the J9/4I contract, and the capture is recorded before
the server accepts requests.

**Served page.** `curl http://127.0.0.1:63651/?page=minimal.qmd` returns
HTTP 200, but the body is the embedded SPA shell (`<script type="module"
src="./assets/main-BXP0_Ad_.js">`) — the executed Pandoc/markdown content is
synced to the browser over the automerge/samod websocket, not returned by a
plain HTTP GET, so curl cannot show the spliced HTML directly (no debug JSON
dump endpoint exists on the hub server; `/api/documents` only returns
`{path, document_id}` pairs, not content).

**Direct evidence: the Phase C.7 filesystem cache.** `record_capture_cached`
(`cache.rs`) persists each capture to `<data_dir>/captures/<sha256>.bin` — a
gzip stream of the JSON-serialized `EngineCapture`, explicitly documented as
*"identical wire format to the samod binary doc Phase C.1 writes, and to the
gzipped capture the WASM side ungzips in Phase C.4"* (`cache.rs:1-24`). This
is the exact byte-for-byte payload that would be spliced into the browser,
so reading it on disk is direct evidence of the splice content, independent
of curl/browser access:

```
$ gunzip -c /tmp/q2-preview-julia-4j-data.bIuNn0/captures/d7df3c04….bin | jq .
```

```json
[{
  "engine_name": "julia",
  "input_qmd": "---\n…\nengine: julia\nexecute:\n  daemon: false\n…\n---\n\n```{julia}\n1 + 1\n```\n",
  "result": {
    "markdown": "…\n::: {#cell-1 .cell execution_count=1}\n``` {.julia .cell-code}\n1 + 1\n```\n\n::: {.cell-output .cell-output-display execution_count=1}\n```\n2\n```\n:::\n:::\n\n\n\n",
    …
  }
}]
```

The `2` is present as `cell-output-display` — a real Julia execute, not an
inert code listing. **4J checklist item 1 (initial preview shows executed
output; capture path logged a real engine execute): confirmed.**

### Invocation 2 — live re-execute on edit

Edited the temp `minimal.qmd` cell `1 + 1` → `2 + 3` (source file edit, not
via the preview edit API). Waited for the 500ms debounce, then drove the
manual-mode re-execute endpoint (the fixture doesn't set `preview.engine`, so
the default policy is `Manual` per the `resolved preview engine policy` log
line above — no `auto`-mode re-execute fires without a POST):

```
$ curl -X POST http://127.0.0.1:63651/api/preview/re-execute \
    -H "Content-Type: application/json" -d '{"path":"minimal.qmd"}'
{"previous_capture_doc_id":"3im7R6f6XfukPvrFvfQJRrAxHHDm"}
HTTP 202
```

stdout (tracing):

```
INFO engine_resolution: engine resolution complete engine_count=1
INFO engine_host: engine-host spawned pid=28808
INFO engine_host: Running [1/1] at line 7:  2 + 3
```

This is the `re_execute.rs:309` call site — a **second, independent**
`engine_host` spawn (pid 28808, distinct from the eager capture's pid 28215;
pid 28215 had already exited by this point). New cache file appeared:

```
$ gunzip -c /tmp/…/captures/d1a28c5b….bin | jq -r '.[0].result.markdown'
…
::: {#cell-1 .cell execution_count=1}
``` {.julia .cell-code}
2 + 3
```

::: {.cell-output .cell-output-display execution_count=1}
```
5
```
:::
:::
```

`5` confirmed. **4J checklist item 2 (live re-execution result 5 through
`/api/preview/re-execute`): confirmed.**

**Note (not a bug, already documented):** each capture — eager and
re-execute — spawned its own engine-host process rather than reusing a warm
one (pid 28215 → pid 28808). This matches plan-1c R5's explicit scope
boundary: *"R5 owns only pointing the registry correctly so re-compute is
correct; on-edit re-execution latency (keeping the Deno host warm…) is
Plan 5's concern, not R5's."* Recorded here as confirming evidence of that
known, accepted limitation — not a new finding.

### Daemon behavior (checklist item 3)

The fixture sets `execute: daemon: false` at the document level. Julia's
shared/detached daemon transport files live at
`/Users/gordon/Library/Caches/quarto/julia/{julia_transport.txt,julia_server_log.txt}`
— checked before and after the whole session:

```
$ stat -f "%Sm %N" .../julia_transport.txt .../julia_server_log.txt
Jul  2 06:21:41 2026 julia_transport.txt
Jul  2 10:32:00 2026 julia_server_log.txt
```

Both timestamps **predate** the preview session start (14:51:16 UTC /
10:51 EDT) and were unchanged after both captures (checked again post
session at 10:52:45 EDT with no newer mtime) — `daemon: false` kept both
Julia executes (`1 + 1`, `2 + 3`) off the shared daemon transport entirely,
consistent with 4E's V-1 findings for `q2 render`. **Hazard note (per the
plan's ask):** this doc opts out explicitly; a real user's document
*without* `execute: daemon: false` would default to `daemon: true` — a
detached, unmanaged Julia server process with **no q2-side lifecycle
surface to stop it**, tracked as **bd-m1jeqhhz**. `q2 preview` is an
*interactive* session (edit → re-execute repeatedly), so this hazard is more
acute there than for a one-shot `q2 render`: a user previewing a
daemon-defaulted doc would accumulate/reuse a detached Julia server across
the whole preview session with no visibility into it from `q2 preview`
itself. **4J checklist item 3: confirmed** (no transport-file activity;
hazard documented).

### Cleanup (checklist item 4)

Sent `SIGINT` to the `q2 preview` process; logged a clean, graceful shutdown:

```
INFO quarto_hub::server: Received Ctrl-C, initiating graceful shutdown...
INFO quarto_hub::server: Server shutting down...
INFO quarto_hub::server: Performing final filesystem sync before shutdown...
INFO quarto_hub::server: Final filesystem sync complete synced=8 errors=0
```

Process counts after shutdown: `ps aux | grep juliaup/julia-1.11.7` → still
**25** (identical to the pre-session baseline — no new entries, no entries
missing); the two `engine_host` PIDs (28215, 28808) were already gone before
shutdown was even requested (each Deno host process appears to exit once its
one capture completes, matching the "no warm reuse" observation above); no
new non-lsp `deno` processes present either. **4J checklist item 4:
confirmed — no orphan julia/QNR/Deno processes attributable to this
session.**

### Divergence check against `q2 render`

Rendered the same (post-edit, `2 + 3`) temp doc through the CLI path for
comparison:

```
$ cargo run --bin q2 -- render /tmp/q2-preview-julia-4j.Fl0jKx/minimal.qmd
INFO engine_resolution: engine resolution complete engine_count=1
INFO engine_host: engine-host spawned pid=30161
INFO engine_host: Running [1/1] at line 7:  2 + 3
INFO q2::commands::render: Output: /private/tmp/q2-preview-julia-4j.Fl0jKx/minimal.html
Rendered 1 of 1 files to /private/tmp/q2-preview-julia-4j.Fl0jKx
```

`minimal.html`:

```html
<div class="cell-output cell-output-display" data-execution_count="1">
<div class="code-copy-outer-scaffold">
<pre class="code-with-copy"><code>5</code></pre>
```

Identical result content (`5`) to the preview-spliced capture's markdown
(`::: {.cell-output .cell-output-display} ``` 5 ``` :::`). No divergence
between `q2 render` and `q2 preview`'s captured/spliced output for this doc.
No new julia process left behind by this render either (still 25).

### Summary — 4J checklist

- [x] Initial preview shows executed output (`2`); capture path logged a
  real engine execute — confirmed via cache-file inspection (curl only
  reaches the SPA shell, documented above as the HTTP-level limitation).
- [x] Live re-execution result (`5`) through `/api/preview/re-execute` —
  confirmed via cache-file inspection + tracing.
- [x] Daemon behavior: no transport-file activity from the `daemon: false`
  session; `daemon: true`-by-default hazard (bd-m1jeqhhz) documented for
  the interactive-preview case specifically.
- [x] Cleanup: no orphan julia/QNR/Deno processes from this session (25
  before, 25 after; pre-existing bd-l9jhy5u0 pool untouched).
- [x] All invocations + snippets recorded above; no divergence found
  between preview-spliced and `q2 render` output for this doc.

No bugs found; no code changes made in this task (evidence-gathering only,
per the plan's V-7 scope).

## 14. Phase 4F — V-6, Q1 output comparison (manual evidence only)

**Q1 identity.** `quarto --version` → `99.9.9` (dev build); `which quarto` →
`/Users/gordon/bin/quarto` → symlink → `/Users/gordon/src/quarto-cli/package/dist/bin/quarto`
(a `quarto-dev-cli` checkout at `~/src/quarto-cli`). This is a real,
functioning Q1 binary — used as-is per the task's guidance (no separate
"install the julia extension into Q1" step needed beyond what upstream
`~/src/quarto-julia-engine` already ships).

**Setup (read-only w.r.t. the real repos).** Copied `~/src/quarto-julia-engine`
to a throwaway temp dir (`rsync -av --exclude='.git' --exclude='.github'
--exclude='.quarto' --exclude='tests' --exclude='example*' --exclude='.DS_Store'`,
same exclude set as §2's fixture copy) — `/tmp/q1-julia-compat.vZqKns/`.
Nothing was written to `~/src/quarto-julia-engine` itself. Added four test
docs matching the committed q2 fixtures **byte-for-byte in frontmatter/cell
content** (only the extension co-location differs — Q1 resolves
`_extensions/julia-engine/` relative to the temp project root, same as q2):

- `minimal.qmd` — identical to `crates/quarto-core/tests/fixtures/extensions/julia-engine/minimal.qmd` (J1).
- `multi-cell.qmd` — identical to the committed fixture (V-5).
- `error-doc.qmd` — identical cell body to J4's inline `ERROR_DOC` const.
- `echo-false.qmd` — identical cell body to J2's inline `ECHO_FALSE_DOC` const.

Rendered each with `quarto render <doc>.qmd` from the temp project root.

### Comparison table (semantics, not byte-for-byte HTML)

| Doc | q2 result (seam) | Q1 result (this session) | Match? |
|---|---|---|---|
| minimal.qmd (`1 + 1`) | `<div class="cell-output cell-output-display">…<code>2</code>…</div>`, source echoed (J1) | `<div class="cell-output cell-output-display" data-execution_count="1"><pre><code>2</code></pre></div>`, source echoed (`1 + 1` highlighted) | **Yes** — executed result present, source shown in both |
| multi-cell.qmd (`x=42` → `println("x is $x")`) | cell 2 stdout `x is 42` (V-5) | cell 2 stdout `x is 42` (`grep` confirms) | **Yes** — state persists across cells in both |
| error-doc.qmd (`error("this should fail gracefully")`) | render errors, message contains `Execution failed in julia:` + `this should fail gracefully`; host not wedged (subsequent render still works) (J4) | render exits 1, stderr contains `this should fail gracefully` + full Julia stacktrace; a subsequent `quarto render minimal.qmd` in the same process still succeeds (host not wedged) | **Yes** — error surfaced, non-zero exit, host not wedged, in both |
| echo-false.qmd (document-level `execute: echo: false`) | source token (`j2_hidden_source_variable`) absent, output token (`j2 output present`) present (J2) | source token absent (`grep -c` → 0), output token present (`grep -c` → 1) | **Yes** — identical semantics |
| plot.qmd (Plots.jl figure, MIME priority) | inline `data:image/png;base64,…` (not a file), traced to `displayDataMimeType`'s HTML-target `text/html`-first quirk (§10) | not re-rendered here (Plots.jl install is not cheap — see below); **verified via source comparison instead** | **Yes** (same quirk, confirmed at the source level — see below) |

### Corrected finding: the "HTML hides source by default" divergence noted in §9 is narrower than stated

§9's Failure-3 writeup says: *"a q2 HTML render currently echoes cell source
by default where Q1's HTML would hide it."* This session's live Q1 render of
`minimal.qmd` **contradicts that as a general HTML claim**: Q1's plain-HTML
output shows the source (`1 + 1`, syntax-highlighted) exactly like q2's does
— see the comparison table's first row. Reading Q1's own format-defaults
source (`~/src/quarto-cli/src/format/formats-shared.ts`,
`~/src/quarto-cli/src/format/formats.ts`,
`~/src/quarto-cli/src/format/pdf/format-pdf.ts`,
`~/src/quarto-cli/src/format/dashboard/format-dashboard.ts`) confirms why:

- `defaultFormat()` (`formats-shared.ts:197-228`, consumed by every format
  including plain `createHtmlFormat`/`createFormat("PDF", …)`) sets
  `echo: true, warning: true` — the SAME base default q2's
  `applyExecuteDefaults`/`kExecuteVisibilityDefaults` uses
  (`ts-packages/quarto-engine-host-deno/src/metadata-as-format.ts:217-224`).
- `echo: false, warning: false` is overridden **only** for
  presentation-family formats: `createHtmlPresentationFormat`
  (`formats-shared.ts:139-142`, used by revealjs), `beamerFormat`
  (`format-pdf.ts:74-89`, its own `execute: {...}` literal — plain
  `pdfFormat()`/`latexFormat()` in the same file do **not** get this
  override, only the beamer-specific function does), `powerpointFormat`
  (`formats.ts:314-329`), and `format-dashboard.ts:76-84`.

So: **for the actual documents exercised by this plan (all default/plain
HTML, no revealjs/beamer/pptx/dashboard target), there is NO echo-default
divergence between q2 and Q1** — both show source by default. The REAL,
narrower gap is that q2's `applyExecuteDefaults` is completely
format-agnostic (same `kExecuteVisibilityDefaults` regardless of target
format), so a Julia (or any TS-engine) document rendered to
revealjs/beamer/pptx/dashboard in q2 would echo source by default where Q1
hides it — because q2 has no writer-format-defaults layer at all, not
because of anything specific to HTML. Filed **bd-cymkcyaf** for this
corrected, narrower gap (see § below).

### MIME-type priority (figure inline-vs-file) — confirmed via source comparison, not a live Plots.jl re-render

Re-running `plot.qmd` through Q1 would require installing `Plots.jl` into a
fresh Julia project under the temp Q1 checkout — the same non-cheap,
network-dependent, multi-minute cold-start cost the committed q2 fixture
paid once (§10). Per the task's "test if cheap" framing, this was **not**
cheap, so instead the underlying logic was compared directly at the source
level (equivalent evidence, zero Julia install cost):

- Q1: `~/src/quarto-cli/src/core/jupyter/display-data.ts:45-106` —
  `displayDataMimeType`. For `options.toHtml`, lines 71-76 unconditionally
  `unshift` `[kApplicationJupyterWidgetState, kApplicationJupyterWidgetView,
  kApplicationJavascript, kTextHtml]` onto the front of `displayPriority`,
  regardless of `options.toMarkdown` — i.e. `text/html` always outranks
  `image/png`/`image/svg+xml` for an HTML target.
- q2: `ts-packages/quarto-api/src/jupyter/display-data.ts`'s
  `displayDataMimeType`, whose doc comment states it reproduces this exact
  "effective behavior" (unconditional front-unshift for `toHtml`) rather
  than re-deriving the duplicate-entry array Q1 builds.

**Confirmed identical** — same quirk, same priority order, by direct
source comparison of the ported function against the Q1 original. This is
why `plot.qmd` (whose `Plots.jl` object is `showable(MIME("text/html"))`)
produces an inline base64 `<img>` in q2, and would plausibly do the same in
a real Q1 render of the identical document (not independently re-verified
end-to-end here, for the cost reason above — flagged honestly, not
inferred as untested).

### Strands filed / reviewed

- **bd-cymkcyaf** (NEW, filed this session) — "TS-engine host applies
  format-agnostic execute defaults; Q1 hides echo/warning by default for
  presentation formats (revealjs/beamer/pptx/dashboard)." Corrects and
  narrows §9's original "HTML hides source" note into the real,
  evidence-scoped gap (a writer-format-defaults layer, keyed by target
  format, is genuinely missing in q2 — but the effect is confined to
  presentation-family formats, not plain HTML/PDF). Related-linked to
  `bd-uf4epv4w` (metadata string mangling — same "no
  writer-format/machine-facing layer" architecture theme) and
  `bd-m1jeqhhz` (daemon management surface — same "future TS-engine
  completeness work" bucket).
- **bd-uf4epv4w** (existing, confirmed still open, no change needed) —
  smart-typography mangling of machine-facing metadata strings. Not
  re-triggered by any V-6 document (none of the four docs pass
  dash-sequence strings through frontmatter).
- **bd-l9jhy5u0** (existing, confirmed still open) — julia-engine leaks a
  QNR worker on execute error. **Observed reproducing under Q1 itself**
  this session: after `error-doc.qmd`'s failed Q1 render, a new Julia
  worker process (`cwd` = the temp Q1 project dir, confirmed via `lsof`)
  was left running on the shared global control server, matching the exact
  shape bd-l9jhy5u0 already describes for q2 (missing try/finally around
  the oneShot `close` call in `julia-engine.ts`, `src/julia-engine.ts:742-749`
  — same TS source in both q2 and Q1, since q2's bundle is byte-identical
  per §4). This is corroborating evidence for bd-l9jhy5u0's own note that
  the bug is "same code shape upstream in Q1 — candidate upstream report,"
  not a new strand. The worker was left running (not forcibly killed, to
  avoid interfering with concurrent workloads on the shared global
  transport); it is additive to the pre-existing leaked pool already
  tracked under bd-l9jhy5u0 and does not need separate cleanup for this
  documentation-only task.
- **bd-m1jeqhhz** (existing, confirmed still open, no change needed) — no
  Q1-specific finding beyond what 4E/4J already recorded (Q1 also has no
  equivalent q2-side management surface by definition — this strand is
  q2-specific tooling debt, not a Q1-vs-q2 compat gap).
- **bd-677297ca** (existing, closed) — not applicable to V-6 (single-file
  Q1 renders here, no project/website render exercised against Q1).

No strands filed for cosmetic HTML differences (e.g. Q1's cell wrapper uses
`<div id="2" class="cell">` with Pandoc's numeric auto-id vs q2's
`<div id="cell-1" class="cell">`; Q1 syntax-highlights the Julia source with
`sourceCode julia` spans, q2's highlighter output differs in class/span
detail) — these are pre-existing, independently-tracked HTML-writer/syntax-
highlighting behaviors, not TS-engine-extension-specific gaps, and out of
this task's scope per the plan's explicit "semantics, not byte-for-byte
HTML" instruction.

### 4F checklist (ticked)

- [x] Run same test documents through Quarto 1 for comparison — done, see
  above (`~/bin/quarto` 99.9.9, temp copy of `~/src/quarto-julia-engine`).
- [x] Document output differences — see comparison table + corrected-finding
  write-up above.
- [x] Verify all existing q2 tests pass (`cargo nextest run --workspace`) —
  **already green at HEAD `1a44b4e2e`** per session facts (verify #2 green
  2026-07-02, exit 0); not re-run per instructions.
- [x] Run `cargo xtask verify` for full validation — **already green at
  HEAD `1a44b4e2e`** per session facts (verify #2 green 2026-07-02, exit 0);
  not re-run per instructions.
- [x] File issues (via `braid create`) for any gaps discovered — bd-cymkcyaf
  filed (see above); no other new gaps found.

**V-6 (plan's row): DONE, not "not-runnable-on-this-machine"** — a real Q1
binary was available and exercised end-to-end for all four documents.

## 15. Engine-side fixes — `julia-engine.ts` is NO LONGER zero-changes (bd-h4rhohhy)

**Supersedes §4/§5's byte-identity claim.** q2's `preview` engine-capture path
exercises the engine harder and longer than one-shot `render` and surfaced two
**pre-existing engine defects** (present in Q1's copy of this engine too). They
were fixed **upstream** on a new local branch `q2-close-busy-fix` off `main`
(NOT pushed — mirrors the marimo `q2-bare-sql-interop` precedent; Gordon's call
to raise the upstream PR). The q2 fixture
(`crates/quarto-core/tests/fixtures/extensions/julia-engine/`) was rebundled
from that branch. **These are engine bug fixes, not q2 adaptations** — they
apply equally to Q1.

**New fixture bundle:** `julia-engine.js` MD5 `82bff64cc5d060cb48983945060a6932`,
45323 bytes (was `d9d5120eb94b187903a43fb500e65eea`, 44512 bytes). The q2
`build-ts-extension` (temp-symlink workaround per §4/§8) and Q1's
`quarto call build-ts-extension` **still produce byte-identical bundles** from
the fixed source — the byte-identity *property* survives; only the specific
hash changed.

### Bug A — oneShot close/busy discarded captures (`Q-PREVIEW-CAP-1`)

`executeJulia`'s pre-run (`:703-718`) and post-run (`:742-749`) closes had zero
busy handling. When a prior client vanished mid-run (EPIPE) and left the shared
server's worker orphaned-busy, a fresh oneShot render's `close` failed with the
bare QNR `"worker is busy"` protocol error and the whole capture was discarded.

**Decision gate (QNR force-close surface):** the QNR socket protocol **does**
expose a forceful close — `ServerCommand` includes `{ type: "forceclose" }`
(`julia-engine.ts:774`, response `forceclose: { status: true }` `:783`), and the
CLI `closeWorker(file, force)` already sends `type: force ? "forceclose" :
"close"` (`:1100-1106`). Live-confirmed by the upstream smoke test
`force-closing a running worker` (GREEN). So the ratified decision rule's **YES
branch = recovery** applies.

**Fix** (extracted into a new pure `src/worker-close.ts` so the logic is
unit-testable without a socket):
- **Pre-run close (PC2):** on a busy `close`, fall back to `forceclose` to
  reclaim the abandoned worker; a *non-busy* close error still propagates
  (not swallowed).
- **Post-run close (PC1):** the run already succeeded, so a failed cleanup
  close is non-fatal — warn (`quarto.console.warning`) and return the results.

**oneShot-reuse design question (documented, NOT gold-plated — for the upstream
PR).** The force-close recovery is correct for an **abandoned** worker. It would
*also* force-close a worker legitimately busy serving a **live concurrent**
render on a shared server, killing that work. `executeJulia` cannot distinguish
abandoned-from-live from the `"worker is busy"` signal alone. The deeper
question this raises: **should a oneShot (`daemon: false`) render reuse a
daemon-started server at all?** `startOrReuseJuliaServer` (`:330-448`) reuses any
existing transport file regardless of `oneShot`. A cleaner design might give
oneShot renders their own ephemeral server (no shared-worker contention, no
force-close-vs-live ambiguity). Left as an explicit upstream-PR discussion item;
the force-close recovery is the minimal fix for the reported (abandoned) case.

### Bug C (engine-side root) — detached server inherited the host's stdout fd

`start_quartonotebookrunner_detached.jl` ran `run(detach(cmd), wait = false)`
with no stdio redirection, so the detached QNR server inherited the launcher's
(and thus the Deno engine-host's) stdout/stderr. The server's early output
(Julia startup/precompile banners, before `quartonotebookrunner.jl` installs its
own `redirect_stdout` pipe) could land on the engine-host's JSON protocol
channel, where a single non-JSON line is a framing error that kills the whole
host and discards every in-flight capture (q2-side reader escalation is a
separate task; see the plan's P1c). **Fix:** redirect the detached child's
stdout/stderr to `devnull` — `run(pipeline(detach(cmd), stdout = devnull,
stderr = devnull), wait = false)`. `quartonotebookrunner.jl` still writes its own
log to `logfile` via its internal pipe, so no server-log diagnostics are lost.
(The Windows path spawns QNR via PowerShell `Start-Process -WindowStyle Hidden`,
which does not inherit the parent's stdio, so it is unaffected and left as-is.)

### Upstream diff summary (`git -C ~/src/quarto-julia-engine diff main..q2-close-busy-fix`)

```
 _extensions/julia-engine/start_quartonotebookrunner_detached.jl |  10 +-   (devnull redirect)
 _extensions/julia-engine/julia-engine.js                        |  +54/-20 (rebundled)
 src/julia-engine.ts                                             |  40 +/-  (call worker-close helpers)
 src/worker-close.ts                                             |  68 ++    (NEW — pure close orchestration)
 tests/unit/julia-engine/worker-close.test.ts                    | 134 ++    (NEW — PC1/PC2 deno unit tests)
 tests/run-tests.{sh,ps1}                                        |   2 each  (discover tests/unit/)
```

**Forward-note:** this diff is bundled from upstream commit `e00b7f2`. The
`q2-close-busy-fix` branch tip has since moved to `c27c88f`, which adds a
third fix (`errorRunClose` — closes the oneShot worker on a *failed* run,
fixing a worker-process leak) on top of the two above; it is intentionally
NOT bundled into this fixture (separate scope, tracked as bd-l9jhy5u0).

### Testing

- **Upstream deno suite** (`tests/run-tests.sh`): 10 passed (existing smoke
  tests + 7 PC1/PC2 unit tests), 0 failed. RED→GREEN proven for PC1/PC2 (the
  pre-fix extraction propagated the busy error; the fix resolves it). The 7th
  test (review follow-up) binds the forceclose-itself-fails contract: a failed
  forced close propagates unchanged (fail-on-revert proven). **This test + its
  contract comment did NOT change the bundle bytes** — `deno bundle` strips
  comments and the test is not bundled, so `julia-engine.js` stays `82bff64…`.
- **q2 fixture (PC4a, live julia):** `pc4a_abandoned_worker_close_busy`'s frozen
  assertion flipped to the YES-branch (fresh `record_capture` **succeeds** with a
  real capture). RED against the pre-fix bundle (`d9d5120…` → `Err … "worker is
  busy"`), GREEN against the rebundled fixture (`82bff64…` → recovers via
  forceclose, capture contains `cell-output`).
- **q2 regression:** `cargo nextest run -p quarto-core` 2633 passed (incl. live
  `j1..j6` julia renders against the rebundled fixture — no render regression);
  `-p quarto-preview` 87 passed.

### Known harness concern (not a product defect)

The PC4a live harness isolates via a temp `HOME`, but on macOS the julia
runtime/transport dir resolves to `QUARTO_JULIA_PROJECT` (the shared, real
`~/Library/Caches/quarto/julia`), so the transport file is **not** actually
isolated — live runs spawn QNR servers on the shared transport and the
temp-HOME-reading cleanup guard misses them. This session's leaked servers were
killed by PID and the stale transport entry removed (user's server pid 9828
untouched throughout), but the harness needs a real runtime-dir override (or an
explicit skip) before it can be run safely/unattended. Same latent leak already
tracked as bd-l9jhy5u0; the isolation gap is worth a dedicated follow-up.
