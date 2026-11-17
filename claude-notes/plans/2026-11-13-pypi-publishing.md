---
title: 'Publish q2 releases to PyPI (bd-2p7dwmqe)'
date: 2026-11-13
description: 'Ship each q2 release on PyPI as a small sdist that downloads and checksum-verifies the platform binary at install time, published by GitHub OIDC trusted publishing straight from the release pipeline.'
status: in-progress  # 0.45.0 live on PyPI; awaiting first automatic release run
braid:
  strand: bd-2p7dwmqe
---

## Overview

We want `pip install <name>` (and `uv tool install`, `pipx install`) to
give users the `q2` binary of a tagged release, and we want the PyPI
upload to be part of the release procedure in
`claude-notes/instructions/release-runbook.md` rather than a separate
manual chore.

The hard constraint is size. Quarto 1 solved the same problem in
`quarto-dev/quarto-cli-pypi` by uploading only an sdist whose `setup.py`
downloads the platform archive from GitHub at install time. This plan
keeps that idea, because the numbers below make real binary wheels
unworkable at our release cadence, and hardens it: pinned checksums,
correct wheel tags for locally built wheels, no manual version bump,
and publishing from this repository with OIDC trusted publishing.

## What Quarto 1 does today

Source: `external-sources/quarto-cli-pypi` (HEAD `0af61c3`, 1.11.9) and
`external-sources/quarto-cli/dev-docs/checklist-make-a-new-quarto-release.md`
(the `GATE PyPI` item).

- **A separate repository** holding `pyproject.toml` (setuptools,
  `name = "quarto-cli"`, `dynamic = ["version"]`), a `setup.py`, a
  `version.txt`, and a tiny `quarto_cli/quarto.py` shim.
- **`setup.py` overrides `build_py`.** At build time it picks a suffix
  from `sys.platform` / `platform.machine()`, downloads
  `quarto-<version>-<suffix>` from the quarto-cli GitHub release with
  `urlretrieve`, extracts it, moves `bin/` and `share/` into the
  `quarto_cli` package, and lists every file as `package_data`.
- **The console script is a Python shim.** `quarto = quarto_cli.quarto:run`
  runs `subprocess.call([<pkg>/bin/quarto, *argv])` (and catches
  `KeyboardInterrupt`, a fix that took its own PR).
- **Only the sdist is uploaded.** `publish.yml` runs `python -m build`,
  then `rm -rf dist/quarto_cli*.whl`. That deletion matters: the wheel
  built on the CI runner contains the *linux* binary but is tagged
  `py3-none-any`, so uploading it would hand Linux binaries to every
  platform.
- **Trusted publishing.** The job has `permissions: id-token: write` and
  uses `pypa/gh-action-pypi-publish` (pinned by SHA); there is no API
  token. A `publish-release` boolean picks TestPyPI (default) or PyPI.
- **The release step is manual.** A human edits `version.txt`, commits
  `Bump version from A to B` to `main`, then runs
  `gh workflow run 'Publish Quarto PyPi' --repo quarto-dev/quarto-cli-pypi --ref main -f publish-release=true`
  and checks pypi.org.
- **Weaknesses we should not copy:** no checksum verification of the
  download; the locally built wheel that pip caches is tagged
  `py3-none-any`, so a pip cache shared across machines (NFS home, CI
  cache) can serve the wrong platform's binary; the shim adds Python
  start-up and signal-forwarding to every invocation; it depends on
  `jupyter`, `nbclient` and `wheel`; and the version lives in a second
  place that someone has to remember to bump.

## The size constraint, in numbers

From the v0.45.0 release (2026-11-13):

| Asset | Size |
|---|---|
| `q2` binary, uncompressed (linux_arm64) | 187 MB |
| `q2-0.45.0-linux_amd64.tar.gz` | 92.3 MB |
| other five platform archives | 83.8 to 89.2 MB |

The linux_amd64 archive was 50.6 MB at v0.20.0, 81.2 MB at v0.35.0 and
92.3 MB at v0.45.0. A wheel is a deflate-compressed zip, so a wheel per
platform would be about the size of these archives.

PyPI's defaults are a 100 MB limit per file and a 10 GB limit per
project. Both can be raised by request on `github.com/pypi/support`, but:

