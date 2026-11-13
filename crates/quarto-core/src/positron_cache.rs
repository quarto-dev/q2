//! Locating Positron's inline-output cache file for a document (RIU-3a Phase C).
//!
//! Native-only (`cfg(not(target_arch = "wasm32"))` at the declaration): it reads the
//! filesystem and the platform's user-data directory. It lives outside the `record` module,
//! which the WASM client compiles. `quarto-hub-provider` needs it too and does not depend on
//! `quarto-preview`, so it is here.
//!
//! A cache file is `<directory>/<sha1(URI.toString())[0:16]>.ipynb`. Three tiers find it:
//!
//! 1. the file named by the hash of the document's `file://` URI;
//! 2. a scan of the directory for a file whose `metadata.quarto_source` names the same path,
//!    compared as decoded, normalized paths (not raw strings);
//! 3. the file whose cells share the most content hashes with the document (Positron's own
//!    fallback for untitled documents), the newest `last_updated` on a tie; a tie that
//!    remains is refused with a diagnostic, never guessed.
//!
//! Read-only: nothing is ever written into Positron's directory.
//!
//! # What is verified
//!
//! The macOS rules and the file name for a plain `file://` path were checked against files
//! Positron wrote. **Unverified** (ported from VS Code's `userDataPath.ts` and `uri.ts` by
//! reading, never run against a real Positron): the Linux, Windows, `POSITRON_PORTABLE` and
//! `POSITRON_APPDATA` directory rules, and the percent-encoding of spaces, non-ASCII
//! characters and Windows drive letters. The scan and content-hash tiers and the explicit
//! directory override exist for exactly that reason.

use std::path::{Path, PathBuf};

use sha1::{Digest, Sha1};

use crate::record::{ImportError, PositronImport, import_positron_cache};

/// The directory under the user-data path where the extension stores its cache.
const CACHE_SUBDIR: &[&str] = &["User", "globalStorage", "quarto-inline-outputs"];

/// The host operating system, a parameter so every rule is testable on any host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostOs {
    MacOs,
    Linux,
    Windows,
}

impl HostOs {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            HostOs::MacOs
        } else if cfg!(windows) {
            HostOs::Windows
        } else {
            HostOs::Linux
        }
    }
}

/// The environment the directory rules read, a parameter for the same reason.
#[derive(Debug, Clone, Default)]
pub struct HostEnv {
    pub home: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>,
    pub appdata: Option<PathBuf>,
    pub userprofile: Option<PathBuf>,
    pub positron_portable: Option<PathBuf>,
    pub positron_appdata: Option<PathBuf>,
    /// `QUARTO_POSITRON_CACHE_DIR`: the explicit directory for hosts whose layout the rules
    /// cannot derive (`--user-data-dir`, a non-standard install). It replaces every rule.
    pub cache_dir_override: Option<PathBuf>,
}

impl HostEnv {
    pub fn from_process() -> Self {
        let var = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        Self {
            home: dirs::home_dir(),
            xdg_config_home: var("XDG_CONFIG_HOME"),
            appdata: var("APPDATA"),
            userprofile: var("USERPROFILE"),
            positron_portable: var("POSITRON_PORTABLE"),
            positron_appdata: var("POSITRON_APPDATA"),
            cache_dir_override: var("QUARTO_POSITRON_CACHE_DIR"),
        }
    }
}

fn cache_dir_under(user_data: &Path) -> PathBuf {
    CACHE_SUBDIR
        .iter()
        .fold(user_data.to_path_buf(), |p, s| p.join(s))
}

