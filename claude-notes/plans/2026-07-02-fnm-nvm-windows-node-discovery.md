# fnm + nvm-windows Node discovery for `q2 mcp` (bd-e1ger90c)

## Overview

`crates/quarto-mcp-launcher/src/node.rs` resolves an ambient Node.js
for `q2 mcp`. Layer 3 (`default_well_known()`) is a fallback probe of
known install locations, used only when `QUARTO_NODE` is unset and
`node` isn't on `PATH`. On Windows this layer only checks Program
Files and Volta — no fnm, no nvm-windows.

**Confirmed broken (bd-e1ger90c, comment c-5vnn5yn2):** with PATH
stripped to Windows system dirs (simulating a GUI-launched MCP host
like Claude Desktop, which doesn't source a shell profile), `q2 mcp`
fails to find Node even with fnm installed and a version set as
default. fnm mutates PATH only inside an interactive shell
(`fnm env`), never persists it — so a GUI host genuinely never sees
it. This is exactly the module's own stated flagship use case
(see the file's module doc, lines 1–6).

nvm-windows is not similarly broken today: it persists its active
version to the user's PATH via the Windows registry, so most GUI
apps inherit it. Adding nvm-windows support here is defense-in-depth,
not a confirmed-broken fix.

**Latent Windows warning fixed as a side effect:** `highest_version_node`
is currently uncalled on Windows (its only caller is the `#[cfg(unix)]`
branch of `default_well_known`) and is *not* cfg-gated, so a Windows
build emits `warning: function highest_version_node is never used`.
CI is unix-only today so `-D warnings` never bites, but it is a real
latent break. Adding Windows call sites makes the function live on all
platforms and removes the warning. (Earlier draft of this plan wrongly
described a `#[cfg(unix)]` gate on the function — there is none; the
symptom is dead_code, not a gate.)

**Verified layouts** (checked live on this machine, not assumed —
the strand description's guess of `installation/bin/node.exe` for
fnm-on-Windows was wrong):

- fnm on Windows, base `%APPDATA%\fnm` (via `dirs::config_dir()`):
  - `node-versions\vX.Y.Z\installation\node.exe` — **no `bin`
    subdir**, unlike unix fnm.
  - `aliases\default` is a junction pointing straight at
    `node-versions\vX.Y.Z\installation` (verified via
    `ls -la $APPDATA/fnm/aliases/default`), so the default-alias
    candidate is `aliases\default\node.exe`.
- nvm-windows, root at `%NVM_HOME%` (the env var nvm-windows itself
  sets and is the only reliable way to find the root — unlike unix
  nvm, there's no fixed `~/.nvm`-style default):
  - `vX.Y.Z\node.exe` — **flat**, no subdir at all.
  - Known gap, accepted: a scoop-packaged nvm-windows install
    redirects its actual root via `settings.txt`'s `root:` line
    (confirmed on this machine — `NVM_HOME` points at the scoop
    shim, not the version root). Parsing `settings.txt` is out of
    scope (YAGNI — nvm-windows isn't the confirmed-broken case;
    `NVM_HOME` alone covers the common installer-based setup).

**Approach (decided 2026-07-02, Option B):** keep
`highest_version_node`'s 1-argument signature. Its internal hardcoded
sub-path list becomes a **superset** covering all four layouts:
`["bin/node", "installation/bin/node", "installation/node.exe",
"node.exe"]`. Then add Windows call sites for the fnm base
(`dirs::config_dir()/fnm/node-versions`) and the nvm-windows root
(`%NVM_HOME%`). No signature change, no unix call-site churn.

Why the superset is safe across platforms: a sub-path that doesn't
match the current OS's layout simply isn't a file, so it's skipped.
Unix binaries are `node` (not `node.exe`) under `bin/` or
`installation/bin/`, so the two `.exe` subs never false-match on unix;
Windows binaries carry `.exe`, so the two unix subs never false-match
on Windows. Cost is a couple of extra `is_file` stats per version
dir — negligible, and existence is already checked lazily.

The rejected alternative (Option A) was to parameterize
`sub_paths: &[&str]` and pass an explicit per-manager list at each of
the five call sites. More self-documenting per site, but more churn
for no behavioral gain; Option B is the smaller reasonable change.

## Global constraints

- No behavior change on unix: the two unix subs stay first in the
  list, so an existing unix install resolves to exactly the same path
  it does today.
- `highest_version_node`'s directory-scan logic stays OS-agnostic so
  tests exercise both unix-style and Windows-style layouts on any
  platform (it's just `fs::read_dir` + path joins, never
  `probe_version`/`Command::new`).
- Match existing style: hardcoded well-known paths, no new config/
  env-var indirection beyond what nvm-windows/fnm themselves require
  (`NVM_HOME`, `dirs::config_dir()`).

## Checklist

- [ ] Write failing tests for the new layouts (flat `node.exe`,
      fnm-Windows `installation/node.exe`) — they RED against the
      current 2-entry sub-path list by returning `None`
- [ ] Widen the internal sub-path list to the 4-entry superset
- [ ] Add fnm-on-Windows and nvm-windows probing to the
      `#[cfg(windows)]` branch of `default_well_known()`
- [ ] Run `cargo nextest run -p quarto-mcp-launcher` and confirm new
      tests pass and the Windows dead_code warning is gone
- [ ] Run `cargo xtask verify --skip-hub-build` (Rust-only change)
- [ ] Manually re-run the bd-e1ger90c repro (fnm active, PATH
      stripped) and confirm `q2 mcp --launcher-info` now finds Node
- [ ] Comment the fix + repro confirmation on bd-e1ger90c, then close

## Details

### Step 1 — failing tests first (meaningful RED)

Add to the `#[cfg(test)] mod tests` block in
`crates/quarto-mcp-launcher/src/node.rs`. These call the *existing*
1-arg signature, so they compile — and the two new-layout tests fail
their **assertion** (the current list has neither `node.exe` nor
`installation/node.exe`, so `highest_version_node` returns `None`).
That's the meaningful RED: a wrong return value, not a compile error.

```rust
use std::fs;

fn touch(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"").unwrap();
}

#[test]
fn highest_version_node_picks_newest_unix_style() {
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("v22.1.0/bin/node"));
    touch(&dir.path().join("v24.15.0/bin/node"));
    assert_eq!(
        highest_version_node(dir.path()),
        Some(dir.path().join("v24.15.0/bin/node"))
    );
}

#[test]
fn highest_version_node_picks_newest_fnm_unix_style() {
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("v24.1.0/installation/bin/node"));
    touch(&dir.path().join("v20.9.0/installation/bin/node"));
    assert_eq!(
        highest_version_node(dir.path()),
        Some(dir.path().join("v24.1.0/installation/bin/node"))
    );
}

#[test]
fn highest_version_node_picks_newest_flat_windows_style() {
    // nvm-windows layout: <root>/vX.Y.Z/node.exe, no subdir.
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("v22.11.0/node.exe"));
    touch(&dir.path().join("v24.18.0/node.exe"));
    assert_eq!(
        highest_version_node(dir.path()),
        Some(dir.path().join("v24.18.0/node.exe"))
    );
}

#[test]
fn highest_version_node_picks_newest_fnm_windows_style() {
    // fnm-on-Windows layout: <root>/vX.Y.Z/installation/node.exe.
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("v24.18.0/installation/node.exe"));
    assert_eq!(
        highest_version_node(dir.path()),
        Some(dir.path().join("v24.18.0/installation/node.exe"))
    );
}

#[test]
fn highest_version_node_ignores_non_version_dirs_and_missing_root() {
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("not-a-version/node.exe"));
    assert_eq!(highest_version_node(dir.path()), None);

    let missing = dir.path().join("does-not-exist");
    assert_eq!(highest_version_node(&missing), None);
}
```

Run: `cargo nextest run -p quarto-mcp-launcher`
Expected RED: `highest_version_node_picks_newest_flat_windows_style`
and `highest_version_node_picks_newest_fnm_windows_style` fail
(`None` vs the expected path). The two unix-style tests already pass;
the ignore/missing test passes.

### Step 2 — widen the sub-path list to the superset

In `highest_version_node` (currently
`crates/quarto-mcp-launcher/src/node.rs:240`), replace:

```rust
        // nvm: <root>/vX.Y.Z/bin/node; fnm: <root>/vX.Y.Z/installation/bin/node
        for sub in ["bin/node", "installation/bin/node"] {
```

with:

```rust
        // Layouts, unix subs first (unix behavior is unchanged):
        // unix nvm  <root>/vX.Y.Z/bin/node
        // unix fnm  <root>/vX.Y.Z/installation/bin/node
        // fnm-win   <root>/vX.Y.Z/installation/node.exe
        // nvm-win   <root>/vX.Y.Z/node.exe   (flat)
        // A sub for the wrong OS simply isn't a file and is skipped.
        for sub in [
            "bin/node",
            "installation/bin/node",
            "installation/node.exe",
            "node.exe",
        ] {
```

Run: `cargo nextest run -p quarto-mcp-launcher`
Expected GREEN: all five tests pass. This is the meaningful GREEN —
the two previously-red Windows-layout assertions now resolve.

### Step 3 — add Windows probing

In `default_well_known()`, extend the `#[cfg(windows)]` branch
(currently `crates/quarto-mcp-launcher/src/node.rs:216-224`) to add,
after the existing Volta push:

```rust
        // fnm on Windows: <config>/fnm/{aliases/default,node-versions}.
        // aliases/default is a junction straight to the version's
        // `installation` dir (no `bin` subdir on Windows).
        if let Some(config) = dirs::config_dir() {
            let fnm_base = config.join("fnm");
            paths.push(fnm_base.join("aliases/default/node.exe"));
            paths.extend(highest_version_node(&fnm_base.join("node-versions")));
        }
        // nvm-windows: root is wherever NVM_HOME points (there's no
        // fixed default the way unix nvm has ~/.nvm), flat
        // <root>/vX.Y.Z/node.exe layout. A scoop-packaged install
        // redirects its real root via settings.txt's `root:` line,
        // which NVM_HOME alone won't see -- accepted gap; nvm-windows
        // already persists its active version to PATH via the registry
        // so this probe is defense-in-depth, not the confirmed-broken
        // case.
        if let Some(nvm_home) = std::env::var_os("NVM_HOME") {
            paths.extend(highest_version_node(&PathBuf::from(nvm_home)));
        }
```

This also makes `highest_version_node` live on Windows, removing the
dead_code warning.

Run: `cargo nextest run -p quarto-mcp-launcher`
Expected: PASS, and the Windows build no longer warns.

### Step 4 — full verification

```bash
cargo nextest run -p quarto-mcp-launcher
cargo xtask verify --skip-hub-build
```

`--skip-hub-build` is correct — this touches only
`quarto-mcp-launcher`, which nothing in `wasm-quarto-hub-client`'s
dependency closure depends on.

### Step 5 — manual repro re-check

Re-run the exact reproduction from bd-e1ger90c comment c-5vnn5yn2:
fnm active with Node 24.18.0 set as default, PATH stripped to
Windows system dirs only:

```bash
eval "$(fnm env --shell bash)"
fnm default 24
PATH="/c/Windows/system32:/c/Windows" ./target/debug/q2.exe mcp --launcher-info
```

Expected: reports a found Node (the fnm well-known path), not
`node: NOT FOUND`. Record actual output in the braid comment.

Note the build chain: `q2 mcp` needs the hub-mcp bundle embedded, but
`--launcher-info` only reports discovery and does not run the bundle,
so a plain `cargo build --bin q2` is enough to exercise the fix.

### Step 6 — close out

Comment the fix + repro confirmation on bd-e1ger90c and close it —
only after Step 5's manual repro actually shows Node found (project
end-to-end-verification rule: unit tests exercise
`highest_version_node` directly, not the real `q2 mcp` discovery path).
