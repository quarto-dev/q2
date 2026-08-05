# CI gate: released linux binaries must run with no glibc (bd-3b47pxmm)

**Date:** 2026-08-05
**Braid:** bd-3b47pxmm (task, P2, filed 2026-08-05 by Carlos while cutting v0.11.0)
**Branch:** `braid/bd-3b47pxmm-ci-assert-released-linux`, off `main` @ `c6ab84c2`
**Related:** bd-dofxhzaj (the musl switch this check guards), plan
`claude-notes/plans/2026-07-28-linux-release-static-musl.md`

## Triage verdict

**Ready.** The strand describes the change precisely, the risky unknown
(does `docker run alpine:latest` work on *both* linux runner images?) is
already answered — the bd-dofxhzaj spike ran exactly that on
`ubuntu-latest` and `ubuntu-24.04-arm` and both came back green — and the
exact shell snippet to reuse is recoverable from git history
(`git show fe016a4b:.github/workflows/musl-spike.yml`).

## Issue context

`release-runbook.md` §6 asks the maintainer to hand-run the published linux
artifact under `alpine:latest` after the first post-musl release. In practice
that check gets skipped: it needs Docker (not installed on the maintainer's
machine) and on Apple Silicon it would only ever exercise `linux_arm64`, so the
amd64 artifact would go unchecked either way. A manual step that reliably gets
skipped is worse than no step — it reads as coverage that does not exist.

The release workflow already has a per-target **Verify binary** gate that runs
the freshly built binary on the runner. That gate cannot catch an accidental
dynamic link against glibc, because the runner *has* glibc. Alpine has musl and
no glibc at all, so a dynamically-linked binary cannot even exec there. That is
the assertion worth automating, and it belongs next to the existing gate.

## Why this is not paranoia

Nothing in the build pipeline pins staticness. `crt-static` is a *default* of
the `*-unknown-linux-musl` targets, and defaults are exactly the kind of thing a
future `RUSTFLAGS`, `.cargo/config.toml`, `-C target-feature=-crt-static`, or a
build-script-injected link flag can silently flip. When that happens the binary
still builds, still passes every existing gate (it runs fine on the runner), and
ships — reaching Alpine and old-glibc users as a broken artifact. The published
promise ("static, any distro") would be false with no signal anywhere in CI.

## Design

### D1 — In the build matrix, not a post-release job

The strand offers both. The build matrix wins:

- It fails **before publishing**. A post-release job that downloads published
  artifacts can only tell you that you already shipped something broken.
- Each linux leg already runs on its own arch's runner (`ubuntu-latest` /
  `ubuntu-24.04-arm`), so each can run its own arch's container **natively** —
  no qemu, no `--platform`, no binfmt setup.
- No artifact download, no extra job, no cross-job plumbing. It is one gated
  step next to the gate it complements.

### D2 — Reuse the bd-dofxhzaj spike invocation verbatim

The docker invocation, the bind-mount shape, and the both-spellings `file`
assertion were all proven green on both arches in the spike (run recorded in the
2026-07-28 plan, "Phase 2 outcome"). Reusing it verbatim rather than reinventing
it removes the only real risk in this change.

The three gotchas the spike paid for, all preserved here:

- `file` says **`static-pie linked`** on x86_64 and plain **`statically
  linked`** on aarch64. Matching one spelling passes on one arch and fails on
  the other; the check must accept both.
- `ldd` reports *not a dynamic executable* on a static binary but **exits
  non-zero**, so it can only be logged, never asserted on.
- Docker is present on both runner images (proven, not assumed).

### D3 — Assert `--version`, not a full render

The spike also rendered a `.qmd` fixture inside the container. That was the
right scope *there* — the question was whether musl broke the tree-sitter/SCSS
pipeline. Here the question is narrower: does the binary exec at all without
glibc? `--version` answers it, and comparing it against
`needs.preflight.outputs.version` reuses the parsing contract the existing
verify step and `install.sh` already depend on (`${RAW##* }`). A render inside
the container would add minutes to every release leg to re-answer a question
bd-dofxhzaj already answered.

`q2 mcp --launcher-info` is deliberately **not** run in the container — it
spawns Node, which `alpine:latest` does not have. That check stays on the
runner, where it already lives.

### D4 — Staticness assertion stays on the runner, before the container

Cheap, fails fast, and localizes the problem: if `file` says the binary is
dynamic, you learn that in milliseconds with a clear message instead of
decoding a container exec failure.

### D5 — Delete the manual runbook step

Once CI enforces it, §6's manual paragraph is stale advice that describes
itself as a one-time check. Replace it with a pointer to the CI gate so a future
reader knows the coverage exists and where.

## Verification strategy

This change is entirely CI config, so there is no `cargo nextest` test to write.
The verification ladder, cheapest first:

1. **Local — shell logic.** Extract the exact inline script and run it against
   fixtures: both real `file` spellings must pass, a dynamic-executable spelling
   must fail, and the `${RAW##* }` version parse must accept `q2 (quarto 2)
   0.11.0` and reject a mismatch.
