//! Classifier (G2): is a change to the working tree a commit or a checkout? Pure; no I/O.
//! See the contract, "Classifier".

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Commit,
    Checkout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Peer {
    Hub,
    Extension,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedFile {
    pub path: String,
    /// Disk content (CRLF-normalized text, or a byte hash for binary files).
    pub disk: String,
    /// The content last synced to the doc.
    pub synced: String,
    /// The content of the blob at the new HEAD.
    pub blob: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifyInput {
    pub peer: Peer,
    /// Reflog subjects since the last decision; `None` (or empty) when there is no reflog, or under colocated jj.
    pub reflog: Option<Vec<String>>,
    pub changed: Vec<ChangedFile>,
}

/// `commit: ...`, `commit (amend): ...`, `commit (initial): ...`; not `commit (merge)`.
fn is_plain_commit(subject: &str) -> bool {
    ["commit:", "commit (amend):", "commit (initial):"]
        .iter()
        .any(|p| subject.starts_with(p))
}

pub fn classify(input: &ClassifyInput) -> Verdict {
    // Content rule first: every changed file's disk equals what was last synced.
    if input.changed.iter().all(|f| f.disk == f.synced) {
        return Verdict::Commit;
    }
    // Tiebreaker: a span made only of plain commits means the author edited and committed.
    match &input.reflog {
        Some(log) if !log.is_empty() && log.iter().all(|s| is_plain_commit(s)) => Verdict::Commit,
        _ => Verdict::Checkout,
    }
}
