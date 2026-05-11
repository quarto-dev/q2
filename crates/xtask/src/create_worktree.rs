//! `cargo xtask create-worktree` — set up a git worktree with beads redirect
//! and a marker-delimited CLAUDE.local.md context section.
//!
//! Three modes (exactly one required):
//!   - positional `<bd-id>` — beads issue (reads `br show`)
//!   - `--issue <N>` — GitHub issue triage (reads `gh issue view`)
//!   - `--upgrade` — cargo dependency upgrade (date-based branch)
//!
//! Filesystem-only: never touches beads state. Skills own beads lifecycle.

use anyhow::Result;

const BEGIN_MARKER: &str =
    "<!-- BEGIN WORKTREE CONTEXT — managed by cargo xtask create-worktree -->";
const END_MARKER: &str = "<!-- END WORKTREE CONTEXT -->";

const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "in", "on", "of", "to", "for", "with", "from", "at", "by", "is",
    "as",
];

// Lock the em-dash in BEGIN_MARKER against accidental editor substitution.
const _: () = {
    let bytes = BEGIN_MARKER.as_bytes();
    // U+2014 EM DASH encodes as 0xE2 0x80 0x94 in UTF-8.
    let mut i = 0;
    let mut found = false;
    while i + 2 < bytes.len() {
        if bytes[i] == 0xE2 && bytes[i + 1] == 0x80 && bytes[i + 2] == 0x94 {
            found = true;
        }
        i += 1;
    }
    assert!(found, "BEGIN_MARKER must contain U+2014 em dash");
};

pub fn derive_slug(title: &str) -> Result<String> {
    let tokens: Vec<String> = title
        .to_lowercase()
        .split(|c: char| c.is_whitespace() || c == '-')
        .map(|tok| {
            tok.chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
        })
        .filter(|tok| !tok.is_empty())
        .filter(|tok| !STOP_WORDS.contains(&tok.as_str()))
        .take(4)
        .collect();

    if tokens.is_empty() {
        anyhow::bail!(
            "unable to derive slug from title \"{title}\" — pass --slug <name> to override"
        );
    }
    Ok(tokens.join("-"))
}

/// Validate a user-provided `--slug` override. Auto-derived slugs already
/// satisfy these rules by construction; this only applies to overrides.
pub fn validate_slug(slug: &str) -> Result<()> {
    if slug.is_empty() {
        anyhow::bail!("--slug must not be empty");
    }
    if slug.len() > 64 {
        anyhow::bail!("--slug too long ({} chars, max 64): {slug:?}", slug.len());
    }
    if slug == "." || slug == ".." {
        anyhow::bail!("--slug must not be {slug:?}");
    }
    if slug.starts_with('-') || slug.ends_with('-') {
        anyhow::bail!("--slug must not start or end with '-': {slug:?}");
    }
    if let Some(bad) = slug
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_'))
    {
        anyhow::bail!(
            "--slug contains invalid character {bad:?} — only ASCII alphanumeric, '-', '_' allowed: {slug:?}"
        );
    }
    Ok(())
}

#[derive(clap::Args)]
#[command(group(clap::ArgGroup::new("mode").required(true).multiple(false)))]
pub struct Args {
    /// Beads issue ID, e.g. `bd-1d3e`. Reads `br show <id>` for title and external_ref.
    #[arg(group = "mode")]
    pub beads_id: Option<String>,

    /// GitHub issue number, e.g. `157`. Reads `gh issue view`.
    #[arg(long, group = "mode")]
    pub issue: Option<u32>,

    /// Cargo dependency upgrade — uses today's date for branch name.
    #[arg(long, group = "mode")]
    pub upgrade: bool,

    /// Override auto-derived slug. In beads mode replaces the derived slug;
    /// in issue/upgrade modes appended as a suffix (for parallel-worktree workflows).
    #[arg(long)]
    pub slug: Option<String>,

    /// Base branch.
    #[arg(long, default_value = "main")]
    pub base: String,
}

pub fn parse_external_ref_to_github_url(ext: Option<&str>) -> Option<String> {
    let ext = ext?;
    let n = ext.strip_prefix("gh-")?;
    if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
        Some(format!("https://github.com/quarto-dev/q2/issues/{n}"))
    } else {
        None
    }
}

pub fn run(_args: Args) -> Result<()> {
    anyhow::bail!("create-worktree not yet implemented");
}

