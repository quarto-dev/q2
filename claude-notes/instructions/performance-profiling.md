# Performance profiling workflow

A playbook for diagnosing and fixing performance hotspots in this repo.
Written after the first such session (bd-h5l7, April 2026). Intended to
be used on every subsequent perf issue — edit it as we learn.

## Core principle: native proxy first, browser last

The Quarto Hub profile target is **WASM in a browser**, but the browser
is a terrible place to iterate on a fix:

- Every iteration requires a rebuild of WASM + reload of the hub.
- Chrome sampling noise, compositor jitter, and cache state are uncontrolled.
- Before/after numbers are hard to compare — you need many runs to see through the noise.
- Instrumentation printouts inside WASM have to round-trip through
  `console.log` and are interleaved with Automerge/React/IndexedDB work.

A **native proxy** running the same code path in the `pampa` binary
removes all of that. You get deterministic timings, stderr output
lands where you want it, and `cargo build` finishes in seconds instead
of minutes. Every fix should be validated *natively* first, and only
cross-checked in the browser at the end.

The tradeoff: not every hub-client hotspot reproduces natively (e.g.
rendering-pipeline vs. React commit vs. Automerge work). If a native
proxy doesn't reproduce the hotspot, **stop and figure out why** rather
than iterating in the browser — you've probably misidentified what the
profile is really telling you.

## Step-by-step workflow

### 1. Identify the native entry point

Given a Chrome flamegraph showing function `F` as a hotspot:

1. Find `F` in `crates/`. The Chrome symbol name usually includes the
   full Rust path (e.g. `quarto_source_map::source_info::SourceInfo as core::cmp::PartialEq::eq`).
2. Trace the call chain upward. The flamegraph shows the stack; map it
   to source in this repo. For hub-client, the WASM entry is
   `parse_qmd_to_ast` (`crates/wasm-quarto-hub-client/src/lib.rs:757`),
   which calls through `quarto_core::pipeline` into `pampa::writers::*`.
3. Decide which binary exercises the same path. For parse/render
   hotspots, **`cargo run --bin pampa -- <fixture>.qmd -t json`** is
   usually enough — it runs the same writer used by the hub-client's
   `pandoc_to_json` WASM entry.

### 2. Pick a representative fixture, then scale it

A single fixture doesn't tell you anything about scaling. Build a
geometric series:

```bash
mkdir -p /tmp/q2-intern-bench
cp <user's representative document>.qmd /tmp/q2-intern-bench/1x.qmd
for n in 2 4 8 16; do
  python3 -c "import sys; sys.stdout.write(open('/tmp/q2-intern-bench/1x.qmd').read() * $n)" \
    > /tmp/q2-intern-bench/${n}x.qmd
done
```

This lets you distinguish O(n) from O(n log n) from O(n²) empirically.
A single wall-time number cannot.

### 3. Instrument before you fix

Do **not** start designing a fix before you have numbers. Add
lightweight env-gated counters:

```rust
struct HotStruct {
    // ... existing fields ...
    stat_calls: usize,
    stat_some_inner_metric: usize,
}

impl Drop for HotStruct {
    fn drop(&mut self) {
        if std::env::var_os("QUARTO_PERF_STATS").is_some_and(|v| v == "1") {
            eprintln!(
                "perf.<your-gauge-name> calls={} inner={}",
                self.stat_calls, self.stat_some_inner_metric,
            );
        }
    }
}
```

Rules of thumb:

- **Counters are `usize` fields on the hot struct.** Free when the env
  var is unset. Don't use `AtomicUsize` unless there's real concurrency
  in the struct.
- **Gate *printing* on an env var, not the *counting*.** Counting is
  cheap; conditional branches on env vars are expensive.
- **One env var for all perf gauges: `QUARTO_PERF_STATS=1`.** Every
  instrumentation site in the repo checks the same variable so you can
  turn all perf output on with a single toggle. Distinguish gauges by
  *output prefix*, not by env var — a dotted namespace like
  `perf.intern`, `perf.<next>`, etc., lets you `grep '^perf.intern'` to
  isolate one gauge's output when multiple are active at once.
