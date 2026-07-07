//! Rust mirror of the editor's execution channel wire format (bd-sfet3264,
//! Phase 4a).
//!
//! The hub-client editor and this provider exchange two ephemeral,
//! project-scoped signals on the **index** `DocHandle` (see the TS side in
//! `hub-client/src/services/executionChannel.ts`):
//!
//!  - a **capability beacon** the provider re-broadcasts periodically
//!    (`exec/beacon` — "I'm online and can run engines X, Y"), and
//!  - an **execute request** the editor sends (`exec/request` — "run this
//!    document now").
//!
//! ## Cross-language encoding
//!
//! Automerge's ephemeral messages carry an opaque byte payload. The browser's
//! `DocHandle.broadcast(obj)` CBOR-encodes `obj` (cbor-x with
//! `{ useRecords: false }` → *standard* CBOR maps with string keys) and the
//! samod docs note the JS side "will only process payloads which are valid
//! CBOR". So we encode/decode the exact same shape with `ciborium`: an
//! internally-tagged enum on `kind`, camelCase field names — byte-compatible
//! with what the editor produces and consumes.
//!
//! In Phase 4a the provider only *sends* beacons and *receives* requests;
//! liveness bookkeeping (applyBeacon/pruneExecutors) is the editor's job and
//! stays on the TS side.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How often the provider re-broadcasts its capability beacon. Mirrors
/// `BEACON_INTERVAL_MS = 3000` on the TS side.
pub const BEACON_INTERVAL: Duration = Duration::from_millis(3000);

/// Liveness window the editor uses to mark a provider offline (1.5× the
/// interval — the locked `TIMEOUT = 1.5 × INTERVAL` contract from D2). The
/// provider doesn't consume this directly; it's mirrored here so both sides
/// name the same contract.
pub const BEACON_TIMEOUT: Duration = Duration::from_millis(4500);

/// One message on the execution channel. Internally tagged on `kind` and
/// serialized with camelCase field names to match the TS `ExecMessage` union.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ExecMessage {
    /// Provider → editors: "I'm online and can run these engines."
    #[serde(rename = "exec/beacon")]
    Beacon {
        #[serde(rename = "actorId")]
        actor_id: String,
        engines: Vec<String>,
        /// Monotonic per-provider counter. Unused in Phase 4a (no claims);
        /// reserved for the D5 `--force` takeover (Phase 6).
        generation: u64,
    },
    /// Editor → provider: "please run this document now."
    #[serde(rename = "exec/request")]
    Request {
        path: String,
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "requesterActorId")]
        requester_actor_id: String,
    },
}

impl ExecMessage {
    /// Build a capability beacon.
    pub fn beacon(actor_id: impl Into<String>, engines: Vec<String>, generation: u64) -> Self {
        ExecMessage::Beacon {
            actor_id: actor_id.into(),
            engines,
            generation,
        }
    }

    /// CBOR-encode this message for `DocHandle::broadcast`.
    pub fn to_cbor(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        ciborium::into_writer(self, &mut buf)
            .expect("serializing an ExecMessage to CBOR cannot fail");
        buf
    }
}

