//! Cross-platform per-user data directory for Quarto

use std::path::PathBuf;

/// Pure branch logic (no IO). Prefer an explicit override (`QUARTO_DATA_DIR`), else
/// the platform data dir, namespaced under `quarto`.
///
/// Semantics (Q1-faithful):
/// - `env_override` is `Some(p)` → use `p` **as-is**. An explicit override is the
///   final dir; the `quarto` suffix is NOT appended (Q1's `quartoDataDir()` honors
///   `QUARTO_DATA_DIR` directly as the quarto data root).
/// - `env_override` is `None` → `data.map(|d| d.join("quarto"))`.
/// - Both `None` → `None`.
fn data_dir_from(env_override: Option<PathBuf>, data: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = env_override {
        Some(p)
    } else {
        data.map(|d| d.join("quarto"))
    }
}

/// Cross-platform per-user data dir for quarto, created if missing. Honors a
/// `QUARTO_DATA_DIR` env override first (Q1-faithful), else `dirs::data_dir()/quarto`.
///
/// Creates the directory if it does not already exist.
pub fn quarto_data_dir() -> std::io::Result<PathBuf> {
    let env_override = std::env::var_os("QUARTO_DATA_DIR").map(PathBuf::from);
    let dir = data_dir_from(env_override, dirs::data_dir()).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "no data directory available")
    })?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_from_override_wins_and_is_used_as_is() {
        // An explicit override must be returned exactly as given — no `quarto` suffix.
        let override_path = PathBuf::from("/custom/data/quarto-root");
        let data = PathBuf::from("/home/user/.local/share");
        let result = data_dir_from(Some(override_path.clone()), Some(data));
        assert_eq!(
            result,
            Some(override_path.clone()),
            "override should be returned as-is, not with /quarto appended"
        );
        // Double-check: result must NOT have /quarto appended
        assert_ne!(
            result,
            Some(override_path.join("quarto")),
            "QUARTO_DATA_DIR override must not have 'quarto' appended"
        );
    }

    #[test]
    fn data_dir_from_falls_back_to_data_dir_with_quarto_suffix() {
        let data = PathBuf::from("/home/user/.local/share");
        let result = data_dir_from(None, Some(data.clone()));
        assert_eq!(result, Some(data.join("quarto")));
    }

    #[test]
    fn data_dir_from_both_none_returns_none() {
        assert_eq!(data_dir_from(None, None), None);
    }

    #[test]
    fn data_dir_from_data_dir_branch_last_component_is_quarto() {
        // When falling back to data_dir, the last path component must be "quarto".
        let result = data_dir_from(None, Some(PathBuf::from("/home/user/.local/share"))).unwrap();
        assert_eq!(
            result.file_name().and_then(|n| n.to_str()),
            Some("quarto"),
            "last component must be 'quarto', got {result:?}"
        );
    }

    #[test]
    fn quarto_data_dir_returns_existing_directory() {
        let dir = quarto_data_dir().expect("quarto_data_dir() should succeed");
        assert!(
            dir.exists(),
            "quarto_data_dir() should create the directory, but {dir:?} does not exist"
        );
        assert!(
            dir.is_dir(),
            "quarto_data_dir() should be a directory, but {dir:?} is not"
        );
        assert_eq!(
            dir.file_name().and_then(|n| n.to_str()),
            Some("quarto"),
            "last component must be 'quarto', got {dir:?}"
        );
    }
}
