# quarto-hub MVP: Automerge-Based Collaborative Infrastructure

**Issue:** k-4wex
**Status:** In Progress
**Created:** 2025-12-08

## Overview

Create a new crate `quarto-hub` that:
1. Produces a binary called `hub` (not `quarto-hub`)
2. Has a library export for reuse
3. Provides automerge-based collaborative editing infrastructure
4. Manages a `.quarto/hub/` directory with lockfiles and persistence

---

## Decisions Made

| Question | Decision |
|----------|----------|
| Web framework | **axum** (v0.8 to match samod) |
| Communication | **WebSocket + REST hybrid** |
| Document scope | **Multiple documents per project** (all .qmd files) |
| Automerge crate | **samod** (v0.6, JS-compatible replacement for automerge_repo) |
| Persistence | **Save on every edit** (low concurrency ~tens of users) |
| Auth (MVP) | **None** - local development, trust network |
| Future sync | Eventually sync with remote source |

---

## Working with samod

### Source Code Reference

**IMPORTANT**: When implementing samod integration, consult the source code at:

```
external-sources/samod/
├── samod/src/           # Main library
│   ├── lib.rs           # Repo, DocHandle exports
│   ├── storage.rs       # Storage trait, TokioFilesystemStorage
│   ├── storage/
│   │   └── filesystem.rs # TokioFilesystemStorage implementation
│   ├── websocket.rs     # Repo::accept_axum() for WebSocket
│   └── doc_handle.rs    # DocHandle API
├── samod/tests/
│   └── smoke.rs         # Real usage examples (VERY USEFUL)
└── samod-core/          # Core types (StorageKey, DocumentId, etc.)
```

The `smoke.rs` tests are particularly valuable - they show real patterns for:
- Creating and configuring a Repo
- Creating documents with `repo.create()`
- Finding documents with `repo.find()`
- Connecting peers
- Listening for changes

### Why samod over automerge_repo

The original Rust `automerge_repo` crate has incompatible filesystem layout and WebSocket protocol with the JavaScript `automerge-repo`. The `samod` crate was created by the same author (alexjg) specifically to achieve JS interoperability.

### Key samod APIs

```rust
// Creating a Repo with storage
let storage = samod::storage::TokioFilesystemStorage::new(&storage_path);
let repo = samod::Repo::build_tokio()
    .with_storage(storage)
    .load()
    .await;

// Creating a document
let doc_handle = repo.create(Automerge::new()).await.unwrap();
let doc_id = doc_handle.document_id().clone();

// Finding a document
let maybe_handle = repo.find(doc_id).await.unwrap();

// Accessing document contents
doc_handle.with_document(|doc| {
    // doc is &mut Automerge
    doc.transact(|tx| {
        tx.put(ROOT, "key", "value")?;
        Ok(())
    })
});

// WebSocket handling in axum
// NOTE: accept_axum() returns Result<Connection, Stopped> - handle errors!
async fn ws_handler(ws: WebSocketUpgrade, State(ctx): State<SharedContext>) -> Response {
    ws.on_upgrade(|socket| async move {
        match ctx.repo.accept_axum(socket) {
            Ok(_conn) => {
                // Connection handles sync automatically.
                // The connection stays alive until the WebSocket closes.
            }
            Err(samod::Stopped) => {
                tracing::warn!("WebSocket rejected: repo is stopped");
            }
        }
    })
}
```

---

## Architecture

### HubContext (Shared State)

```rust
#[derive(Clone)]
struct HubContext {
    /// samod Repo - handles document storage, sync, and concurrency internally
    /// Clone is cheap: Repo wraps Arc<Mutex<Inner>>
    repo: samod::Repo,

    /// Manages .quarto/hub/ directory, lockfile, hub.json config
    /// Arc because StorageManager contains File (not Clone)
    storage: Arc<StorageManager>,
}
```

**axum State requirement**: `State<T>` requires `T: Clone`. Both fields satisfy this:
- `samod::Repo` implements Clone (cloning gives another handle to the same repo)
- `Arc<StorageManager>` is Clone (cloning gives another reference)

