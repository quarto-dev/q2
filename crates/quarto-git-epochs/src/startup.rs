//! Startup rule (G10): per-file plan from a persisted `{head, blobs}` record. Pure.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupRecord {
    pub head: String,
    pub blobs: BTreeMap<String, String>,
    /// Paths the peer left detached; they stay detached across a restart while their blob is unchanged.
    #[serde(default)]
    pub detached: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupInput {
    pub record: Option<StartupRecord>,
    pub head: String,
    /// Participating files and their blob ids at HEAD now.
    pub blobs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StartupCase {
    Reconcile,
    ArriveDiffering,
    ArriveAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileStartup {
    Reconcile,
    Arrive,
    StayDetached,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupPlan {
    pub case: StartupCase,
    pub files: BTreeMap<String, FileStartup>,
}

pub fn startup_decision(input: &StartupInput) -> StartupPlan {
    let mut files = BTreeMap::new();
    let Some(record) = &input.record else {
        for p in input.blobs.keys() {
            files.insert(p.clone(), FileStartup::Arrive);
        }
        return StartupPlan {
            case: StartupCase::ArriveAll,
            files,
        };
    };
    let head_same = record.head == input.head;
    for (p, blob) in &input.blobs {
        let unchanged = head_same || record.blobs.get(p) == Some(blob);
        let detached = record.detached.contains(p);
        let plan = if detached && unchanged {
            FileStartup::StayDetached
        } else if unchanged {
            FileStartup::Reconcile
        } else {
            FileStartup::Arrive
        };
        files.insert(p.clone(), plan);
    }
    StartupPlan {
        case: if head_same {
            StartupCase::Reconcile
        } else {
            StartupCase::ArriveDiffering
        },
        files,
    }
}
