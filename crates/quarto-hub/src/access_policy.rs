use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex};

use samod::{AccessPolicy, DocumentId, PeerId};

/// Audit-logging access policy for document sync.
///
/// Always allows access (returns `true`), and logs the authenticated user's
/// email the first time they touch a given document. samod consults the policy
/// on every inbound sync message, so `is_allowed` fires several times per
/// document open; the `logged` set collapses those into a single audit line per
/// `(peer, document)` pair. When auth is disabled the `peer_emails` map is empty
/// and no log entry is emitted. Call [`AuditAccessPolicy::forget_peer`] on
/// disconnect to drop a peer's dedup entries.
#[derive(Clone)]
pub struct AuditAccessPolicy {
    peer_emails: Arc<StdMutex<HashMap<PeerId, String>>>,
    /// `(peer, document)` pairs already audit-logged, so each first access is
    /// logged exactly once. Pruned per-peer on disconnect via `forget_peer`.
    logged: Arc<StdMutex<HashSet<(PeerId, DocumentId)>>>,
}

impl AuditAccessPolicy {
    pub fn new(peer_emails: Arc<StdMutex<HashMap<PeerId, String>>>) -> Self {
        Self {
            peer_emails,
            logged: Arc::new(StdMutex::new(HashSet::new())),
        }
    }

    /// Drop all dedup entries for `peer_id`, called when the peer disconnects.
    /// Keeps the set bounded across connections and lets a reconnecting peer
    /// audit-log its next access afresh.
    pub fn forget_peer(&self, peer_id: &PeerId) {
        self.logged.lock().unwrap().retain(|(p, _)| p != peer_id);
    }
}

