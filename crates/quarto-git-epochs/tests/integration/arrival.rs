use crate::support::read_fixture_json;
use quarto_git_epochs::arrival::{
    ArrivalAction, ArrivalInput, ArrivalStep, Attached, Kind, Policy, arrive, normalize_text,
};
use quarto_git_epochs::classify::Peer;
use quarto_git_epochs::vocabulary::EventKind;

#[test]
fn every_fixture_row() {
    let rows = read_fixture_json("decision-tables/arrival.json");
    let rows = rows.as_array().unwrap();
    assert!(rows.len() >= 29);
    for r in rows {
        let input: ArrivalInput = serde_json::from_value(r.clone()).unwrap();
        let out = arrive(&input);
        let e = &r["expected"];
        let id = &r["id"];
        let step: ArrivalStep = serde_json::from_value(e["step"].clone()).unwrap();
        let action: ArrivalAction = serde_json::from_value(e["action"].clone()).unwrap();
        assert_eq!(out.step, step, "{id}: step");
        assert_eq!(out.action, action, "{id}: action");
        let event: Option<EventKind> = serde_json::from_value(e["event"].clone()).unwrap();
        assert_eq!(out.event, event, "{id}: event");
        let discarded = e["discarded"].as_u64().map(|n| n as u32);
        assert_eq!(out.discarded, discarded, "{id}: discarded");
    }
}

fn base(peer: Peer) -> ArrivalInput {
    ArrivalInput {
        peer,
        policy: if peer == Peer::Hub {
            Some(Policy::WriteBack)
        } else {
            None
        },
        kind: Kind::Text,
        disk: "S".into(),
        doc: Some("T".into()),
        blob: "B".into(),
        attached: Attached::None,
        retired_by_other: false,
        jj: false,
        canonical_unavailable: false,
        live_under_other_path: false,
        fork_syncs: None,
        case_collision: false,
    }
}

#[test]
fn already_attached_with_s_equal_b_and_t_different_retires() {
    let mut i = base(Peer::Extension);
    i.attached = Attached::Canonical;
    i.disk = "B".into();
    let out = arrive(&i);
    assert_eq!(out.step, ArrivalStep::S2b);
    assert_eq!(out.action, ArrivalAction::RetireCreateFlip);
    assert_eq!(out.discarded, Some(1));
}

#[test]
fn already_attached_with_s_equal_b_and_t_equal_rebaselines() {
    let mut i = base(Peer::Extension);
    i.attached = Attached::Canonical;
    i.disk = "B".into();
    i.doc = Some("B".into());
    assert_eq!(arrive(&i).step, ArrivalStep::S2a);
}

#[test]
fn read_only_with_disk_different_from_doc_seeds_from_disk() {
    let mut i = base(Peer::Hub);
    i.policy = Some(Policy::ReadOnly);
    for disk in ["B", "S"] {
        i.disk = disk.into();
        let out = arrive(&i);
        assert_eq!(out.action, ArrivalAction::CreateFromDisk, "disk {disk}");
        assert_eq!(out.event, Some(EventKind::Seeded));
    }
}

#[test]
fn jj_leaves_step_one_and_the_attached_steps_alone() {
    let mut i = base(Peer::Hub);
    i.jj = true;
    i.doc = None;
    assert_eq!(arrive(&i).step, ArrivalStep::S1);
    let mut i = base(Peer::Hub);
    i.jj = true;
    i.attached = Attached::Canonical;
    assert_eq!(arrive(&i).step, ArrivalStep::S2a);
}

#[test]
fn text_normalizes_crlf_binary_compares_as_given() {
    assert_eq!(normalize_text("a\r\nb\rc\n"), "a\nb\nc\n");
}

#[test]
fn a_doc_attached_to_another_doc_that_nobody_retired_runs_the_ordinary_steps() {
    let mut i = base(Peer::Extension);
    i.attached = Attached::Other;
    i.disk = "T".into();
    assert_eq!(arrive(&i).step, ArrivalStep::S3);
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn crlf_checkout_text_attaches_silently_after_normalization() {
    let (_t, dir) = crate::support::temp_dir();
    let blob = "title: a\n\nline two\n";
    let disk = crate::support::crlf_checkout(&dir, "a.qmd", blob.as_bytes());
    let disk = String::from_utf8(disk).unwrap();
    assert!(disk.contains("\r\n"), "autocrlf=true wrote CRLF");

    // Normalized disk equals the blob and the doc: ordinary silent attach (step 3).
    let mut i = base(Peer::Hub);
    i.disk = normalize_text(&disk);
    i.doc = Some(blob.into());
    i.blob = blob.into();
    let out = arrive(&i);
    assert_eq!(out.step, ArrivalStep::S3);
    assert_eq!(out.action, ArrivalAction::AttachSilently);

    // Without normalization the same file would look edited: the raw comparison is what the rule prevents.
    let mut raw = i.clone();
    raw.disk = disk;
    assert_ne!(arrive(&raw).action, ArrivalAction::AttachSilently);
}

#[test]
fn crlf_checkout_binary_compares_by_bytes() {
    let (_t, dir) = crate::support::temp_dir();
    // A NUL byte makes git treat the file as binary: no conversion, bytes survive unchanged.
    let blob: &[u8] = b"\x89PNG\r\n\x1a\n\0\0data\r\nmore\n";
    let disk = crate::support::crlf_checkout(&dir, "img.png", blob);
    assert_eq!(disk, blob);

    let mut i = base(Peer::Hub);
    i.kind = Kind::Binary;
    i.disk = hex(&disk);
    i.doc = Some(hex(blob));
    i.blob = hex(blob);
    assert_eq!(arrive(&i).action, ArrivalAction::AttachSilently);
}