**Key insight**: samod's `Repo` handles all document concurrency internally. We don't need `DashMap` or per-document locking.

### StorageManager (Directory + Config)

```rust
pub struct StorageManager {
    project_root: PathBuf,
    hub_dir: PathBuf,
    lock_file: File,              // NOT Clone - hence Arc<StorageManager>
    config: HubStorageConfig,     // Owned; read-only after startup
}
```

**Design decision**: StorageManager owns both the paths AND the config (`hub.json` contents). This keeps all `.quarto/hub/` concerns in one place.

**Config lifecycle**:
1. On startup: load `hub.json`, update `last_started_at`, save back to disk
2. During runtime: config is read-only via `storage.config()`
3. Future: if runtime mutation needed, change to `config: RwLock<HubStorageConfig>` with an `update_config()` method that handles both mutation and serialization

**Lockfile**: Uses `fs2` for advisory file locking. Lock is held for lifetime of StorageManager and automatically released on process death (even crashes).

---

## Hub Storage Directory Structure

```
.quarto/hub/
├── hub.lock              # OUR lockfile (advisory lock held while running)
├── hub.json              # OUR config with version (for migrations)
└── automerge/            # SAMOD's storage directory
    └── (managed by TokioFilesystemStorage - do not manually modify)
```

**Important**: The `automerge/` subdirectory is managed entirely by samod's `TokioFilesystemStorage`. Its internal structure (splayed directories, file naming) is samod's concern, not ours. We only configure the path.

### hub.json Format

```json
{
  "version": 1,
  "created_at": "1765224570",
  "last_started_at": "1765224600",
  "index_document_id": "2eH8f7kJ..."
}
```

Fields:
- `version`: Enables future migrations. If hub encounters a version newer than it supports, it refuses to start with a helpful error message.
- `index_document_id`: The bs58-encoded DocumentId for the project index document (stores path → doc-id mappings). This field is `null` or absent on first run, then populated after the index document is created.

### Code changes for Phase 2

**storage.rs** updates needed:

```rust
// In HubStorageConfig:
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubStorageConfig {
    pub version: u32,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_document_id: Option<String>,  // NEW: bs58-encoded DocumentId
}

// In StorageManager:
impl StorageManager {
    /// Returns the path where samod stores automerge documents.
    /// Renamed from documents_dir() for clarity.
    pub fn automerge_dir(&self) -> PathBuf {
        self.hub_dir.join("automerge")
    }

    /// Update and persist the index document ID.
    /// Called after creating the index document for the first time.
    pub fn set_index_document_id(&mut self, doc_id: &str) -> Result<()> {
        self.config.index_document_id = Some(doc_id.to_string());
        self.config.save(&self.hub_dir)
    }
}
```

**Note on mutability**: The current implementation wraps `HubContext` in `Arc<HubContext>` (as `SharedContext`). To call `set_index_document_id(&mut self)`, we have two options:

1. **Option A (simpler)**: Call `set_index_document_id` during `HubContext::new()` before wrapping in Arc, since that's when we create/load the index document anyway.

2. **Option B (more flexible)**: Change `config` to `RwLock<HubStorageConfig>` if we need runtime mutation after server starts.

**Recommendation**: Option A - do all index document setup in `HubContext::new()` before the server starts accepting requests.

---

## Crate Structure (Implemented)

```
crates/quarto-hub/
├── Cargo.toml
├── resources/
│   └── qmd-project-example/   # Example Quarto project for testing
│       ├── _quarto.yml
│       ├── index.qmd
│       ├── about.qmd
│       └── styles.css
├── src/
│   ├── lib.rs            # Library exports
│   ├── main.rs           # Binary entry point (CLI)
│   ├── error.rs          # Error types
│   ├── context.rs        # HubContext (shared state, repo + storage + index)
│   ├── storage.rs        # StorageManager, lockfile, hub.json
│   ├── discovery.rs      # Project file discovery
│   ├── index.rs          # IndexDocument (path → doc-id mapping) [NEW in Phase 2]
│   └── server.rs         # axum server setup, REST endpoints
└── (tests inline with #[cfg(test)])
```

