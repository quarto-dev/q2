//! Ambient Node.js discovery.
//!
//! GUI-launched MCP hosts (Claude Desktop in particular) start servers
//! with a minimal environment: on macOS, not the user's shell PATH —
//! so nvm/fnm/volta-managed Node is invisible to a naive PATH lookup
//! exactly in the flagship use case. Discovery is therefore layered:
//!
//! 1. `QUARTO_NODE` env var — explicit override, must satisfy the
//!    version floor (a too-old override is an error, not a fallthrough:
//!    the user asked for that binary specifically).
//! 2. `node` on PATH.
//! 3. Well-known install locations, including version-manager trees
//!    (Homebrew, /usr/local, volta, fnm, nvm, Windows Program Files).
//!
//! Layers 2–3 skip candidates below the floor and keep looking; if
//! nothing satisfies, the error names every candidate found and how to
//! fix it. The floor is Node 24 (current LTS; decided 2026-06-11, see
//! the plan's resolved open question 1).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

pub const MIN_NODE_MAJOR: u32 = 24;

#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub path: PathBuf,
    pub version: (u32, u32, u32),
}

#[derive(Debug, Error)]
pub enum NodeError {
    #[error(
        "QUARTO_NODE points at {path} but `--version` failed ({detail}); \
         fix or unset QUARTO_NODE"
    )]
    OverrideUnusable { path: PathBuf, detail: String },
    #[error(
        "QUARTO_NODE points at {path} (Node {found}), but `q2 mcp` needs Node {min}+; \
         point QUARTO_NODE at a newer Node or unset it to use automatic discovery"
    )]
    OverrideTooOld {
        path: PathBuf,
        found: String,
        min: u32,
    },
    #[error(
        "no usable Node.js found (need Node {min}+).{found_summary}\n\
         `q2 mcp` runs the Quarto Hub MCP server on your Node.js. Fixes:\n\
         - install Node {min}+ (https://nodejs.org), or\n\
         - set QUARTO_NODE to the full path of a Node {min}+ binary\n\
         (GUI apps like Claude Desktop don't see your shell PATH, so a\n\
         version-manager Node may need the QUARTO_NODE setting.)"
    )]
    NotFound { min: u32, found_summary: String },
}

/// Inputs to discovery, injectable for tests.
pub struct Discovery {
    pub quarto_node: Option<OsString>,
    pub path_var: Option<OsString>,
    pub well_known: Vec<PathBuf>,
}

impl Discovery {
    /// Discovery against the real environment.
    pub fn from_env() -> Self {
        Self {
            quarto_node: std::env::var_os("QUARTO_NODE"),
            path_var: std::env::var_os("PATH"),
            well_known: default_well_known(),
        }
    }
}

pub fn find_node(d: &Discovery) -> Result<NodeInfo, NodeError> {
    // Layer 1: explicit override.
    if let Some(raw) = &d.quarto_node {
        let path = PathBuf::from(raw);
        return match probe_version(&path) {
            Ok(version) if version.0 >= MIN_NODE_MAJOR => Ok(NodeInfo { path, version }),
            Ok(version) => Err(NodeError::OverrideTooOld {
                path,
                found: format!("{}.{}.{}", version.0, version.1, version.2),
                min: MIN_NODE_MAJOR,
            }),
            Err(detail) => Err(NodeError::OverrideUnusable { path, detail }),
        };
    }

    let mut rejected: Vec<String> = Vec::new();

    // Layer 2: PATH.
    let exe = if cfg!(windows) { "node.exe" } else { "node" };
    if let Some(path_var) = &d.path_var {
        for dir in std::env::split_paths(path_var) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            let candidate = dir.join(exe);
            if let Some(found) = consider(&candidate, &mut rejected) {
                return Ok(found);
            }
        }
    }

    // Layer 3: well-known locations.
    for candidate in &d.well_known {
        if let Some(found) = consider(candidate, &mut rejected) {
            return Ok(found);
        }
    }

    let found_summary = if rejected.is_empty() {
        String::new()
    } else {
        format!(" Found, but too old: {}.", rejected.join(", "))
    };
    Err(NodeError::NotFound {
        min: MIN_NODE_MAJOR,
        found_summary,
    })
}