- **Keep the printout machine-readable.** Space-separated `key=value`
  after the prefix so it's easy to grep / parse in a table. The bd-h5l7
  instrumentation (`perf.intern`) was left in place after the fix —
  counters stay, they're cheap, and they model the pattern for the next
  perf session.

Run across the scaled fixtures:

```bash
for n in 1 2 4 8 16; do
  echo "=== ${n}x ==="
  /usr/bin/time -p sh -c \
    "QUARTO_PERF_STATS=1 target/release/pampa /tmp/q2-intern-bench/${n}x.qmd \
     -t json --json-source-location full > /dev/null" 2>&1
done
```

Always use **release** builds for timing. `cargo build -p <crate> --release`.

For profile-based investigations (samply, perf, flamegraph), you need
debug symbols on top of release-level optimization. The workspace
defines a `release-perf` profile that inherits from `release` with
`debug = true` / `strip = false`:

```bash
cargo build --profile=release-perf -p <crate>
# Binary lives at target/release-perf/<binary>
```

Profiling against plain `--release` gives you a forest of raw
addresses with no symbols — don't do it.

### samply workflow

```bash
# Record with presymbolication so a sidecar .syms.json is produced
# alongside the profile. The sidecar lets you analyze offline without
# launching the samply web UI.
samply record -s -n --unstable-presymbolicate \
  -o /tmp/q2-perf-profiles/<name>.json.gz -- \
  target/release-perf/<binary> <args>
```

`-s` saves only (no web server), `-n` skips opening the browser,
`--unstable-presymbolicate` emits `.syms.json` next to the profile.
Default sampling rate is 1000 Hz — good enough for most runs.

For offline self-time analysis, resolve the profile's address strings
through the syms sidecar. The repo ships an analyzer for this at
`crates/perf-harness/scripts/analyze_profile.py`:

```bash
./crates/perf-harness/scripts/analyze_profile.py \
  /tmp/q2-perf-profiles/<name>.json.gz --top 30
```

stdlib-only Python, reads the profile + sidecar `.syms.json`, emits a
top-N self-time table. The head of the script documents both the
profile format and the samply sidecar structure — if samply's format
changes, patch there.

### Where native drivers live

`crates/perf-harness/` — an internal (non-published, `publish = false`)
crate that hosts native driver binaries mirroring hub-client entry
points for profiling. Add a new binary per hot entry point as they
come up. Prefer this over adding subcommands to the user-facing
`quarto` CLI, because these drivers should not appear as user
features.

### Profiling hub-client TypeScript code

When the hotspot lives in `hub-client/src/` (attribution, presence,
render pipeline, etc.) rather than in Rust, the playbook still applies
but the toolchain is different. Use Node's built-in CPU profiler on a
standalone driver — **not vitest**.

**Two gotchas that will eat your session if you don't know them:**

1. `NODE_OPTIONS=--cpu-prof npm run bench` appears to work but only
   profiles the npm and vitest orchestration processes. Vitest's fork
   pool drops the flag, so the actual worker writes no profile. Don't
   waste time trying to plumb `execArgv` through `poolOptions.forks`
   either — the result is still empty. Run the driver directly with
   `node --cpu-prof` and skip vitest entirely.
2. Any production code that yields via `requestIdleCallback` will show
   ~99 % `(idle)` in the profile because Node falls back to
   `setTimeout(0)` per yield. Override it at the top of the driver
   before importing the code under test:

   ```js
   globalThis.requestIdleCallback = (cb) => {
     cb({ didTimeout: false, timeRemaining: () => 50 });
     return 0;
   };
   ```

   Seen in attribution profiling (2026-04-23): vitest reported ~1 s per
   1M-char build, actual CPU was 15 ms — the other 985 ms was
   `setTimeout(0)` round trips. Profiling without the override
   measures the scheduler, not your code.

**Standalone driver template.** Place under `/tmp/<target>-perf/` or
similar — these are throwaway per-session, like the Rust native-proxy
fixtures. The pieces:

- **Shim files** for any external packages you need to stub. Each shim
  is a normal ESM module that exports the subset of the API the code
  under test imports, with hooks the driver controls:

  ```js
  // shim-someDep.mjs
  let impl = () => [];
  export function setImpl(fn) { impl = fn; }
  export function someApiFn(...args) { return impl(...args); }
  ```

