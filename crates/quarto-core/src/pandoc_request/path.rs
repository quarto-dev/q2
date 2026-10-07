//! Pure path normalization for request paths.
//!
//! Request paths are `/`-separated absolute strings on every host: the pandoc
//! worker mounts bytes at identical absolute paths, and a native Windows
//! request must not carry backslashes. This is deliberately *not*
//! `VirtualFileSystem::normalize_path`, which is an instance method joining
//! against `project_root`.

use std::path::Path;

/// Normalize a path for a request: `/` separators, no `.`/`..`/empty
/// components, verbatim (`\\?\`) and UNC prefixes understood, upper-case
/// drive letter. Works on the string form so the Windows cases are testable on
/// every OS.
pub fn normalize_request_path(path: &Path) -> String {
    normalize_request_path_str(&path.to_string_lossy())
}

/// String form of [`normalize_request_path`].
pub fn normalize_request_path_str(raw: &str) -> String {
    let s = raw.replace('\\', "/");

    // Verbatim prefixes: `//?/UNC/server/share/...` and `//?/C:/...`.
    let s = if let Some(rest) = s.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = s.strip_prefix("//?/") {
        rest.to_string()
    } else {
        s
    };

    let (prefix, rest): (String, &str) = if let Some((drive, tail)) = split_drive(&s) {
        (format!("{drive}:"), tail)
    } else if let Some(unc) = s.strip_prefix("//") {
        // `//server/share/rest`: the server and share survive normalization.
        let mut parts = unc.splitn(3, '/');
        let server = parts.next().unwrap_or("");
        let share = parts.next().unwrap_or("");
        let tail = parts.next().unwrap_or("");
        if server.is_empty() {
            (String::new(), s.trim_start_matches('/'))
        } else {
            (format!("//{server}/{share}"), tail)
        }
    } else {
        (String::new(), s.as_str())
    };

    let rooted = !prefix.is_empty() || s.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for comp in rest.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|p| *p != "..") {
                    parts.pop();
                } else if !rooted {
                    parts.push("..");
                }
            }
            c => parts.push(c),
        }
    }

    let body = parts.join("/");
    if rooted {
        if body.is_empty() {
            format!("{prefix}/")
        } else {
            format!("{prefix}/{body}")
        }
    } else {
        body
    }
}

/// `C:` or `C:/...` -> (`C`, rest after the colon), drive letter upper-cased.
fn split_drive(s: &str) -> Option<(char, &str)> {
    let mut chars = s.chars();
    let letter = chars.next()?;
    if !letter.is_ascii_alphabetic() || chars.next()? != ':' {
        return None;
    }
    let rest = &s[2..];
    if rest.is_empty() || rest.starts_with('/') {
        Some((letter.to_ascii_uppercase(), rest))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_request_path_str as n;

    #[test]
    fn windows_drive_forms() {
        assert_eq!(n(r"C:\proj\a.qmd"), "C:/proj/a.qmd");
        assert_eq!(n(r"c:\proj\a.qmd"), "C:/proj/a.qmd");
        assert_eq!(n(r"\\?\C:\proj\a.qmd"), "C:/proj/a.qmd");
        assert_eq!(n(r"\\?\c:\proj\..\x\.\a.qmd"), "C:/x/a.qmd");
        assert_eq!(n(r"C:\"), "C:/");
        assert_eq!(n("C:"), "C:/");
    }

    #[test]
    fn unc_forms() {
        assert_eq!(n(r"\\server\share\a.qmd"), "//server/share/a.qmd");
        assert_eq!(n(r"\\?\UNC\server\share\d\a.qmd"), "//server/share/d/a.qmd");
        assert_eq!(n(r"\\server\share"), "//server/share/");
    }

    #[test]
    fn posix_forms() {
        assert_eq!(n("/a/b/../c/./d"), "/a/c/d");
        assert_eq!(n("/a//b/"), "/a/b");
        assert_eq!(n("/"), "/");
        assert_eq!(n("/.."), "/");
        assert_eq!(
            n("/__q2_share__/pandoc-share"),
            "/__q2_share__/pandoc-share"
        );
    }

    #[test]
    fn relative_forms_stay_relative() {
        assert_eq!(n("a/./b/../c"), "a/c");
        assert_eq!(n("../a"), "../a");
    }

    #[test]
    fn colon_in_posix_name_is_not_a_drive() {
        assert_eq!(n("/a:b/c"), "/a:b/c");
        assert_eq!(n("ab:/c"), "ab:/c");
    }
}
