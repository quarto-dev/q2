//! Turns a raw capture (`scripts/pandoc-capture.sh`) into a committed,
//! machine-independent recording.
//!
//! Recording layout, under a set root `R`:
//! ```text
//! R/share/<tree-id>/...            the share tree, stored once per distinct content
//! R/share/<tree-id>.manifest.json
//! R/<name>/argv.json               ["pandoc", "-f", "json", ...] (argv[0] included)
//! R/<name>/env.json                {NAME: value}
//! R/<name>/meta.json               share tree id, exit status, output path, reference
//! R/<name>/fs/__q2_tmp__/...       pipeline temp files (input JSON, params, ...)
//! R/<name>/fs/__q2_doc__/...       the document directory
//! R/<name>/fs/__q2_inN__/...       any other path an argument names
//! R/<name>/reference/<output>      the native `-o` output
//! R/<name>/manifest.json           every file above (a browser cannot list directories)
//! ```

use crate::mapping::{DOC_ROOT, OUT_ROOT, PARAMS_VAR, PathMap, SHARE_ROOT, TMP_ROOT};
use crate::tree::{list_files, tree_id, write_manifest};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Pipeline temp entries pandoc never reads: the typst compile step's package
/// and font cache (fonts alone are ~2.5 MB per recording).
const UNUSED_TEMP_ENTRIES: &[&str] = &["typst-packages"];

/// Env vars that belong to the capture harness, not to the pandoc run.
const HARNESS_VARS: &[&str] = &["QUARTO_PANDOC", "PANDOC_CAPTURE_DIR", "PANDOC_CAPTURE_REAL"];

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Meta {
    pub share_tree: String,
    pub status: i32,
    /// The placeholder path of the `-o` output.
    pub output: String,
    /// The comparison reference, `reference/<file>`: the output of a native
    /// replay at the canonical work dir (see `replay::canonical_work_dir`),
    /// because outputs embed the paths pandoc ran at (image descriptions).
    pub reference: Option<String>,
    /// The output of the original capture run, `capture/<file>`, whose
    /// embedded paths are the capture machine's (compare semantically).
    pub capture_output: Option<String>,
}

/// `(placeholder, original)` roots, longest original first is handled by `PathMap`.
fn read_kv(path: &Path) -> Result<BTreeMap<String, String>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(text
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect())
}

fn read_argv(path: &Path) -> Result<Vec<String>> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let mut argv: Vec<String> = bytes
        .split(|b| *b == 0)
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    argv.pop(); // trailing NUL leaves one empty element
    Ok(argv)
}

fn copy_tree(from: &Path, to: &Path, map: &PathMap, leaks: &mut Vec<String>) -> Result<()> {
    for entry in WalkDir::new(from).sort_by_file_name() {
        let entry = entry?;
        let rel = entry.path().strip_prefix(from)?;
        let dest = to.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&dest)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let bytes = std::fs::read(entry.path())?;
            match map.apply_bytes(&bytes) {
                Some(mapped) => {
                    if let Some(root) = map.leaks_in(std::str::from_utf8(&mapped).unwrap_or("")) {
                        leaks.push(format!("{} still contains {root}", dest.display()));
                    }
                    std::fs::write(&dest, mapped)?;
                }
                None => std::fs::write(&dest, bytes)?,
            }
        }
    }
    Ok(())
}

