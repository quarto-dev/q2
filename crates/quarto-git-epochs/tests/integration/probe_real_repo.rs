//! The subprocess probe against real temporary repositories.

use crate::support::{commit_all, git, init_repo, put, temp_dir};
use quarto_git_epochs::probe::{
    ControllerProbe, GitProbe, MarkerKind, ProbeError, ProbeOptions, ReflogMark, SubprocessProbe,
};
use std::path::Path;

fn probe(dir: &Path) -> SubprocessProbe {
    SubprocessProbe::new(ProbeOptions::new(dir))
}

fn names(p: &SubprocessProbe) -> Vec<(String, MarkerKind)> {
    p.cheap_state()
        .markers
        .into_iter()
        .map(|m| (m.name, m.kind))
        .collect()
}

#[test]
fn head_is_none_for_an_unborn_repo_and_the_commit_after_the_first_commit() {
    let (_t, dir) = temp_dir();
    init_repo(&dir);
    let p = probe(&dir);
    assert_eq!(p.head_sha().unwrap(), None);
    put(&dir, "a.qmd", b"one\n");
    let sha = commit_all(&dir, "base");
    assert_eq!(p.head_sha().unwrap(), Some(sha));
}

#[test]
fn show_prefix_is_empty_at_the_root_and_the_subpath_in_a_subdirectory() {
    let (_t, dir) = temp_dir();
    init_repo(&dir);
    put(&dir, "site/a.qmd", b"x\n");
    commit_all(&dir, "base");
    assert_eq!(probe(&dir).show_prefix().unwrap(), "");
    assert_eq!(probe(&dir.join("site")).show_prefix().unwrap(), "site");
}

#[test]
fn root_commits_finds_two_roots() {
    let (_t, dir) = temp_dir();
    init_repo(&dir);
    put(&dir, "a.qmd", b"1\n");
    let r1 = commit_all(&dir, "one");
    git(&dir, &["switch", "-q", "--orphan", "other"]);
    put(&dir, "b.qmd", b"2\n");
    let r2 = commit_all(&dir, "two");
    git(
        &dir,
        &[
            "merge",
            "-q",
            "--allow-unrelated-histories",
            "-m",
            "merge",
            "main",
        ],
    );
    let mut roots = probe(&dir).root_commits().unwrap();
    roots.sort();
    let mut want = vec![r1, r2];
    want.sort();
    assert_eq!(roots, want);
}

#[test]
fn is_shallow_is_false_for_a_repo_and_true_for_a_depth_one_clone_through_a_file_url() {
    let (_t, src) = temp_dir();
    init_repo(&src);
    put(&src, "a.qmd", b"1\n");
    commit_all(&src, "one");
    put(&src, "a.qmd", b"2\n");
    commit_all(&src, "two");
    let (_t2, parent) = temp_dir();
    let dest = parent.join("clone");
    let s = src.to_string_lossy().replace('\\', "/");
    let url = format!("file://{}{}", if s.starts_with('/') { "" } else { "/" }, s);
    git(
        &parent,
        &["clone", "-q", "--depth", "1", &url, dest.to_str().unwrap()],
    );
    assert!(!probe(&src).is_shallow().unwrap());
    assert!(probe(&dest).is_shallow().unwrap());
}

#[test]
fn ls_tree_lists_regular_files_under_the_prefix_only_and_filters_symlinks_and_submodules() {
    let (_t, dir) = temp_dir();
    init_repo(&dir);
    put(&dir, "site/a.qmd", b"a\n");
    put(&dir, "site/deep/b.qmd", b"b\n");
    put(&dir, "site/run.sh", b"#!/bin/sh\n");
    put(&dir, "other/x.qmd", b"x\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["update-index", "--chmod=+x", "site/run.sh"]);
    let blob = git(&dir, &["rev-parse", ":site/a.qmd"]);
    git(
        &dir,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("120000,{blob},site/link"),
        ],
    );
    git(
        &dir,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{},site/sub", "1".repeat(40)),
        ],
    );
    git(&dir, &["commit", "-q", "-m", "base"]);
    let files = probe(&dir.join("site")).ls_tree().unwrap();
    let mut paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    paths.sort_unstable();
    assert_eq!(paths, vec!["a.qmd", "deep/b.qmd", "run.sh"]);
    assert_eq!(
        files.iter().find(|f| f.path == "run.sh").unwrap().mode,
        "100755"
    );
    assert_eq!(files.iter().find(|f| f.path == "a.qmd").unwrap().blob, blob);
}

