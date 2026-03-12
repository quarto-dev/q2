use std::collections::HashMap;
use std::future::Future;
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
    fn should_allow(
        &self,
        doc_id: DocumentId,
        peer_id: PeerId,
    ) -> impl Future<Output = bool> + Send + 'static {
        let email = self.peer_emails.lock().unwrap().get(&peer_id).cloned();

        if let Some(ref email) = email {
            tracing::info!(
                email = %email,
                document_id = %doc_id,
                peer_id = %peer_id,
                "Document accessed"
            );
        }

        async { true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn should_allow_always_returns_true() {
        let peer_emails = Arc::new(StdMutex::new(HashMap::new()));
        let policy = AuditAccessPolicy::new(peer_emails);

        let doc_id = DocumentId::new(&mut rand::rng());
        let peer_id = PeerId::from("test-peer");

        assert!(policy.should_allow(doc_id, peer_id).await);
    }

    #[tokio::test]
    async fn should_allow_returns_true_with_known_peer() {
        let peer_emails = Arc::new(StdMutex::new(HashMap::new()));
        let peer_id = PeerId::from("known-peer");
        peer_emails
            .lock()
            .unwrap()
            .insert(peer_id.clone(), "user@example.com".to_string());

        let policy = AuditAccessPolicy::new(peer_emails);
        let doc_id = DocumentId::new(&mut rand::rng());

        assert!(policy.should_allow(doc_id, peer_id).await);
    }

    #[tokio::test]
    async fn no_log_when_peer_unknown() {
        // When peer_emails has no mapping (auth disabled scenario),
        // should_allow still returns true without logging.
        let peer_emails = Arc::new(StdMutex::new(HashMap::new()));
        let policy = AuditAccessPolicy::new(peer_emails);

        let doc_id = DocumentId::new(&mut rand::rng());
        let peer_id = PeerId::from("unknown-peer");

        // Just verify it returns true and doesn't panic
        assert!(policy.should_allow(doc_id, peer_id).await);
    }
}
