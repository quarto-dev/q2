# Plan: Add `document_served` effect to samod-core

## Context

When authentication is enabled on the hub server, there is currently no audit trail of which user accessed which document. The server authenticates users at WebSocket upgrade time but does not log per-document access.

**Goal**: Emit a one-time event when document sync data is first transmitted to a remote peer for a given `(connection, document)` pair. This enables audit logging of "peer X was served document Y" without re-parsing outgoing CBOR at the network layer, and without noise from subsequent sync round-trips.

**Approach**: Use existing state in `PeerDocConnection` to detect the first serve. The `last_sent` field is already `Option<UnixTimestamp>` — it starts as `None` and becomes `Some` the first time a sync message is generated. This is a natural one-time transition with zero overhead on subsequent messages.

## Design

The signal flows from the document actor to the Hub:

```
DocState::generate_sync_messages()
    │ (checks conn.has_been_served() before calling generate;
    │  if !served && message returned → first_served for that conn)
    ▼
DocumentActor::generate_sync_messages()
    │ (emits DocToHubMsgPayload::DocumentServed when first_served)
    ▼
Hub State::handle_event()
    │ (looks up peer_id from connection_id, pushes to HubResults)
    ▼
HubResults.documents_served
    │
    ▼
Runtime layer (callback)
```

No HashSet, no per-message bookkeeping — just a boolean check on state that is already being written. Ready and Request are untouched; the detection lives in DocState which already iterates over connections.

## Phase 1: samod-core changes

### 1a: Add `has_been_served()` accessor to `PeerDocConnection`

**File**: `samod-core/src/actors/document/peer_doc_connection.rs`

- [x] Add a one-line accessor:

```rust
pub(super) fn has_been_served(&self) -> bool {
    self.state.last_sent.is_some()
}
```

`generate_sync_message()` is unchanged — it continues to return `Option<sync::Message>`.

### 1b: Detect first serve in `DocState::generate_sync_messages()`

**File**: `samod-core/src/actors/document/doc_state.rs`

- [x] Check `conn.has_been_served()` before calling the existing generate path. If the connection had not been served and a message is returned, flag it:

```rust
// Return type adds a Vec of first-served connection IDs
pub(crate) fn generate_sync_messages(
    &mut self,
    now: UnixTimestamp,
    connections: &mut PeerDocConnections,
) -> (HashMap<ConnectionId, Vec<SyncMessage>>, Vec<ConnectionId>) {
    let mut messages = HashMap::new();
    let mut first_served = Vec::new();

    // existing iteration over connections:
    for (conn_id, conn) in connections.iter_mut() {
        let was_unserved = !conn.has_been_served();

        // existing call through Ready/Request (unchanged signatures)
        if let Some(msg) = self.ready.generate_sync_message(now, doc, conn) {
            if was_unserved {
                first_served.push(conn_id);
            }
            messages.entry(conn_id).or_default().push(msg);
        }
    }

    (messages, first_served)
}
```

**Ready and Request are unchanged** — no signature or return-type modifications needed.

### 1c: Emit from `DocumentActor::generate_sync_messages()`

**File**: `samod-core/src/actors/document/document_actor.rs`

- [x] Consume the first_served list alongside the messages:

```rust
fn generate_sync_messages(&mut self, now: UnixTimestamp, out: &mut DocActorResult) {
    let doc_id = self.document_id.clone();
    let (messages, first_served) = self
        .doc_state
        .generate_sync_messages(now, &mut self.peer_connections);

    for conn_id in first_served {
        out.send_message(DocToHubMsgPayload::DocumentServed {
            connection_id: conn_id,
            document_id: doc_id.clone(),
        });
    }

    for (conn_id, msgs) in messages {
        for message in msgs {
            out.send_message(DocToHubMsgPayload::SendSyncMessage {
                connection_id: conn_id,
                document_id: doc_id.clone(),
                message,
            });
        }
    }
}
```

### 1d: Add `DocumentServed` variant to `DocToHubMsgPayload`

**File**: `samod-core/src/actors/messages/doc_to_hub_msg.rs`