- A per-platform wheel set is about 6 × 90 MB = 540 MB per release.
  We released 13 versions between 2026-10-15 and 2026-11-13. At that
  rate a 10 GB project fills in under two months, and PyPI files are
  never deleted unless we delete releases ourselves.
- The largest wheel is already 92 % of the per-file limit and growing.

An sdist that downloads at install time is a few kilobytes per release
and makes both limits irrelevant. That is the recommendation.

## Options considered

1. **sdist that downloads at build time (Quarto 1 pattern, hardened).**
   *Recommended.* Proven on `quarto-cli` for years; install fails loudly
   and at install time if the network is unavailable; zero storage
   pressure on PyPI. Costs: every install runs a build step (users with
   `--only-binary :all:` or `uv`\'s `--no-build` cannot install), and the
   install needs to reach `github.com`.
2. **Tiny pure-Python wheel that downloads on first run.** A
   `py3-none-any` wheel containing a launcher plus the checksum table;
   the first `q2` invocation downloads into a user cache dir. Works with
   `--only-binary`, but `pip install` succeeds offline and the failure
   moves to first use (a Docker image built with `pip install` and no
   warm-up step breaks at run time). It also needs a writable cache dir
   and a cache-eviction story. Worth keeping as a fallback if the sdist
   build step turns out to be a real problem for users.
3. **Real per-platform wheels** (the ruff/uv approach, e.g. maturin
   `bindings = "bin"` or repackaging our archives). Best user experience,
   but ruled out by the numbers above unless PyPI grants a large project
   quota and we accept pruning old releases. Revisit if the binary
   shrinks a lot.

## Design (option 1)

**Location: in this repository**, under `packaging/pypi/`, not a new
`quarto-dev/q2-pypi` repository. The publish workflow then lives next to
the release pipeline, reads the version from the tag, and can be chained
from the release automatically. Nobody edits a `version.txt`.

**Files** (as built; see `packaging/pypi/README.dev.md`):

- `packaging/pypi/pyproject.toml`: only a `[build-system]` table pointing
  at an **in-tree PEP 517 backend** (`backend-path = ["backend"]`,
  `requires = []`). This replaced the setuptools design: setuptools reads
  scripts as text (`tokenize.open`), which a native binary breaks, and its
  wheel tags needed overriding anyway. Writing the wheel directly is short,
  and installs no longer fetch any build dependency.
- `packaging/pypi/backend/q2_pypi_backend.py`: the hooks `build_sdist`,
  `build_wheel`, `prepare_metadata_for_build_wheel` (no download).
- `packaging/pypi/backend/q2_pypi_lib.py`: all logic, stdlib only: platform
  detection, archive names, version mapping, checksum pins, download with
  verification, extraction; plus the `prepare` CLI that writes
  `release.json` and copies `LICENSE`.
- `packaging/pypi/release.json`, `LICENSE`: generated, gitignored.
- `packaging/pypi/quarto2/`: `binary_path()` (reads the package's own
  RECORD), `__version__`, `python -m quarto2`.
- `packaging/pypi/README.md` (PyPI description), `README.dev.md`.
- `packaging/pypi/tests/`: `unittest`, offline (a fake release served
  through a `file://` `QUARTO2_DOWNLOAD_BASE`), including a real
  `pip install` of the sdist into a fresh venv.

**Install-time behaviour** (`build_py` override):

1. Map the host to one of our six platforms: `sys.platform` (one of
   `linux`, `darwin`, `win32`) × `platform.machine()` normalised
   (`x86_64`/`AMD64` → `amd64`, `aarch64`/`arm64`/`ARM64` → `arm64`).
   Anything else fails with a message pointing at `install.sh --from-source`.
   Linux needs no glibc check: our linux binaries are static musl.
2. Download `https://github.com/quarto-dev/q2/releases/download/<tag>/<name>`
   to a temp file (urllib honours `HTTPS_PROXY`). An environment variable
   (name to decide, e.g. `Q2_PYPI_DOWNLOAD_BASE`) can point at a mirror
   for air-gapped sites.
3. Verify SHA-256 against `release.json`; on mismatch, abort the install.
   The checksum table travels inside the sdist that PyPI serves over
   TLS, so PyPI is the trust root and we do not need minisign in Python.
4. Extract the single `q2` (or `q2.exe`) member, refusing any other
   member, link or path, and write it into the wheel under
   `quarto2-X.Y.Z.data/scripts/` with mode 0755, so pip/uv install the
   real binary straight into the environment's `bin/` (`Scripts\` on
   Windows). No Python shim, no `subprocess`, no signal forwarding.
