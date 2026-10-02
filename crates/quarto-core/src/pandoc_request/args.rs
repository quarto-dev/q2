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

/// The native-Windows spelling of one request argv entry or path-valued env
/// value: `X:/a/b` -> `X:\a\b`, `//server/share/a` -> `\\server\share\a`,
/// also behind a `--flag=` prefix. Anything else (plain text, a built-in
/// style name, a `/`-rooted wasm path) is returned unchanged.
///
/// The request carries `/`-normalized paths so one string works natively and
/// in the wasm worker. Quarto 1 hands pandoc native Windows paths, and the
/// vendored `init.lua`'s `is_absolute_path` accepts `C:` only when followed by
/// `pandoc.path.separator` (`\\` on Windows), so a native Windows `execute()`
/// converts back. Every path `prepare()` puts in argv is absolute, so a
/// shape test finds exactly those: no plain-text argument starts with a drive
/// letter and colon or a double slash.
pub fn to_native_windows_arg(arg: &str) -> String {
    fn is_native_root(s: &str) -> bool {
        is_drive_path(s) || (s.starts_with("//") && !s.starts_with("///"))
    }
    fn is_drive_path(s: &str) -> bool {
        let b = s.as_bytes();
        b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/'
    }
    if is_native_root(arg) {
        return arg.replace('/', "\\");
    }
    if arg.starts_with("--")
        && let Some((flag, value)) = arg.split_once('=')
        && is_native_root(value)
    {
        return format!("{flag}={}", value.replace('/', "\\"));
    }
    arg.to_string()
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
    fn native_windows_args() {
        use super::to_native_windows_arg as n;
        assert_eq!(n("C:/a/b/main.lua"), r"C:\a\b\main.lua");
        assert_eq!(n("//server/share/a.qmd"), r"\\server\share\a.qmd");
        assert_eq!(n("--css=D:/doc/x.css"), r"--css=D:\doc\x.css");
        assert_eq!(
            n("--include-in-header=C:/t/h.html"),
            r"--include-in-header=C:\t\h.html"
        );
        for plain in [
            "-f",
            "json",
            "docx",
            "--toc",
            "png",
            "tango",
            "/__q2_share__/x",
            "///x",
            "C:x",
            "--epub-subdirectory=EPUB",
            "--epub-subdirectory=/x",
            "--metadata=a/b",
            "-o",
            "highlighting-definitions=#let x = 1 // c",
        ] {
            assert_eq!(n(plain), plain, "{plain}");
        }
    }

    #[test]
    fn absolute_forms() {
        assert!(is_absolute_request_path("/x"));
        assert!(is_absolute_request_path("C:/x"));
        assert!(!is_absolute_request_path("x/y"));
        assert!(!is_absolute_request_path("C:x"));
    }
}