#[test]
fn ls_tree_lists_nothing_for_an_unborn_repository() {
    let (_t, dir) = temp_dir();
    init_repo(&dir);
    assert!(probe(&dir).ls_tree().unwrap().is_empty());
}

#[test]
fn cat_file_batch_returns_blob_bytes_and_a_missing_id_is_absent() {
    let (_t, dir) = temp_dir();
    init_repo(&dir);
    git(&dir, &["config", "core.autocrlf", "true"]);
    put(&dir, "a.qmd", b"one\r\ntwo\r\n");
    put(&dir, "bin.dat", &[0, 255, 1, 2, 10, 13]);
    commit_all(&dir, "base");
    let p = probe(&dir);
    let files = p.ls_tree().unwrap();
    let a = files.iter().find(|f| f.path == "a.qmd").unwrap();
    let bin = files.iter().find(|f| f.path == "bin.dat").unwrap();
    let missing = "0".repeat(40);
    let got = p
        .cat_blobs(&[a.blob.clone(), bin.blob.clone(), missing.clone()])
        .unwrap();
    assert_eq!(
        got[&a.blob], b"one\ntwo\n",
        "with core.autocrlf the stored blob is LF"
    );
    assert_eq!(got[&bin.blob], vec![0, 255, 1, 2, 10, 13]);
    assert!(!got.contains_key(&missing));
}

#[test]
fn without_autocrlf_crlf_bytes_are_stored_as_written() {
    let (_t, dir) = temp_dir();
    init_repo(&dir);
    put(&dir, "a.qmd", b"one\r\n");
    commit_all(&dir, "base");
    let p = probe(&dir);
    let a = p.ls_tree().unwrap().remove(0);
    assert_eq!(p.cat_blobs(&[a.blob.clone()]).unwrap()[&a.blob], b"one\r\n");
}

fn repo_with_history() -> (tempfile::TempDir, std::path::PathBuf, String) {
    let (t, dir) = temp_dir();
    init_repo(&dir);
    put(&dir, "a.qmd", b"1\n");
    let c1 = commit_all(&dir, "base");
    git(&dir, &["switch", "-q", "-c", "side"]);
    put(&dir, "a.qmd", b"2\n");
    commit_all(&dir, "side");
    git(&dir, &["switch", "-q", "main"]);
    (t, dir, c1)
}

#[test]
fn reflog_returns_every_subject_for_a_null_mark_and_the_tail_of_the_newest_entry() {
    let (_t, dir, c1) = repo_with_history();
    let s = probe(&dir).reflog_since(None).unwrap().unwrap();
    assert_eq!(s.subjects.len(), 4);
    assert!(s.subjects[0].starts_with("commit (initial): base"));
    assert!(s.subjects[3].starts_with("checkout: moving from side to main"));
    assert_eq!(
        s.tail,
        Some(ReflogMark {
            sha: c1,
            lines: Some(4)
        })
    );
    assert!(s.mark_found);
}

#[test]
fn reflog_mark_that_appears_twice_uses_the_last_occurrence_or_the_exact_line() {
    let (_t, dir, c1) = repo_with_history();
    let p = probe(&dir);
    let last = p
        .reflog_since(Some(&ReflogMark {
            sha: c1.clone(),
            lines: None,
        }))
        .unwrap()
        .unwrap();
    assert!(last.subjects.is_empty());
    let exact = p
        .reflog_since(Some(&ReflogMark {
            sha: c1,
            lines: Some(1),
        }))
        .unwrap()
        .unwrap();
    assert_eq!(exact.subjects.len(), 3);
}

#[test]
fn reflog_with_an_absent_sha_returns_everything_and_mark_found_false() {
    let (_t, dir, _) = repo_with_history();
    let s = probe(&dir)
        .reflog_since(Some(&ReflogMark {
            sha: "f".repeat(40),
            lines: None,
        }))
        .unwrap()
        .unwrap();
    assert!(!s.mark_found);
    assert_eq!(s.subjects.len(), 4);
}