- **Resolve hook** that redirects the package specifier to the shim:

  ```js
  // resolve-hook.mjs
  import { fileURLToPath } from 'node:url';
  import { dirname, join } from 'node:path';
  const here = dirname(fileURLToPath(import.meta.url));
  const shim = 'file://' + join(here, 'shim-someDep.mjs');
  export async function resolve(specifier, context, next) {
    if (specifier === '@scope/some-dep') {
      return { url: shim, shortCircuit: true, format: 'module' };
    }
    return next(specifier, context);
  }
  ```

- **Register bootstrap** that installs the hook on startup:

  ```js
  // register.mjs
  import { register } from 'node:module';
  register('./resolve-hook.mjs', import.meta.url);
  ```

- **Driver** that dynamically imports the code under test (it has to
  be dynamic, not static, so the register hook is in place first):

  ```js
  // driver.mjs
  globalThis.requestIdleCallback = (cb) => { cb({ didTimeout: false, timeRemaining: () => 50 }); return 0; };
  const mod = await import(new URL('../../hub-client/src/services/<module>.ts', import.meta.url).href);
  const shim = await import('./shim-someDep.mjs');
  shim.setImpl(/* mock that returns workload patches */);
  // ... call mod.hotFunction() in a timed loop over scaled fixtures
  ```

**Invocation:**

```bash
node \
  --cpu-prof --cpu-prof-dir=/tmp/q2-ts-perf --cpu-prof-interval=200 \
  --enable-source-maps \
  --import tsx/esm \
  --import /path/to/register.mjs \
  /path/to/driver.mjs
```

`tsx/esm` (already hoisted at repo-root `node_modules/`) is what lets
Node load `.ts` files directly. `--cpu-prof-interval=200` (µs) is a
finer default than Node's 1 ms — useful because the actual work after
removing the yield overhead is often only tens of ms per iteration.

**Analysis:** `hub-client/scripts/perf/analyze-cpuprofile.mjs` is the
TS counterpart of `crates/perf-harness/scripts/analyze_profile.py`.
Reads a `.cpuprofile`, prints a top-N self-time table and
bucketed-by-origin totals. Add `--include <substring>` once per code
area of interest:

```bash
node hub-client/scripts/perf/analyze-cpuprofile.mjs \
  /tmp/q2-ts-perf/CPU.*.0.001.cpuprofile --top 30 \
  --include /src/services/<module>
```

**Caveats:**

- Node writes one `.cpuprofile` per process/thread. Main-thread output
  ends in `.0.001.cpuprofile`; tsx's loader worker writes a `.1.002`
  sibling you can ignore.
- Frame line numbers often show as `:1` because V8's profile doesn't
  pick up tsx's source maps. Function names are accurate; for precise
  line-level hotspots, pre-transpile to `.js` with inline maps and
  profile that.
