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
}
