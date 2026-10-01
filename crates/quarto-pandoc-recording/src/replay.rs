//! Replays a recording with a native pandoc: materializes the recording's
//! file tree under a work directory, maps the placeholder roots to it, and
//! runs the recorded argv and env.

use crate::mapping::{DOC_ROOT, OUT_ROOT, PathMap, SHARE_ROOT, TMP_ROOT};
use crate::rewrite::Meta;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

/// Per-replay changes to the recorded request (exploration and fault testing).
#[derive(Default, Clone)]
pub struct Overrides {
    /// `NAME=VALUE` sets, a bare `NAME` removes.
    pub env: Vec<String>,
    /// Extra arguments inserted before the recorded ones.
    pub args: Vec<String>,
}

pub struct ReplayResult {
    pub status: i32,
    pub stderr: String,
    /// The bytes of the file named by `-o`, if pandoc wrote it.
    pub output: Option<Vec<u8>>,
    /// The recording's native reference output.
    pub reference: Option<Vec<u8>>,
}

fn placeholder_roots(rec: &Path) -> Result<Vec<String>> {
    let mut roots = vec![
        TMP_ROOT.to_string(),
        SHARE_ROOT.to_string(),
        DOC_ROOT.to_string(),
        OUT_ROOT.to_string(),
    ];
    if let Ok(rd) = std::fs::read_dir(rec.join("fs")) {
        for e in rd {
            let name = e?.file_name().to_string_lossy().into_owned();
            let root = format!("/{name}");
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
    }
    Ok(roots)
}

fn copy_mapped(from: &Path, to: &Path, map: &PathMap) -> Result<()> {
    for entry in WalkDir::new(from).sort_by_file_name() {
        let entry = entry?;
        let dest = to.join(entry.path().strip_prefix(from)?);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&dest)?;
        } else if entry.file_type().is_file() {
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p)?;
            }
            let bytes = std::fs::read(entry.path())?;
            let bytes = if bytes.windows(5).any(|w| w == b"/__q2") {
                map.apply_bytes(&bytes).unwrap_or(bytes)
            } else {
                bytes
            };
            std::fs::write(&dest, bytes)?;
        }
    }
    Ok(())
}

/// The fixed directory a recording is replayed in. Outputs embed the paths
/// pandoc ran at (docx image descriptions, for one), so byte-equal replays
/// need the same path every time; one directory per recording keeps
/// concurrent replays of different recordings apart. Unix only.
pub fn canonical_work_dir(name: &str) -> PathBuf {
    PathBuf::from("/tmp/q2-pandoc-replay").join(name)
}

fn resolve_program(program: &Path) -> PathBuf {
    if program.components().count() > 1 {
        return program.to_path_buf();
    }
    std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|d| d.join(program))
                .find(|c| c.is_file())
        })
        .unwrap_or_else(|| program.to_path_buf())
}

/// `set_root` holds `share/<tree-id>/`; `rec` is one recording directory;
/// `work` is an empty directory the replay may populate.
pub fn replay(rec: &Path, set_root: &Path, pandoc: &Path, work: &Path) -> Result<ReplayResult> {
    replay_with(rec, set_root, pandoc, work, &Overrides::default())
}

pub fn replay_with(
    rec: &Path,
    set_root: &Path,
    pandoc: &Path,
    work: &Path,
    overrides: &Overrides,
) -> Result<ReplayResult> {
    let meta: Meta = serde_json::from_slice(&std::fs::read(rec.join("meta.json"))?)?;
    let argv: Vec<String> = serde_json::from_slice(&std::fs::read(rec.join("argv.json"))?)?;
    let mut env: BTreeMap<String, String> =
        serde_json::from_slice(&std::fs::read(rec.join("env.json"))?)?;
    for o in &overrides.env {
        match o.split_once('=') {
            Some((k, v)) => env.insert(k.to_string(), v.to_string()),
            None => env.remove(o.as_str()),
        };
    }

    let work = work.to_path_buf();
    if work.exists() {
        std::fs::remove_dir_all(&work)?;
    }
    std::fs::create_dir_all(&work)?;
    let real = |root: &str| work.join(root.trim_start_matches('/'));
    let map = PathMap::new(
        placeholder_roots(rec)?
            .into_iter()
            .map(|root| {
                let to = real(&root).to_string_lossy().into_owned();
                (root, to)
            })
            .collect(),
    );

    std::fs::create_dir_all(real(OUT_ROOT))?;
    copy_mapped(
        &set_root.join("share").join(&meta.share_tree),
        &real(SHARE_ROOT),
        &map,
    )?;
    if rec.join("fs").is_dir() {
        copy_mapped(&rec.join("fs"), &work, &map)?;
    }
    let output_path = PathBuf::from(map.apply(&meta.output));
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // `env_clear` below also drops PATH, so resolve a bare name first.
    let pandoc = resolve_program(pandoc);
    let mut cmd = Command::new(&pandoc);
    cmd.args(&overrides.args)
        .args(argv[1..].iter().map(|a| map.apply(a)))
        .env_clear()
        .current_dir(&work);
    for (k, v) in &env {
        cmd.env(k, map.apply_env(k, v)?);
    }
    let out = cmd
        .output()
        .with_context(|| format!("running {}", pandoc.display()))?;
    Ok(ReplayResult {
        status: out.status.code().unwrap_or(-1),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        output: std::fs::read(&output_path).ok(),
        reference: match &meta.reference {
            Some(r) => Some(std::fs::read(rec.join(r))?),
            None => None,
        },
    })
}
