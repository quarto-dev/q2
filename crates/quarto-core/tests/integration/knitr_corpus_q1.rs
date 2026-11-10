//! KNR-1 Phase 1D: the Quarto 1 oracle (epic decision 9, plan item "Quarto 1 oracle").
//!
//! How Quarto 1 output is gathered: when `quarto` on `PATH` is TypeScript Quarto (version `1.x`,
//! or `99.9.9`, which a quarto-cli development checkout reports), each fixture is rendered with
//! `quarto render doc.<ext> --to html -M keep-md:true` in a scratch directory, and the kept
//! post-engine markdown (`doc.html.md`, front matter removed) is the Quarto 1 reference. It is
//! compared with q2's *current* knitr output (the live path), not with the baseline: a
//! difference there is a gap in what q2 does today. Skipped (no Q1 column) when no such
//! `quarto` is found, or unless `KNR_Q1` is set (a render takes seconds).

#![cfg(not(target_arch = "wasm32"))]

use std::process::Command;
use std::sync::OnceLock;

use super::knitr_corpus_run::materialize;
use super::knitr_corpus_support::Fixture;

/// The version string of the `quarto` on `PATH`, when it is TypeScript Quarto.
pub fn q1_version() -> Option<&'static str> {
    static V: OnceLock<Option<String>> = OnceLock::new();
    V.get_or_init(|| {
        let out = Command::new("quarto").arg("--version").output().ok()?;
        let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (v.starts_with("1.") || v == "99.9.9").then_some(v)
    })
    .as_deref()
}

/// Whether the Q1 column was asked for and can be produced.
pub fn q1_enabled() -> bool {
    std::env::var_os("KNR_Q1").is_some() && q1_version().is_some()
}

/// Q1's kept post-engine markdown for `f`, front matter removed.
pub fn q1_markdown(f: &Fixture) -> Result<String, String> {
    let (dir, doc) = materialize(f);
    let out = Command::new("quarto")
        .arg("render")
        .arg(&doc)
        .args(["--to", "html", "-M", "keep-md:true"])
        .current_dir(dir.path())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr)
            .lines()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .join(" | "));
    }
    let kept = dir.path().join("doc.html.md");
    let text = std::fs::read_to_string(&kept).map_err(|e| format!("{}: {e}", kept.display()))?;
    Ok(strip_front_matter(&text))
}

/// Remove a leading YAML block.
pub fn strip_front_matter(text: &str) -> String {
    let Some(rest) = text.strip_prefix("---\n") else {
        return text.to_string();
    };
    match rest.find("\n---\n") {
        Some(i) => rest[i + 5..].trim_start_matches('\n').to_string(),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::strip_front_matter;

    #[test]
    fn front_matter_is_removed() {
        assert_eq!(strip_front_matter("---\na: 1\n---\n\nBody\n"), "Body\n");
        assert_eq!(strip_front_matter("No front matter\n"), "No front matter\n");
        assert_eq!(
            strip_front_matter("---\nunterminated\n"),
            "---\nunterminated\n"
        );
    }
}