---

## Implementation Phases

### Phase 1: Foundation (COMPLETE)
- [x] Create crate with binary + library setup
- [x] Implement StorageManager with lockfile
- [x] Basic HubContext (samod Repo integration deferred to Phase 2)
- [x] Skeleton axum server with health endpoint (`/health`, `/api/files`)
- [x] Project directory walking to find `.qmd` files
- [x] Hub storage config (`hub.json`) with version number for migrations

### Phase 2: Document Management (COMPLETE)

**Implementation order** (each step builds on previous):

1. [x] **Update HubStorageConfig** to include `index_document_id: Option<String>`
   - Add field to struct and hub.json serialization
   - Add method to update and persist the config

2. [x] **Initialize samod Repo** with TokioFilesystemStorage in HubContext
   - Storage path: `.quarto/hub/automerge/`
   - Rename existing `documents_dir()` → `automerge_dir()` for clarity
   - See `external-sources/samod/samod/tests/smoke.rs` for initialization patterns

3. [x] **Index document lifecycle** (new `src/index.rs` module):
   - On startup, check `hub.json` for `index_document_id`
   - If present: `repo.find(doc_id)` to load existing index
   - If absent: `repo.create(Automerge::new())` to create new index, save ID to config
   - Use automerge Map at ROOT with relative paths as keys, document ID strings as values

4. [x] **Reconcile discovered `.qmd` files with index** on startup:
   - New files (in filesystem, not in index): create automerge doc, add to index
   - Missing files (in index, not in filesystem): deferred - see k-yvfo

5. [x] **REST endpoints**:
   - `GET /api/documents` - list all documents from index
   - `GET /api/documents/:id` - get document by ID (with reverse path lookup)
   - `PUT /api/documents/:id` - update document (simple key/value for testing)

### Phase 3: Real-time Collaboration (COMPLETE)

**Implementation order**:

1. [x] **Add peer configuration to CLI and storage**:
   - Add `--peer <URL>` flag to CLI (can be specified multiple times)
   - Add `peers: Vec<String>` to `HubStorageConfig` in `hub.json`
   - On first run with `--peer` flags, persist to `hub.json`
   - On subsequent runs: CLI peers override stored peers

2. [x] **Add peer configuration to HubConfig**:
   - Add `peers: Vec<String>` to `HubConfig`
   - Pass from CLI args through to context

3. [x] **WebSocket server endpoint** (`/ws`):
   - Add WebSocket upgrade endpoint using axum's `WebSocketUpgrade`
   - Use `repo.accept_axum(socket)` for incoming connections
   - See `external-sources/samod/samod/src/websocket.rs` for API

4. [x] **Outgoing peer connections** (new `src/peer.rs` module):
   - Add `tokio-tungstenite` v0.27 dependency for WebSocket client
   - Enable samod's `tungstenite` feature
   - On startup, spawn background tasks to connect to each configured peer
   - Use `repo.connect_tungstenite(socket, ConnDirection::Outgoing)`
   - Reconnection with exponential backoff (1s to 60s)

5. [x] **AnnouncePolicy** (generous for MVP):
   - Uses `AlwaysAnnounce` (default) for MVP
   - All documents announced to all peers
   - Future: add per-peer policy configuration

6. [ ] **Multi-client sync testing** (deferred - requires test setup):
   - Manual testing with multiple hub instances
   - Test with sync.automerge.org (if accessible)

### Phase 4: Polish
- [ ] Basic SIGTERM handling (call `repo.stop().await`)
- [ ] Error handling improvements
- [ ] Logging with `tracing`

### Future (separate design sessions)
- [ ] Automerge schema design
- [ ] Filesystem serialization (automerge → .qmd files)
- [ ] Conflict resolution strategy

