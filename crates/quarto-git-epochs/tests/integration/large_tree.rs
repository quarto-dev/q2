//! The gate against a real `git switch` of a large tree (Windows run item a2, W2).
//!
//! The controller runs on the real probe while real git rewrites thousands of files. By default the test builds
//! its own repository; set `GIT_EPOCHS_LARGE_TREE_REPO` to a repository made by
//! `scripts/git-epochs-windows/make_large_tree.py` (branches `a` and `b`) to use that one instead.

use crate::support::{commit_all, git, init_repo, temp_dir};
use quarto_git_epochs::controller::{Action, Controller, ControllerOptions, GateMode};
use quarto_git_epochs::probe::{ProbeOptions, SubprocessProbe};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const FILES: usize = 4000;
/// Files whose content differs between the two branches.
const DIFFERING: usize = 1200;
const KNOWN_MARKERS: [&str; 9] = [
    "index.lock",
    "HEAD.lock",
    "shallow.lock",
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "rebase-merge",
    "rebase-apply",
    "BISECT_LOG",
];

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn write_tree(dir: &Path, variant: &str) {
    for i in 0..FILES {
        let sub = dir.join(format!("d{:02}", i % 40));
        std::fs::create_dir_all(&sub).unwrap();
        let body = if i < DIFFERING {
            format!("file {i} variant {variant}\n{}\n", "x".repeat(600))
        } else {
            format!("file {i} shared\n{}\n", "x".repeat(600))
        };
        std::fs::write(sub.join(format!("f{i:04}.qmd")), body).unwrap();
    }
}

/// Builds a repository with branches `a` and `b`; returns it checked out on `a`.
fn build_repo(dir: &Path) {
    init_repo(dir);
    git(dir, &["symbolic-ref", "HEAD", "refs/heads/a"]);
    write_tree(dir, "a");
    commit_all(dir, "tree a");
    git(dir, &["switch", "-q", "-c", "b"]);
    write_tree(dir, "b");
    commit_all(dir, "tree b");
    git(dir, &["switch", "-q", "a"]);
}

fn repo_dir() -> (Option<tempfile::TempDir>, PathBuf) {
    if let Ok(p) = std::env::var("GIT_EPOCHS_LARGE_TREE_REPO") {
        let dir = PathBuf::from(p);
        git(&dir, &["switch", "-q", "a"]);
        return (None, dir);
    }
    let (t, dir) = temp_dir();
    build_repo(&dir);
    (Some(t), dir)
}

#[test]
fn the_generated_tree_has_the_expected_size_and_two_branches() {
    let (_t, dir) = repo_dir();
    let tracked = git(&dir, &["ls-files"]).lines().count();
    assert!(tracked >= FILES, "tracked {tracked}");
    let differing = git(&dir, &["diff", "--name-only", "a", "b"])
        .lines()
        .count();
    assert_eq!(differing, DIFFERING);
}

#[test]
fn the_gate_closes_for_real_markers_during_a_switch_and_releases_after() {
    let (_t, dir) = repo_dir();
    let probe = SubprocessProbe::new(ProbeOptions::new(&dir));
    let mut c = Controller::new(probe, ControllerOptions::default());
    c.init();

    let mut saw_closed = false;
    let mut unknown: Vec<String> = Vec::new();
    // Several switches: one fast switch can finish between two polls, so the claim is over the whole run.
    for round in 0..6 {
        let target = if round % 2 == 0 { "b" } else { "a" };
        let d = dir.clone();
        let handle = std::thread::spawn(move || git(&d, &["switch", "-q", target]));
        while !handle.is_finished() {
            let g = c.check(now_ms());
            if g.mode == GateMode::Closed {
                saw_closed = true;
                for m in &g.markers {
                    if !KNOWN_MARKERS.contains(&m.as_str()) {
                        unknown.push(m.clone());
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        handle.join().unwrap();

        // Let the quiet window pass in virtual time, then expect exactly one decision for the new HEAD.
        let mut t = now_ms();
        let mut decision = None;
        for _ in 0..40 {
            t += 250;
            let res = c.tick(t);
            if let Some(Action::Decision(d)) = res.actions.into_iter().next() {
                decision = Some(d);
                break;
            }
        }
        let decision = decision.expect("a decision after the switch");
        assert_eq!(
            decision.head.as_deref(),
            Some(git(&dir, &["rev-parse", "HEAD"]).as_str())
        );
        assert!(decision.tracked.len() >= FILES);
        let reflog = decision.reflog.expect("a reflog");
        assert!(
            reflog.iter().any(|s| s.starts_with("checkout:")),
            "reflog {reflog:?}"
        );
        // Released: open and idle once the decision is out.
        let g = c.check(t + 1);
        assert_eq!(g.mode, GateMode::Open, "round {round}");
        assert!(!g.pending, "round {round}");
        assert!(g.markers.is_empty(), "round {round}: {:?}", g.markers);
    }
    assert!(unknown.is_empty(), "unrecognized markers: {unknown:?}");
    assert!(
        saw_closed,
        "never saw the gate closed during six real switches of {FILES} files"
    );
}
