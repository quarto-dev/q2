//! The languages Pandoc's highlighter recognizes, for Quarto 1 parity.
//!
//! Quarto 1 (Pandoc + skylighting) wraps a code block in
//! `div.sourceCode` whenever one of its classes names a syntax
//! skylighting knows, even when that syntax adds no colors (`default`,
//! `ini`, …). q2 highlights far fewer languages, so the annotate step
//! asks [`is_pandoc_known_language`] to decide which un-highlighted
//! blocks still get the wrapper (GH #1079).
//!
//! The table is generated (see `scripts/gen_pandoc_languages.py`); the
//! lookup mirrors skylighting's `lookupSyntax`: full name, then short
//! name (both case-insensitive), then the class as a file extension
//! matched against each syntax's globs (case-sensitive).

#[rustfmt::skip]
#[path = "pandoc_languages_table.rs"]
mod table;

use table::PANDOC_SYNTAXES;

/// True when Pandoc 3.10 (as configured by Quarto 1) would resolve
/// `class` to a syntax — i.e. when Quarto 1 would wrap a code block
/// carrying this class in `div.sourceCode`.
pub fn is_pandoc_known_language(class: &str) -> bool {
    if class.is_empty() {
        return false;
    }
    // skylighting special-cases these two before the generic lookup.
    let class = match class {
        "csharp" => "cs",
        "fortran" => "for",
        other => other,
    };
    if PANDOC_SYNTAXES
        .iter()
        .any(|s| s.name.eq_ignore_ascii_case(class) || s.short.eq_ignore_ascii_case(class))
    {
        return true;
    }
    // `syntaxesByExtension`: match the pseudo-filename "*.<class>"
    // (with a literal `*`) against each glob.
    let file_name = format!("*.{class}");
    PANDOC_SYNTAXES.iter().any(|s| {
        s.globs
            .iter()
            .any(|g| match_glob(g.as_bytes(), file_name.as_bytes()))
    })
}

/// skylighting's `matchGlob`: `*` matches any run of characters;
/// everything else matches literally.
fn match_glob(glob: &[u8], name: &[u8]) -> bool {
    match glob.split_first() {
        None => name.is_empty(),
        Some((b'*', rest)) => (0..=name.len()).any(|i| match_glob(rest, &name[i..])),
        Some((c, rest)) => name.first() == Some(c) && match_glob(rest, &name[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_by_short_name() {
        for class in [
            "ini",
            "default",
            "toml",
            "bash",
            "markdown",
            "dockerfile",
            "alert",
        ] {
            assert!(is_pandoc_known_language(class), "{class}");
        }
    }

    #[test]
    fn name_lookup_is_case_insensitive() {
        assert!(is_pandoc_known_language("INI"));
        assert!(is_pandoc_known_language("Python"));
        // Full name, not short name.
        assert!(is_pandoc_known_language("alerts"));
    }

    #[test]
    fn known_by_extension() {
        // conf/cfg come from the INI/TOML globs, txt from Default's.
        for class in ["conf", "cfg", "txt", "py", "sh", "md", "yml", "Dockerfile"] {
            assert!(is_pandoc_known_language(class), "{class}");
        }
        // `cmakelists.txt` matches Default's `*.txt` glob, as in Pandoc.
        assert!(is_pandoc_known_language("cmakelists.txt"));
    }

    #[test]
    fn quarto_extra_syntaxes_are_known() {
        for class in ["k", "q", "r", "scss"] {
            assert!(is_pandoc_known_language(class), "{class}");
        }
    }

    #[test]
    fn special_cases() {
        assert!(is_pandoc_known_language("csharp"));
        assert!(is_pandoc_known_language("fortran"));
    }

    #[test]
    fn unknown_classes() {
        for class in [
            "",
            "text",
            "plaintext",
            "nginx",
            "cell-output",
            "cell-output-stdout",
            "cell-code",
            "code-with-copy",
        ] {
            assert!(!is_pandoc_known_language(class), "{class:?}");
        }
    }

    #[test]
    fn match_glob_semantics() {
        assert!(match_glob(b"*.ini", b"*.ini"));
        assert!(match_glob(b"*", b"anything"));
        assert!(!match_glob(b"Cargo.lock", b"*.lock"));
        assert!(!match_glob(b"*.INI", b"*.ini"));
    }
}
