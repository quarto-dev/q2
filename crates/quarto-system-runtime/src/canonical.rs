/*
 * canonical.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The one canonicalize q2 uses, so every canonical path it emits or
 * compares has the same spelling.
 */

use std::path::{Path, PathBuf};

/// Canonicalize `path`: absolute, symlinks resolved, `.`/`..` removed.
///
/// Call this instead of `std::fs::canonicalize` / `Path::canonicalize`
/// wherever the result is emitted or compared with another canonical
/// path, so all such paths share one spelling. `NativeRuntime::canonicalize`
/// delegates here.
///
/// On Windows the result is the plain `C:\…` form whenever one exists;
/// the verbatim `\\?\` form is kept only where a plain spelling would
/// not name the same file (paths past `MAX_PATH`, reserved names,
/// network shares). Elsewhere this is `std::fs::canonicalize`.
#[cfg(not(target_arch = "wasm32"))]
pub fn canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    dunce::canonicalize(path)
}

#[cfg(target_arch = "wasm32")]
pub fn canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path)
}

/// Canonicalize the deepest existing ancestor of `path`, then
/// re-append the components below it. This resolves OS-level
/// path aliases (e.g. macOS's `/var` → `/private/var`) for paths
/// that haven't been created yet, such as a planned output whose
/// project root exists.
///
/// Best-effort: any [`canonicalize`] failure leaves the lexical
/// form in place.
pub fn canonicalize_deepest_existing(path: &Path) -> PathBuf {
    let mut current = path.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    while !current.exists() {
        match current.file_name() {
            Some(name) => {
                tail.push(name.to_os_string());
                if !current.pop() {
                    return path.to_path_buf();
                }
            }
            None => return path.to_path_buf(),
        }
    }
    let mut result = canonicalize(&current).unwrap_or(current);
    for name in tail.into_iter().rev() {
        result.push(name);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deepest_existing_reappends_the_missing_tail() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = canonicalize(temp.path()).unwrap();
        let planned = temp.path().join("out").join("doc.html");
        assert_eq!(
            canonicalize_deepest_existing(&planned),
            root.join("out").join("doc.html")
        );
    }

    #[cfg(windows)]
    #[test]
    fn short_existing_path_comes_back_plain() {
        let temp = tempfile::TempDir::new().unwrap();
        let file = temp.path().join("doc.qmd");
        std::fs::write(&file, "x").unwrap();
        // TEMP on a network share or behind a junction to one can resolve
        // to a path that must stay verbatim; require a plain form first.
        let std_canonical = std::fs::canonicalize(&file).unwrap();
        let plain_form = dunce::simplified(&std_canonical)
            .to_string_lossy()
            .into_owned();
        assert!(
            !plain_form.starts_with(r"\\?\") && plain_form.encode_utf16().count() < 200,
            "test setup: TEMP must resolve to a short path with a plain form: {plain_form}"
        );

        let result = canonicalize(&file).unwrap();
        assert!(
            !result.to_string_lossy().starts_with(r"\\?\"),
            "a path with a plain form must come back plain, got: {}",
            result.display()
        );
        assert_eq!(
            std::fs::canonicalize(&result).unwrap(),
            std::fs::canonicalize(&file).unwrap()
        );
    }

    /// A path over the 260-unit `MAX_PATH` limit has no plain form that
    /// every Windows API accepts, so it stays verbatim.
    #[cfg(windows)]
    #[test]
    fn existing_path_over_max_path_stays_verbatim() {
        use std::os::windows::ffi::OsStrExt;
        // Rooted at the `\\?\` spelling, so std can create (and TempDir
        // can remove) a path past MAX_PATH.
        let root = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        let temp = tempfile::TempDir::new_in(root).unwrap();
        let mut long = temp.path().to_path_buf();
        while long.as_os_str().encode_wide().count() <= 300 {
            long.push("a".repeat(40));
        }
        std::fs::create_dir_all(&long).unwrap();
        let file = long.join("doc.qmd");
        std::fs::write(&file, "x").unwrap();

        let result = canonicalize(&file).unwrap();
        assert!(
            result.to_string_lossy().starts_with(r"\\?\"),
            "a path over MAX_PATH must stay verbatim, got: {}",
            result.display()
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn matches_std_canonicalize() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(temp.path().join("sub")).unwrap();
        let file = temp.path().join("doc.qmd");
        std::fs::write(&file, "x").unwrap();
        let spelled = temp.path().join("sub").join("..").join("doc.qmd");
        assert_eq!(
            canonicalize(&spelled).unwrap(),
            std::fs::canonicalize(&spelled).unwrap()
        );
    }
}