#[test]
fn reflog_picks_up_entries_appended_after_a_recorded_tail() {
    let (_t, dir, _) = repo_with_history();
    let p = probe(&dir);
    let tail = p.reflog_since(None).unwrap().unwrap().tail;
    git(&dir, &["switch", "-q", "side"]);
    let s = p.reflog_since(tail.as_ref()).unwrap().unwrap();
    assert_eq!(s.subjects.len(), 1);
    assert!(s.subjects[0].starts_with("checkout: moving from main to side"));
}

#[test]
fn no_reflog_is_none_and_cheap_state_reports_it_without_error() {
    let (_t, dir) = temp_dir();
    init_repo(&dir);
    git(&dir, &["config", "core.logAllRefUpdates", "false"]);
    put(&dir, "a.qmd", b"1\n");
    commit_all(&dir, "base");
    let _ = std::fs::remove_dir_all(dir.join(".git").join("logs"));
    let p = probe(&dir);
    assert!(p.head_sha().unwrap().is_some());
    assert!(p.reflog_since(None).unwrap().is_none());
    assert!(p.cheap_state().reflog.is_none());
}

#[test]
fn index_lock_is_resolved_through_git_path_for_a_linked_worktree() {
    let (_t, dir) = temp_dir();
    init_repo(&dir);
    put(&dir, "a.qmd", b"1\n");
    commit_all(&dir, "base");
    let (_t2, parent) = temp_dir();
    let wt = parent.join("wt");
    git(
        &dir,
        &[
            "worktree",
            "add",
            "-q",
            wt.to_str().unwrap(),
            "-b",
            "wt-branch",
        ],
    );
    let p = probe(&wt);
    p.head_sha().unwrap();
    assert!(names(&p).is_empty());
    let lock = git(
        &wt,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "index.lock",
        ],
    );
    assert!(lock.contains("worktrees"), "{lock}");
    std::fs::write(&lock, b"").unwrap();
    let m = p.cheap_state().markers;
    assert_eq!(m.len(), 1);
    assert_eq!(
        (m[0].name.as_str(), m[0].kind),
        ("index.lock", MarkerKind::Lock)
    );
    assert!(m[0].mtime_ms > 0);
}

fn diverge(dir: &Path) {
    put(dir, "a.qmd", b"one\n");
    commit_all(dir, "base");
    git(dir, &["switch", "-q", "-c", "side"]);
    put(dir, "a.qmd", b"side\n");
    commit_all(dir, "side");
    git(dir, &["switch", "-q", "main"]);
    put(dir, "a.qmd", b"main2\n");
    commit_all(dir, "main2");
}