---

## Selective Sync: Peering with External Servers

**Research completed**: 2025-12-09

### The Problem

quarto-hub needs to peer with external sync servers (like `sync.automerge.org` for prototyping) but we only want to sync documents related to our specific quarto project, not receive/process every document on the public server.

### Key Insight: Pull-Based Protocol

The automerge sync protocol is **pull-based**, not broadcast-based:

1. **No "here's all my documents" messages**: Peers don't automatically announce every document they have
2. **Request-specific syncing**: A peer sends a `Request` message for a specific document ID, and the other peer responds with `Sync` messages (or `DocUnavailable`)
3. **Selective announcement**: Each peer controls which documents it **announces** (proactively starts syncing) to other peers

### How sync.automerge.org Works

The public sync server runs with `sharePolicy: async () => false`, meaning:
- It **does NOT** announce any documents to connected peers
- It **DOES** respond to requests for specific document IDs
- Comment from their code: "Since this is a server, we don't share generously — meaning we only sync documents they already know about and can ask for by ID."

### How Changes Propagate Between Peers (The `their_heads` Mechanism)

**Clarification**: "doesn't announce" only affects the *initiation* of sync. Once a peer requests a document, ongoing changes ARE relayed. Here's the detailed mechanism:

1. **When a peer sends a Request or Sync message** to sync.automerge.org:
   - The server processes the message via `receive_sync_message`
   - This sets `their_heads` to `Some(...)` for that peer-document connection
   - From `ready.rs:47-51`: sync messages are generated if `their_heads().is_some() || announce_policy == Announce`

2. **When user 1 makes a change**:
   - The change syncs to sync.automerge.org
   - sync.automerge.org's `generate_sync_messages` is called
   - For each connected peer where `their_heads` is Some, a Sync message is generated and sent
   - User 2 receives the change (because they previously sent a Request, so their `their_heads` is Some)

**Key insight**: The sync server's `sharePolicy: false` means it won't *proactively* tell you about documents you don't know about. But once you request a document, you're "subscribed" and will receive all future changes.

From TypeScript `CollectionSynchronizer.ts:271-278`:
```typescript
async #shouldShare(peerId: PeerId, documentId: DocumentId): Promise<boolean> {
  const [announce, access] = await Promise.all([...])
  const hasRequested = this.#hasRequested.get(documentId)?.has(peerId) ?? false
  return announce || (access && hasRequested)  // <-- key logic!
}
```

**Conclusion for the two-user collaboration scenario**: YES, sync.automerge.org will relay changes between users who have both requested the same document. The server remembers who has requested what.

### Mechanisms for Selective Sync

#### 1. AnnouncePolicy (samod)