/// The directories Positron's cache may be in on this host, most likely first. An explicit
/// override replaces them (the caller handles that; see [`Locator`]).
pub fn default_directories(os: HostOs, env: &HostEnv) -> Vec<PathBuf> {
    if let Some(dir) = &env.cache_dir_override {
        return vec![dir.clone()];
    }
    let mut out = Vec::new();
    // `userDataPath.ts:58-62` and `:68-72` (unverified): the portable and app-data overrides
    // win over the platform default.
    if let Some(p) = &env.positron_portable {
        out.push(cache_dir_under(&p.join("user-data")));
    }
    if let Some(p) = &env.positron_appdata {
        out.push(cache_dir_under(&p.join("Positron")));
    }
    match os {
        // Verified on disk: a release build is `Positron`, a dev build `code-oss-dev`.
        HostOs::MacOs => {
            if let Some(home) = &env.home {
                let base = home.join("Library").join("Application Support");
                out.push(cache_dir_under(&base.join("Positron")));
                out.push(cache_dir_under(&base.join("code-oss-dev")));
            }
        }
        // The config directory, not `dirs::data_dir()` (`userDataPath.ts:110`, unverified).
        HostOs::Linux => {
            let config = env
                .xdg_config_home
                .clone()
                .or_else(|| env.home.as_ref().map(|h| h.join(".config")));
            if let Some(config) = config {
                out.push(cache_dir_under(&config.join("Positron")));
            }
        }
        // `%APPDATA%`, else `%USERPROFILE%\AppData\Roaming` (`:96-103`, unverified).
        HostOs::Windows => {
            let roaming = env.appdata.clone().or_else(|| {
                env.userprofile
                    .as_ref()
                    .map(|u| u.join("AppData").join("Roaming"))
            });
            if let Some(roaming) = roaming {
                out.push(cache_dir_under(&roaming.join("Positron")));
            }
        }
    }
    out
}

/// VS Code's `URI.toString()` for a `file` URI, ported by reading `uri.ts` (unverified for
/// spaces, non-ASCII characters and drive letters; verified for a plain macOS path). It is not
/// the `url` crate's serialization: it percent-encodes everything outside
/// `[A-Za-z0-9-._~]` and `/` as UTF-8 escapes with upper-case hex, lower-cases a Windows drive
/// letter and writes its colon as `%3A`.
pub fn vscode_file_uri(path: &str, os: HostOs) -> String {
    let mut p = if os == HostOs::Windows {
        path.replace('\\', "/")
    } else {
        path.to_string()
    };
    if !p.starts_with('/') {
        p.insert(0, '/');
    }
    let b = p.as_bytes();
    if b.len() >= 3 && b[2] == b':' && b[1].is_ascii_alphabetic() {
        p = format!("/{}:{}", (b[1] as char).to_ascii_lowercase(), &p[3..]);
    }
    let mut out = String::from("file://");
    for byte in p.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The cache file name for a document URI: `sha1(uri)[0:16]` plus `.ipynb` (`_hashUri`).
pub fn cache_file_name(uri: &str) -> String {
    let digest = Sha1::digest(uri.as_bytes());
    format!("{}.ipynb", &hex::encode(digest)[..16])
}

/// A `file://` URI or a path, reduced to a comparable form: percent-decoded, backslashes as
/// `/`, a drive letter lower-cased with its colon (`%3A` read as `:`).
pub fn normalize_location(location: &str) -> String {
    let rest = location.strip_prefix("file://").unwrap_or(location);
    let decoded = percent_encoding::percent_decode_str(rest)
        .decode_utf8_lossy()
        .replace('\\', "/");
    let b = decoded.as_bytes();
    // `/C:/x` (a URI path) or `C:/x` (a path): both become `c:/x`.
    let trimmed = if b.len() >= 3 && b[0] == b'/' && b[2] == b':' && b[1].is_ascii_alphabetic() {
        &decoded[1..]
    } else {
        decoded.as_str()
    };
    let tb = trimmed.as_bytes();
    if tb.len() >= 2 && tb[1] == b':' && tb[0].is_ascii_alphabetic() {
        format!("{}{}", (tb[0] as char).to_ascii_lowercase(), &trimmed[1..])
    } else {
        trimmed.to_string()
    }
}

/// Which tier found the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    UriName,
    QuartoSource,
    ContentHash,
}

/// A cache file that was found and imported.
#[derive(Debug)]
pub struct Located {
    pub path: PathBuf,
    pub tier: Tier,
    pub import: PositronImport,
}

/// Why no cache was supplied. None of these is an error for the caller: the walk reports the
/// message and moves to the next source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocateError {
    /// No directory holds a file for this document.
    NotFound,
    /// A file was found and cannot be used (a version other than 1, invalid structure, not
    /// readable). The file is left alone.
    Rejected(String),
    /// Two files fit equally well; q2 does not guess.
    Ambiguous(String),
}