/// Decode an untrusted ephemeral payload into a typed [`ExecMessage`], or
/// `None` if it isn't a well-formed execution message. The index handle may
/// carry other ephemeral traffic, so anything that isn't an `exec/*` message we
/// recognize is ignored (mirrors the TS `parseExecMessage`).
pub fn parse_exec_message(bytes: &[u8]) -> Option<ExecMessage> {
    ciborium::from_reader(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ciborium::value::Value;

    #[test]
    fn beacon_round_trips_through_cbor() {
        let msg = ExecMessage::beacon("actor-1", vec!["knitr".into(), "jupyter".into()], 3);
        let bytes = msg.to_cbor();
        assert_eq!(parse_exec_message(&bytes), Some(msg));
    }

    #[test]
    fn request_round_trips_through_cbor() {
        let msg = ExecMessage::Request {
            path: "notebook.qmd".into(),
            request_id: "req-abc".into(),
            requester_actor_id: "actor-2".into(),
        };
        let bytes = msg.to_cbor();
        assert_eq!(parse_exec_message(&bytes), Some(msg));
    }

    /// The strongest cross-language check we can make without the browser: the
    /// CBOR bytes must decode to a *standard* CBOR map whose keys are exactly
    /// the ones the TS `parseExecMessage` inspects. If serde ever emitted an
    /// array-tagged enum or snake_case keys, the editor would silently drop
    /// our beacon.
    #[test]
    fn beacon_cbor_shape_matches_the_ts_contract() {
        let msg = ExecMessage::beacon("actor-1", vec!["knitr".into()], 7);
        let value: Value = ciborium::from_reader(msg.to_cbor().as_slice()).unwrap();
        let Value::Map(entries) = value else {
            panic!("beacon must encode as a CBOR map, got {value:?}");
        };
        let keys: Vec<String> = entries
            .iter()
            .filter_map(|(k, _)| k.as_text().map(str::to_string))
            .collect();
        assert!(keys.contains(&"kind".to_string()), "keys: {keys:?}");
        assert!(keys.contains(&"actorId".to_string()), "keys: {keys:?}");
        assert!(keys.contains(&"engines".to_string()), "keys: {keys:?}");
        assert!(keys.contains(&"generation".to_string()), "keys: {keys:?}");

        // `kind` must be the exact discriminator string the editor switches on.
        let kind = entries
            .iter()
            .find(|(k, _)| k.as_text() == Some("kind"))
            .and_then(|(_, v)| v.as_text())
            .unwrap();
        assert_eq!(kind, "exec/beacon");
    }

    #[test]
    fn request_cbor_shape_uses_camel_case_keys() {
        let msg = ExecMessage::Request {
            path: "a.qmd".into(),
            request_id: "r1".into(),
            requester_actor_id: "actor".into(),
        };
        let value: Value = ciborium::from_reader(msg.to_cbor().as_slice()).unwrap();
        let Value::Map(entries) = value else {
            panic!("request must encode as a CBOR map");
        };
        let keys: Vec<String> = entries
            .iter()
            .filter_map(|(k, _)| k.as_text().map(str::to_string))
            .collect();
        for expected in ["kind", "path", "requestId", "requesterActorId"] {
            assert!(
                keys.contains(&expected.to_string()),
                "missing {expected}: {keys:?}"
            );
        }
    }

    #[test]
    fn junk_and_unknown_kinds_parse_to_none() {
        assert_eq!(parse_exec_message(&[0xff, 0x00, 0x13]), None);
        assert_eq!(parse_exec_message(b"not cbor at all"), None);

        // Valid CBOR, but not an exec message.
        let mut other = Vec::new();
        ciborium::into_writer(
            &Value::Map(vec![(
                Value::Text("kind".into()),
                Value::Text("presence/hello".into()),
            )]),
            &mut other,
        )
        .unwrap();
        assert_eq!(parse_exec_message(&other), None);
    }

    #[test]
    fn a_request_from_the_editor_is_decoded_from_a_cbor_map() {
        // Simulate exactly what the browser sends: a plain CBOR map (as cbor-x
        // with useRecords:false produces) with the editor's field names.
        let mut bytes = Vec::new();
        ciborium::into_writer(
            &Value::Map(vec![
                (
                    Value::Text("kind".into()),
                    Value::Text("exec/request".into()),
                ),
                (Value::Text("path".into()), Value::Text("report.qmd".into())),
                (Value::Text("requestId".into()), Value::Text("req-9".into())),
                (
                    Value::Text("requesterActorId".into()),
                    Value::Text("editor-actor".into()),
                ),
            ]),
            &mut bytes,
        )
        .unwrap();

        assert_eq!(
            parse_exec_message(&bytes),
            Some(ExecMessage::Request {
                path: "report.qmd".into(),
                request_id: "req-9".into(),
                requester_actor_id: "editor-actor".into(),
            })
        );
    }
}
