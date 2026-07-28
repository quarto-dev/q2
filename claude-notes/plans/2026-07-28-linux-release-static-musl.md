# Switch linux release targets to static musl (bd-dofxhzaj)

**Date:** 2026-07-28
**Braid:** bd-dofxhzaj (task, P1, filed 2026-06-13 by Carlos while cutting v0.1.1)
**Branch:** `main` @ `581e45c0` (invoked in the primary checkout; no worktree created — see *Where this should land* below)
**Status:** Design settled with Carlos on 2026-07-28 (see **Decisions**). Ready to implement from Phase 1. Phase 0 is done.

## Triage verdict

**Ready to design → design done.** The blocker named in the strand is genuinely
gone, the two risks flagged in the original spike (`openssl-sys`, `aws-lc-sys`)
both check out as musl-supported on static inspection, and the artifact-name
contract with `install.sh` is unchanged — but nothing here can be *proven* from
a mac, so the plan's centre of gravity is a **CI dry-run on a branch**, not a
code change.

## Issue context

> The release workflow builds linux on `*-unknown-linux-gnu` (ubuntu-22.04,
> glibc 2.35 floor) because rusty_v8 had no musl prebuilts. rusty_v8/deno_core
> have since been removed from q2 (bd-3e3sam51), so static musl is now viable:
> one artifact per arch covering every distro incl. Alpine, no glibc floor.
> Remaining consideration is openssl/aws-lc (vendorable / musl-buildable).
> Work: flip the linux matrix targets in `.github/workflows/release.yml` to
> `x86_64/aarch64-unknown-linux-musl`, restore musl-tools install, confirm
> vendored-openssl + aws-lc build static, drop the glibc-floor note from release
> notes.

Type `task`, priority 1, filed 2026-06-13, six weeks old (`open` at
investigation time; moved to `in_progress` by this investigation). Nothing in
the strand has gone stale: the code shape it describes is exactly what is on
`main` today.

## Dependency graph

Thin but informative — one edge, and it is the important one.

