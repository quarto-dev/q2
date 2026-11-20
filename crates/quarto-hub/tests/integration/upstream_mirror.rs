//! A standalone hub mirroring a project from an upstream hub
//! (bd-yq3awhmw, `q2 preview --hub-url`).
//!
//! The upstream is a project-mode hub on a real TCP port; the mirror is a
//! standalone hub with `HubConfig::upstream` set. These pin the three
//! behaviours the share-policy change exists for: the mirror can *fetch*
//! documents from the upstream (announce-off would block that), it
//! *pushes* documents created locally (the upstream never asks), and
//! edits flow both ways.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use automerge::{Automerge, ObjType, ROOT, ReadDoc, transaction::Transactable};
use quarto_hub::context::{HubConfig, HubContext, SharedContext};
use quarto_hub::server::build_router_with_state;
use quarto_hub::storage::StorageManager;
use quarto_hub::sync::DiskWritePolicy;
use quarto_hub::upstream::UpstreamConfig;
use samod::{DocHandle, DocumentId, Repo};
use tempfile::TempDir;
use tokio::net::TcpListener;

const TIMEOUT: Duration = Duration::from_secs(15);

/// Start a project-mode hub over `files` and serve it on a free port.
/// Returns the context and the `ws://…/ws` URL.
async fn start_upstream(files: &[(&str, &str)]) -> (SharedContext, String, TempDir, TempDir) {
    let project = TempDir::new().unwrap();
    for (name, text) in files {
        std::fs::write(project.path().join(name), text).unwrap();
    }
    let data = TempDir::new().unwrap();
    let storage = StorageManager::new_with_data_dir(project.path(), data.path()).unwrap();
    let config = HubConfig {
        port: 0,
        sync_interval_secs: None,
        watch_enabled: false,
        register_root_ws: false,
        ..HubConfig::default()
    };
    let ctx: SharedContext = Arc::new(HubContext::new(storage, config).await.unwrap());
    let url = serve(ctx.clone()).await;
    (ctx, url, project, data)
}

/// Serve `ctx` on a free port; returns its `ws://…/ws` URL.
async fn serve(ctx: SharedContext) -> String {
    let router = build_router_with_state(ctx.clone()).await.unwrap();
    let router = router.with_state(ctx);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("ws://{addr}/ws")
}

async fn start_mirror(
    url: String,
    index_doc_id: String,
) -> (quarto_hub::error::Result<HubContext>, TempDir) {
    start_mirror_with(url, index_doc_id, DiskWritePolicy::default()).await
}

async fn start_mirror_with(
    url: String,
    index_doc_id: String,
    disk_write_policy: DiskWritePolicy,
) -> (quarto_hub::error::Result<HubContext>, TempDir) {
    let data = TempDir::new().unwrap();
    let storage = StorageManager::new_standalone_ephemeral(data.path()).unwrap();
    let config = HubConfig {
        port: 0,
        sync_interval_secs: None,
        watch_enabled: false,
        register_root_ws: false,
        upstream: Some(UpstreamConfig { url, index_doc_id }),
        disk_write_policy,
        ..HubConfig::default()
    };
    (HubContext::new(storage, config).await, data)
}

fn text_of(handle: &DocHandle) -> Option<String> {
    handle.with_document(|doc| {
        let (_, obj) = doc.get(ROOT, "text").ok().flatten()?;
        doc.text(&obj).ok()
    })
}

async fn find(repo: &Repo, doc_id: &str) -> Option<DocHandle> {
    let bare = doc_id.strip_prefix("automerge:").unwrap_or(doc_id);
    repo.find(DocumentId::from_str(bare).unwrap())
        .await
        .unwrap()
}