samod provides `AnnouncePolicy` trait (equivalent to automerge-repo's `sharePolicy`):

```rust
// From external-sources/samod/samod/src/announce_policy.rs
pub trait AnnouncePolicy: Clone + Send + 'static {
    /// Whether we should announce the given document to the given peer ID
    fn should_announce(
        &self,
        doc_id: DocumentId,
        peer_id: PeerId,
    ) -> impl Future<Output = bool> + Send + 'static;
}
```

Called in three scenarios:
1. When connecting to a new peer (checks for each existing DocHandle)
2. When creating a new DocHandle (checks for each connected peer)
3. When requesting a document via `Repo::find` (prevents leaking document IDs)

**Usage**:
```rust
let repo = samod::Repo::build_tokio()
    .with_storage(storage)
    .with_announce_policy(|doc_id, peer_id| {
        // Return true to announce this document to this peer
        // Return false to keep it private from this peer
        true
    })
    .load()
    .await;
```

#### 2. Default Behavior: AlwaysAnnounce

By default, samod uses `AlwaysAnnounce` which announces every document to every peer. This is appropriate for local/trusted networks.

### Recommended quarto-hub Strategy

For connecting to `sync.automerge.org`:

1. **quarto-hub announces project documents**: Set `AnnouncePolicy` to return `true` for:
   - Our project's index document
   - All document IDs stored in our project index

2. **sync.automerge.org doesn't announce to us**: Since it runs with `sharePolicy: false`, it won't push any documents to quarto-hub

3. **Only known documents sync**: quarto-hub will only receive sync messages for documents it has actively requested (i.e., documents in its project index)

### Implementation Considerations

**Option A: Simple (MVP)**
- Use `AlwaysAnnounce` for initial development
- Only peer with local instances or trusted sync servers
- Add selective announcement later when needed

**Option B: Project-Aware Policy**
```rust
struct ProjectAnnouncePolicy {
    project_docs: Arc<RwLock<HashSet<DocumentId>>>,
}

impl AnnouncePolicy for ProjectAnnouncePolicy {
    fn should_announce(&self, doc_id: DocumentId, _peer_id: PeerId) -> impl Future<Output = bool> {
        let docs = self.project_docs.clone();
        async move {
            let docs = docs.read().await;
            docs.contains(&doc_id)
        }
    }
}
```

**Note on Access Control**: The TypeScript automerge-repo has an experimental `shareConfig.access` field that controls whether to respond to incoming requests. samod currently only has `AnnouncePolicy` (outbound). If we need to refuse incoming requests for certain documents, that would require either:
- Extending samod
- Not storing documents we don't want to share
- Network-level isolation (different repos for different trust levels)

### Dynamic Document Subscription (New Files Created During Collaboration)

**Scenario**: User 1 creates a new `.qmd` file. User 2 needs to automatically start syncing it.

**The Flow**:
1. User 1 creates `new.qmd`, which creates a new automerge document (new document ID)
2. User 1's hub updates the root index document with the new document ID
3. Root index change syncs: User 1 → sync.automerge.org → User 2
4. User 2's hub detects the change to root index (via `DocHandle.on("change", ...)`)
5. User 2's hub sees an unknown document ID in the root index
6. User 2's hub calls `repo.find(new_doc_id)` to start syncing it
7. **Critical**: The announce policy is checked before sending a Request
8. If announce returns `true`, Request is sent to sync.automerge.org
9. sync.automerge.org has the document (from user 1), sends it back

**The Announce Policy Challenge**:

When `repo.find(new_doc_id)` is called in step 6, samod checks the announce policy (from `request.rs:63`):
```rust
state: PeerState::Requesting(c.announce_policy().into())
```

If the policy returns `false` for the new document ID, the Request won't be sent and the document won't sync!

**Solutions**:

**Solution A: Update policy before find (explicit)**
```rust
// When root index changes, extract new document IDs
let new_doc_ids = extract_doc_ids_from_index(&index_doc);
// Update the policy's known set
policy.add_known_docs(new_doc_ids);
// Now find will work
for doc_id in new_doc_ids {
    repo.find(doc_id).await;
}
```

**Solution B: Query-based policy (implicit)**
```rust
struct ProjectAnnouncePolicy {
    index_handle: DocHandle,  // Handle to root index
}

impl AnnouncePolicy for ProjectAnnouncePolicy {
    fn should_announce(&self, doc_id: DocumentId, _peer_id: PeerId) -> impl Future<Output = bool> {
        let index = self.index_handle.clone();
        async move {
            // Always announce the index itself
            if doc_id == index.document_id() {
                return true;
            }
            // Check if doc_id is in the current index
            index.with_doc(|doc| {
                doc.get("files")
                   .and_then(|files| files.get(&doc_id.to_string()))
                   .is_some()
            })
        }
    }
}
```

**Solution C: Generous policy for sync servers (simplest)**
```rust
impl AnnouncePolicy for ProjectAnnouncePolicy {
    fn should_announce(&self, _doc_id: DocumentId, peer_id: PeerId) -> impl Future<Output = bool> {
        async move {
            // Always announce to our sync server (we trust it)
            peer_id.as_str().contains("sync.automerge.org")
        }
    }
}
```

**Recommendation**: For MVP, use Solution C (generous to sync servers). Since we only store project documents locally anyway, there's no privacy leak. For production with untrusted peers, use Solution A or B.

**Dynamic Policy Updates**:

samod supports updating the announce policy dynamically via `set_announce_policy` (from `doc_state.rs:382-396`). When the policy changes from `DontAnnounce` to `Announce` for a peer, requests that were blocked will be sent:

```rust
// From request.rs:306-308
Requesting::NotSentDueToAnnouncePolicy => {
    if policy == AnnouncePolicy::Announce {
        peer.state = PeerState::Requesting(Requesting::AwaitingSend);
    }
}
```

This means quarto-hub can:
1. Create a DocHandle with `repo.find(new_doc_id)` (blocked by policy)
2. Update the policy to include the new document ID
3. The pending request automatically gets sent

### Source Code References

- **samod AnnouncePolicy**: `external-sources/samod/samod/src/announce_policy.rs`
- **samod RepoBuilder**: `external-sources/samod/samod/src/builder.rs`
- **samod Ready state (sync generation logic)**: `external-sources/samod/samod-core/src/actors/document/ready.rs:47-51`
- **samod Request state (announce policy handling)**: `external-sources/samod/samod-core/src/actors/document/request.rs`
- **samod DocState (phase transitions)**: `external-sources/samod/samod-core/src/actors/document/doc_state.rs`
- **automerge-repo SharePolicy**: `external-sources/automerge-repo/packages/automerge-repo/src/Repo.ts:1050-1074`
- **automerge-repo CollectionSynchronizer**: `external-sources/automerge-repo/packages/automerge-repo/src/synchronizer/CollectionSynchronizer.ts`
- **Sync server example**: `external-sources/automerge-repo/examples/sync-server/index.js:69-71`
- **Wire protocol**: `external-sources/samod/samod-core/src/network/wire_protocol.rs`

---

## Resolved Questions

1. **Graceful shutdown**: Err on the side of ease of prototyping. Keep it simple, don't paint into architectural corners. Basic SIGTERM handling is fine for MVP.

2. **Document discovery**:
   - Walk directory to find all `.qmd` files (and other editable files)
   - Create an **index automerge document** with key-value mapping:
     - Keys: paths to `.qmd` files (relative to project root)
     - Values: automerge document IDs for each file
   - **Deferred decision**: What happens when existing project's filesystem doesn't match automerge state (requires dedicated design session)

3. **Crash recovery**: samod's `TokioFilesystemStorage` handles persistence; recovery is automatic on restart.

---

## Deferred Design Sessions

These topics require dedicated sessions (tracked as separate issues):

1. **k-libc**: Automerge schema design - Full schema for documents and the project index document
2. **k-r2t1**: Filesystem serialization - How/when to serialize automerge state back to `.qmd` files on disk
3. **k-yvfo**: Conflict resolution - What happens when filesystem and automerge state diverge

---

## References

- [samod crate](https://crates.io/crates/samod)
- [samod docs](https://docs.rs/samod/latest/samod/)
- [samod GitHub](https://github.com/alexjg/samod)
- [Automerge Rust](https://github.com/automerge/automerge)
- [automerge-repo TypeScript](https://github.com/automerge/automerge-repo)
- [Axum](https://github.com/tokio-rs/axum)
- [fs2 crate](https://docs.rs/fs2/latest/fs2/)

### Local Source Code References

For detailed implementation research, these external sources are available locally:
- **samod source**: `external-sources/samod/` - Rust automerge repo implementation
- **automerge-repo source**: `external-sources/automerge-repo/` - TypeScript monorepo (reference implementation)

### Automerge LLM Documentation

For Claude/LLM-friendly automerge documentation:
- **Local copy**: `external-sources/automerge-llmstxt` (preferred - no network needed)
- **Summary**: https://automerge.org/llms.txt
- **Full docs**: https://automerge.org/llms-full.txt

Use the local copy when available. Fall back to WebFetch URLs if the local copy is outdated or missing.
