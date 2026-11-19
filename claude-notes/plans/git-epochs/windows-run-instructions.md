---
title: 'GE-15 and GE-16 Windows runs: getting started'
date: 2026-11-19
description: 'What to do on the Windows machine for the git epochs Rust, hub, TypeScript, and extension items: get the branch, run one script, and send the results back through the branch. Also what each item proves and the failures to ignore.'
---

**Plans:** [GE-15](ge-15-windows-run-a.md) (items `a0` to `a6`, run A) and [GE-16](ge-16-windows-run-b.md) (items `b1` to
`b7`, run B; section 9). This page and the scripts under `scripts/git-epochs-windows/` exist only on
the temporary branch `windows/git-epochs`; the branch never lands on `feature/git-epochs`, and only the plan\'s "Notes
from the run" stays.

## 1. One-time setup on the Windows machine

You need Git for Windows, Rust (`rustup`, MSVC host), `cargo-nextest`, `uv`, NASM and CMake (the `aws-lc-sys` crates
that `quarto-hub` pulls in compile C), and the Visual Studio C++ Build Tools. The script checks all of this first and
prints the install command for anything missing, so you can also run it and read the top of its output.

```powershell
winget install Git.Git
winget install Rustlang.Rustup
rustup default stable-x86_64-pc-windows-msvc
cargo install cargo-nextest --locked
winget install astral-sh.uv
winget install NASM.NASM
winget install Kitware.CMake
git config --global core.longpaths true
```

Node at the major in `.nvmrc` is optional for run A (a few `quarto` tests use it and skip without it) and required for
run B (section 9).

## 2. Get the branch

The macOS agent pushes `windows/git-epochs` to `origin` after Gordon says go. Clone with `core.autocrlf=true` (the Git
for Windows default, and the setting item `a3` needs):

```powershell
git clone -c core.autocrlf=true https://github.com/quarto-dev/q2.git
cd q2
git switch -c windows/git-epochs origin/windows/git-epochs
```

If you already have a clone, run `git fetch origin windows/git-epochs` and `git switch windows/git-epochs`, then
`git pull origin windows/git-epochs`. The script prints a loud warning when your checkout is not the current tip of the
remote branch, because two earlier runs of another plan ran against a stale base.

## 3. Run it

