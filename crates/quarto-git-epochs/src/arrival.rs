//! Arrival (G9, G14): what one file does after a checkout-type decision. Pure.
//! See the contract, "Arrival". Field and action names come from `fixtures/decision-tables/arrival.json`.

use crate::classify::Peer;
use crate::vocabulary::EventKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Policy {
    ReadOnly,
    WriteBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Text,
    Binary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Attached {
    None,
    Canonical,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrivalInput {
    pub peer: Peer,
    /// Required for the hub; an extension row may carry it too (the `ReadOnly` variant).
    #[serde(default)]
    pub policy: Option<Policy>,
    pub kind: Kind,
    /// Disk content: CRLF-normalized text, or a byte hash.
    pub disk: String,
    /// Canonical doc content (or its hash); absent when no doc is registered.
    #[serde(default)]
    pub doc: Option<String>,
    /// Blob content (or its hash) at the new HEAD.
    pub blob: String,
    pub attached: Attached,
    #[serde(default)]
    pub retired_by_other: bool,
    #[serde(default)]
    pub jj: bool,
    #[serde(default)]
    pub canonical_unavailable: bool,
    #[serde(default)]
    pub live_under_other_path: bool,
    #[serde(default)]
    pub fork_syncs: Option<bool>,
    #[serde(default)]
    pub case_collision: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrivalStep {
    #[serde(rename = "1")]
    S1,
    #[serde(rename = "2a")]
    S2a,
    #[serde(rename = "2b")]
    S2b,
    #[serde(rename = "3")]
    S3,
    #[serde(rename = "4")]
    S4,
    #[serde(rename = "5")]
    S5,
}

impl ArrivalStep {
    /// The G9 step as the status route spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            ArrivalStep::S1 => "1",
            ArrivalStep::S2a => "2a",
            ArrivalStep::S2b => "2b",
            ArrivalStep::S3 => "3",
            ArrivalStep::S4 => "4",
            ArrivalStep::S5 => "5",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrivalAction {
    #[serde(rename = "create-from-disk")]
    CreateFromDisk,
    #[serde(rename = "attach-silently")]
    AttachSilently,
    #[serde(rename = "re-baseline")]
    ReBaseline,
    #[serde(rename = "retire-create-flip")]
    RetireCreateFlip,
    #[serde(rename = "join-with-notice")]
    JoinWithNotice,
    #[serde(rename = "join-and-warn")]
    JoinAndWarn,
    #[serde(rename = "detach-and-ask")]
    DetachAndAsk,
    #[serde(rename = "create-from-disk-unregistered")]
    CreateFromDiskUnregistered,
    #[serde(rename = "wait-5s-then-create")]
    WaitFiveSecondsThenCreate,
    #[serde(rename = "fork-then-attach")]
    ForkThenAttach,
    #[serde(rename = "create-from-content-no-history")]
    CreateFromContentNoHistory,
    #[serde(rename = "ordinary-sync")]
    OrdinarySync,
    #[serde(rename = "stay-live")]
    StayLive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrivalOutput {
    /// The step taken, or for a row outside the five steps the step it would otherwise take.
    pub step: ArrivalStep,
    pub action: ArrivalAction,
    pub event: Option<EventKind>,
    /// Discarded edits a retire step set aside (2b only).
    pub discarded: Option<u32>,
}

/// Text compares after CRLF normalization (G9, `core.autocrlf`).
pub fn normalize_text(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

fn out(step: ArrivalStep, action: ArrivalAction, event: Option<EventKind>) -> ArrivalOutput {
    ArrivalOutput {
        step,
        action,
        event,
        discarded: None,
    }
}

pub fn arrive(input: &ArrivalInput) -> ArrivalOutput {
    let hub = input.peer == Peer::Hub;
    let ev = |ext: EventKind, hub_ev: EventKind| Some(if hub { hub_ev } else { ext });

    // A doc that another peer retired: stay in it, live (ordinary arrival at its next change).
    if input.attached != Attached::None && input.retired_by_other {
        return out(ArrivalStep::S2a, ArrivalAction::StayLive, None);
    }

    let mut result = base_step(input, hub);

    // Colocated jj: files that would join or attach take the step 5 behavior. Step 1 has no doc, and 2a/2b
    // are about an already attached doc.
    if input.jj
        && matches!(
            result.step,
            ArrivalStep::S3 | ArrivalStep::S4 | ArrivalStep::S5
        )
    {
        result = if hub {
            out(
                ArrivalStep::S5,
                ArrivalAction::CreateFromDiskUnregistered,
                Some(EventKind::Seeded),
            )
        } else {
            out(
                ArrivalStep::S5,
                ArrivalAction::DetachAndAsk,
                Some(EventKind::Detached),
            )
        };
    }

    // ReadOnly: attach only when disk equals the doc; otherwise create from disk. Never join, never detach.
    if input.policy == Some(Policy::ReadOnly)
        && matches!(result.step, ArrivalStep::S4 | ArrivalStep::S5)
    {
        result = out(
            result.step,
            ArrivalAction::CreateFromDisk,
            Some(EventKind::Seeded),
        );
    }

    // A doc live under another path is forked, never attached twice (G12).
    if input.live_under_other_path
        && matches!(
            result.action,
            ArrivalAction::AttachSilently
                | ArrivalAction::JoinWithNotice
                | ArrivalAction::JoinAndWarn
        )
    {
        result.action = if input.fork_syncs == Some(false) {
            ArrivalAction::CreateFromContentNoHistory
        } else {
            ArrivalAction::ForkThenAttach
        };
    }

    if input.canonical_unavailable && result.step == ArrivalStep::S1 {
        result = out(
            ArrivalStep::S1,
            ArrivalAction::WaitFiveSecondsThenCreate,
            ev(EventKind::Switched, EventKind::Flipped),
        );
    }

    if input.case_collision {
        return out(result.step, ArrivalAction::OrdinarySync, None);
    }
    result
}

fn base_step(i: &ArrivalInput, hub: bool) -> ArrivalOutput {
    let ev = |ext: EventKind, hub_ev: EventKind| Some(if hub { hub_ev } else { ext });
    let no_doc = i.doc.is_none() || i.canonical_unavailable;
    if i.attached == Attached::Canonical && !no_doc {
        if Some(&i.disk) != i.doc.as_ref() && i.disk == i.blob {
            return ArrivalOutput {
                step: ArrivalStep::S2b,
                action: ArrivalAction::RetireCreateFlip,
                event: Some(EventKind::Retired),
                discarded: Some(1),
            };
        }
        return out(ArrivalStep::S2a, ArrivalAction::ReBaseline, None);
    }
    if no_doc {
        return out(
            ArrivalStep::S1,
            ArrivalAction::CreateFromDisk,
            ev(EventKind::Switched, EventKind::Flipped),
        );
    }
    if Some(&i.disk) == i.doc.as_ref() {
        return out(
            ArrivalStep::S3,
            ArrivalAction::AttachSilently,
            ev(EventKind::Switched, EventKind::Flipped),
        );
    }
    if i.disk == i.blob {
        return if hub {
            out(
                ArrivalStep::S4,
                ArrivalAction::JoinAndWarn,
                Some(EventKind::Joined),
            )
        } else {
            out(
                ArrivalStep::S4,
                ArrivalAction::JoinWithNotice,
                Some(EventKind::Switched),
            )
        };
    }
    if hub {
        out(
            ArrivalStep::S5,
            ArrivalAction::CreateFromDiskUnregistered,
            Some(EventKind::Seeded),
        )
    } else {
        out(
            ArrivalStep::S5,
            ArrivalAction::DetachAndAsk,
            Some(EventKind::Detached),
        )
    }
}
