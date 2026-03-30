# json_filter Windows Support Implementation Plan

**Goal:** Make `apply_json_filter` work on Windows by dispatching script filters to the correct interpreter, using a Pandoc-style exists-then-dispatch pattern.

**Architecture:** If the filter path exists on disk and we're on Windows, dispatch by file extension (`.py` → Python, `.sh` → bash if available). If the file doesn't exist, treat it as a bare command name and let the OS resolve it via PATH. On Unix, always pass through to `Command::new` (shebangs handle everything). `find_python` caches the discovered Python interpreter, probing `.bat`/`.cmd` variants for pyenv-win compatibility. `find_bash` caches bash availability for conditional `.sh` dispatch.

**Tech Stack:** Rust stdlib (`std::process::Command`, `std::sync::OnceLock`, `std::path::Path`)

**PR:** #89

---

## Design Decisions

- **Pandoc-style exists-then-dispatch**: Researched how Pandoc handles this (`src/Text/Pandoc/Filter/JSON.hs`). Pandoc checks if the file exists, then dispatches by extension if not executable. On Windows, Pandoc's `getPermissions` always reports files as executable, so its extension dispatch is never reached — a bug. Our approach fixes this by gating on `cfg!(windows) && filter_path.exists()`.

- **PATH-resolved commands**: `FilterSpec::Json` accepts bare command names like `pandoc-crossref`. The `filter_path.exists()` gate naturally handles this — bare names don't exist as files, so they fall through to `Command::new` for PATH resolution.

- **Python discovery**: Windows MS Store stub for `python3` exits with code 9009 (success at spawn, failure at exit). We check `status.success()` not just `is_ok()`. Pyenv-win uses `.bat` shims, so we probe those too. Python 3 variants are probed before Python 2 to avoid selecting a legacy interpreter.

- **Bash availability**: Rather than `#[cfg(unix)]` on the `.sh` test, we check bash availability at runtime via `find_bash()`. This means `.sh` filters work on Windows with Git Bash, and the test runs everywhere bash exists.

- **Case-insensitive extensions**: `to_ascii_lowercase()` on the extension for Windows filesystem compatibility.

## Implementation (completed)

- [x] Task 0: Reset branch to main
- [x] Task 1: Add `find_python` helper with OnceLock caching and Windows .bat/.cmd probing
- [x] Task 2: Add `build_filter_command` with exists-then-dispatch, wire into `apply_json_filter`
- [x] Task 3: Add `find_bash` helper, make `.sh` dispatch conditional, runtime skip on test
- [x] Task 4: Fix python candidate ordering (python3 variants before python)

## Files Modified

- `crates/pampa/src/json_filter.rs` — all changes in this one file (+130 lines)
