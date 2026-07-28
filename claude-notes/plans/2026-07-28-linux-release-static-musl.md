# Switch linux release targets to static musl (bd-dofxhzaj)

**Date:** 2026-07-28
**Braid:** bd-dofxhzaj (task, P1, filed 2026-06-13 by Carlos while cutting v0.1.1)
**Branch:** `main` @ `581e45c0` (invoked in the primary checkout; no worktree created — see *Where this should land* below)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The blocker named in the strand is genuinely gone, the two
risks flagged in the original spike (`openssl-sys`, `aws-lc-sys`) both check out
as musl-supported on static inspection, and the artifact-name contract with
`install.sh` is unchanged — but nothing here can be *proven* from a mac, so the
plan's centre of gravity is a **CI dry-run on a branch**, not a code change.
The design questions below are about how to run that dry-run safely and whether
to ship musl-only or musl+gnu.

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
build" rather than as a build failure. See Q4.

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
ship-both option expensive — Q2.)

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
separate, cheap, branch-triggered spike workflow. See Q1 — this is the main
design decision in the whole strand.

## Proposed phases (draft)

- **Phase 0 — Reconcile with bd-h7s7bsbk.** Confirm whether this strand
  duplicates it; close one, or link them. (Q0. Blocks nothing else, but should
  be settled before work starts so the record is clean.)
- **Phase 1 — Branch-only musl spike.** A throwaway
  `.github/workflows/musl-spike.yml`, `workflow_dispatch` + branch push,
  no secrets, no publish. Both arches: `rustup target add`, `apt-get install
  musl-tools`, `cargo build --release --locked --target <musl> -p quarto
  --features vendored-openssl`, then assert `file q2` reports *static* and
  `./q2 --version` runs on the runner. This is the phase that answers the
  aws-lc/openssl question for real. Delete the workflow at the end.
- **Phase 2 — Functional smoke on the musl artifact.** Beyond `--version`:
  render a real fixture (`q2 render`) and, ideally, run it in an
  `alpine:latest` container to prove the "covers every distro" claim that
  motivates the whole strand. Alpine is the *point* of this change — not
  testing it would leave the headline benefit unverified.
- **Phase 3 — Flip `release.yml`.** Two matrix legs → musl targets; restore the
  `Install musl-tools` step (`if: contains(matrix.target, 'musl')`, exactly as
  `6080bd7a` removed it); decide the runner images (Q5); keep `vendored-openssl`
  as-is.
- **Phase 4 — Prose.** `release.yml` header note (lines 48-55), the matrix
  comment (lines 341-349), the release-notes platform table (lines 661-662:
  "glibc 2.35+" → "static musl"), and `release-runbook.md:203-211`. Add a
  runbook gotcha capturing whatever Phase 1 learned.
- **Phase 5 — Ship it.** The switch is only truly verified by a real release
  run, so this lands and then gets exercised by the next version cut; the
  runbook note should say the first post-switch release warrants extra
  post-publish verification (download the linux artifact, run it on Alpine).

No `cargo nextest` test is written for any of this — the change is entirely in
CI config. The TDD analogue here is Phase 1: prove the build fails/succeeds in
CI *before* touching the release path. Flag if you disagree with that reading of
the project's TDD rule.

## Open design questions for the user

0. **bd-h7s7bsbk is the same strand — which id survives?** Both are open, both
   `discovered-from: bd-3e3sam51`, same body. Recommendation: keep
   **bd-dofxhzaj** (higher priority — P1 vs P2 — and its description is the more
   complete work list, naming the musl-tools restore and the release-notes
   prose), close bd-h7s7bsbk with a `duplicates` edge pointing at it. Say the
   word and I'll do it; I haven't touched either beyond marking this one
   `in_progress`.

