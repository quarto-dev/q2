# Plan: Use Samod's AccessPolicy for Document Access Audit Logging

## Context

When authentication is enabled on the hub server, we want an audit trail of which user accessed which document. The previous approach (`2026-03-02-document-access-logging.md`) used a custom `document_served` callback added to the samod fork. Samod now has an upstream `AccessPolicy` trait (added in `1b6ded8`, 2026-03-12) that provides a cleaner integration point.

**Goal**: When auth is enabled, log the authenticated user's email address the first time they access (sync) a document per session, using Samod's `AccessPolicy` trait. When auth is disabled, use the default `AllowAll` policy with no logging overhead.

**Key difference from previous approach**: AccessPolicy operates on the **request** side (peer asks to sync a document) rather than the **serve** side (data is first sent). It receives `(DocumentId, PeerId)` rather than `ConnectionId`, so we need a `PeerId -> email` mapping instead of `ConnectionId -> email`.

## Samod AccessPolicy API

```rust
// samod/src/access_policy.rs
pub trait AccessPolicy: Clone + Send + 'static {
    fn should_allow(
        &self,
        doc_id: DocumentId,
        peer_id: PeerId,
    ) -> impl Future<Output = bool> + Send + 'static;
}

// Blanket impl for closures: Fn(DocumentId, PeerId) -> bool
```

- Called once per (peer, document) session when a peer first requests a document
- Returning `false` sends `doc-unavailable` and blocks sync
- PeerId is **not authenticated by samod** — authentication must happen at the network layer (which quarto-hub already does via OIDC at WebSocket upgrade)

## Design

### Architecture

```
WebSocket upgrade
    | (OIDC auth -> extract email)
    v
AcceptorEvent::ClientConnected { peer_info, connection_id }
    | (map peer_info.peer_id -> email in shared HashMap)
    v
AccessPolicy::should_allow(doc_id, peer_id)
    | (look up email from peer_id, log access, return true)
    v
AcceptorEvent::ClientDisconnected
    | (remove peer_id -> email mapping)
```

### Key Decisions

1. **Always allow, only log**: The AccessPolicy always returns `true`. We're using it for audit logging, not authorization. Authorization can be layered on later.
2. **PeerId -> email mapping**: Replace the existing `connection_emails: HashMap<ConnectionId, String>` with `peer_emails: HashMap<PeerId, String>`. The AccessPolicy receives PeerId, not ConnectionId.
3. **Struct impl, not closure**: Use a named struct (`AuditAccessPolicy`) implementing `AccessPolicy` rather than a closure, because:
   - Closures returning `bool` can't do async logging (the blanket impl is sync-only)
   - A struct is more readable and testable
   - We can add fields later (e.g., for authorization rules)
4. **No conditional typing**: Always register the AuditAccessPolicy. When auth is disabled, the peer_emails map is empty, so lookups silently return `None` and no logging occurs. This avoids builder type gymnastics.
5. **Log DocumentId only**: Don't attempt to resolve DocumentId -> file path in the policy. Operators can correlate via the `/api/documents` endpoint or index logs. Path resolution can be added later.
6. **No dedup needed**: Samod now calls `should_allow()` once per (peer, document) session, so no client-side deduplication is required.

---

## Work Items

### Phase 1: Create AuditAccessPolicy

- [x] **Create `access_policy.rs`** module in `crates/quarto-hub/src/`
  - Define `AuditAccessPolicy` struct:
    ```rust
    #[derive(Clone)]
    pub struct AuditAccessPolicy {
        peer_emails: Arc<StdMutex<HashMap<PeerId, String>>>,
    }
    ```
  - Implement `samod::AccessPolicy` for it:
    ```rust
    impl AccessPolicy for AuditAccessPolicy {
        fn should_allow(
            &self,
            doc_id: DocumentId,
            peer_id: PeerId,
        ) -> impl Future<Output = bool> + Send + 'static {
            let email = self.peer_emails
                .lock()
                .unwrap()
                .get(&peer_id)
                .cloned();

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
    ```
  - Add `pub mod access_policy;` to `lib.rs`
- [x] **Write unit tests** in `access_policy.rs` (`#[cfg(test)] mod tests`)
  - Test that `should_allow()` always returns `true`
  - Test that when `peer_emails` contains a mapping, the email is logged
  - Test that when `peer_emails` has no mapping (auth disabled), no log entry is emitted