- This is a *native proxy* in the Rust-playbook sense, with the same
  limitation: anything you shim (e.g. `@automerge/automerge`'s `diff`)
  disappears from the profile. Design your workload accordingly — if
  the real bottleneck is inside the shimmed dep, you'll need a
  browser profile to see it.

### 4. Read the numbers before designing the fix

Put the numbers in a table with expected shapes next to them. If you
suspect O(n²), compute `n*(n-1)/2` in the table and check the ratio —
it's the quickest way to confirm or reject the shape. In bd-h5l7 the
measured `eq_comparisons` matched `n(n-1)/2` to the digit at every
scale, which made the diagnosis conclusive.

Look at the cache hit counters, not just the expensive ones. bd-h5l7's
biggest finding was that the *existing* caches had 0% hit rate — the
fix wasn't "make the caches faster," it was "delete the caches that
aren't paying rent."

Document findings in `claude-notes/plans/YYYY-MM-DD-<topic>.md` under a
"Findings" section *before* proposing a fix. Tables, command lines, and
the counter output verbatim. The plan is both artifact and audit trail.

### 5. Design the fix against the data

Once you know the shape, the fix often gets smaller than initial
intuition. Resist the urge to add a fancier data structure if the
simpler answer is "this code path is doing unnecessary work." Before
committing to an approach:

- Audit who depends on the current behavior. bd-h5l7's audit found no
  consumer depended on pool-ID canonicality, which made "just delete
  the dedup" the right answer instead of "add a hash-keyed dedup."
- If you're removing an invariant, grep the repo for consumers that
  might rely on it (see bd-h5l7's audit of `.s` field usage across
  `hub-client/src/`, `crates/`, and the readers).
- Break the fix into small phases, each independently verifiable
  against the test suite. bd-h5l7 was A (restructure cache) → B (delete
  content_map) → C (delete precomputation), each leaving the repo
  green before the next.

### 6. Verify structural equivalence for serialization refactors

If your fix changes pool IDs, snapshot numbering, or any "surface"
representation, snapshot tests will flag every affected file. The risk:
surface diff overwhelms the signal on "did the *meaning* change?"

The technique that worked for bd-h5l7: **write a canonicalizer** that
resolves pool references and compares semantics. Template:

```python
# /tmp/check_snap_diffs.py
import json
from pathlib import Path

SNAP_DIR = Path("<your snapshots dir>")
SIMPLE_ID_FIELDS = {"s", "key_source", "citationIdS"}  # repo-specific

def resolve_pool_entry(pool, id_val):
    """Walk parent refs until you bottom out at structural data."""
    entry = pool[id_val]
    # recursively resolve any ID-typed fields inside entry
    # ...
    return entry

def canonicalize(doc):
    pool = doc["astContext"]["sourceInfoPool"]
    # walk the whole doc, replacing id-carrying fields with resolved content
    # return the canonicalized doc

for newp in sorted(SNAP_DIR.glob("*.snap.new")):
    oldp = newp.with_suffix("")
    if canonicalize(parse(oldp)) == canonicalize(parse(newp)):
        print(f"OK  {newp.name}")
    else:
        print(f"STRUCTURAL {newp.name}")
```

If every diff canonicalizes to identity, the refactor is
representation-only. Accept the snapshots. If any diff is genuinely
structural, investigate before accepting.

See `/tmp/check_snap_diffs2.py` from bd-h5l7 for a working version.

### 7. Run `cargo xtask verify` before declaring the native fix done

Not just `cargo nextest run --workspace` — also the hub-client WASM
build and its vitest. See `CLAUDE.md` → "Full Project Verification"
for why: WASM is a separate compilation target and `cargo build
--workspace` doesn't cover it.

```bash
cargo xtask verify
```

Only after this passes is the native side of the work complete.

### 8. Cross-validate in the browser

Last step. Build the hub-client, load the original document, take a
fresh Chrome performance profile under the same conditions as the
original capture, and confirm the hotspot is gone:

```bash
cd hub-client
npm run build:all
npm run dev
# ... then in the browser: open the fixture, record a new profile,
#     compare to the profile that triggered the investigation.
```

Treat the browser number as a **sanity check** on the native result,
not the primary signal. If the native fix is clearly better but the
browser profile is ambiguous, trust the native measurement — the
browser is too noisy to be the final arbiter. If they disagree in
direction, something is wrong: either the native proxy isn't actually
exercising the same path, or there's a second hotspot you haven't
spotted.

## What not to do

- **Don't pipe `cargo nextest run` through `tail`** — it hangs. Project
  convention, see `CLAUDE.md`.
- **Don't iterate on fixes in the browser.** You'll burn hours and
  learn nothing stable.
- **Don't skip the scaled-fixture step** because "the file is already
  representative." Scaling confirms *complexity class*, which is what
  actually matters. A 2× improvement on one fixture could be a constant
  factor; a ratio-of-ratios across scales tells you if you fixed the
  right thing.
- **Don't remove diagnostic counters after the fix lands** unless they
  have real cost. Leave them behind an env var — the next perf session
  will want them.

## Case study

`claude-notes/plans/2026-04-22-sourceinfo-eq-hotspot.md` — bd-h5l7.
Full worked example: Chrome profile → native proxy → scaling
empirically confirmed O(n²) → history audit → consumer audit →
three-phase fix → workspace verify → browser confirmation. Read that
plan end-to-end before starting the next perf session.
