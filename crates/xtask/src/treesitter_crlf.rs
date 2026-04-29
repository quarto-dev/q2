//! Tree-sitter CRLF parity check.
//!
//! Re-runs `tree-sitter test` against a copy of the corpus where every
//! line ending has been converted to CRLF. Locks in the scanner-level
//! CRLF handling so future grammar changes cannot silently regress it
//! on Linux CI (where corpus files are checked out as LF).

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

/// Convert all line endings in `input` to CRLF. Idempotent: input that
/// already has CRLF endings is unchanged. Lone `\r` characters (rare,
/// classic-Mac) are left alone.
pub(crate) fn to_crlf(input: &str) -> String {
    let normalized: String = input.replace("\r\n", "\n");
    normalized.replace('\n', "\r\n")
}

/// Run `tree-sitter test` against a CRLF-converted copy of the grammar's
/// corpus. The grammar source files are copied unchanged; only files
/// matching `test/corpus/**/*.txt` are transformed.
pub(crate) fn run_parity_check(grammar_dir: &Path) -> Result<()> {
    let tempdir = tempfile::tempdir()
        .context("Failed to create tempdir for tree-sitter CRLF parity check")?;
    let dest = tempdir.path();

    copy_dir_recursive(grammar_dir, dest, &|relative| {
        !relative.starts_with("target") && !relative.starts_with("node_modules")
    })?;

    convert_corpus_to_crlf(&dest.join("test").join("corpus"))?;

    let status = Command::new("tree-sitter")
        .arg("test")
        .current_dir(dest)
        .status()
        .context("Failed to invoke `tree-sitter test` for CRLF parity run")?;

    if !status.success() {
        bail!("Tree-sitter CRLF parity tests failed");
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path, keep: &dyn Fn(&Path) -> bool) -> Result<()> {
    std::fs::create_dir_all(dst).with_context(|| format!("create_dir_all {}", dst.display()))?;
    for entry in std::fs::read_dir(src).with_context(|| format!("read_dir {}", src.display()))? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let from = entry.path();
        let to = dst.join(&name);
        let relative = Path::new(&name);
        if !keep(relative) {
            continue;
        }
        if file_type.is_dir() {
            let relative_owned = std::path::PathBuf::from(relative);
            copy_dir_recursive(&from, &to, &|sub| {
                let mut joined = relative_owned.clone();
                joined.push(sub);
                keep(&joined)
            })?;
        } else if file_type.is_file() {
            std::fs::copy(&from, &to)
                .with_context(|| format!("copy {} -> {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

fn convert_corpus_to_crlf(corpus_dir: &Path) -> Result<()> {
    if !corpus_dir.is_dir() {
        bail!(
            "Expected corpus directory at {} after copy",
            corpus_dir.display()
        );
    }
    for entry in walkdir::WalkDir::new(corpus_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|s| s.to_str()) != Some("txt") {
            continue;
        }
        let contents = std::fs::read_to_string(entry.path())
            .with_context(|| format!("read {}", entry.path().display()))?;
        let crlf = to_crlf(&contents);
        std::fs::write(entry.path(), crlf)
            .with_context(|| format!("write {}", entry.path().display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_unchanged() {
        assert_eq!(to_crlf(""), "");
    }

    #[test]
    fn lf_becomes_crlf() {
        assert_eq!(to_crlf("a\nb\n"), "a\r\nb\r\n");
    }

    #[test]
    fn crlf_input_is_idempotent() {
        assert_eq!(to_crlf("a\r\nb\r\n"), "a\r\nb\r\n");
    }

    #[test]
    fn mixed_input_normalizes_to_crlf() {
        assert_eq!(to_crlf("a\r\nb\nc\r\n"), "a\r\nb\r\nc\r\n");
    }

    #[test]
    fn no_newlines_unchanged() {
        assert_eq!(to_crlf("abc"), "abc");
    }
}
