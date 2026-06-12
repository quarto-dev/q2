//! ts-packages workspace enumeration, shared by `build-all` and `verify`.
//!
//! The `ts-packages/*` npm workspaces compile with plain `tsc` and are
//! consumed two ways: hub-client (and the other SPAs) bundle them **from
//! source** — each package's exports map points `types`/`source` at
//! `src/index.ts` — while Node consumers (the quarto-hub-mcp server)
//! resolve the `"import": "./dist/index.js"` condition and need `dist/`
//! built. Neither hub-client's `tsc -b` nor any cargo step produces those
//! `dist/` directories, so the build orchestrators run
//! `npm run build --if-present -w <pkg> ...` over the list returned here.
//! Build order doesn't matter: types resolve via `src/`, so each
//! package's `tsc` compiles without its dependencies' `dist/` present.
//! See bd-6rczoll3.

use std::path::Path;

/// Relative npm workspace paths (`ts-packages/<name>`) for every
/// ts-package that has a `package.json`, sorted by name. Empty when
/// `ts-packages/` is absent (e.g. older branches). Paths always use `/`
/// separators — npm accepts them on every platform.
pub fn workspace_paths(project_root: &Path) -> Vec<String> {
    let ts_dir = project_root.join("ts-packages");
    let Ok(entries) = std::fs::read_dir(&ts_dir) else {
        return Vec::new();
    };

    let mut paths: Vec<String> = entries
        .flatten()
        .filter(|entry| {
            let path = entry.path();
            path.is_dir() && path.join("package.json").is_file()
        })
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .map(|name| format!("ts-packages/{}", name))
        })
        .collect();
    paths.sort();
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_package(root: &Path, name: &str) {
        let dir = root.join("ts-packages").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("package.json"), "{}").unwrap();
    }

    #[test]
    fn returns_packages_sorted_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        make_package(tmp.path(), "zeta-pkg");
        make_package(tmp.path(), "alpha-pkg");
        make_package(tmp.path(), "mid-pkg");

        assert_eq!(
            workspace_paths(tmp.path()),
            vec![
                "ts-packages/alpha-pkg".to_string(),
                "ts-packages/mid-pkg".to_string(),
                "ts-packages/zeta-pkg".to_string(),
            ]
        );
    }

    #[test]
    fn skips_directories_without_package_json() {
        let tmp = tempfile::tempdir().unwrap();
        make_package(tmp.path(), "real-pkg");
        fs::create_dir_all(tmp.path().join("ts-packages/no-manifest")).unwrap();

        assert_eq!(
            workspace_paths(tmp.path()),
            vec!["ts-packages/real-pkg".to_string()]
        );
    }

    #[test]
    fn skips_plain_files_in_ts_packages() {
        let tmp = tempfile::tempdir().unwrap();
        make_package(tmp.path(), "real-pkg");
        fs::write(tmp.path().join("ts-packages/README.md"), "hi").unwrap();

        assert_eq!(
            workspace_paths(tmp.path()),
            vec!["ts-packages/real-pkg".to_string()]
        );
    }

    #[test]
    fn missing_ts_packages_dir_yields_empty() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(workspace_paths(tmp.path()).is_empty());
    }
}
