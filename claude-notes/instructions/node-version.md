# Node version: the pin, what enforces it, and how to set up your machine

The repo pins one Node major for everything npm-driven (hub-client, ts-packages,
the preview SPA, the MCP bundle). CI runs that major; local machines must too, or
tests fail in ways that look unrelated to Node.

## The pin

| Where | Value (2026-09) | Read by |
| --- | --- | --- |
| `.nvmrc` (repo root) | `24` | fnm, nvm, mise, and other version managers |
| `package.json` → `engines.node` | `^24.0.0` | npm (`engine-strict`), `cargo xtask verify`, `cargo xtask dev-setup` |
| `.github/workflows/*.yml` → `actions/setup-node` | `node-version: '24'` | CI |

These three must agree. When bumping the major, change all three in one commit
(see § Bumping the pin).

## What enforces it

A pin that nothing reads is advisory, and advisory pins get undone by the next
package-manager upgrade — that is exactly what happened here (May 2026 pin
`ca6d47c8`, undone by a `brew upgrade` on 2026-09-04, found on 2026-09-08 as 23
vitest failures; bd-lh30hlvd). Three layers now make the drift fail fast, with
the cause named:

1. **`cargo xtask verify`** runs a preflight before anything else: it reads
   `engines.node`, runs `node --version`, and **fails** on a mismatch or a
   missing `node` — before the Rust build, so the wait is seconds. On success
   it prints the version it found, so a verify log always names the toolchain.
   The check is skipped only when every npm-driven step is disabled.
   Implementation: `crates/xtask/src/node_version.rs` (npm's own range grammar
   via the `nodejs-semver` crate; an unparsable range is an error, not a pass).
2. **`.npmrc` → `engine-strict=true`** makes `npm install` / `npm ci` fail with
   `EBADENGINE` under a Node outside the range, instead of printing a warning
   that scrolls past. It applies to the whole dependency tree, so a dependency
   that declares an incompatible `engines` range also fails loudly.
3. **`cargo xtask dev-setup`** reports the Node it finds and, on a mismatch,
   prints version-manager install hints. Warn-only.

### Escape hatches (deliberate experiments only)

- `Q2_ALLOW_NODE_MISMATCH=1 cargo xtask verify …` — runs the preflight, prints
  a warning, and continues on the mismatched Node. Use it to *try* the next
  Node major; do not leave it set in a shell profile.
- `npm install --engine-strict=false` — one-off override of `.npmrc`.

## Setting up your machine

Use a version manager that reads `.nvmrc`; do not fight your OS package
manager. **fnm** is the smallest one and what this note assumes; **mise** and
**nvm** work the same way.

### macOS (zsh)

```bash
brew install fnm
# in ~/.zprofile (see the note below on why not ~/.zshenv):
eval "$(fnm env --use-on-cd --version-file-strategy=recursive)"
# then, from anywhere inside the repo:
fnm install          # installs the .nvmrc version
node --version       # v24.x
```

`--use-on-cd` switches Node whenever you `cd`, and once at shell start;
`--version-file-strategy=recursive` finds `.nvmrc` from subdirectories, so
`cd hub-client` gets the pin too. `fnm default system` keeps Homebrew's Node in
effect outside pinned projects, if you want it for other tools.

**Why `~/.zprofile`, not `~/.zshenv`.** On macOS, login shells run
`/etc/zprofile`, which calls `path_helper`. `path_helper` rebuilds `PATH` from
`/etc/paths` and `/etc/paths.d/*` (Homebrew registers `/opt/homebrew/bin`
there) and *appends* whatever was already in `PATH`. An fnm init in
`~/.zshenv` runs before that and ends up behind `/opt/homebrew/bin`, so
Homebrew's `node` wins anyway. `~/.zprofile` and `~/.zshrc` run after
`/etc/zprofile`; `~/.zprofile` covers every login shell (Terminal, iTerm, VS
Code's integrated terminal, Claude Code's tool shell).

Shells started *before* you edit the profile keep their old `PATH`. Open a new
terminal (or restart the Claude Code session) after setup.

### Linux

```bash
curl -fsSL https://fnm.vercel.app/install | bash   # or your distro's package
eval "$(fnm env --use-on-cd --version-file-strategy=recursive)"   # in ~/.bashrc / ~/.zshrc
fnm install
```

### Windows (PowerShell)

```powershell
winget install Schniz.fnm
fnm env --use-on-cd | Out-String | Invoke-Expression   # in $PROFILE
fnm install
```

## The Homebrew relink trap

Homebrew's unversioned `node` formula tracks the *current* Node release, not
the LTS. Every `brew upgrade` that touches it relinks `/opt/homebrew/bin/node`
to the new major, silently displacing a `node@24` you linked by hand
(`brew link --overwrite node@24` does not survive the next upgrade). If
something else on the machine depends on the unversioned formula (here:
`bitwarden-cli`), you cannot uninstall it either. A version manager sidesteps
all of this: Homebrew's Node stays where it is, and the repo gets the pinned
one from `~/.local/share/fnm`.

## Bumping the pin

Do it deliberately, in one PR:

1. `.nvmrc`, `engines.node` in the root `package.json`, and every
   `node-version:` in `.github/workflows/`.
2. Run `cargo xtask verify` on the new major and fix what breaks. Known item
   for **Node ≥ 25**: Node defines a `localStorage` global that vitest 4.x's
   jsdom environment refuses to overwrite, so every jsdom test touching
   `localStorage` fails. The fix is vitest ≥ 5.0.0 (allowlists
   `localStorage`/`sessionStorage`) or a `setupFiles` shim that re-points the
   global at `globalThis.jsdom.window.localStorage` — a verified prototype is
   in `claude-notes/plans/node26-vitest-localstorage-investigation/`. Details:
   `claude-notes/plans/2026-09-08-node26-vitest-localstorage.md`.
3. `npm ci` under the new major: `engine-strict` also validates every
   dependency's `engines`.