- [x] Add variant (includes `document_id` directly, matching the `SendSyncMessage` pattern):

```rust
pub enum DocToHubMsgPayload {
    // ... existing variants ...
    DocumentServed {
        connection_id: ConnectionId,
        document_id: DocumentId,
    },
}
```

### 1e: Handle in Hub and add to `HubResults`

**File**: `samod-core/src/actors/hub/hub_results.rs`

- [x] Define `DocumentServed` struct here (where it's consumed) and add field to `HubResults`:

```rust
pub struct DocumentServed {
    pub document_id: DocumentId,
    pub connection_id: ConnectionId,
    pub peer_id: PeerId,
}
```

- [x] Add `documents_served: Vec<DocumentServed>` to `HubResults` and its `Default` impl

**File**: `samod-core/src/actors/hub/state.rs`

- [x] Handle the new message variant, looking up the peer_id from the connection (document_id comes directly from the message — no reverse lookup needed):

```rust
DocToHubMsgPayload::DocumentServed { connection_id, document_id } => {
    if let Some((_, peer_id)) = self.established_connection(connection_id) {
        results.documents_served.push(DocumentServed {
            document_id,
            connection_id,
            peer_id,
        });
    }
}
```

---

## Phase 2: samod runtime layer

### 2a: Re-export `DocumentServed`

- [x] Re-export `DocumentServed` from `samod-core`'s public API (so `samod` and downstream crates can import it)
- [x] Re-export from `samod`'s public API (so `quarto-hub` can import it)

### 2b: Surface in samod runtime

**File**: `samod/src/lib.rs` (in `Inner::handle_event()`)

- [x] After processing `HubResults`, iterate `results.documents_served` and forward to the callback
- [x] Update the `HubResults` destructuring in `Inner::handle_event()` to include the new `documents_served` field

**Approach: Boxed callback on RepoBuilder** (unlike `announce_policy` which uses a generic type parameter for hot-path performance, this callback is cold-path so we use a boxed trait object to avoid adding a fourth type parameter):

**File**: `samod/src/builder.rs`

- [x] Add field to `RepoBuilder`: `on_document_served: Option<Box<dyn Fn(DocumentServed) + Send + Sync>>`
- [x] Default to `None` — fully backwards compatible, no existing API changes
- [x] Add builder method:

```rust
pub fn with_on_document_served(
    mut self,
    callback: impl Fn(DocumentServed) + Send + Sync + 'static,
) -> Self {
    self.on_document_served = Some(Box::new(callback));
    self
}
```

- [x] The callback receives `DocumentServed` synchronously during event processing; it must not block

### 2c: Update test harness

**File**: `samod-test-harness/src/samod_wrapper.rs`

- [x] Collect `documents_served` from `HubResults` during `process_events()` so tests can assert on them

---

## Phase 3: quarto-hub integration

### 3a: Switch to forked Samod (if not already done)

**File**: `crates/quarto-hub/Cargo.toml`

- [x] Ensure samod dependency points to the fork with `document_served` support

### 3b: Add connection→email map to HubContext

**File**: `crates/quarto-hub/src/context.rs`

- [x] Add `connection_emails: Arc<Mutex<HashMap<ConnectionId, String>>>` field to `HubContext`
  - Always present; empty when auth is disabled (avoids `Option` wrapping throughout)
- [x] Initialize in `HubContext::new()`: `Arc::new(Mutex::new(HashMap::new()))`
- [x] Add accessor: `pub fn connection_emails(&self) -> &Arc<Mutex<HashMap<ConnectionId, String>>>`
- [x] Wire the callback on the repo builder (only when auth is enabled):

```rust
let connection_emails = Arc::new(Mutex::new(HashMap::new()));

let mut builder = Repo::build_tokio()
    .with_storage(samod_storage)
    .with_announce_policy(|_doc_id, _peer_id| false);

if auth_config.is_some() {
    let emails = connection_emails.clone();
    builder = builder.with_on_document_served(move |served| {
        if let Some(email) = emails.lock().unwrap().get(&served.connection_id) {
            tracing::info!(
                email = %email,
                document_id = %served.document_id,
                "Document served"
            );
        }
    });
}

let repo = builder.load().await;
```

### 3c: Thread email from ws_handler into handle_websocket

**File**: `crates/quarto-hub/src/server.rs`

- [x] In `ws_handler`: when auth is enabled, call `authenticate_claims()` instead of `authenticate()` to capture the email. When auth is disabled, pass `None`:

```rust
async fn ws_handler(
    State(ctx): State<SharedContext>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> std::result::Result<impl IntoResponse, StatusCode> {
    let email = if ctx.auth_config().is_some() {
        if !ctx.allow_insecure_auth() {
            check_ws_origin(&headers)?;
        }
        let claims = ctx.authenticate_claims(cookie_token(&headers).as_deref()).await?;
        Some(claims.email)
    } else {
        None
    };

    Ok(ws.on_upgrade(move |socket| handle_websocket(socket, ctx, email)))
}
```

- [x] In `handle_websocket`: accept `email: Option<String>`, insert into the map after `accept_axum()`, remove on disconnect:

```rust
async fn handle_websocket(socket: WebSocket, ctx: SharedContext, email: Option<String>) {
    match ctx.repo().accept_axum(socket) {
        Ok(connection) => {
            let conn_id = connection.id();

            // Store connection→email mapping for document_served callback
            if let Some(email) = &email {
                ctx.connection_emails().lock().unwrap().insert(conn_id, email.clone());
            }

            info!(
                peer_info = ?connection.info(),
                email = email.as_deref().unwrap_or("-"),
                "WebSocket client connected"
            );

            let reason = connection.finished().await;

            // Clean up mapping
            ctx.connection_emails().lock().unwrap().remove(&conn_id);

            info!(
                peer_info = ?connection.info(),
                email = email.as_deref().unwrap_or("-"),
                reason = ?reason,
                "WebSocket client disconnected"
            );
        }
        Err(samod::Stopped) => {
            tracing::warn!("WebSocket rejected: repo is stopped");
        }
    }
}
```

---

## Phase 4: Verification

- [x] `cargo build --workspace` (verifies forked samod compiles)
- [x] `cargo nextest run --workspace`
- [x] `cd hub-client && npm run build:all` (includes WASM build)
- [x] `cd hub-client && npm run test:ci`
- [ ] Manual test with auth enabled (requires live server):
  1. Start hub with `--google-client-id`
  2. Connect with hub-client
  3. Open a document
  4. Verify server logs show `Document served` with email and document_id
  5. Verify the log fires only once per (connection, document) pair

---

## Files touched

| Repository | File | Change |
|---|---|---|
| **shikokuchuo/samod** | `samod-core/src/actors/document/peer_doc_connection.rs` | Add `has_been_served()` accessor (1 line) |
| | `samod-core/src/actors/document/doc_state.rs` | Check `has_been_served()` before generate; return first-served list |
| | `samod-core/src/actors/document/document_actor.rs` | Emit `DocumentServed` for first-served connections |
| | `samod-core/src/actors/messages/doc_to_hub_msg.rs` | New `DocToHubMsgPayload::DocumentServed` variant |
| | `samod-core/src/actors/hub/hub_results.rs` | `DocumentServed` struct + `documents_served` field |
| | `samod-core/src/actors/hub/state.rs` | Handle new message, look up peer_id |
| | `samod/src/builder.rs` | `Option<Box<dyn Fn>>` field + `with_on_document_served()` builder method |
| | `samod/src/lib.rs` | Forward effect to callback |
| | `samod-test-harness/src/samod_wrapper.rs` | Collect for testing |
| **kyoto** | `crates/quarto-hub/Cargo.toml` | Ensure forked samod dependency |
| | `crates/quarto-hub/src/context.rs` | Wire `on_document_served` callback with logging |
| | `crates/quarto-hub/src/server.rs` | Capture email on WS connect; store connection→email mapping |

## Not in scope

- **Access control** (blocking the serve based on the callback return value). That would require the effect to be a gate rather than a notification, which changes the sans-IO model significantly. Could be a follow-up.
- **Tracking ephemeral message delivery** (different concern).
- **Client-side changes** — this approach requires no client modifications.
