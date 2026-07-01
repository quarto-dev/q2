use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use samod::{AccessPolicy, DocumentId, PeerId};

/// Audit-logging access policy for document sync.
///
/// Always allows access (returns `true`), but logs the authenticated user's
/// email the first time they request a document. When auth is disabled the
/// `peer_emails` map is empty and no log entry is emitted.
#[derive(Clone)]
pub struct AuditAccessPolicy {
    peer_emails: Arc<StdMutex<HashMap<PeerId, String>>>,
}

impl AuditAccessPolicy {
    pub fn new(peer_emails: Arc<StdMutex<HashMap<PeerId, String>>>) -> Self {
        Self { peer_emails }
    }
}

impl AccessPolicy for AuditAccessPolicy {
    fn is_allowed(&self, doc_id: &DocumentId, peer_id: &PeerId) -> bool {
        let email = self.peer_emails.lock().unwrap().get(peer_id).cloned();

        if let Some(ref email) = email {
            tracing::info!(
                email = %email,
                document_id = %doc_id,
                peer_id = %peer_id,
                "Document accessed"
            );
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
}
