//! Runs every scenario step list in `fixtures/scenarios` against a real temp repo with the real git and
//! compares the classifier's verdict after each step. The simulated peer treats a `write` step as the
//! author's editor edit (synced at once); git steps change the disk without syncing. After each git step the
//! peer decides, then re-baselines. Set `SCENARIO_TRACE=1` to print every step's verdict.
//! Colocated-jj scenarios run when `jj` is on PATH and skip with a message otherwise.

use crate::support::{fixtures_dir, git, put, temp_dir};
use quarto_git_epochs::arrival::normalize_text;
use quarto_git_epochs::classify::{ChangedFile, ClassifyInput, Peer, Verdict, classify};
use quarto_git_epochs::probe::{
    ControllerProbe, GitProbe, ProbeOptions, ReflogMark, SubprocessProbe,
};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Text compares after CRLF normalization; a file with a NUL byte is binary and compares by its bytes.
fn content_key(bytes: &[u8]) -> String {
    let head = &bytes[..bytes.len().min(8000)];
    if head.contains(&0) {
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        return format!("bytes:{hex}");
    }
    normalize_text(&String::from_utf8_lossy(bytes))
}

fn read_key(dir: &Path, rel: &str) -> String {
    std::fs::read(dir.join(rel))
        .map(|b| content_key(&b))
        .unwrap_or_default()
}

struct Outcome {
    step: String,
    verdict: &'static str,
}

fn verdict_name(v: Verdict) -> &'static str {
    match v {
        Verdict::Commit => "commit",
        Verdict::Checkout => "checkout",
    }
}