2. **Local — `actionlint`** on the edited workflow.
3. **CI — the real thing.** See "Open question" below.

## Open question (for Carlos)

How much CI dry-run does this warrant before it lands? Three options, and the
tradeoff is real because **no musl release exists yet** — v0.10.0 predates the
switch, so there are no published musl artifacts to test the check against.

- **(a) Land it; v0.11.0 exercises it.** The step runs *before* packaging and
  publishing, so a bug fails the release run without shipping anything; the fix
  is a one-liner and a re-pushed tag. Cheapest, and the failure mode is
  "annoying", not "dangerous".
- **(b) Temporary spike workflow that builds q2 for musl on both arches** and
  runs the new step, deleted before merge. This is the bd-dofxhzaj precedent
  (decision D1 there). Highest confidence, ~25 min per leg.
- **(c) Temporary spike workflow that builds a trivial static musl binary**
  instead of q2 and runs the identical snippet. Proves docker presence, the
  `file` spellings, and the version parse in ~2 min per arch. Does not prove q2
  itself is static — but the spike already proved that, and the new step asserts
  it anyway.

Recommendation: **(a)**. The invocation is copied from a run that was already
green on both arches, and the blast radius of being wrong is a failed release
run that publishes nothing.

## Work items

### Phase 1 — Implement ✅

- [x] Add `Assert the binary needs no glibc (Alpine)` step to the build matrix
      in `.github/workflows/release.yml`, gated `if: contains(matrix.target,
      'musl')`, immediately after `Verify binary`
- [x] Carry the three gotcha comments (both `file` spellings, `ldd` exit code,
      docker-on-both-runners) into the step so the next reader does not
      re-derive them

### Phase 2 — Verify locally ✅

- [x] Shell-logic harness: both `file` spellings pass, dynamic spelling fails,
      version parse accepts and rejects correctly
- [x] `actionlint .github/workflows/release.yml` clean

### Phase 3 — Prose ✅

- [x] `release-runbook.md` §6 — delete the manual docker paragraph, point at the
      CI gate
- [x] `release-runbook.md` gotchas — note that the `file`-spellings gotcha now
      has a consumer in CI; added a second bullet recording that staticness is
      a *default*, not a pin, so the gate reads as load-bearing
- [x] `release.yml` header comment — mention the no-glibc gate alongside the
      existing anti-stale-embed description of the verify step

### Phase 4 — Land

- [x] `cargo xtask verify --skip-hub-build` green (14/14, exit 0)
- [ ] Resolve the open question with Carlos; run whatever dry-run he picks
- [ ] PR, review, merge
- [ ] Close bd-3b47pxmm

## Outcomes

### Phase 2 outcome — shell logic proven locally

Docker is not installed on the maintainer's machine (that is half of why this
strand exists), so the container itself cannot be exercised locally. What *can*
be, and was: the two pieces of inline shell that can be silently wrong.

The harness rebuilt the step's assertions verbatim and ran them against
fixtures — real `file` output for both musl arches, plus the two ways the check
should fail:

```
file staticness assertion:
  ok   x86_64 'static-pie linked' passes
  ok   aarch64 'statically linked' passes
  ok   'dynamically linked' fails
  ok   glibc interpreter fails
container version parse (POSIX sh):
  ok   matching version passes
  ok   stale version fails
  ok   mismatch in the other direction fails

all checks passed
```

The version-parse cases run under `sh` (not bash), because that is what
executes inside `alpine:latest` — `${RAW##* }` is POSIX, but it is worth
proving rather than assuming. `actionlint .github/workflows/release.yml` is
clean.

The harness is scaffolding, not a deliverable, and lives only in the session
scratchpad; the fixture strings above are the part worth keeping, and they are
recorded here.

**What remains unproven locally:** that `docker run alpine:latest` succeeds on
the runners. That is not a guess — the bd-dofxhzaj spike ran this exact
invocation green on both `ubuntu-latest` and `ubuntu-24.04-arm` (2026-07-28
plan, "Phase 2 outcome"), and the arm64 runner image lists Docker Client/Server
28.0.4. But it is inherited evidence, not evidence from this branch. See the
open question.

### Phase 4 note — a stale-WASM false alarm on the way through

The first `cargo xtask verify --skip-hub-build` failed with 16 `smoke-all` WASM
fixtures red (shortcode/extension contracts: `contract-env`, `contract-var`,
`q1-compat-minimal-manifest`, …). This branch's diff is `.yml` + `.md` only, so
it could not have caused them — but "could not have" is not verification.
Rebuilding with `npm run build:wasm` and re-running gave 130/130 pass: the
checked-in WASM artifact predated some extensions work. Full re-run of the gate
afterwards: **14/14, exit 0**.

Recorded because this is the same trap CLAUDE.md documents for `q2 preview` —
a stale WASM artifact reads as a code regression.