/// Probe one candidate; record floor-failures for the error message.
fn consider(candidate: &Path, rejected: &mut Vec<String>) -> Option<NodeInfo> {
    if !candidate.is_file() {
        return None;
    }
    match probe_version(candidate) {
        Ok(version) if version.0 >= MIN_NODE_MAJOR => Some(NodeInfo {
            path: candidate.to_path_buf(),
            version,
        }),
        Ok(version) => {
            rejected.push(format!(
                "{} (v{}.{}.{})",
                candidate.display(),
                version.0,
                version.1,
                version.2
            ));
            None
        }
        Err(_) => None,
    }
}

/// Run `<path> --version` and parse `vMAJOR.MINOR.PATCH`.
pub fn probe_version(path: &Path) -> Result<(u32, u32, u32), String> {
    let output = Command::new(path)
        .arg("--version")
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!("exit status {}", output.status));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_version(stdout.trim())
        .ok_or_else(|| format!("unrecognized --version output {:?}", stdout.trim()))
}

pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.strip_prefix('v').unwrap_or(s);
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts
        .next()
        .map(|p| {
            // tolerate suffixes like "0-nightly..."
            p.split(|c: char| !c.is_ascii_digit())
                .next()
                .unwrap_or("0")
                .to_string()
        })
        .unwrap_or_else(|| "0".to_string())
        .parse()
        .ok()?;
    Some((major, minor, patch))
}

/// Static probe list for layer 3. Existence is checked at probe time,
/// so listing paths that usually don't exist is free. Version-manager
/// trees with per-version dirs get expanded to the highest version.
fn default_well_known() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    let home = dirs::home_dir();

    #[cfg(unix)]
    {
        paths.extend([
            PathBuf::from("/opt/homebrew/bin/node"),
            PathBuf::from("/usr/local/bin/node"),
            PathBuf::from("/usr/bin/node"),
        ]);
        if let Some(home) = &home {
            // volta and fnm keep stable "current default" paths.
            paths.push(home.join(".volta/bin/node"));
            paths.push(home.join(".local/share/fnm/aliases/default/bin/node"));
            paths.push(home.join("Library/Application Support/fnm/aliases/default/bin/node"));
            // nvm has one dir per installed version; take the highest.
            paths.extend(highest_version_node(&home.join(".nvm/versions/node")));
            paths.extend(highest_version_node(
                &home.join("Library/Application Support/fnm/node-versions"),
            ));
            paths.extend(highest_version_node(
                &home.join(".local/share/fnm/node-versions"),
            ));
        }
    }
    #[cfg(windows)]
    {
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            paths.push(PathBuf::from(pf).join("nodejs").join("node.exe"));
        }
        if let Some(home) = &home {
            paths.push(home.join("AppData/Local/Volta/bin/node.exe"));
        }
    }
    let _ = home;
    paths
}

/// In a version-manager tree (`<root>/v24.1.0/...`), the node binary
/// of the highest version present.
fn highest_version_node(root: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    let mut best: Option<((u32, u32, u32), PathBuf)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(version) = parse_version(&name.to_string_lossy()) else {
            continue;
        };
        // nvm: <root>/vX.Y.Z/bin/node; fnm: <root>/vX.Y.Z/installation/bin/node
        for sub in ["bin/node", "installation/bin/node"] {
            let candidate = entry.path().join(sub);
            if candidate.is_file() && best.as_ref().is_none_or(|(v, _)| version > *v) {
                best = Some((version, candidate.clone()));
            }
        }
    }
    best.map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_versions() {
        assert_eq!(parse_version("v24.15.0"), Some((24, 15, 0)));
        assert_eq!(parse_version("22.1.3"), Some((22, 1, 3)));
        assert_eq!(parse_version("v25.0.0-nightly20260101"), Some((25, 0, 0)));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("node"), None);
    }
}