impl std::fmt::Display for LocateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LocateError::NotFound => f.write_str("no cache file found for this document"),
            LocateError::Rejected(m) | LocateError::Ambiguous(m) => f.write_str(m),
        }
    }
}

/// Find and import the cache file for the document at `doc_path`.
///
/// `directories` are searched in order, each through all three tiers before the next.
/// `doc_hashes` are the content hashes of the document's units, for the third tier.
pub fn locate_cache(
    directories: &[PathBuf],
    doc_path: &Path,
    doc_hashes: &[String],
    os: HostOs,
) -> Result<Located, LocateError> {
    let doc = doc_path.to_string_lossy();
    let uri = vscode_file_uri(&doc, os);
    let doc_normal = normalize_location(&doc);
    let mut ambiguous = None;
    for dir in directories {
        // Tier 1: the name the URI hashes to.
        let named = dir.join(cache_file_name(&uri));
        if named.is_file() {
            return import_file(&named, Tier::UriName);
        }
        let files = cache_files(dir);
        // Tier 2: a file whose `quarto_source` is the same location.
        if let Some((path, _)) = files.iter().find(|(_, meta)| {
            meta.source.as_deref().map(normalize_location).as_deref() == Some(&doc_normal)
        }) {
            return import_file(path, Tier::QuartoSource);
        }
        // Tier 3: the most shared content hashes.
        let wanted: std::collections::BTreeSet<&str> =
            doc_hashes.iter().map(String::as_str).collect();
        let mut scored: Vec<(usize, u64, &PathBuf)> = files
            .iter()
            .map(|(p, m)| {
                let shared = m
                    .hashes
                    .iter()
                    .filter(|h| wanted.contains(h.as_str()))
                    .count();
                (shared, m.last_updated, p)
            })
            .filter(|(shared, _, _)| *shared > 0)
            .collect();
        scored.sort_by_key(|s| std::cmp::Reverse((s.0, s.1)));
        match scored.as_slice() {
            [] => {}
            [only] => return import_file(only.2, Tier::ContentHash),
            [first, second, ..] if (first.0, first.1) != (second.0, second.1) => {
                return import_file(first.2, Tier::ContentHash);
            }
            [first, second, ..] => {
                ambiguous = Some(format!(
                    "{} and {} fit this document equally well (same number of matching cells, same \
                     last-updated time); not choosing between them",
                    first.2.display(),
                    second.2.display()
                ));
            }
        }
    }
    Err(match ambiguous {
        Some(m) => LocateError::Ambiguous(m),
        None => LocateError::NotFound,
    })
}

fn import_file(path: &Path, tier: Tier) -> Result<Located, LocateError> {
    let bytes = std::fs::read(path)
        .map_err(|e| LocateError::Rejected(format!("cannot read {}: {e}", path.display())))?;
    match import_positron_cache(&bytes) {
        Ok(import) => Ok(Located {
            path: path.to_path_buf(),
            tier,
            import,
        }),
        Err(
            e @ (ImportError::Json(_)
            | ImportError::InvalidStructure(_)
            | ImportError::VersionMismatch { .. }),
        ) => Err(LocateError::Rejected(format!(
            "{} is not a Positron cache this version of q2 can read: {e}",
            path.display()
        ))),
    }
}

/// What the directory scan reads from each file.
struct FileMeta {
    source: Option<String>,
    last_updated: u64,
    hashes: Vec<String>,
}