1. **How do we dry-run?** Recommendation: a **throwaway branch-triggered spike
   workflow** (Phase 1), deleted before merge. The alternative — bumping the
   version and burning a real tag — costs a release-shaped artifact and can only
   be iterated by delete-and-re-push of the tag (the v0.1.0 dry-run loop the
   runbook warns about). Do you want the spike workflow, or would you rather do
   this the tag way to exercise the *actual* release path end to end?

2. **musl only, or musl + gnu?** The strand says "one artifact per arch". That
   is the cheap option and needs no `install.sh` change. Shipping both would
   mean 4 linux artifacts and teaching `detect_platform` to sniff libc
   (`ldd --version` / `/lib/ld-musl-*`), plus a matching change to
   `bootstrap_sh.rs`. Recommendation: **musl only** — a static binary is a
   strict superset for end users. Confirm, or is there a reason to keep a gnu
   artifact (perf? some downstream packager?).

3. **Do we also drop the second TLS stack?** q2 links both openssl (via our
   samod fork's `native-tls` wiring) and rustls/aws-lc (via reqwest). Since we
   own the samod fork, moving its `tungstenite` feature to rustls would let us
   delete `vendored-openssl` and the whole openssl build leg — the "longer-term
   sound alternative" S2 already identified. Recommendation: **out of scope
   here, file a follow-up strand.** It touches an external fork and is
   orthogonal to the musl switch. Agree?

4. **Do we care about musl allocator performance?** Static musl uses mallocng,
   which is slower than glibc's malloc on allocation-heavy multithreaded work —
   which describes q2's parser. Options: (a) accept it; (b) measure it in Phase 2
   (render a large fixture under both binaries, compare wall time); (c) pre-empt
   it by adding `mimalloc` as the global allocator (the workspace sets none
   today). Recommendation: **(b) — measure, then decide.** If it's within a few
   percent, ship; if it's a real regression, file (c) as its own strand rather
   than smuggling an allocator change into a CI-config PR.

5. **Which runner images?** With musl the runner's glibc no longer constrains
   anything, so `ubuntu-22.04`/`ubuntu-22.04-arm` can go back to
   `ubuntu-latest`/`ubuntu-24.04-arm` (what `6080bd7a` changed away from) —
   newer toolchains, and it may make the `minisign`-not-on-jammy runbook gotcha
   moot for the build legs. Recommendation: **move to latest**, since the whole
   reason for pinning jammy disappears. Any objection?

## Risks / tradeoffs (draft)

- **The one unfalsifiable-from-here risk is aws-lc-sys on musl.** Static
  inspection is encouraging (pregenerated bindings + cc-builder configs for both
  musl arches) but the failure mode, if it comes, is a CI build error that only
  appears on the runner. Phase 1 exists exactly to surface it early and cheaply.
  If it does bite, the fallback ladder is: set `CC=musl-gcc` / install `cmake`
  → force `AWS_LC_SYS_CMAKE_BUILDER` off → worst case, Q3's rustls-provider
  change becomes a prerequisite rather than a follow-up.
- **This change is only really verified by a real release.** Everything before
  Phase 5 is a proxy. The runbook note in Phase 4 should say so plainly.
- **Low blast radius if it goes wrong.** Reverting is the same one-line matrix
  flip in the other direction, and `install.sh` is untouched either way — so a
  bad musl artifact is fixable by a patch release, not by an installer
  migration. This is a good argument for just doing it rather than
  over-engineering the dry-run.
- **Nothing depends on this strand** (no incoming `blocks`), so it can be
  scheduled whenever. Its value is user-facing reach (Alpine, old-glibc distros,
  containers) rather than unblocking internal work.

## Where this should land

`/investigate-beads` was invoked in the **primary checkout on `main`**, so this
plan commits there. The implementation itself — which is CI-config-only but
wants a throwaway workflow file pushed to a branch to trigger it — would be
better off on its own branch (`braid/bd-dofxhzaj-linux-release-static-musl`).
Recommend setting that up before Phase 1; not doing it unilaterally.
