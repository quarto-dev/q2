//! Tests for scripts/homebrew-formula.sh, which renders the Homebrew
//! formula for a release from its `checksums.sha256`.
//!
//! Unix-only like nightly_gate.rs: the script is bash.

#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const PLATFORMS: [&str; 4] = ["darwin_arm64", "darwin_amd64", "linux_arm64", "linux_amd64"];

/// A checksums file like the release job's, with a distinct sha per
/// archive (the platform's index repeated) plus a Windows zip.
fn checksums(version: &str, skip: Option<&str>) -> String {
    let mut out = String::new();
    for (i, p) in PLATFORMS.iter().enumerate() {
        if Some(*p) != skip {
            out += &format!(
                "{}  q2-{version}-{p}.tar.gz\n",
                (i + 1).to_string().repeat(64)
            );
        }
    }
    out + &format!("{}  q2-{version}-windows_amd64.zip\n", "9".repeat(64))
}

fn render(version: &str, checksums: &str) -> Output {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("checksums.sha256");
    fs::write(&file, checksums).unwrap();
    Command::new("bash")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/homebrew-formula.sh"))
        .arg(version)
        .arg(&file)
        .output()
        .unwrap()
}

fn assert_fails_mentioning(out: &Output, needle: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "expected failure");
    assert!(stderr.contains(needle), "stderr lacks {needle:?}: {stderr}");
    assert!(out.stdout.is_empty(), "printed a formula on failure");
}

#[test]
fn renders_each_platform_url_with_its_sha() {
    let out = render("1.2.3", &checksums("1.2.3", None));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let formula = String::from_utf8(out.stdout).unwrap();
    assert!(formula.contains("version \"1.2.3\""), "{formula}");

    let mut last = formula.find("on_macos do").unwrap();
    for (i, p) in PLATFORMS.iter().enumerate() {
        let url = format!(
            "url \"https://github.com/quarto-dev/q2/releases/download/v1.2.3/q2-1.2.3-{p}.tar.gz\""
        );
        let at = formula
            .find(&url)
            .unwrap_or_else(|| panic!("no {url}\n{formula}"));
        // Platforms appear in on_macos/on_linux, on_arm/on_intel order.
        assert!(at > last, "{p} out of order\n{formula}");
        last = at;
        let sha = formula[at..].lines().nth(1).unwrap().trim();
        assert_eq!(
            sha,
            format!("sha256 \"{}\"", (i + 1).to_string().repeat(64))
        );
    }
    assert!(formula.find("on_linux do").unwrap() > formula.find("darwin_amd64").unwrap());
    assert!(!formula.contains("windows"), "{formula}");

    // Ruby syntax check where ruby is available (it is on CI runners).
    if Command::new("ruby").arg("--version").output().is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let rb = tmp.path().join("q2.rb");
        fs::write(&rb, &formula).unwrap();
        let check = Command::new("ruby").arg("-c").arg(&rb).output().unwrap();
        assert!(
            check.status.success(),
            "{}",
            String::from_utf8_lossy(&check.stderr)
        );
    }
}

#[test]
fn missing_archive_fails() {
    let out = render("1.2.3", &checksums("1.2.3", Some("linux_arm64")));
    assert_fails_mentioning(&out, "q2-1.2.3-linux_arm64.tar.gz");
}

#[test]
fn another_versions_archives_do_not_match() {
    let out = render("1.2.3", &checksums("1.2.30", None));
    assert_fails_mentioning(&out, "q2-1.2.3-darwin_arm64.tar.gz");
}

#[test]
fn only_bare_stable_versions_are_accepted() {
    for bad in ["0.43.0-nightly.20261107", "v1.2.3"] {
        assert_fails_mentioning(&render(bad, &checksums(bad, None)), "version");
    }
}