impl AccessPolicy for AuditAccessPolicy {
    fn is_allowed(&self, doc_id: &DocumentId, peer_id: &PeerId) -> bool {
        let email = self.peer_emails.lock().unwrap().get(peer_id).cloned();

        if let Some(ref email) = email {
            // Log only the first access to this (peer, doc) pair. `insert`
            // returns true when the pair was newly added.
            let first_access = self
                .logged
                .lock()
                .unwrap()
                .insert((peer_id.clone(), doc_id.clone()));
            if first_access {
                tracing::info!(
                    email = %email,
                    document_id = %doc_id,
                    peer_id = %peer_id,
                    "Document accessed"
                );
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `f` with a scoped subscriber that captures formatted log output,
    /// returning everything logged during the call. Lets us assert on the
    /// audit "Document accessed" line without a live authenticated hub.
    fn capture_logs(f: impl FnOnce()) -> String {
        use std::io::Write;
        use tracing_subscriber::fmt::MakeWriter;

        #[derive(Clone)]
        struct BufWriter(Arc<StdMutex<Vec<u8>>>);
        impl Write for BufWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        impl<'a> MakeWriter<'a> for BufWriter {
            type Writer = BufWriter;
            fn make_writer(&'a self) -> Self::Writer {
                self.clone()
            }
        }

        let buf = Arc::new(StdMutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_writer(BufWriter(buf.clone()))
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        tracing::subscriber::with_default(subscriber, f);
        String::from_utf8(buf.lock().unwrap().clone()).unwrap()
    }

    #[test]
    fn is_allowed_always_returns_true() {
        let peer_emails = Arc::new(StdMutex::new(HashMap::new()));
        let policy = AuditAccessPolicy::new(peer_emails);

        let doc_id = DocumentId::new(&mut rand::rng());
        let peer_id = PeerId::from("test-peer");

        assert!(policy.is_allowed(&doc_id, &peer_id));
    }

    #[test]
    fn is_allowed_returns_true_with_known_peer() {
        let peer_emails = Arc::new(StdMutex::new(HashMap::new()));
        let peer_id = PeerId::from("known-peer");
        peer_emails
            .lock()
            .unwrap()
            .insert(peer_id.clone(), "user@example.com".to_string());

        let policy = AuditAccessPolicy::new(peer_emails);
        let doc_id = DocumentId::new(&mut rand::rng());

        assert!(policy.is_allowed(&doc_id, &peer_id));
    }

    #[test]
    fn no_log_when_peer_unknown() {
        // When peer_emails has no mapping (auth disabled scenario),
        // is_allowed still returns true without logging.
        let peer_emails = Arc::new(StdMutex::new(HashMap::new()));
        let policy = AuditAccessPolicy::new(peer_emails);
        let doc_id = DocumentId::new(&mut rand::rng());
        let peer_id = PeerId::from("unknown-peer");

        let mut allowed = false;
        let logs = capture_logs(|| allowed = policy.is_allowed(&doc_id, &peer_id));

        assert!(allowed);
        assert!(
            !logs.contains("Document accessed"),
            "unknown peer must not be audit-logged; got: {logs}"
        );
    }

    #[test]
    fn logs_document_accessed_with_email_for_known_peer() {
        // The migration kept the audit "Document accessed" log inside the now
        // synchronous is_allowed. Assert it still emits with the peer's email.
        let peer_id = PeerId::from("known-peer");
        let peer_emails = Arc::new(StdMutex::new(HashMap::new()));
        peer_emails
            .lock()
            .unwrap()
            .insert(peer_id.clone(), "user@example.com".to_string());
        let policy = AuditAccessPolicy::new(peer_emails);
        let doc_id = DocumentId::new(&mut rand::rng());

        let logs = capture_logs(|| {
            assert!(policy.is_allowed(&doc_id, &peer_id));
        });

        assert!(
            logs.contains("Document accessed"),
            "expected audit log line; got: {logs}"
        );
        assert!(
            logs.contains("user@example.com"),
            "audit log must include the peer's email; got: {logs}"
        );
    }

    #[test]
    fn logs_document_accessed_once_per_peer_doc_pair() {
        // samod 0.12 consults the policy on every inbound sync message, so a
        // single document open calls is_allowed several times. The audit line
        // must be emitted only on the first access to a given (peer, doc) pair.
        let peer_id = PeerId::from("known-peer");
        let peer_emails = Arc::new(StdMutex::new(HashMap::new()));
        peer_emails
            .lock()
            .unwrap()
            .insert(peer_id.clone(), "user@example.com".to_string());
        let policy = AuditAccessPolicy::new(peer_emails);
        let doc_id = DocumentId::new(&mut rand::rng());

        let logs = capture_logs(|| {
            assert!(policy.is_allowed(&doc_id, &peer_id));
            assert!(policy.is_allowed(&doc_id, &peer_id));
            assert!(policy.is_allowed(&doc_id, &peer_id));
        });

        let count = logs.matches("Document accessed").count();
        assert_eq!(
            count, 1,
            "repeated access to the same doc must log once; got {count}:\n{logs}"
        );
    }

    #[test]
    fn logs_each_distinct_document_once() {
        // Dedup is per (peer, doc) pair, not per peer: a second document for the
        // same peer is a distinct access and must be logged.
        let peer_id = PeerId::from("known-peer");
        let peer_emails = Arc::new(StdMutex::new(HashMap::new()));
        peer_emails
            .lock()
            .unwrap()
            .insert(peer_id.clone(), "user@example.com".to_string());
        let policy = AuditAccessPolicy::new(peer_emails);
        let doc_a = DocumentId::new(&mut rand::rng());
        let doc_b = DocumentId::new(&mut rand::rng());

        let logs = capture_logs(|| {
            policy.is_allowed(&doc_a, &peer_id);
            policy.is_allowed(&doc_a, &peer_id);
            policy.is_allowed(&doc_b, &peer_id);
        });

        let count = logs.matches("Document accessed").count();
        assert_eq!(
            count, 2,
            "two distinct docs must log twice; got {count}:\n{logs}"
        );
    }

    #[test]
    fn forget_peer_allows_relogging_after_disconnect() {
        // On disconnect the hub calls forget_peer so a reconnection audit-logs
        // afresh (and the dedup set can't grow unbounded across connections).
        let peer_id = PeerId::from("known-peer");
        let peer_emails = Arc::new(StdMutex::new(HashMap::new()));
        peer_emails
            .lock()
            .unwrap()
            .insert(peer_id.clone(), "user@example.com".to_string());
        let policy = AuditAccessPolicy::new(peer_emails);
        let doc_id = DocumentId::new(&mut rand::rng());

        let logs = capture_logs(|| {
            assert!(policy.is_allowed(&doc_id, &peer_id)); // logs
            assert!(policy.is_allowed(&doc_id, &peer_id)); // deduped
            policy.forget_peer(&peer_id); // simulate disconnect
            assert!(policy.is_allowed(&doc_id, &peer_id)); // logs again
        });

        let count = logs.matches("Document accessed").count();
        assert_eq!(
            count, 2,
            "forget_peer must let a later access re-log; got {count}:\n{logs}"
        );
    }
}