5. Tag the wheel `py3-none-<host platform>` (from
   `sysconfig.get_platform()`), `Root-Is-Purelib: false`, so the wheel a
   frontend caches can never be reused on another platform.
6. A small importable package (`quarto2/__init__.py` or similar) exposes
   `binary_path()` and `__version__` for Python callers, and
   `python -m <pkg>` execs the binary.

**Versions.** Cargo `X.Y.Z` → PEP 440 `X.Y.Z`; `X.Y.Z-rcN` → `X.Y.ZrcN`
(the tag in `release.json` keeps the Cargo form for the URL). Nightlies
are **not** published to PyPI. A packaging-only fix to an already
published version ships as `X.Y.Z.post1` against the same tag, because
PyPI never lets a version be re-uploaded.

## Publishing workflow and release integration

New top-level workflow `.github/workflows/publish-pypi.yml`:

- `on: workflow_dispatch` with inputs `tag` (required) and `target`
  (`testpypi` | `pypi`, default `testpypi`). It must stay a top-level
  workflow under this exact file name: the PyPI trusted publisher is
  registered against it. (PyPI matches the top-level workflow file;
  reusable-workflow publishers were not supported last time I checked,
  so it cannot be a `workflow_call` job inside `release-pipeline.yml`.
  This is the same constraint `publish-npm.yml` documents.)
- Job `build` (`contents: read`): checkout `main` (the packaging code),
  download `checksums.sha256` for `tag` from the GitHub Release (fail if
  the release or any of the six platform archives is missing), generate
  `release.json`, run the unit tests, `python -m build --sdist`, and
  assert `dist/` holds exactly one `.tar.gz` under 1 MB and no wheel.
- Job `smoke` (matrix: `ubuntu-latest`, `ubuntu-24.04-arm`, `macos-latest`,
  `windows-latest`, `windows-11-arm`): `pip install dist/*.tar.gz` and
  `uv tool install dist/*.tar.gz` into fresh environments; assert
  `q2 --version` ends with the expected version. This runs against the
  real GitHub Release, so it also proves the published URLs.
- Job `publish` (`id-token: write`, `environment: pypi` or `testpypi`):
  `pypa/gh-action-pypi-publish` pinned by SHA, uploading the sdist (with
  PEP 740 attestations, which the action generates by default).

Chaining from the release: after `gh release create` succeeds in the
`release` job of `release-pipeline.yml`, and only when
`channel == 'release'` and `publish` is true, run
`gh workflow run publish-pypi.yml --ref main -f tag=$TAG -f target=pypi`
(`GITHUB_TOKEN` may trigger `workflow_dispatch`; the job needs
`actions: write`). Dry runs and nightlies never reach it.

**Gate (decided):** automatic dispatch, but the `pypi` environment has a
required reviewer, so the upload waits for an approval click for the first
few releases. Removing the reviewer later is a settings change only.

**Runbook changes** (`release-runbook.md`): a "What a release produces"
entry for PyPI; in step 6 (verify), `pipx run --spec <name>==X.Y.Z q2 --version`
or `uvx --from <name>==X.Y.Z q2 --version`; a "PyPI" subsection with the
manual retry command, the TestPyPI rehearsal command, the `.post1` rule,
and "never rebuild a version's binaries once its sdist is on PyPI" (the
pinned checksums would no longer match; today a tag is only moved before
the release is published, so this holds as long as that stays true);
the trusted-publisher row in the prerequisites table; `publish-pypi.yml`
and `packaging/pypi/` in the files table.

## Package name

`q2` on PyPI belongs to an unrelated 2018 project (a reinforcement
learning framework, version 0.1.4). As of today `quarto2`, `quarto-2`,
`quarto2-cli`, `quarto-q2` and `q2-quarto` all return 404 from the PyPI
JSON API.