#[test]
fn a_stopped_rebase_shows_the_rebase_marker_as_a_state_marker() {
    let (_t, dir) = temp_dir();
    init_repo(&dir);
    diverge(&dir);
    git(&dir, &["switch", "-q", "side"]);
    let out = std::process::Command::new("git")
        .args(["rebase", "main"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let p = probe(&dir);
    p.head_sha().unwrap();
    assert!(names(&p).contains(&("rebase-merge".to_string(), MarkerKind::State)));
    git(&dir, &["rebase", "--abort"]);
    assert!(names(&p).is_empty());
}

#[test]
fn a_merge_in_progress_shows_merge_head() {
    let (_t, dir) = temp_dir();
    init_repo(&dir);
    diverge(&dir);
    let out = std::process::Command::new("git")
        .args(["merge", "side"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let p = probe(&dir);
    p.head_sha().unwrap();
    assert!(names(&p).iter().any(|(n, _)| n == "MERGE_HEAD"));
}

#[test]
fn the_colocated_jj_lock_is_a_lock_marker() {
    let (_t, dir) = temp_dir();
    init_repo(&dir);
    put(&dir, "a.qmd", b"1\n");
    commit_all(&dir, "base");
    std::fs::create_dir_all(dir.join(".jj").join("working_copy")).unwrap();
    let p = probe(&dir);
    p.head_sha().unwrap();
    assert!(p.is_colocated_jj());
    assert!(names(&p).is_empty());
    std::fs::write(
        dir.join(".jj")
            .join("working_copy")
            .join("working_copy.lock"),
        b"",
    )
    .unwrap();
    assert_eq!(
        names(&p),
        vec![(
            ".jj/working_copy/working_copy.lock".to_string(),
            MarkerKind::Lock
        )]
    );
}

#[test]
fn a_missing_git_is_unavailable() {
    let (_t, dir) = temp_dir();
    let (_t2, other) = temp_dir();
    let mut o = ProbeOptions::new(&dir);
    o.git_path = Some(other.join("no-such-git"));
    assert!(matches!(
        SubprocessProbe::new(o).head_sha(),
        Err(ProbeError::Unavailable(_))
    ));
}

#[test]
fn a_directory_that_is_not_a_repository_is_unavailable() {
    let (_t, dir) = temp_dir();
    // An ancestor of the temp dir must not be a repository for this to hold; temp dirs are outside any.
    assert!(matches!(
        probe(&dir).head_sha(),
        Err(ProbeError::Unavailable(_))
    ));
}

/// A stand-in `git` that sleeps, so the per-call timeout has something to kill. Unix only: the stand-in is a
/// shell script. On other platforms the helper returns `None` and the test skips.
#[cfg(unix)]
fn sleeping_git(dir: &Path) -> Option<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("slow-git");
    std::fs::write(&p, "#!/bin/sh\nsleep 10\n").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    Some(p)
}

#[cfg(not(unix))]
fn sleeping_git(_dir: &Path) -> Option<std::path::PathBuf> {
    None
}

#[test]
fn a_call_that_exceeds_its_timeout_fails_without_hanging() {
    let (_t, dir) = temp_dir();
    let Some(slow) = sleeping_git(&dir) else {
        eprintln!("skipping: no stand-in git on this platform");
        return;
    };
    let mut o = ProbeOptions::new(&dir);
    o.git_path = Some(slow);
    o.timeout_ms = 200;
    let started = std::time::Instant::now();
    let err = SubprocessProbe::new(o).root_commits().unwrap_err();
    // The first call resolves the repository paths, which is where the timeout lands: unavailable.
    assert!(err.to_string().contains("timed out"), "{err}");
    assert!(started.elapsed() < std::time::Duration::from_secs(8));
}

/// A clean filter and `core.fsmonitor` that touch a marker file: the probe must run neither.
#[test]
fn the_probe_runs_no_repository_configured_program() {
    let (_t, dir) = temp_dir();
    let (_t2, scratch) = temp_dir();
    init_repo(&dir);
    let marker = scratch.join("ran");
    let marker_s = marker.to_string_lossy().replace('\\', "/");
    // `sh` exists wherever git does (Git for Windows ships it).
    let prog = format!("sh -c 'echo ran > \"{marker_s}\"; cat'");
    git(&dir, &["config", "filter.touch.clean", &prog]);
    git(&dir, &["config", "filter.touch.smudge", &prog]);
    git(
        &dir,
        &[
            "config",
            "core.fsmonitor",
            &format!("sh -c 'echo ran > \"{marker_s}\"'"),
        ],
    );
    put(&dir, ".gitattributes", b"*.qmd filter=touch\n");
    put(&dir, "a.qmd", b"one\n");
    // Commit without the program: only the probe's behavior is under test.
    git(
        &dir,
        &[
            "-c",
            "core.fsmonitor=false",
            "-c",
            "filter.touch.clean=cat",
            "add",
            "-A",
        ],
    );
    git(
        &dir,
        &["-c", "core.fsmonitor=false", "commit", "-q", "-m", "base"],
    );
    let _ = std::fs::remove_file(&marker);
    let p = probe(&dir);
    p.head_sha().unwrap();
    p.show_prefix().unwrap();
    p.root_commits().unwrap();
    p.is_shallow().unwrap();
    let files = p.ls_tree().unwrap();
    p.cat_blobs(&files.iter().map(|f| f.blob.clone()).collect::<Vec<_>>())
        .unwrap();
    p.reflog_since(None).unwrap();
    p.cheap_state();
    assert!(!marker.exists(), "a repository-configured program ran");
}
