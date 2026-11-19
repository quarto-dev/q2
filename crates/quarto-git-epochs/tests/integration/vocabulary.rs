use crate::support::read_fixture_json;
use quarto_git_epochs::numbers;
use quarto_git_epochs::vocabulary::{EVENTS, EventKind, SWAP_EVENTS};

#[test]
fn exported_event_kinds_equal_the_vocabulary_fixture() {
    let v = read_fixture_json("vocabulary.json");
    let events: Vec<&str> = v["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert_eq!(EVENTS.to_vec(), events);
    let swap: Vec<&str> = v["swapEvents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert_eq!(SWAP_EVENTS.to_vec(), swap);
}

#[test]
fn event_kind_serializes_to_the_vocabulary_spelling() {
    for (kind, name) in EventKind::ALL.iter().zip(EVENTS) {
        assert_eq!(serde_json::to_string(kind).unwrap(), format!("\"{name}\""));
        assert_eq!(kind.as_str(), name);
    }
}

#[test]
fn the_numbers_of_the_contract_live_in_one_module() {
    assert_eq!(numbers::QUIET_WINDOW_MS, 1_000);
    assert_eq!(numbers::DECISION_CAP_MS, 30_000);
    assert_eq!(numbers::STALE_LOCK_MS, 60_000);
    assert_eq!(numbers::UNAVAILABLE_CANONICAL_MS, 5_000);
    assert_eq!(numbers::CONTROLLER_TICK_PENDING_MS, 250);
    assert_eq!(numbers::MTIME_SKIP_MS, 500);
    assert_eq!(numbers::REGISTRY_SOFT_LIMIT_PATHS, 15_000);
    assert_eq!(numbers::PREVIEW_PERIODIC_SYNC_MS, 5_000);
    assert_eq!(numbers::CLIENT_UNAVAILABLE_TIMEOUT_MS, 5_000);
    assert_eq!(numbers::EDITOR_ONLY_FLIP_WAIT_MS, 10_000);
    assert_eq!(numbers::PROXY_READINESS_MS, 15_000);
    assert_eq!(numbers::PUBLISH_CAP_BYTES_PER_FILE, 10 * 1024 * 1024);
    assert_eq!(numbers::RETIRED_HANDLES_KEPT_PER_PATH, 8);
    assert_eq!(numbers::IDLE_TICK_MS, 5_000);
}
