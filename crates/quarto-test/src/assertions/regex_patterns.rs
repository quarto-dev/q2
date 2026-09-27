use anyhow::{Context, Result, bail};
use regex::Regex;

/// Compile multiline regex patterns for an output assertion.
pub(super) fn compile_patterns(
    matches: &[String],
    no_matches: &[String],
) -> Result<(Vec<Regex>, Vec<Regex>)> {
    let compile = |patterns: &[String]| {
        patterns
            .iter()
            .map(|pattern| {
                Regex::new(&format!("(?m){pattern}"))
                    .with_context(|| format!("invalid regex pattern: {pattern}"))
            })
            .collect::<Result<Vec<_>>>()
    };

    Ok((compile(matches)?, compile(no_matches)?))
}

/// Verify required and forbidden patterns against text content.
pub(super) fn verify_patterns(
    content: &str,
    matches: &[Regex],
    no_matches: &[Regex],
    match_patterns: &[String],
    no_match_patterns: &[String],
    description: &str,
) -> Result<()> {
    let mut failures = Vec::new();

    for (regex, pattern) in matches.iter().zip(match_patterns) {
        if !regex.is_match(content) {
            failures.push(format!("Required pattern not found: {pattern}"));
        }
    }

    for (regex, pattern) in no_matches.iter().zip(no_match_patterns) {
        if regex.is_match(content) {
            failures.push(format!("Illegal pattern found: {pattern}"));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        bail!(
            "{} regex mismatch(es) in {}:\n  - {}",
            failures.len(),
            description,
            failures.join("\n  - ")
        )
    }
}