### Phase 2: Wire into HubContext

- [x] **Replace `connection_emails` with `peer_emails`** in `context.rs`
  - Change type from `Arc<StdMutex<HashMap<ConnectionId, String>>>` to `Arc<StdMutex<HashMap<PeerId, String>>>`
  - Rename field and accessor: `connection_emails` -> `peer_emails`
  - Import `PeerId` from samod (already re-exported: `samod_core::PeerId`)
- [x] **Create `AuditAccessPolicy` and pass to builder** in `HubContext::new()`
  ```rust
  let peer_emails = Arc::new(StdMutex::new(HashMap::new()));
  let audit_policy = AuditAccessPolicy::new(peer_emails.clone());

  let builder = Repo::build_tokio()
      .with_storage(samod_storage)
      .with_announce_policy(NeverAnnounce)
      .with_access_policy(audit_policy);

  let repo = builder.load().await;
  ```
  - Note: `RepoBuilder` has 4 type parameters `<S, R, A, Ac>` where `Ac` defaults to `AllowAll`. Calling `.with_access_policy(audit_policy)` sets `Ac = AuditAccessPolicy`.

### Phase 3: Update WebSocket handler

- [x] **Update `handle_websocket()`** in `server.rs`
  - On `ClientConnected`: insert `peer_info.peer_id -> email` into `peer_emails` (instead of `connection_id -> email`)
  - On `ClientDisconnected`: remove by peer_id
  - Store `peer_id` locally so we can clean up on disconnect:
    ```rust
    AcceptorEvent::ClientConnected { peer_info, connection_id } => {
        let peer_id = peer_info.peer_id.clone();
        if let Some(ref email) = email {
            ctx.peer_emails().lock().unwrap()
                .insert(peer_id.clone(), email.clone());
        }
        // ... existing logging ...
    }
    AcceptorEvent::ClientDisconnected { connection_id, reason } => {
        if let Some(ref peer_id) = connected_peer_id {
            ctx.peer_emails().lock().unwrap().remove(peer_id);
        }
        // ... existing logging ...
    }
    ```

### Phase 4: Verification

- [x] `cargo build --workspace`
- [x] `cargo nextest run --workspace` (6541 tests passed)
- [x] `cargo xtask verify` — lint, format, build all pass; tree-sitter step fails due to missing binary (pre-existing env issue)
- [ ] Manual test with auth enabled:
  1. Start hub with OIDC auth enabled
  2. Connect with hub-client, authenticate
  3. Open a document
  4. Verify server logs show `Document accessed` with email, document_id, peer_id
  5. Verify log fires once per new document synced (not on repeated sync messages)
  6. Verify no logging occurs when auth is disabled

---

## Files touched

| File | Change |
|---|---|
| `crates/quarto-hub/src/access_policy.rs` | **New**: `AuditAccessPolicy` struct implementing `samod::AccessPolicy`, with unit tests |
| `crates/quarto-hub/src/context.rs` | Replace `connection_emails` with `peer_emails`; wire `AuditAccessPolicy` into repo builder |
| `crates/quarto-hub/src/server.rs` | Update `handle_websocket` to map `peer_id -> email` instead of `connection_id -> email` |
| `crates/quarto-hub/src/lib.rs` | Add `pub mod access_policy;` |
| `crates/quarto-hub/Cargo.toml` | Possibly update samod git ref (if AccessPolicy is on a newer commit) |

## Behavioral differences from previous approach

| Aspect | Previous (document_served callback) | New (AccessPolicy) |
|---|---|---|
| **Trigger** | First time sync data is sent to peer | First time peer requests a document per session |
| **Identifier** | ConnectionId | PeerId |
| **Frequency** | Once per (connection, document) pair | Once per (peer, document) session (deduplicated by samod) |
| **Location** | Outgoing path (serve) | Incoming path (request) |
| **Upstream support** | Custom fork addition | Upstream samod trait |
| **Future extensibility** | Notification only | Can evolve into authorization (return false to deny) |

## Not in scope

- **Authorization** (denying access based on email/document). The policy always returns `true`. Authorization rules can be added to `AuditAccessPolicy::should_allow()` later.
- **Document path resolution** in log messages. Log raw DocumentId; path correlation is done externally.
- **Client-side changes** — no client modifications needed.