Recommendation: **`quarto2`**. It is short, reads naturally next to
Quarto 1's `quarto-cli`, and the command it installs is `q2`. Usage:
`pip install quarto2`, `uv tool install quarto2`, and for one-off runs
`uvx --from quarto2 q2 render doc.qmd`. A `quarto2` command is deferred
(see Phase 0): if `quarto2` becomes the official binary name after 0.x,
the package will ship the real binary under that name then.

A 404 is not a reservation: the name is only ours once the first upload
succeeds, so the trusted-publisher setup and the first publish should
happen close together (below).

## Credentials: trusted publishing, no API tokens

We do not need a PyPI API token or any GitHub secret. PyPI's trusted
publishing lets the workflow exchange its GitHub OIDC token for a
short-lived upload token, scoped to one repository, workflow file and
environment. Steps for you (I can do the GitHub side with `gh` once you
say so):

1. **PyPI account.** Make sure 2FA is on (PyPI requires it to publish).
   TestPyPI is a separate site with a separate account; create one at
   `test.pypi.org` too, with 2FA.
2. **Pending publisher on PyPI.** pypi.org → Your account → Publishing →
   "Add a new pending publisher" → GitHub:
   - PyPI project name: `quarto2`
   - Owner: `quarto-dev`
   - Repository name: `q2`
   - Workflow name: `publish-pypi.yml`
   - Environment name: `pypi`

   A pending publisher creates the project on its first successful
   upload. It does not reserve the name, so do this right before the
   first real publish.
3. **Same on TestPyPI** with environment `testpypi`.
4. **GitHub environments** in `quarto-dev/q2` (Settings → Environments):
   `pypi` and `testpypi`. For `pypi`, restrict deployments to the `main`
   branch and (optionally) add yourself as a required reviewer.
   Equivalent `gh api` calls: `PUT repos/quarto-dev/q2/environments/pypi`.
5. **After the first publish:** add co-maintainers on the PyPI project
   (bus factor; Quarto 1's `quarto-cli` maintainers are the natural
   set), or move it under a PyPI organization if Posit/Quarto has one.
   Then the pending publisher has become a normal trusted publisher; no
   further credential maintenance is needed, and there is nothing to
   rotate.

If we ever have to fall back to a token (we should not), it would be a
project-scoped API token stored as an environment secret on `pypi`,
never a user-wide token.

## Checklist

### Phase 0: decisions (with you)

- [x] Option 1 (sdist download at install) confirmed over options 2 and 3 (2026-11-13)
- [x] Package name: `quarto2` (2026-11-13)
- [x] Install the binary directly as a wheel script; no Python shim (2026-11-13).
      The Quarto 1 shim had no reason we could find beyond habit.
- [x] Extra `quarto2` command: deferred (2026-11-13). `quarto2` may become
      the official binary name after 0.x; at that point the package ships
      the real binary under that name. Adding it now would mean either a
      Python shim (the wrapper we just ruled out) or a second 187 MB copy.
- [x] Publish chaining: automatic dispatch from the release pipeline, with
      a required reviewer on the `pypi` environment for the first few
      releases; drop the reviewer once it has run cleanly (2026-11-13)
- [x] Mirror override environment variable: `QUARTO2_DOWNLOAD_BASE` (2026-11-13)
- [x] No Jupyter dependency or extra; add one if users ask (2026-11-13)

### Phase 1: tests first

- [x] `packaging/pypi/tests/`: platform mapping (all six, plus unsupported
      hosts → error naming `--from-source`); Cargo → PEP 440 mapping
      (`X.Y.Z`, `-rcN`, `-beta.N`; nightlies rejected); `release.json` from a
      `checksums.sha256` fixture (missing platform, malformed digest, bad
      tag → error; `.postN`); checksum mismatch aborts and leaves no wheel;
      extraction rejects extra, path-escaping and symlink members; binary
      bytes survive unchanged; wheel tag never `any`; RECORD hashes; the
      metadata hook does not download; offline `pip install` of the sdist
      gives a working `q2`, `python -m quarto2` and `binary_path()`
- [x] Confirmed failing before implementation (module missing)

### Phase 2: package

- [x] Script-vs-shim spike: settled by writing the wheel in an in-tree
      backend (see Files); binary is a wheel script
- [x] Backend, lib, package module, README, README.dev.md
- [x] 26 tests green (`python3 -I -m unittest discover -s packaging/pypi/tests`)
- [x] Local end-to-end against the real v0.45.0 release (record below)