- **`discovered-from` → bd-3e3sam51** (*closed* 2026-06-13, "Chase down
  deno_core/rusty_v8 dependency"). This is the strand that **removed the
  blocker**. Its close reason is unambiguous: deno_core/rusty_v8 fully removed
  from the workspace, with a guard test `test_no_v8_in_workspace_lockfile` to
  prevent reintroduction. Item (4) of its own work list was literally "revisit
  static-musl linux release targets", and its close reason says that revisit was
  filed as `bd-h7s7bsbk`.
- **`bd-h7s7bsbk` is a confirmed duplicate, and it is still open.** "Revisit
  static-musl Linux release targets now that rusty_v8 is gone", task, P2, filed
  2026-06-13T01:10, also `discovered-from: bd-3e3sam51`. Same body: flip the two
  targets, openssl vendorable + aws-lc musl-buildable, "validate with a release
  dry-run before merging; this touches CI release workflows, so **test on a
  branch with `workflow_dispatch`**". bd-dofxhzaj was filed ~16 hours later
  during the v0.1.1 cut, apparently without noticing it. There is no edge
  between them. See Q0. (Note that bd-h7s7bsbk independently reaches the same
  conclusion as Phase 1 below about how to dry-run — that is corroboration, not
  coincidence.)
- **No incoming `blocks`.** Nothing is waiting on this. No urgency pressure;
  the gnu artifacts work fine today for glibc ≥ 2.35 users.
- **Sibling context (not an edge): bd-yomgkxoc** ("Release v0.1.1 + author
  release runbook", closed 2026-06-13). Its comment `c-v8woesfa` records that
  the *stale rusty_v8/musl prose* in `release.yml` + the runbook was corrected
  during the v0.1.1 cut, and that this strand was filed for the actual switch.
  So the docs already say "musl is now viable, we just haven't switched" —
  this strand is the switch.

## What the code looks like today

Everything the strand points at still exists, unchanged since 2026-06-12.

### The thing to flip

`.github/workflows/release.yml:350-361` — the two linux matrix legs:

```yaml
- platform: linux_amd64
  target: x86_64-unknown-linux-gnu
  os: ubuntu-22.04
  ext: tar.gz
  keyring: linux-x64-gnu,linux-x64-musl
  cargo_flags: --features vendored-openssl
- platform: linux_arm64
  target: aarch64-unknown-linux-gnu
  os: ubuntu-22.04-arm
  ...
```

The gnu fallback landed in `6080bd7a` (PR #280). That commit is a clean,
self-contained revert target: it flipped the two targets, moved the runners
from `ubuntu-latest`/`ubuntu-24.04-arm` to `ubuntu-22.04`/`ubuntu-22.04-arm`,
**dropped the `Install musl-tools` step**, replaced the musl-scoped
`[target.'cfg(target_env = "musl")']` openssl dep with the `vendored-openssl`
cargo feature + `cargo_flags` matrix field, and rewrote the release-notes
platform table. Reading `git show 6080bd7a` is the single best orientation for
this work; roughly, we are un-doing its *target* changes while **keeping** its
`vendored-openssl` feature (which is strictly better than the musl-scoped dep
table it replaced, and is target-agnostic).

### Prior art: the S2 spike

`claude-notes/plans/2026-06-12-q2-github-releases-bundled-mcp.md` §D4 + the
**S2 outcome (2026-06-12)** block is the original musl analysis. It named two
risks and one longer-term alternative. All three are re-checked below.

### Risk 1 — `openssl-sys`: confirmed, already solved

`cargo tree -p quarto -i openssl-sys` (gnu target) shows exactly one root:

```
openssl-sys → native-tls → tokio-tungstenite / tungstenite
                            ├── quarto-hub-provider
                            └── samod (quarto-dev fork, branch access-policy)
```

`crates/quarto/Cargo.toml:68` already provides
`vendored-openssl = ["dep:openssl-sys", "openssl-sys/vendored"]`, enabled per-leg
via `cargo_flags`. `openssl-src 300.6.1+3.6.3` builds fine against musl-gcc.
**No change needed** — the feature is target-agnostic and carries over verbatim.

Worth noting for Q3 below: the *reason* we need openssl at all is that our own
samod fork hard-wires it —
`samod/Cargo.toml:13`: `tungstenite = [..., "tungstenite/native-tls", "tokio-tungstenite/native-tls"]`
— and `crates/quarto-hub-provider/Cargo.toml:48` mirrors it. Meanwhile
`rustls` + `aws-lc-rs` is *already* in the tree via reqwest. So q2 links **two**
TLS stacks today.

### Risk 2 — `aws-lc-sys`: looks resolved since the spike

The S2 note called this "historically finicky". Inspecting the vendored crate
source at `aws-lc-sys 0.40.0` (the version in `Cargo.lock`):

- **Pregenerated bindings exist for both musl targets** —
  `src/x86_64_unknown_linux_musl_crypto.rs` and
  `src/aarch64_unknown_linux_musl_crypto.rs` are shipped in the crate. No
  `bindgen`/libclang needed on either leg.
- **`builder/cc_builder/` has `linux_x86_64.rs` and `linux_aarch64.rs`** — the
  collected cc-builder configs are libc-agnostic, so the no-cmake path is
  available for both arches.

This is a meaningfully better picture than 2026-06 assumed. It is still *static*
evidence: whether the build actually selects the cc path (vs. cmake) and whether
cmake needs an explicit `CC=musl-gcc` can only be settled by running it.

### Risk 3 (new, not in the original spike) — nothing that would break at runtime

Checked, all clean:

- **No `dlopen`/`libloading` anywhere in `crates/`** — nothing that a static
  binary would fail to load.
- **No explicit `thread::Builder` / `stack_size` / `RUST_MIN_STACK` usage** —
  so no code relying on a hand-set stack. (Rust's std sets a 2 MiB default on
  spawned threads explicitly, so musl's small 128 KB pthread default does not
  apply; the classic musl stack trap does not bite here.)
- Remaining C-building deps (`mlua`/`lua-src`, tree-sitter grammars, `ring`)
  were already assessed musl-clean in S2 and nothing has changed.

The one thing genuinely *not* de-risked by inspection is **musl's allocator
performance**. q2 is allocation-heavy (parsing, AST traversal), and musl's
mallocng is materially slower than glibc's under concurrent allocation. This is
the risk most likely to show up as "the release binary is slower than my dev
build" rather than as a build failure. **Explicitly accepted** — see D4.

### The artifact contract is unaffected

`install.sh:169-181` (`detect_platform`) is **os × arch only** — it does not
sniff libc:

```sh
case "$(uname -s)" in Linux*) os="linux" ;; ... esac
case "$(uname -m)" in x86_64|amd64) arch="amd64" ;; aarch64|arm64) arch="arm64" ;; ... esac
printf '%s_%s\n' "$os" "$arch"
```

So a musl-only switch keeps `q2-<ver>-linux_amd64.tar.gz` / `linux_arm64` byte-
for-byte identical in *name*, and `install.sh` + its test
(`crates/quarto/tests/integration/bootstrap_sh.rs`) need **zero changes**.
(This is precisely what makes the musl-only option cheap and the
ship-both option expensive — see D2, where musl-only was chosen.)

Also checked: `README.md` and `docs/` never mention the glibc floor, so the
prose to update is confined to `release.yml` (header comment + matrix comment +
release-notes table) and `claude-notes/instructions/release-runbook.md:203-211`.
`install.sh:307` already lists `apk add minisign` for Alpine.

### The dry-run problem

`release.yml` cannot be used as-is for a spike:

- it is gated `if: github.repository == 'quarto-dev/q2'`;
- `preflight` requires an **existing tag** whose name equals the workspace
  `Cargo.toml` version;
- the build step **hard-fails** when the `QUARTO_HUB_BUNDLED_*` secrets are
  empty;
- a green run *publishes a GitHub Release*.

So validating musl means either burning a throwaway tag or standing up a
separate, cheap, branch-triggered spike workflow. **Decided: spike workflow**
(D1) — this was the main design decision in the strand.

## Decisions (settled with Carlos, 2026-07-28)

All six open questions are answered. Recorded here so the phases below read as
a real plan rather than a menu.

- **D0 — bd-h7s7bsbk is a duplicate; bd-dofxhzaj survives.** ✅ *Done.*
  bd-h7s7bsbk closed with a `duplicates` → bd-dofxhzaj edge, close reason
  pointing at this plan.
- **D1 — Dry-run on a throwaway branch**, not a real tag. A temporary spike
  workflow, deleted before the PR merges.
- **D2 — musl only.** No gnu artifact. Rationale (Carlos): the goal is *"a good,
  universally runnable binary"*; users who need a specific libc can build from
  source (`install.sh --from-source` already exists for exactly this). This
  keeps `install.sh` and `bootstrap_sh.rs` untouched.
- **D3 — Do not drop openssl here.** ✅ *Filed as `bd-r7s13dfb`* ("Unify on
  rustls: drop native-tls/openssl from the q2 dependency tree",
  `discovered-from: bd-dofxhzaj`). It needs a change to the samod fork, which is
  a different blast radius. `vendored-openssl` stays exactly as-is in this work.
- **D4 — musl allocator performance: accepted, not measured.** Carlos: *"We
  don't care about perf right now, not without a demonstrated pathological case
  from a real scenario."* So **no benchmark phase and no `mimalloc` change**. If
  a real slowdown shows up in real use, that becomes its own strand with an
  actual repro attached — which is the right trigger for an allocator decision
  anyway.
- **D5 — Upgrade the runners to `ubuntu-latest`** (and `ubuntu-24.04-arm`) as
  part of this change. The jammy pin existed *only* to set the glibc floor; with
  a static binary the runner's glibc is irrelevant, so the pin has no remaining
  justification.

## Phases

Branch: `braid/bd-dofxhzaj-switch-linux-release-targets` (off `main` @ `581e45c0`;
carries the two plan commits, so local `main` was reset back to `origin/main` —
everything lands via PR).

### Phase 0 — Reconcile with bd-h7s7bsbk ✅

- [x] Close bd-h7s7bsbk with a `duplicates` → bd-dofxhzaj edge (D0)
- [x] File the rustls follow-up as `bd-r7s13dfb` (D3)
- [x] Record all six decisions in this plan

### Phase 1 — Branch-only musl spike

The phase that answers the aws-lc/openssl question for real. Throwaway
workflow, no secrets, no publish, deleted before the PR merges (D1).

- [x] Confirm `-p quarto` builds from a payload-less checkout — all three
      `include_dir!` sites fall back to a placeholder dir with only a
      `cargo:warning`, unconditionally (read
      `crates/quarto-preview/build.rs:24-28` and the mirrored
      `quarto-{mcp-launcher,trace-server}/build.rs`). So the spike does not
      need the web-payloads job, and those payloads are target-independent
      anyway — they cannot affect whether musl links.
- [x] Write `.github/workflows/musl-spike.yml`: push on
      `feature|braid/bd-dofxhzaj-**` + `workflow_dispatch`; matrix of
      `x86_64-unknown-linux-musl` on `ubuntu-latest` and
      `aarch64-unknown-linux-musl` on `ubuntu-24.04-arm`; `fail-fast: false`
      so each arch reports independently; `if: github.repository ==
      'quarto-dev/q2'` so forks don't burn minutes
- [x] Steps per leg: `rustup target add` (the E0463 pinned-nightly trap
      release.yml documents), `apt-get install -y musl-tools`, rust-cache,
      `cargo build --release --locked --target <musl> -p quarto --features
      vendored-openssl`
- [x] Assert the binary is genuinely static — `file` must report
      `static-pie linked` or `statically linked` (Rust's musl targets default
      to `crt-static` and emit static-pie); `ldd` logged informationally only,
      since it exits non-zero on a static binary and must not fail the step
- [x] Assert `./q2 --version` runs on the runner
- [x] `actionlint` clean; YAML parses; heredoc verified locally to emit the
      fixture with no leading indentation
- [ ] Push, run, iterate to green — record the outcome in **Phase 1 outcome**
      below (including the aws-lc build path actually taken)

### Phase 2 — Functional smoke on the musl artifact

Correctness only; per D4 there is no timing comparison. **Phases 1 and 2 share
one workflow run** — the steps are strictly ordered, so a build failure
short-circuits before the smoke steps and attribution stays unambiguous, while
`rust-cache` keeps re-runs cheap.

- [x] Author the fixture + assertions. The fixture exercises what is most
      likely to be libc-sensitive: YAML front matter, the tree-sitter grammars
      (C), the HTML writer, and `grass` (the pure-Rust SCSS compiler — no
      dart-sass subprocess on native, so nothing external to install).
      Assertions verified against real local output: the title `<h1>`, a
      `<strong>`, an `<a href>`, and `styles.css` > 100 KB (observed 318,718
      bytes — a good canary that the whole SCSS pipeline ran rather than
      emitting a stub).
- [ ] Render a real `.qmd` fixture with the musl binary on the runner and
      inspect the output (not just the exit code)
- [ ] Run the same binary inside an `alpine:latest` container — the "covers
      every distro incl. Alpine" claim is the whole point of the strand, so
      leaving it untested would leave the headline benefit unverified. Alpine
      has musl and *no glibc at all*, so this doubles as the strongest
      staticness proof.
- [ ] Record both in **Phase 2 outcome** below

### Phase 3 — Flip `release.yml`

Written while the spike was still running, since the edits are fully
determined by D2/D3/D5 and do not depend on *what* the spike finds — only on
it being green. If the spike comes back red, these get amended (e.g. with a
`CC=musl-gcc` or `cmake` step) rather than discarded.

- [x] Two linux matrix legs → `x86_64-unknown-linux-musl` /
      `aarch64-unknown-linux-musl`
- [x] Runners → `ubuntu-latest` / `ubuntu-24.04-arm` (D5)
- [x] Restore the `Install musl-tools` step, `if: contains(matrix.target, 'musl')`
      — exactly as `6080bd7a` removed it
- [x] Keep `--features vendored-openssl` in `cargo_flags` (D3)
- [x] `actionlint` clean
- [ ] Confirmed by a green spike run (gating item — see Phase 1)

### Phase 4 — Prose

- [x] `release.yml` header note — replaced the "musl is viable, we just haven't
      switched" paragraph with what is actually true, keeping the rusty_v8
      history as parenthetical context and adding the `bd-r7s13dfb` pointer for
      why `vendored-openssl` is still there
- [x] `release.yml` matrix comment — dropped the glibc-floor note; **added an
      explicit warning not to "fix" the keyring lists**, which stay libc-plural
      on purpose (the addon must match the user's *node*, not how q2 was
      linked — an Alpine user runs a musl node regardless)
- [x] `release.yml` release-notes table — "glibc 2.35+" → "static, any distro",
      plus a short paragraph telling users the linux binaries are static musl
      and pointing anyone who wants a dynamic build at `install.sh
      --from-source` (verified that flag exists: `install.sh:109,146`)
- [x] `release-runbook.md` — rewrote the linux bullet; added a bullet recording
      that the runner image no longer sets a compatibility floor, so nobody
      re-pins it later for a reason that isn't glibc; corrected the signing
      bullet, whose stated rationale ("jammy has no minisign") stopped being
      true for the linux legs
- [x] `release-runbook.md` §6 — the first post-switch release gets one extra
      post-publish check: run the *published* artifact under `alpine:latest`.
      CI proves it runs on the runner, not on the distros the switch is for.
      Explicitly noted as one-time, not per-release ceremony.
- [x] Marked D4 of the 2026-06-12 release plan **SUPERSEDED** with a pointer
      here, keeping the historical reasoning intact rather than rewriting it
- [ ] Add a runbook gotcha capturing whatever Phase 1 actually learned
      (pending the spike result)

### Phase 5 — Land and ship

- [ ] Delete `musl-spike.yml` (D1 — it is scaffolding, not a deliverable)
- [ ] `cargo xtask verify --skip-hub-build` green
- [ ] PR, review, merge
- [ ] Exercised for real by the next version cut (out of scope for this strand,
      but the runbook note above is what makes it safe)

No `cargo nextest` test is written for any of this — the change is entirely in
CI config. The TDD analogue is Phase 1: prove the build succeeds in CI *before*
touching the release path, and only then edit `release.yml`.

## Phase outcomes

_Filled in as each phase completes — the durable record of what CI actually
did, per the project's end-to-end verification rule._

### Phase 1 outcome

_pending_

### Phase 2 outcome

_pending_

## Remaining open questions

None on design — all six are settled in **Decisions** above. The one thing
still genuinely unknown is empirical and only CI can answer it: **does
`aws-lc-sys` actually build on both musl legs?** That is what Phase 1 is for.

## Risks / tradeoffs

- **The one unfalsifiable-from-here risk is aws-lc-sys on musl.** Static
  inspection is encouraging (pregenerated bindings + cc-builder configs for both
  musl arches) but the failure mode, if it comes, is a CI build error that only
  appears on the runner. Phase 1 exists exactly to surface it early and cheaply.
  If it does bite, the fallback ladder is: set `CC=musl-gcc` / install `cmake`
  → force `AWS_LC_SYS_CMAKE_BUILDER` off → worst case, **`bd-r7s13dfb`
  (rustls unification) is promoted from follow-up to prerequisite**, since
  dropping native-tls also changes which crypto provider has to build.
- **This change is only really verified by a real release.** Everything before
  Phase 5 is a proxy. The runbook note in Phase 4 should say so plainly.
- **Low blast radius if it goes wrong.** Reverting is the same one-line matrix
  flip in the other direction, and `install.sh` is untouched either way — so a
  bad musl artifact is fixable by a patch release, not by an installer
  migration. This is a good argument for just doing it rather than
  over-engineering the dry-run.
- **Dropping the gnu artifact is user-visible, even though it is strictly more
  compatible.** Anyone who was specifically fetching a dynamically-linked q2
  (a distro packager, someone `LD_PRELOAD`-ing something) loses that option
  silently — the filename does not change. Cheap mitigation: say so explicitly
  in the first post-switch release notes, and point at
  `install.sh --from-source` (D2's stated escape hatch) rather than leaving
  people to discover it.
- **Nothing depends on this strand** (no incoming `blocks`), so it can be
  scheduled whenever. Its value is user-facing reach (Alpine, old-glibc distros,
  containers) rather than unblocking internal work.

## Where this should land

`/investigate-beads` was invoked in the **primary checkout on `main`**, so this
plan commits there. The implementation itself — which is CI-config-only but
wants a throwaway workflow file pushed to a branch to trigger it — would be
better off on its own branch (`braid/bd-dofxhzaj-linux-release-static-musl`).
Recommend setting that up before Phase 1; not doing it unilaterally.