From the repository root, in Windows PowerShell 5.1 or PowerShell 7:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\git-epochs-windows\run.ps1 -WithTools -Publish
```

`-WithTools` downloads pandoc and typst (the versions CI pins) as zips under `scripts\git-epochs-windows\results\tools`
and puts them on `PATH` for the run; without them, tests that need those tools fail in item `a6`. `-Publish` commits and
pushes the results at the end. Other switches: `-Only a1,a5` runs just those items, `-SkipBuild` skips the up-front
build.

Expected cost (estimates; nothing here has been run on Windows): a cold build of the four crates and their test
binaries is the long part, plausibly 15 to 30 minutes and 10 to 20 GB under `target\`. Item `a2` creates a 4,000-file
repository under `results\large-tree-<run>` (a few hundred megabytes at most) and switches it six times. Items are
independent, so a failure never stops the rest.

## 4. Send the results back

The script writes, under `scripts\git-epochs-windows\results\`:

- `gea-<time>-<host>.json`, the machine-readable file the macOS side parses;
- `gea-<time>-<host>.md`, a short summary with the output tail of every failure;
- `logs\`, full logs (not committed).

Either rerun with `-Publish`, or do it by hand (the script prints the exact commands):

```powershell
git add scripts/git-epochs-windows/results/*.json scripts/git-epochs-windows/results/*.md
git commit -m "GE-15: Windows results"
git push origin HEAD:windows/git-epochs
```

Then tell the macOS session "results are pushed". If pushing is awkward, paste the `.md` summary into the session
instead; the `.json` is what records the items, so attach or paste that too.

## 5. What each item checks

| Item | Catalog | How it is tested | What a PASS proves | What it does not prove |
|---|---|---|---|---|
| `a1` | W1 | `probe_real_repo::` | `rev-parse --git-path` output with a drive letter (`C:/...`) and from a linked worktree parses; `ls-tree -r -z` with a project prefix returns the right paths; symlink entries (mode 120000) are filtered; `cat-file --batch` frames binary content; reflog subjects parse. | Any Git build other than Git for Windows, or colocated jj. |
| `a2` | W2 | `large_tree::` against a repository from `make_large_tree.py` | The gate reports closed for the real markers git creates during six switches of a 4,000-file tree, and releases afterward. | Antivirus or OneDrive interplay, or cold-cache timings. |
| `a3` | W3 | `classify::` and `arrival::`, including the CRLF-checkout cases | A CRLF-checked-out file compares equal to its blob after normalization, for text; binaries compare by bytes. | `.gitattributes` filters (LFS, `ident`). The script marks a caveat when `core.autocrlf` is not `true`. |
| `a4` | W4 | `registry::` and the hub\'s `path_keys::` | Registry keys use `/`; a case pair is flagged as a collision; drive-letter and UNC forms are rejected; a path over 260 characters round-trips through disk. | NTFS 8.3 short names or Unicode normalization. |
| `a5` | W5 | the hub\'s `git_epochs::` integration tests with real temp repos | The gate inside `sync_all` holds with a real repo; the checkpoint is written before the pointer flips; the injected-`PermissionDenied` test skips and never deletes. | A real sharing violation (needs a second process holding the file), network filesystems, Dev Drive. |
| `a6` | W10 | `cargo clippy ... -D warnings`, then `cargo nextest run`, for `quarto-git-epochs`, `quarto-hub`, `quarto-preview`, `quarto` | The epic\'s own gate is green on the Windows host target. | Tests needing pandoc or typst unless `-WithTools` was given. |

## 6. Known failures to ignore

Two `quarto` Julia engine tests (`julia_plain_render_is_presented_from_a_record` and `typst_plain_and_recorded_agree`)
failed intermittently on macOS during GE-4 with `QuartoNotebookRunner` raising `ArgumentError: startpath must be
non-empty`; they passed in the GE-15 baseline. If they fail in `a6-nextest` on Windows, leave them in the results and do
not investigate. Anything else that fails is a finding: it goes into the plan\'s notes with its output, and the macOS
session proposes a fix plan. Product code is not changed in this plan.

## 7. Explaining the failures

If the run reports failures, run this once from the repository root. It reads the logs the run already wrote, so it does
not rerun any test:

```powershell
uv run scripts/git-epochs-windows/extract_failures.py
git add scripts/git-epochs-windows/results
git commit -m "GE-15: failure details"
git push origin HEAD:windows/git-epochs
```

It writes `results/failure-details-<run>.md`: the tools on `PATH`, every failing test grouped by its first panic
message, a failure count per test module, and the full output of the failing tests of items `a1` to `a5`. Add
`--show NAME` to include the full output of any failing test whose name contains `NAME`.

## 8. Refreshing the branch

When the macOS side changes the tests or the scripts, it merges `feature/git-epochs` into `windows/git-epochs` and
pushes; it never rebases a pushed branch. Pull with `git pull origin windows/git-epochs`.

## 9. Run B: the TypeScript and extension items (GE-16)

Run B uses the same script and the same branch. It checks only the new TypeScript package
(`ts-packages/quarto-git-epochs`) and the VS Code extension\'s vscode-free core (`vscode-sync-experiment/`). It needs no
Rust toolchain, NASM, or CMake: when you pass only `b` items, the script requires just Git for Windows, Node, and npm.

**Setup beyond section 1.**

- **Node at the `.nvmrc` major (currently 24).** `npm ci` refuses other majors (`engine-strict`). Use `fnm` or `nvm`
  (`winget install Schniz.fnm`, then `fnm use`); do not rely on a Node that a package manager may relink. The script
  prints a warning when the major differs.
- **A clean clone** made with `core.autocrlf=true` and `core.longpaths=true`, as in section 2
  (`git config --global core.longpaths true` before cloning). The script prints both values and marks a caveat when
  `core.autocrlf` is not `true`.
- **Optional: `jj`** (`winget install jj-vcs.jj`). Without it the jj scenarios of `b1` skip, which is not a failure.
- **Optional: `-SkipBuild`** only matters when you run the `a` items as well and `target\` is already built.

**Run it.** From the repository root:

```powershell
git pull origin windows/git-epochs
powershell -ExecutionPolicy Bypass -File scripts\git-epochs-windows\run.ps1 -Only b1,b2,b3,b4,b5,b6,b7 -Publish
```

`b7` runs first and does a clean `npm ci` at the root; the other items stand on it. Expect a few minutes for the install
and the builds and about a minute for each vitest item. If `node_modules\` already exists and you pass no `b7`, the
script reuses it. The results file is the same shape as run A, so section 4 and the extractor of section 7 apply. The
extractor reads nextest logs only; for a vitest failure, read the `output_tail` of the item in the `.json` (it holds the
last 60 lines of the failing step) or the full log under `results\logs\`.

**What each item checks.**

| Item | Catalog | How it is tested | What a PASS proves | What it does not prove |
|---|---|---|---|---|
| `b1` | W6 | `npm test -w @quarto/quarto-git-epochs` with `git.exe` on `PATH` | The probe parses Git for Windows output (`C:/...` from `--git-path`, `ls-tree -z` quoting, reflog); the controller sees real `index.lock` and `rebase-merge`; the shared scenario step lists run on real repositories in `%TEMP%`. | Other Git builds; jj on Windows unless installed. |
| `b2` | W4 | the package\'s `paths` and `registry` tests, the extension\'s `safe-path` and `ignore` tests | `/` keys, case collisions, drive letters and UNC prefixes, nested `.quarto-hub` and `.git` segments, the `safeJoin` NTFS forms (`.git.`, trailing space, `.git::$INDEX_ALLOCATION`, `GIT~1` where the volume has short names), a directory junction outside the root, a path over 260 characters. | Filesystems other than NTFS; Dev Drive or OneDrive. The `GIT~1` row skips when short names are off. |
| `b3` | W3 | the package\'s `arrival`, `classify`, `paths`, and `scenarios` tests (including the `autocrlf-checkout` scenario) and the extension\'s clone `arrival` and origination `dirty` tests | A CRLF checkout does not look dirty; comparisons normalize as the macOS tests assume. | `.gitattributes` filters; `autocrlf=input`. |
| `b4` | none | the extension core and the headless peer | The engine runs on Windows: the injected watcher, writes to disk, rebind deleting the Automerge store after shutdown (no `EBUSY`), the atomic `git-epochs.json` write retrying `EPERM` or `EBUSY`. | VS Code\'s own file watcher; a real scanner holding the file. |
| `b5` | W7 | `join` and `join-proxy` tests against a stub `q2.cmd` | The ticket in argv, a free port, the readiness poll, stdin close ending the stub, tree kill on Leave, stderr surfaced, no ticket in any log; `.cmd` spawned through a shell. | The iroh tunnel across real networks. |
| `b6` | none | the headless owner and publish tests and the origination tests, on an in-process hub | Publish order and resume on NTFS paths; the share link only after the heads are confirmed; a second peer reads every file. | An out-of-process `q2 hub`; the public server; very large uploads. |
| `b7` | none | `npm ci`, then typecheck and build of the extension and build of every `ts-packages` workspace | The extension joined the workspaces cleanly and its bundle builds. | `vsce package`; installing a `.vsix`; the hub-client and preview-SPA builds. |

**Optional, by hand.** If the network allows, run a real `q2 preview --share` host and a `q2 preview --join` guest on
this machine and write down what happened. It is not scripted.

**Known failures to ignore.** None at the start of run B. Anything that fails is a finding.