fn run_scenario(s: &serde_json::Value) -> Vec<Outcome> {
    let (_t, dir) = temp_dir();
    let id = s["id"].as_str().unwrap();
    let trace = std::env::var("SCENARIO_TRACE").as_deref() == Ok("1");
    let probe = SubprocessProbe::new(ProbeOptions::new(&dir));
    let mut synced: HashMap<String, String> = HashMap::new();
    let mut last_blobs: HashMap<String, String> = HashMap::new();
    let mut last_head: Option<String> = None;
    let mut mark: Option<ReflogMark> = None;
    let mut out = Vec::new();

    for step in s["steps"].as_array().unwrap() {
        let label = step
            .get("git")
            .map(|g| g.to_string())
            .or_else(|| step["write"]["path"].as_str().map(str::to_string))
            .or_else(|| step["remove"].as_str().map(str::to_string))
            .or_else(|| step["writeBinary"]["path"].as_str().map(str::to_string))
            .unwrap_or_default();
        if let Some(w) = step.get("write") {
            let (p, c) = (w["path"].as_str().unwrap(), w["content"].as_str().unwrap());
            put(&dir, p, c.as_bytes());
            synced.insert(p.to_string(), content_key(c.as_bytes()));
            continue;
        }
        if let Some(w) = step.get("writeBinary") {
            let p = w["path"].as_str().unwrap();
            let hex = w["hex"].as_str().unwrap();
            let bytes: Vec<u8> = (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
                .collect();
            put(&dir, p, &bytes);
            synced.insert(p.to_string(), content_key(&bytes));
            continue;
        }
        if let Some(r) = step.get("remove") {
            let p = r.as_str().unwrap();
            std::fs::remove_file(dir.join(p)).unwrap();
            synced.remove(p);
            continue;
        }
        // A git step.
        let args: Vec<&str> = step["git"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap())
            .collect();
        let res = Command::new("git")
            .args(&args)
            .current_dir(&dir)
            .output()
            .unwrap();
        let expect_fail = step["expectFail"].as_bool().unwrap_or(false);
        if expect_fail {
            assert!(!res.status.success(), "{label} was expected to fail");
        } else {
            assert!(
                res.status.success(),
                "{id}: git {args:?}: {}",
                String::from_utf8_lossy(&res.stderr)
            );
        }

        let head = probe.head_sha().unwrap();
        let verdict: &'static str = if head.is_none() || !probe.cheap_state().markers.is_empty() {
            "none" // unborn, or the gate is closed: no decision yet
        } else {
            let slice = probe.reflog_since(mark.as_ref()).unwrap();
            let n = slice.as_ref().map_or(0, |s| s.subjects.len());
            if head == last_head && n == 0 {
                "none"
            } else {
                let tracked = probe.ls_tree().unwrap();
                let blobs = probe
                    .cat_blobs(&tracked.iter().map(|t| t.blob.clone()).collect::<Vec<_>>())
                    .unwrap();
                let mut changed = Vec::new();
                let mut next_blobs = HashMap::new();
                let mut next_synced = HashMap::new();
                for t in &tracked {
                    next_blobs.insert(t.path.clone(), t.blob.clone());
                    let disk = read_key(&dir, &t.path);
                    next_synced.insert(t.path.clone(), disk.clone());
                    let was = synced.get(&t.path).cloned().unwrap_or_default();
                    if last_blobs.get(&t.path) != Some(&t.blob) || disk != was {
                        changed.push(ChangedFile {
                            path: t.path.clone(),
                            disk,
                            synced: was,
                            blob: content_key(&blobs[&t.blob]),
                        });
                    }
                }
                // A file that left the tree (a checkout removed it) is a change too: its disk is gone, its doc is not.
                for p in last_blobs.keys() {
                    if next_blobs.contains_key(p) {
                        continue;
                    }
                    let disk = read_key(&dir, p);
                    let was = synced.get(p).cloned().unwrap_or_default();
                    if disk != was {
                        changed.push(ChangedFile {
                            path: p.clone(),
                            disk,
                            synced: was,
                            blob: String::new(),
                        });
                    }
                    synced.remove(p);
                }
                let v = classify(&ClassifyInput {
                    peer: Peer::Extension,
                    reflog: slice.as_ref().map(|s| s.subjects.clone()),
                    changed: changed.clone(),
                });
                last_blobs = next_blobs;
                synced.extend(next_synced);
                if let Some(t) = slice.and_then(|s| s.tail) {
                    mark = Some(t);
                }
                last_head = head.clone();
                if trace {
                    eprintln!(
                        "  {id}: {label} -> {} changed={:?}",
                        verdict_name(v),
                        changed.iter().map(|c| &c.path).collect::<Vec<_>>()
                    );
                }
                verdict_name(v)
            }
        };
        if head != last_head {
            last_head = head;
        }
        if trace && verdict == "none" {
            eprintln!("  {id}: {label} -> none");
        }
        if let Some(e) = step.get("expect") {
            assert_eq!(
                verdict,
                e["verdict"].as_str().unwrap(),
                "{id}: after {label}"
            );
        }
        out.push(Outcome {
            step: label,
            verdict,
        });
    }
    out
}

#[test]
fn every_scenario_file_runs_against_real_git() {
    let mut files: Vec<_> = std::fs::read_dir(fixtures_dir().join("scenarios"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    assert!(files.len() >= 11, "found {} scenarios", files.len());
    for f in files {
        let s: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        let results = run_scenario(&s);
        let expected = s["steps"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|x| x.get("expect").is_some())
            .count();
        assert!(results.len() >= expected, "{}", f.display());
        assert!(
            results
                .iter()
                .all(|o| !o.step.is_empty() && !o.verdict.is_empty())
        );
    }
}

fn has_jj() -> bool {
    Command::new("jj")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Under colocated jj every changed file takes the step 5 fallback verdict; the controller reports the
/// colocation so the caller can apply it.
#[test]
fn a_real_colocated_jj_repository_is_detected() {
    if !has_jj() {
        eprintln!("skipping: jj is not on PATH");
        return;
    }
    let (_t, dir) = temp_dir();
    crate::support::init_repo(&dir);
    put(&dir, "a.qmd", b"1\n");
    crate::support::commit_all(&dir, "base");
    let out = Command::new("jj")
        .args(["git", "init", "--colocate"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(out.status.success());
    let p = SubprocessProbe::new(ProbeOptions::new(&dir));
    p.head_sha().unwrap();
    assert!(p.is_colocated_jj());
    let _ = git(&dir, &["status", "--short"]);
}