/// Poll `check` until it returns `Some`, or panic after [`TIMEOUT`].
async fn eventually<T, F, Fut>(what: &str, mut check: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        if let Some(v) = check().await {
            return v;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn mirror_fetches_upstream_index_and_files() {
    let (upstream, url, _p, _d) = start_upstream(&[("a.qmd", "# Hello from upstream\n")]).await;
    let (mirror, _m) = start_mirror(url, upstream.index().document_id()).await;
    let mirror = mirror.expect("mirror should load the upstream index");

    assert_eq!(mirror.index().document_id(), upstream.index().document_id());
    let doc_id = mirror.index().get_file("a.qmd").expect("index lists a.qmd");
    let repo = mirror.repo().clone();
    let text = eventually("a.qmd text on the mirror", || {
        let repo = repo.clone();
        let doc_id = doc_id.clone();
        async move { find(&repo, &doc_id).await.and_then(|h| text_of(&h)) }
    })
    .await;
    assert_eq!(text, "# Hello from upstream\n");
}

#[tokio::test]
async fn mirror_refuses_an_unknown_project_instead_of_creating_one() {
    let (_upstream, url, _p, _d) = start_upstream(&[("a.qmd", "x\n")]).await;
    // A real document id that the upstream has never seen.
    let unknown = Repo::build_tokio()
        .load()
        .await
        .create(Automerge::new())
        .await
        .unwrap()
        .document_id()
        .to_string();
    let (mirror, _m) = start_mirror(url, unknown).await;
    let err = mirror
        .err()
        .expect("an unknown project index must be an error");
    assert!(err.to_string().contains("does not have"), "{err}");
}

#[tokio::test]
async fn edits_and_new_files_flow_from_the_mirror_to_the_upstream() {
    let (upstream, url, _p, _d) = start_upstream(&[("a.qmd", "before\n")]).await;
    let (mirror, _m) = start_mirror(url, upstream.index().document_id()).await;
    let mirror = mirror.unwrap();

    // Edit an existing file on the mirror.
    let a_id = mirror.index().get_file("a.qmd").unwrap();
    let mirror_repo = mirror.repo().clone();
    let a = eventually("a.qmd on the mirror", || {
        let repo = mirror_repo.clone();
        let a_id = a_id.clone();
        async move { find(&repo, &a_id).await.filter(|h| text_of(h).is_some()) }
    })
    .await;
    a.with_document(|doc| {
        doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            let (_, obj) = tx.get(ROOT, "text")?.unwrap();
            tx.splice_text(&obj, 0, 6, "after")?;
            Ok(())
        })
        .unwrap();
    });

    // Create a new file on the mirror. The upstream never requests it
    // (its policy has announce off), so this only arrives if the mirror
    // pushes it.
    let mut doc = Automerge::new();
    doc.transact::<_, _, automerge::AutomergeError>(|tx| {
        let obj = tx.put_object(ROOT, "text", ObjType::Text)?;
        tx.splice_text(&obj, 0, 0, "made on the mirror\n")?;
        Ok(())
    })
    .unwrap();
    let new_handle = mirror.repo().create(doc).await.unwrap();
    let new_id = new_handle.document_id().to_string();
    mirror.index().add_file("new.qmd", &new_id).unwrap();

    let upstream_repo = upstream.repo().clone();
    let a_text = eventually("the edit to a.qmd on the upstream", || {
        let repo = upstream_repo.clone();
        let a_id = a_id.clone();
        async move {
            find(&repo, &a_id)
                .await
                .and_then(|h| text_of(&h))
                .filter(|t| t.starts_with("after"))
        }
    })
    .await;
    assert_eq!(a_text, "after\n");

    let listed = eventually("new.qmd in the upstream index", || {
        let up = upstream.clone();
        async move { up.index().get_file("new.qmd") }
    })
    .await;
    assert_eq!(listed, new_id);
    let new_text = eventually("new.qmd content on the upstream", || {
        let repo = upstream_repo.clone();
        let new_id = new_id.clone();
        async move { find(&repo, &new_id).await.and_then(|h| text_of(&h)) }
    })
    .await;
    assert_eq!(new_text, "made on the mirror\n");
}

/// Connect a client repo (a browser stand-in) to the mirror, append
/// `suffix` to `a.qmd` through it, and report whether the upstream saw it.
async fn client_edit_reaches_upstream(disk_write_policy: DiskWritePolicy) -> bool {
    let (upstream, url, _p, _d) = start_upstream(&[("a.qmd", "base\n")]).await;
    let (mirror, _m) =
        start_mirror_with(url, upstream.index().document_id(), disk_write_policy).await;
    let mirror: SharedContext = Arc::new(mirror.unwrap());
    let mirror_url = serve(mirror.clone()).await;
    let a_id = mirror.index().get_file("a.qmd").unwrap();

    let client = Repo::build_tokio().load().await;
    client
        .dial_websocket(mirror_url.parse().unwrap(), samod::BackoffConfig::default())
        .unwrap()
        .established()
        .await
        .unwrap();
    let a = eventually("a.qmd on the client", || {
        let client = client.clone();
        let a_id = a_id.clone();
        async move { find(&client, &a_id).await.filter(|h| text_of(h).is_some()) }
    })
    .await;
    a.with_document(|doc| {
        doc.transact::<_, _, automerge::AutomergeError>(|tx| {
            let (_, obj) = tx.get(ROOT, "text")?.unwrap();
            tx.splice_text(&obj, 4, 0, " from the client")?;
            Ok(())
        })
        .unwrap();
    });

    // Give the edit ample time to cross both hops; the positive case
    // usually lands in well under 100 ms.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        let text = find(upstream.repo(), &a_id).await.and_then(|h| text_of(&h));
        if text.as_deref() == Some("base from the client\n") {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

/// `--hub-url --allow-edit`: a client's edit reaches the upstream.
#[tokio::test]
async fn writable_mirror_forwards_client_edits() {
    assert!(client_edit_reaches_upstream(DiskWritePolicy::WriteBack).await);
}

/// `--hub-url` without `--allow-edit`: the mirror gives clients read-only
/// access, so their edits never reach the upstream project.
#[tokio::test]
async fn read_only_mirror_drops_client_edits() {
    assert!(!client_edit_reaches_upstream(DiskWritePolicy::ReadOnly).await);
}