pub fn detect_line_ending(content: &str) -> &'static str {
    // Sniff up to first 1 KiB, snapped to a char boundary so slicing is valid.
    let mut sniff_end = content.len().min(1024);
    while sniff_end > 0 && !content.is_char_boundary(sniff_end) {
        sniff_end -= 1;
    }
    let sniff = &content[..sniff_end];

    let crlf_count = sniff.matches("\r\n").count();
    let lf_total = sniff.matches('\n').count();
    let bare_lf = lf_total - crlf_count;

    if crlf_count > 0 && bare_lf == 0 {
        "\r\n"
    } else {
        "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_drops_stop_words_and_kebab_splits() {
        let s = derive_slug("Fix CRLF test failures in quarto-doctemplate on Windows").unwrap();
        assert_eq!(s, "fix-crlf-test-failures");
    }

    #[test]
    fn slug_caps_at_four_tokens() {
        let s = derive_slug("alpha beta gamma delta epsilon zeta").unwrap();
        assert_eq!(s, "alpha-beta-gamma-delta");
    }

    #[test]
    fn slug_strips_punctuation_and_unicode() {
        let s = derive_slug("Don't panic — handle naïve input (v2)!").unwrap();
        // apostrophe / em dash / accent / parens / digits-with-letters all collapse
        assert_eq!(s, "dont-panic-handle-nave");
    }

    #[test]
    fn slug_empty_result_errors() {
        let err = derive_slug("the and of on in").unwrap_err().to_string();
        assert!(err.contains("unable to derive slug"));
        assert!(err.contains("--slug"));
    }

    #[test]
    fn slug_only_punctuation_errors() {
        let err = derive_slug("!!! ??? ---").unwrap_err().to_string();
        assert!(err.contains("unable to derive slug"));
    }

    #[test]
    fn validate_slug_accepts_safe_input() {
        assert!(validate_slug("e2e-beads").is_ok());
        assert!(validate_slug("issue42").is_ok());
        assert!(validate_slug("a_b-c").is_ok());
    }

    #[test]
    fn validate_slug_rejects_empty() {
        let err = validate_slug("").unwrap_err().to_string();
        assert!(err.contains("must not be empty"));
    }

    #[test]
    fn validate_slug_rejects_path_separators_and_traversal() {
        assert!(validate_slug("foo/bar").is_err());
        assert!(validate_slug("foo\\bar").is_err());
        assert!(validate_slug("..").is_err());
        assert!(validate_slug(".").is_err());
    }

    #[test]
    fn validate_slug_rejects_whitespace_and_other_punct() {
        assert!(validate_slug("foo bar").is_err());
        assert!(validate_slug("foo.bar").is_err());
        assert!(validate_slug("foo:bar").is_err());
    }

    #[test]
    fn validate_slug_rejects_leading_or_trailing_dash() {
        assert!(validate_slug("-leading").is_err());
        assert!(validate_slug("trailing-").is_err());
    }

    #[test]
    fn validate_slug_rejects_too_long() {
        let too_long = "a".repeat(65);
        assert!(validate_slug(&too_long).is_err());
    }

    #[test]
    fn external_ref_gh_prefix_to_url() {
        assert_eq!(
            parse_external_ref_to_github_url(Some("gh-157")),
            Some("https://github.com/quarto-dev/q2/issues/157".to_string())
        );
    }

    #[test]
    fn external_ref_none_returns_none() {
        assert_eq!(parse_external_ref_to_github_url(None), None);
    }

    #[test]
    fn external_ref_empty_string_returns_none() {
        assert_eq!(parse_external_ref_to_github_url(Some("")), None);
    }

    #[test]
    fn external_ref_non_gh_prefix_returns_none() {
        assert_eq!(
            parse_external_ref_to_github_url(Some("linear-ABC-12")),
            None
        );
    }

    #[test]
    fn external_ref_malformed_gh_returns_none() {
        // Non-numeric suffix
        assert_eq!(parse_external_ref_to_github_url(Some("gh-foo")), None);
        // Empty suffix
        assert_eq!(parse_external_ref_to_github_url(Some("gh-")), None);
    }

    #[test]
    fn detect_le_empty_defaults_to_lf() {
        assert_eq!(detect_line_ending(""), "\n");
    }

    #[test]
    fn detect_le_no_newlines_defaults_to_lf() {
        assert_eq!(detect_line_ending("hello world"), "\n");
    }

    #[test]
    fn detect_le_lf_only() {
        assert_eq!(detect_line_ending("a\nb\nc\n"), "\n");
    }

    #[test]
    fn detect_le_crlf_pure() {
        assert_eq!(detect_line_ending("a\r\nb\r\nc\r\n"), "\r\n");
    }

    #[test]
    fn detect_le_mixed_falls_back_to_lf() {
        // CRLF + bare LF -> LF (do not propagate inconsistency)
        assert_eq!(detect_line_ending("a\r\nb\nc\r\n"), "\n");
    }

    #[test]
    fn detect_le_sniffs_only_first_1kb() {
        // Pad the head with LF, place a CRLF beyond the sniff window
        let mut s = "x\n".repeat(600); // 1200 bytes of LF-terminated lines
        s.push_str("z\r\n");
        assert_eq!(detect_line_ending(&s), "\n");
    }
}