### Phase 3: workflows

- [x] `publish-pypi.yml`: build (unit tests, `prepare`, `python -m build
      --sdist`, one-file and size guard) → smoke (pip + `uv tool install`
      on ubuntu x64/arm, macOS, Windows x64/arm) → publish (OIDC,
      `pypa/gh-action-pypi-publish` pinned by SHA) → check-published
- [x] `pypi-package.yml` PR workflow (Linux/macOS/Windows, plus Python 3.9
      floor) and its `cargo xtask verify` counterpart in step 1
      (`--skip-pypi-tests`), with a test that holds the two commands in sync
- [x] Credentials 1 and 2: PyPI 2FA, TestPyPI account, pending publishers
      on both (done by Carlos, 2026-11-13)
- [x] GitHub environments `pypi` (deploys from `main` only, required
      reviewer: Carlos) and `testpypi` (no rules), created with `gh api`
      2026-11-13; PR #1201 opened
- [x] TestPyPI rehearsal for v0.45.0 (run 43492748781): all five smoke
      legs and the install-back passed; `uvx --index-url
      https://test.pypi.org/simple/ --from quarto2==0.45.0 q2 --version` on
      macOS printed `q2 (quarto 2) 0.45.0`
- [x] First real publish to PyPI (run 43493152159, approved by Carlos):
      https://pypi.org/project/quarto2/ shows 0.45.0, an 8669-byte sdist
      with a PEP 740 attestation naming `quarto-dev/q2`,
      `publish-pypi.yml`, environment `pypi`; `uvx --no-cache --from
      quarto2 q2 --version` printed `q2 (quarto 2) 0.45.0`
- [ ] Add co-maintainers on the PyPI project (Carlos, on pypi.org)

### Phase 4: release integration

- [x] `release.yml` job `pypi` dispatches `publish-pypi.yml` after a
      published release (skipped on dry runs; nightlies never reach it)
- [x] Runbook: PyPI in "What a release produces", prerequisites rows,
      step 6 check, § PyPI (approval, retry, rehearsal, `.postN`, never
      rebuild archives, prereleases, mirror), files table
- [x] README: PyPI install instructions
- [ ] Verify on the next real release end to end, record here

## End-to-end record (Phase 2, 2026-11-13)

On macOS arm64, from `packaging/pypi/`, against the published v0.45.0:

```bash
python3 -I backend/q2_pypi_lib.py prepare --tag v0.45.0 \
  --checksums $S/checksums.sha256 --license ../../LICENSE
uv build --sdist -o $S/dist .          # → quarto2-0.45.0.tar.gz, 8624 bytes
python3 -m venv venv && venv/bin/pip install --no-cache-dir $S/dist/quarto2-0.45.0.tar.gz
```

Observed (inspected): `venv/bin/q2` is a 177950240-byte `Mach-O 64-bit
executable arm64`; `q2 --version` prints `q2 (quarto 2) 0.45.0`, and so
does `python -m quarto2 --version`; `quarto2.binary_path()` returns
`venv/bin/q2`. `q2 render smoke.qmd` wrote HTML containing
`<strong>quarto2</strong> on PyPI.` and `generator" content="quarto-rust-0.45.0`.
`uv tool install` of the same sdist also gives a working `q2`, and the
wheel uv cached is tagged `py3-none-macosx_11_0_arm64`. `python -m build
--sdist` (build 1.6.1, as CI runs it) produces the same 8624-byte sdist,
and `twine check --strict` passes on it. Linux and Windows installs are
covered by the `smoke` matrix in `publish-pypi.yml`, not yet run.

## Open questions

- Should the package depend on, or offer an extra for, Jupyter
  (`quarto-cli` depends on `jupyter` and `nbclient`)? Users installing
  q2 into a Python environment are the ones most likely to run Python
  cells. My default is no dependency, documented instead.
- `q2` from `install.sh` and `q2` from pip can both be on `PATH`. Do we
  want `q2 --version` (or an install-source probe) to say which one is
  running? Probably out of scope; note it in the docs.
- If `--only-binary`/`--no-build` users complain, option 2 can ship
  alongside as a pure wheel; decide only on evidence.