fn cache_files(dir: &Path) -> Vec<(PathBuf, FileMeta)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "ipynb"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| {
            let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).ok()?).ok()?;
            let meta = value.get("metadata")?;
            let hashes = value
                .get("cells")?
                .as_array()?
                .iter()
                .filter_map(|c| c.get("metadata")?.get("quarto_content_hash")?.as_str())
                .map(str::to_string)
                .collect();
            Some((
                p,
                FileMeta {
                    source: meta
                        .get("quarto_source")
                        // lint:allow(metadata-as-str) serde_json::Value from a Positron cache file, not document metadata
                        .and_then(|s| s.as_str())
                        .map(str::to_string),
                    last_updated: meta
                        .get("last_updated")
                        .and_then(|n| n.as_u64())
                        .unwrap_or(0),
                    hashes,
                },
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // File names (expected values computed independently from the algorithm) -------------

    #[test]
    fn the_file_name_of_a_real_positron_cache_is_reproduced() {
        // Verified: Positron wrote `bcfd94bdc5437bc3.ipynb` for this document.
        let uri = vscode_file_uri("/Users/gordon/docs/inline-output/python.qmd", HostOs::MacOs);
        assert_eq!(uri, "file:///Users/gordon/docs/inline-output/python.qmd");
        assert_eq!(cache_file_name(&uri), "bcfd94bdc5437bc3.ipynb");
    }

    #[test]
    fn uris_encode_the_way_vs_code_does_unverified_against_a_real_positron() {
        let name = |p: &str, os| cache_file_name(&vscode_file_uri(p, os));
        assert_eq!(
            vscode_file_uri("/Users/me/my docs/a.qmd", HostOs::MacOs),
            "file:///Users/me/my%20docs/a.qmd"
        );
        assert_eq!(
            name("/Users/me/my docs/a.qmd", HostOs::MacOs),
            "0650a77c57196fba.ipynb"
        );
        assert_eq!(
            name("/Users/me/café/a.qmd", HostOs::MacOs),
            "027e10c7b096d18a.ipynb"
        );
        assert_eq!(
            name("/home/me/a.qmd", HostOs::Linux),
            "92495bf11450a768.ipynb"
        );
        // A Windows drive letter is lower-cased and its colon is `%3A`.
        assert_eq!(
            vscode_file_uri(r"C:\Users\me\a.qmd", HostOs::Windows),
            "file:///c%3A/Users/me/a.qmd"
        );
        assert_eq!(
            name(r"C:\Users\me\a.qmd", HostOs::Windows),
            "e3981b1f31215ad4.ipynb"
        );
    }

    #[test]
    fn locations_normalize_to_one_comparable_form() {
        let want = "c:/Users/me/a.qmd";
        assert_eq!(normalize_location("file:///c%3A/Users/me/a.qmd"), want);
        assert_eq!(normalize_location("file:///C:/Users/me/a.qmd"), want);
        assert_eq!(normalize_location(r"C:\Users\me\a.qmd"), want);
        assert_eq!(
            normalize_location("file:///Users/me/caf%C3%A9/a.qmd"),
            "/Users/me/café/a.qmd"
        );
    }

    // Directory rules, one per row of the plan's table ----------------------------------

    fn env() -> HostEnv {
        HostEnv {
            home: Some(PathBuf::from("/h")),
            ..Default::default()
        }
    }
    fn tail(p: &Path) -> String {
        // The tail every rule ends with, so each test names only what differs.
        let rest: PathBuf = p
            .components()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        rest.to_string_lossy().replace('\\', "/")
    }

    #[test]
    fn macos_tries_the_release_directory_then_the_dev_build() {
        let d = default_directories(HostOs::MacOs, &env());
        assert_eq!(d.len(), 2);
        assert!(d[0].ends_with(Path::new(
            "Library/Application Support/Positron/User/globalStorage/quarto-inline-outputs"
        )));
        assert!(d[1].ends_with(Path::new(
            "Library/Application Support/code-oss-dev/User/globalStorage/quarto-inline-outputs"
        )));
        assert_eq!(tail(&d[0]), "User/globalStorage/quarto-inline-outputs");
    }

    #[test]
    fn linux_uses_the_config_directory_with_xdg_taking_precedence() {
        let d = default_directories(HostOs::Linux, &env());
        assert_eq!(
            d,
            [PathBuf::from(
                "/h/.config/Positron/User/globalStorage/quarto-inline-outputs"
            )]
        );
        let with_xdg = HostEnv {
            xdg_config_home: Some("/x".into()),
            ..env()
        };
        assert_eq!(
            default_directories(HostOs::Linux, &with_xdg),
            [PathBuf::from(
                "/x/Positron/User/globalStorage/quarto-inline-outputs"
            )]
        );
    }

    #[test]
    fn windows_uses_appdata_else_the_roaming_directory_under_the_profile() {
        let a = HostEnv {
            appdata: Some("/ad".into()),
            ..Default::default()
        };
        assert_eq!(
            default_directories(HostOs::Windows, &a),
            [PathBuf::from(
                "/ad/Positron/User/globalStorage/quarto-inline-outputs"
            )]
        );
        let u = HostEnv {
            userprofile: Some("/up".into()),
            ..Default::default()
        };
        assert_eq!(
            default_directories(HostOs::Windows, &u),
            [PathBuf::from(
                "/up/AppData/Roaming/Positron/User/globalStorage/quarto-inline-outputs"
            )]
        );
    }

    #[test]
    fn the_portable_and_app_data_overrides_come_first() {
        let e = HostEnv {
            positron_portable: Some("/p".into()),
            positron_appdata: Some("/pa".into()),
            ..env()
        };
        let d = default_directories(HostOs::Linux, &e);
        assert_eq!(
            d[0],
            PathBuf::from("/p/user-data/User/globalStorage/quarto-inline-outputs")
        );
        assert_eq!(
            d[1],
            PathBuf::from("/pa/Positron/User/globalStorage/quarto-inline-outputs")
        );
        assert_eq!(d.len(), 3);
    }

    #[test]
    fn an_explicit_directory_replaces_every_rule() {
        let e = HostEnv {
            cache_dir_override: Some("/chosen".into()),
            positron_portable: Some("/p".into()),
            ..env()
        };
        assert_eq!(
            default_directories(HostOs::MacOs, &e),
            [PathBuf::from("/chosen")]
        );
    }

    #[test]
    fn without_a_home_there_is_nothing_to_search() {
        assert!(default_directories(HostOs::MacOs, &HostEnv::default()).is_empty());
    }

    // The three tiers --------------------------------------------------------------------

    fn cache(source: &str, last_updated: u64, hashes: &[&str]) -> Vec<u8> {
        let cells: Vec<_> = hashes
            .iter()
            .map(|h| json!({"cell_type": "code", "metadata": {"quarto_content_hash": h}, "outputs": [], "source": []}))
            .collect();
        serde_json::to_vec(&json!({
            "nbformat": 4, "nbformat_minor": 5,
            "metadata": {"quarto_source": source, "quarto_cache_version": 1, "last_updated": last_updated},
            "cells": cells
        }))
        .unwrap()
    }

    fn write(dir: &Path, name: &str, bytes: Vec<u8>) {
        std::fs::write(dir.join(name), bytes).unwrap();
    }

    const DOC: &str = "/Users/gordon/docs/inline-output/python.qmd";

    fn locate(dir: &Path, hashes: &[&str]) -> Result<Located, LocateError> {
        let hashes: Vec<String> = hashes.iter().map(|s| s.to_string()).collect();
        locate_cache(&[dir.to_path_buf()], Path::new(DOC), &hashes, HostOs::MacOs)
    }

    #[test]
    fn tier_one_reads_the_file_the_uri_hashes_to() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "bcfd94bdc5437bc3.ipynb",
            cache("file:///elsewhere.qmd", 1, &["a"]),
        );
        // A decoy that would win on content hashes: tier one does not look at it.
        write(
            dir.path(),
            "0000000000000000.ipynb",
            cache("file:///other.qmd", 9, &["a", "b"]),
        );
        let got = locate(dir.path(), &["a", "b"]).unwrap();
        assert_eq!(got.tier, Tier::UriName);
        assert!(got.path.ends_with("bcfd94bdc5437bc3.ipynb"));
    }

    #[test]
    fn tier_two_finds_a_file_by_its_quarto_source_compared_as_a_decoded_path() {
        let dir = tempfile::tempdir().unwrap();
        // A different name (a hand-built or renamed cache) and an encoded source.
        write(
            dir.path(),
            "renamed.ipynb",
            cache(
                "file:///Users/gordon/docs/inline-output/python.qmd",
                1,
                &["z"],
            ),
        );
        write(
            dir.path(),
            "decoy.ipynb",
            cache("file:///other.qmd", 1, &["a"]),
        );
        let got = locate(dir.path(), &["a"]).unwrap();
        assert_eq!(
            got.tier,
            Tier::QuartoSource,
            "the source beats the content-hash decoy"
        );
        assert!(got.path.ends_with("renamed.ipynb"));
    }

    #[test]
    fn tier_two_ignores_an_encoding_difference() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "x.ipynb",
            cache("file:///Users/me/caf%C3%A9/a.qmd", 1, &["z"]),
        );
        let got = locate_cache(
            &[dir.path().to_path_buf()],
            Path::new("/Users/me/café/a.qmd"),
            &[],
            HostOs::MacOs,
        )
        .unwrap();
        assert_eq!(got.tier, Tier::QuartoSource);
    }

    #[test]
    fn tier_three_takes_the_most_shared_hashes_and_the_newest_on_a_tie() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "few.ipynb", cache("file:///a.qmd", 9, &["a"]));
        write(
            dir.path(),
            "many.ipynb",
            cache("file:///b.qmd", 1, &["a", "b"]),
        );
        let got = locate(dir.path(), &["a", "b", "c"]).unwrap();
        assert_eq!(got.tier, Tier::ContentHash);
        assert!(got.path.ends_with("many.ipynb"));

        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "old.ipynb",
            cache("file:///a.qmd", 1, &["a", "b"]),
        );
        write(
            dir.path(),
            "new.ipynb",
            cache("file:///b.qmd", 2, &["a", "b"]),
        );
        assert!(
            locate(dir.path(), &["a", "b"])
                .unwrap()
                .path
                .ends_with("new.ipynb")
        );
    }

    #[test]
    fn a_tie_that_remains_is_refused_with_a_message_naming_both_files() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "one.ipynb", cache("file:///a.qmd", 5, &["a"]));
        write(dir.path(), "two.ipynb", cache("file:///b.qmd", 5, &["a"]));
        let Err(LocateError::Ambiguous(m)) = locate(dir.path(), &["a"]) else {
            panic!("expected an ambiguity")
        };
        assert!(m.contains("one.ipynb") && m.contains("two.ipynb"), "{m}");
    }

    #[test]
    fn no_file_and_no_shared_cell_is_not_found_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            locate(dir.path(), &["a"]).unwrap_err(),
            LocateError::NotFound
        );
        write(dir.path(), "x.ipynb", cache("file:///a.qmd", 1, &["q"]));
        assert_eq!(
            locate(dir.path(), &["a"]).unwrap_err(),
            LocateError::NotFound
        );
        let missing = dir.path().join("missing");
        assert_eq!(
            locate_cache(&[missing], Path::new(DOC), &[], HostOs::MacOs).unwrap_err(),
            LocateError::NotFound
        );
    }

    #[test]
    fn a_file_with_another_cache_version_is_rejected_with_a_message_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let mut v: serde_json::Value =
            serde_json::from_slice(&cache("file:///a.qmd", 1, &["a"])).unwrap();
        v["metadata"]["quarto_cache_version"] = json!(2);
        let bytes = serde_json::to_vec(&v).unwrap();
        write(dir.path(), "bcfd94bdc5437bc3.ipynb", bytes.clone());
        let Err(LocateError::Rejected(m)) = locate(dir.path(), &["a"]) else {
            panic!("expected a rejection")
        };
        assert!(m.contains("version"), "{m}");
        assert_eq!(
            std::fs::read(dir.path().join("bcfd94bdc5437bc3.ipynb")).unwrap(),
            bytes,
            "never modified"
        );
    }

    #[test]
    fn a_plain_notebook_without_a_cache_version_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let plain = json!({"nbformat": 4, "nbformat_minor": 5, "metadata": {}, "cells": []});
        write(
            dir.path(),
            "bcfd94bdc5437bc3.ipynb",
            serde_json::to_vec(&plain).unwrap(),
        );
        let Err(LocateError::Rejected(m)) = locate(dir.path(), &[]) else {
            panic!("expected a rejection")
        };
        assert!(m.contains("quarto_cache_version"), "{m}");
    }

    #[test]
    fn later_directories_are_tried_when_an_earlier_one_has_nothing() {
        let empty = tempfile::tempdir().unwrap();
        let full = tempfile::tempdir().unwrap();
        write(
            full.path(),
            "bcfd94bdc5437bc3.ipynb",
            cache("file:///a.qmd", 1, &["a"]),
        );
        let got = locate_cache(
            &[empty.path().to_path_buf(), full.path().to_path_buf()],
            Path::new(DOC),
            &[],
            HostOs::MacOs,
        )
        .unwrap();
        assert!(got.path.starts_with(full.path()));
    }
}
