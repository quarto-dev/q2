use crate::support::read_fixture_json;
use quarto_git_epochs::classify::{ChangedFile, ClassifyInput, Peer, Verdict, classify};

fn changed(disk: &str, synced: &str) -> Vec<ChangedFile> {
    vec![ChangedFile {
        path: "a.qmd".into(),
        disk: disk.into(),
        synced: synced.into(),
        blob: "b".into(),
    }]
}

#[test]
fn every_fixture_row() {
    let rows = read_fixture_json("decision-tables/classify.json");
    let rows = rows.as_array().unwrap();
    assert!(rows.len() >= 16);
    for r in rows {
        let input: ClassifyInput = serde_json::from_value(r.clone()).unwrap();
        let expected: Verdict = serde_json::from_value(r["expected"].clone()).unwrap();
        assert_eq!(classify(&input), expected, "row {}", r["id"]);
    }
}

#[test]
fn content_rule_wins_when_disk_equals_synced() {
    let input = ClassifyInput {
        peer: Peer::Extension,
        reflog: Some(vec!["checkout: moving from a to b".into()]),
        changed: changed("v", "v"),
    };
    assert_eq!(classify(&input), Verdict::Commit);
}

#[test]
fn save_then_commit_is_an_author_edit() {
    let input = ClassifyInput {
        peer: Peer::Extension,
        reflog: Some(vec!["commit: saved".into()]),
        changed: changed("new", "old"),
    };
    assert_eq!(classify(&input), Verdict::Commit);
}

#[test]
fn any_non_commit_entry_makes_the_span_checkout_type() {
    let input = ClassifyInput {
        peer: Peer::Extension,
        reflog: Some(vec![
            "checkout: moving from a to b".into(),
            "commit: x".into(),
        ]),
        changed: changed("new", "old"),
    };
    assert_eq!(classify(&input), Verdict::Checkout);
}

#[test]
fn stash_and_reset_hard_with_a_differing_disk_are_checkout_type() {
    // `reset: moving to HEAD` is what both `git stash` and `git reset --hard` log.
    let input = ClassifyInput {
        peer: Peer::Extension,
        reflog: Some(vec!["reset: moving to HEAD".into()]),
        changed: changed("edited", "edited-synced-before"),
    };
    assert_eq!(classify(&input), Verdict::Checkout);
}

#[test]
fn merge_commits_are_not_plain_commits() {
    let input = ClassifyInput {
        peer: Peer::Extension,
        reflog: Some(vec!["commit (merge): Merge branch side".into()]),
        changed: changed("new", "old"),
    };
    assert_eq!(classify(&input), Verdict::Checkout);
}

#[test]
fn no_reflog_or_an_empty_span_uses_the_content_rule_alone() {
    for reflog in [None, Some(vec![])] {
        let dirty = ClassifyInput {
            peer: Peer::Hub,
            reflog: reflog.clone(),
            changed: changed("new", "old"),
        };
        assert_eq!(classify(&dirty), Verdict::Checkout);
        let clean = ClassifyInput {
            peer: Peer::Hub,
            reflog,
            changed: changed("v", "v"),
        };
        assert_eq!(classify(&clean), Verdict::Commit);
    }
}

#[test]
fn crlf_checkout_is_not_an_edit_after_normalization() {
    use quarto_git_epochs::arrival::normalize_text;
    let (_t, dir) = crate::support::temp_dir();
    let blob = "one\ntwo\n";
    let disk = crate::support::crlf_checkout(&dir, "a.qmd", blob.as_bytes());
    let disk = String::from_utf8(disk).unwrap();
    assert!(disk.contains("\r\n"), "autocrlf=true wrote CRLF");

    // Synced content is the LF text; the disk holds CRLF. Normalized, disk equals synced: commit by content.
    let normalized = ClassifyInput {
        peer: Peer::Hub,
        reflog: None,
        changed: changed(&normalize_text(&disk), blob),
    };
    assert_eq!(classify(&normalized), Verdict::Commit);
    // Raw, the CRLF text would differ from synced and, with no commit tiebreaker, read as a checkout.
    let raw = ClassifyInput {
        peer: Peer::Hub,
        reflog: None,
        changed: changed(&disk, blob),
    };
    assert_eq!(classify(&raw), Verdict::Checkout);
}