/// Rewrites one raw run (`<capture>/run-N`) into `set_root/<name>`.
///
/// With `pandoc`, also replays the recording at its canonical work dir and
/// stores that output as the byte-comparison reference.
pub fn rewrite_run(raw: &Path, set_root: &Path, name: &str, pandoc: Option<&Path>) -> Result<()> {
    let paths = read_kv(&raw.join("paths.txt"))?;
    let temp_root = paths.get("temp_root").cloned().unwrap_or_default();
    let out_path = paths.get("out_path").cloned().unwrap_or_default();
    let out_dir = paths.get("out_dir").cloned().unwrap_or_default();
    if temp_root.is_empty() {
        bail!(
            "{}: capture has no pipeline temp root (QUARTO_SHARE_PATH was unset)",
            raw.display()
        );
    }
    let argv = read_argv(&raw.join("argv.nul"))?;

    // files.txt: "N<TAB>original"; the value after --resource-path is the document dir.
    let resource_path = argv
        .iter()
        .position(|a| a == "--resource-path")
        .and_then(|i| argv.get(i + 1))
        .cloned();
    let mut inputs: Vec<(String, String, String)> = Vec::new(); // (N, original, placeholder)
    let files_txt = std::fs::read_to_string(raw.join("files.txt")).unwrap_or_default();
    for line in files_txt.lines() {
        let (n, orig) = line.split_once('\t').context("malformed files.txt line")?;
        let placeholder = if Some(orig) == resource_path.as_deref() {
            DOC_ROOT.to_string()
        } else {
            format!("/__q2_in{n}__")
        };
        inputs.push((n.to_string(), orig.to_string(), placeholder));
    }

    let mut pairs = vec![
        (format!("{temp_root}/pandoc-share"), SHARE_ROOT.to_string()),
        (temp_root.clone(), TMP_ROOT.to_string()),
    ];
    for (_, orig, placeholder) in &inputs {
        pairs.push((orig.clone(), placeholder.clone()));
    }
    // The default output directory is the document directory; keep its doc mapping.
    let out_is_doc = inputs
        .iter()
        .any(|(_, orig, ph)| ph == DOC_ROOT && *orig == out_dir);
    if !out_dir.is_empty() && !out_is_doc {
        pairs.push((out_dir.clone(), OUT_ROOT.to_string()));
    }
    let map = PathMap::new(pairs);

    let dest = set_root.join(name);
    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    std::fs::create_dir_all(dest.join("fs"))?;
    let mut leaks = Vec::new();

    // Share tree: stored once, addressed by content.
    let share_tmp = dest.join(".share-staging");
    copy_tree(&raw.join("temp/pandoc-share"), &share_tmp, &map, &mut leaks)?;
    let share_entries = list_files(&share_tmp, &[])?;
    let id = tree_id(&share_entries);
    let share_dir = set_root.join("share").join(&id);
    if share_dir.exists() {
        std::fs::remove_dir_all(&share_tmp)?;
    } else {
        std::fs::create_dir_all(set_root.join("share"))?;
        std::fs::rename(&share_tmp, &share_dir)?;
        write_manifest(
            &set_root.join("share").join(format!("{id}.manifest.json")),
            &share_entries,
        )?;
    }

    // Everything else in the pipeline temp root.
    let tmp_dest = dest.join("fs/__q2_tmp__");
    std::fs::create_dir_all(&tmp_dest)?;
    for entry in std::fs::read_dir(raw.join("temp"))? {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name == "pandoc-share" || UNUSED_TEMP_ENTRIES.contains(&file_name.as_str()) {
            continue;
        }
        let target = tmp_dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target, &map, &mut leaks)?;
        } else {
            let bytes = std::fs::read(entry.path())?;
            std::fs::write(&target, map.apply_bytes(&bytes).unwrap_or(bytes))?;
        }
    }
    for (n, _, placeholder) in &inputs {
        let to = dest.join("fs").join(placeholder.trim_start_matches('/'));
        std::fs::create_dir_all(&to)?;
        copy_tree(&raw.join("files").join(n), &to, &map, &mut leaks)?;
    }

    // argv and env.
    let argv_json: Vec<String> = argv.iter().map(|a| map.apply(a)).collect();
    let mut env_json = BTreeMap::new();
    for (k, v) in read_kv(&raw.join("env.txt"))? {
        if HARNESS_VARS.contains(&k.as_str()) {
            continue;
        }
        env_json.insert(k.clone(), map.apply_env(&k, &v)?);
    }
    for a in &argv_json {
        if let Some(root) = map.leaks_in(a) {
            leaks.push(format!("argv `{a}` still contains {root}"));
        }
    }
    for (k, v) in &env_json {
        let text = if k == PARAMS_VAR {
            use base64::Engine;
            String::from_utf8(base64::engine::general_purpose::STANDARD.decode(v)?)?
        } else {
            v.clone()
        };
        if let Some(root) = map.leaks_in(&text) {
            leaks.push(format!("env {k} still contains {root}"));
        }
    }
    if !leaks.is_empty() {
        bail!(
            "capture leaks machine-specific paths:\n{}",
            leaks.join("\n")
        );
    }
    std::fs::write(
        dest.join("argv.json"),
        serde_json::to_string_pretty(&argv_json)? + "\n",
    )?;
    std::fs::write(
        dest.join("env.json"),
        serde_json::to_string_pretty(&env_json)? + "\n",
    )?;

    // Reference output and meta.
    let mut capture_output = None;
    if let Ok(mut rd) = std::fs::read_dir(raw.join("reference"))
        && let Some(Ok(entry)) = rd.next()
    {
        let file = entry.file_name().to_string_lossy().into_owned();
        std::fs::create_dir_all(dest.join("capture"))?;
        std::fs::copy(entry.path(), dest.join("capture").join(&file))?;
        capture_output = Some(format!("capture/{file}"));
    }
    let status: i32 = std::fs::read_to_string(raw.join("status.txt"))?
        .trim()
        .parse()?;
    let mut meta = Meta {
        share_tree: id,
        status,
        output: map.apply(&out_path),
        reference: None,
        capture_output,
    };
    let write_meta = |m: &Meta| -> Result<()> {
        std::fs::write(
            dest.join("meta.json"),
            serde_json::to_string_pretty(m)? + "\n",
        )?;
        Ok(())
    };
    write_meta(&meta)?;
    if let Some(pandoc) = pandoc {
        let work = crate::replay::canonical_work_dir(name);
        let res = crate::replay::replay(&dest, set_root, pandoc, &work)?;
        if res.status != 0 {
            bail!(
                "canonical replay of {name} exited {}: {}",
                res.status,
                res.stderr
            );
        }
        let bytes = res.output.context("canonical replay wrote no output")?;
        let file = meta
            .output
            .rsplit('/')
            .next()
            .unwrap_or("output")
            .to_string();
        std::fs::create_dir_all(dest.join("reference"))?;
        std::fs::write(dest.join("reference").join(&file), bytes)?;
        meta.reference = Some(format!("reference/{file}"));
        write_meta(&meta)?;
    }

    write_manifest(
        &dest.join("manifest.json"),
        &list_files(&dest, &["manifest.json"])?,
    )?;
    Ok(())
}

/// The set root's recordings (directories holding `argv.json`).
pub fn list_recordings(set_root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(set_root)? {
        let p = entry?.path();
        if p.join("argv.json").is_file() {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}
