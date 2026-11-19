//! Event vocabulary (contract: "Event vocabulary"); equal to `fixtures/vocabulary.json`.

use serde::{Deserialize, Serialize};

pub const EVENTS: [&str; 10] = [
    "flipped",
    "switched",
    "detached",
    "reattached",
    "held",
    "reset",
    "retired",
    "moved-forward",
    "joined",
    "seeded",
];

/// Swap phases (G26).
pub const SWAP_EVENTS: [&str; 3] = ["pending", "done", "unavailable"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EventKind {
    Flipped,
    Switched,
    Detached,
    Reattached,
    Held,
    Reset,
    Retired,
    MovedForward,
    Joined,
    Seeded,
}

impl EventKind {
    pub const ALL: [EventKind; 10] = [
        EventKind::Flipped,
        EventKind::Switched,
        EventKind::Detached,
        EventKind::Reattached,
        EventKind::Held,
        EventKind::Reset,
        EventKind::Retired,
        EventKind::MovedForward,
        EventKind::Joined,
        EventKind::Seeded,
    ];

    pub fn as_str(self) -> &'static str {
        EVENTS[self as usize]
    }
}
