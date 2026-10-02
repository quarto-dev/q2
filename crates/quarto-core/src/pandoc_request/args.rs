//! Typed pandoc arguments: a path-valued argument is a path until
//! `prepare()` normalizes it into the request.
//!
//! The argv builders emit both `--flag value` pairs and `--flag=<path>`
//! single tokens, so a bare `OsString` would hide which entries are paths.

use std::borrow::Cow;
use std::path::PathBuf;

use super::normalize_request_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PandocArg {
    /// Passed through verbatim.
    Text(String),
    /// A whole token that is a path (the value of a `--flag value` pair).
    Path(PathBuf),
    /// A single `--flag=<path>` token; `flag` includes the `=`.
    FlagPath { flag: String, path: PathBuf },
}

impl PandocArg {
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    /// The argument as written by its builder, paths un-normalized.
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        match self {
            Self::Text(s) => Cow::Borrowed(s),
            Self::Path(p) => p.to_string_lossy(),
            Self::FlagPath { flag, path } => {
                Cow::Owned(format!("{flag}{}", path.to_string_lossy()))
            }
        }
    }

    /// The request form: paths `/`-normalized. A non-UTF-8 path or a path
    /// that is not absolute after normalization is an error.
    pub fn to_request_string(&self) -> Result<String, String> {
        let path_string = |p: &PathBuf| -> Result<String, String> {
            if p.to_str().is_none() {
                return Err(format!("path is not valid UTF-8: {}", p.display()));
            }
            let normalized = normalize_request_path(p);
            if !is_absolute_request_path(&normalized) {
                return Err(format!("path is not absolute: {}", p.display()));
            }
            Ok(normalized)
        };
        match self {
            Self::Text(s) => Ok(s.clone()),
            Self::Path(p) => path_string(p),
            Self::FlagPath { flag, path } => Ok(format!("{flag}{}", path_string(path)?)),
        }
    }
}

/// `/...` or `X:/...`.
pub fn is_absolute_request_path(s: &str) -> bool {
    s.starts_with('/')
        || (s.len() >= 3 && s.as_bytes()[0].is_ascii_alphabetic() && &s.as_bytes()[1..3] == b":/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_and_normalizes() {
        assert_eq!(
            PandocArg::text("--toc").to_request_string().unwrap(),
            "--toc"
        );
        assert_eq!(
            PandocArg::Path(PathBuf::from("/a/./b/../c"))
                .to_request_string()
                .unwrap(),
            "/a/c"
        );
        let arg = PandocArg::FlagPath {
            flag: "--css=".into(),
            path: PathBuf::from("/d/x.css"),
        };
        assert_eq!(arg.to_request_string().unwrap(), "--css=/d/x.css");
        assert_eq!(arg.to_string_lossy(), "--css=/d/x.css");
    }

    #[test]
    fn relative_paths_are_rejected() {
        assert!(
            PandocArg::Path(PathBuf::from("rel/x"))
                .to_request_string()
                .is_err()
        );
        assert!(
            PandocArg::FlagPath {
                flag: "--css=".into(),
                path: PathBuf::from("")
            }
            .to_request_string()
            .is_err()
        );
    }

    #[test]
    fn absolute_forms() {
        assert!(is_absolute_request_path("/x"));
        assert!(is_absolute_request_path("C:/x"));
        assert!(!is_absolute_request_path("x/y"));
        assert!(!is_absolute_request_path("C:x"));
    }
}
