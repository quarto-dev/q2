//! Hub integration of git epochs (GE-3): the gate, the registry, arrival, flips, unlist and relist.
//!
//! Every test builds a real temporary git repository, a real `HubContext`, and real Automerge docs. The
//! controller's clock is explicit (`git_tick_at`), so no test sleeps for the quiet window.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use automerge::{Automerge, ROOT, ReadDoc, transaction::Transactable};
use quarto_git_epochs::registry::doc as regdoc;
use quarto_git_epochs::registry::{self, Entry, RegistryOp};
use quarto_hub::context::{HubConfig, HubContext};
use quarto_hub::git_epochs::{GitTracking, blob_id, now_ms};
use quarto_hub::index::{CaptureRef, CaptureState};
use quarto_hub::storage::StorageManager;
use quarto_hub::sync::{DiskWritePolicy, SyncResult};
use samod::{DocHandle, DocumentId, Repo};
use tempfile::TempDir;

// ---------------------------------------------------------------- harness

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

struct Rig {
    ctx: Arc<HubContext>,
    dir: PathBuf,
    data: PathBuf,
    policy: DiskWritePolicy,
    _project: TempDir,
    _data: TempDir,
}

fn config(policy: DiskWritePolicy, tracking: GitTracking) -> HubConfig {
    HubConfig {
        port: 0,
        sync_interval_secs: None,
        watch_enabled: false,
        register_root_ws: false,
        disk_write_policy: policy,
        git_tracking: tracking,
        ..HubConfig::default()
    }
}

impl Rig {
    async fn new(policy: DiskWritePolicy, files: &[(&str, &[u8])]) -> Rig {
        Self::build(policy, files, GitTracking::Track, true).await
    }

    async fn build(
        policy: DiskWritePolicy,
        files: &[(&str, &[u8])],
        tracking: GitTracking,
        commit: bool,
    ) -> Rig {
        let project = TempDir::new().unwrap();
        let dir = project.path().canonicalize().unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.email", "t@example.com"]);
        git(&dir, &["config", "user.name", "T"]);
        git(&dir, &["config", "core.autocrlf", "false"]);
        for (name, bytes) in files {
            let p = dir.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        }
        if commit {
            git(&dir, &["add", "-A"]);
            git(&dir, &["commit", "-q", "-m", "init"]);
        }
        let data = TempDir::new().unwrap();
        let data_path = data.path().canonicalize().unwrap();
        let rig = Rig {
            ctx: Self::start(&dir, &data_path, policy, tracking).await,
            dir,
            data: data_path,
            policy,
            _project: project,
            _data: data,
        };
        rig.ctx.sync_all().await;
        rig
    }

    async fn start(
        dir: &Path,
        data: &Path,
        policy: DiskWritePolicy,
        tracking: GitTracking,
    ) -> Arc<HubContext> {
        let storage = StorageManager::new_with_data_dir(dir, data).unwrap();
        Arc::new(
            HubContext::new(storage, config(policy, tracking))
                .await
                .unwrap(),
        )
    }

    /// Stops the hub and starts a new one over the same project and data directories.
    async fn restart(&mut self) {
        self.restart_with(GitTracking::Track).await;
    }

    async fn restart_with(&mut self, tracking: GitTracking) {
        self.ctx.shutdown().await;
        // The data directory is locked until the old context is dropped, so park a throwaway context in its
        // place first.
        let scratch = TempDir::new().unwrap();
        let scratch_data = TempDir::new().unwrap();
        let parked = Self::start(
            scratch.path(),
            scratch_data.path(),
            self.policy,
            GitTracking::Ignore,
        )
        .await;
        drop(std::mem::replace(&mut self.ctx, parked));
        self.ctx = Self::start(&self.dir, &self.data, self.policy, tracking).await;
    }

    fn git(&self, args: &[&str]) -> String {
        git(&self.dir, args)
    }

    fn write(&self, rel: &str, content: &str) {
        let p = self.dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.dir.join(rel)).unwrap()
    }

    /// Two ticks a quiet window apart: the first sees the change, the second decides.
    async fn settle(&self) {
        let t = now_ms();
        self.ctx.git_tick_at(t).await;
        self.ctx.git_tick_at(t + 1_100).await;
    }

    fn doc_id(&self, rel: &str) -> String {
        self.ctx
            .index()
            .get_file(rel)
            .unwrap_or_else(|| panic!("{rel} not in index"))
    }

    async fn handle(&self, doc_id: &str) -> DocHandle {
        find(self.ctx.repo(), doc_id).await
    }

    async fn text(&self, rel: &str) -> String {
        text_of(&self.handle(&self.doc_id(rel)).await)
    }

    /// A browser-style edit: replaces the doc's text.
    async fn edit_doc(&self, doc_id: &str, new_text: &str) {
        let h = self.handle(doc_id).await;
        h.with_document(|d| {
            d.transact::<_, _, automerge::AutomergeError>(|tx| {
                let (_, obj) = tx.get(ROOT, "text")?.unwrap();
                tx.update_text(&obj, new_text)?;
                Ok(())
            })
            .unwrap();
        });
    }

    fn sidecar(&self) -> DocHandle {
        self.ctx
            .git_epochs()
            .expect("tracking")
            .sidecar_doc()
            .expect("sidecar exists")
    }

    fn view(&self) -> registry::RegistryView {
        self.sidecar().with_document(|d| regdoc::view_of(&*d))
    }

    fn entry_for(&self, rel: &str, blob: &str) -> Vec<Entry> {
        self.view()
            .entries
            .into_iter()
            .filter(|e| e.path == rel && e.blob == blob)
            .collect()
    }

    fn index_changes_since(&self, heads: &[automerge::ChangeHash]) -> usize {
        self.ctx
            .index()
            .handle()
            .with_document(|d| d.get_changes(heads).len())
    }

    fn index_heads(&self) -> Vec<automerge::ChangeHash> {
        self.ctx.index().handle().with_document(|d| d.get_heads())
    }

    fn blob_of(&self, rev: &str, rel: &str) -> String {
        self.git(&["rev-parse", &format!("{rev}:{rel}")])
    }
}

fn text_of(handle: &DocHandle) -> String {
    handle.with_document(|doc| {
        let (_, obj) = doc.get(ROOT, "text").unwrap().unwrap();
        doc.text(&obj).unwrap()
    })
}

async fn find(repo: &Repo, doc_id: &str) -> DocHandle {
    let bare = doc_id.strip_prefix("automerge:").unwrap_or(doc_id);
    repo.find(DocumentId::from_str(bare).unwrap())
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("doc {doc_id} not found"))
}

/// A doc id this hub can never fetch: the doc lives in another repo.
async fn unreachable_doc_id(text: &str) -> String {
    let other = Repo::build_tokio()
        .with_storage(samod::storage::InMemoryStorage::new())
        .load()
        .await;
    let mut doc = Automerge::new();
    doc.transact::<_, _, automerge::AutomergeError>(|tx| {
        let obj = tx.put_object(ROOT, "text", automerge::ObjType::Text)?;
        tx.update_text(&obj, text)?;
        Ok(())
    })
    .unwrap();
    let h = other.create(doc).await.unwrap();
    h.document_id().to_string()
}

const A: &str = "# A\n\nmain text\n";
const B: &str = "# B\n\nmain b\n";

async fn two_files(policy: DiskWritePolicy) -> Rig {
    Rig::new(policy, &[("a.qmd", A.as_bytes()), ("b.qmd", B.as_bytes())]).await
}

/// `a.qmd` differs on branch `other`.
fn make_other_branch(rig: &Rig, a: &str) {
    rig.git(&["switch", "-q", "-c", "other"]);
    rig.write("a.qmd", a);
    rig.git(&["commit", "-qam", "other a"]);
    rig.git(&["switch", "-q", "main"]);
}

const BOTH: [DiskWritePolicy; 2] = [DiskWritePolicy::WriteBack, DiskWritePolicy::ReadOnly];

// ---------------------------------------------------------------- gate

#[tokio::test]
async fn a_marker_stops_every_entry_point_and_removal_resumes_them() {
    for policy in BOTH {
        let rig = two_files(policy).await;
        let id = rig.doc_id("a.qmd");
        let lock = rig.dir.join(".git/index.lock");
        std::fs::write(&lock, "").unwrap();

        // A guest edit reaches the doc; the periodic sync must not write it.
        rig.edit_doc(&id, "# A\n\nguest edit\n").await;
        let all = rig.ctx.sync_all().await;
        assert_eq!(all.skipped, 2, "{policy:?}: the full sync skips everything");
        assert_eq!(rig.read("a.qmd"), A);

        // The watcher path syncs nothing either.
        let one = rig.ctx.sync_file(&rig.dir.join("a.qmd")).await.unwrap();
        assert_eq!(one, Some(SyncResult::Skipped));
        assert_eq!(rig.read("a.qmd"), A);

        // A disk edit does not merge into the doc.
        rig.write("b.qmd", "# B\n\nedited on disk\n");
        rig.ctx.sync_all().await;
        assert_eq!(rig.text("b.qmd").await, B);

        std::fs::remove_file(&lock).unwrap();
        rig.settle().await;
        // The periodic pass leaves a file written in the last 500 ms for the next tick (G1).
        tokio::time::sleep(Duration::from_millis(700)).await;
        rig.ctx.sync_all().await;
        assert_eq!(rig.text("b.qmd").await, "# B\n\nedited on disk\n");
        if policy == DiskWritePolicy::WriteBack {
            assert_eq!(rig.read("a.qmd"), "# A\n\nguest edit\n");
        }
    }
}

#[tokio::test]
async fn the_initial_sync_and_reconcile_wait_for_an_open_gate() {
    let project = TempDir::new().unwrap();
    let dir = project.path().canonicalize().unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "t@example.com"]);
    git(&dir, &["config", "user.name", "T"]);
    std::fs::write(dir.join("a.qmd"), A).unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "init"]);
    std::fs::write(dir.join(".git/index.lock"), "").unwrap();

    let data = TempDir::new().unwrap();
    let ctx = Rig::start(
        &dir,
        data.path(),
        DiskWritePolicy::WriteBack,
        GitTracking::Track,
    )
    .await;
    assert!(
        ctx.index().get_all_files().is_empty(),
        "reconcile is skipped while a marker is set"
    );

    std::fs::remove_file(dir.join(".git/index.lock")).unwrap();
    let t = now_ms();
    ctx.git_tick_at(t).await;
    ctx.git_tick_at(t + 1_100).await;
    assert!(
        ctx.index().has_file("a.qmd"),
        "startup ran on the first open tick"
    );
    assert!(ctx.git_epochs().unwrap().sidecar_doc().is_some());
}

#[tokio::test]
async fn a_marker_that_appears_after_the_read_wins() {
    for policy in BOTH {
        let rig = two_files(policy).await;
        let lock = rig.dir.join(".git/index.lock");
        let l = lock.clone();
        *rig.ctx
            .git_epochs()
            .unwrap()
            .hooks
            .after_read
            .lock()
            .unwrap() = Some(Box::new(move |_| {
            std::fs::write(&l, "").unwrap();
        }));
        rig.write("a.qmd", "# A\n\ndisk edit\n");
        let r = rig.ctx.sync_file(&rig.dir.join("a.qmd")).await.unwrap();
        assert_eq!(r, Some(SyncResult::Skipped), "{policy:?}");
        assert_eq!(rig.text("a.qmd").await, A, "nothing merged");
        assert_eq!(rig.read("a.qmd"), "# A\n\ndisk edit\n", "nothing written");
    }
}

#[tokio::test]
async fn a_reflog_change_alone_makes_the_gate_pending_before_any_tick() {
    let rig = two_files(DiskWritePolicy::WriteBack).await;
    assert!(!rig.ctx.git_status().pending);
    let reflog = rig.dir.join(".git/logs/HEAD");
    let mut text = std::fs::read_to_string(&reflog).unwrap();
    text.push_str("0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 T <t@example.com> 1 +0000\treset: moving to HEAD\n");
    std::fs::write(&reflog, text).unwrap();
    let all = rig.ctx.sync_all().await;
    assert_eq!(all.skipped, 2, "the entry point holds its files first");
    assert!(rig.ctx.git_status().pending);
}

// ---------------------------------------------------------------- commit, checkout

#[tokio::test]
async fn a_commit_moves_the_registry_forward_and_changes_no_index_entry() {
    for policy in BOTH {
        let rig = two_files(policy).await;
        let id = rig.doc_id("a.qmd");
        let old_blob = rig.blob_of("HEAD", "a.qmd");
        assert_eq!(rig.entry_for("a.qmd", &old_blob).len(), 1);
        let before_heads = rig.index_heads();

        rig.write("a.qmd", "# A\n\nedited\n");
        rig.ctx.sync_all().await;
        rig.git(&["commit", "-qam", "edit a"]);
        rig.settle().await;

        let new_blob = rig.blob_of("HEAD", "a.qmd");
        let moved = rig.entry_for("a.qmd", &new_blob);
        assert_eq!(moved.len(), 1, "{policy:?}");
        assert_eq!(moved[0].doc, id);
        assert_eq!(moved[0].generation, 0);
        assert!(rig.entry_for("a.qmd", &old_blob).is_empty());
        assert_eq!(rig.doc_id("a.qmd"), id, "no flip");
        assert_eq!(rig.index_changes_since(&before_heads), 0, "no index write");
        let st = rig.ctx.git_status();
        assert!(
            st.decisions
                .iter()
                .any(|d| d.kind == "moved-forward" && d.files == ["a.qmd"])
        );
    }
}

#[tokio::test]
async fn a_checkout_with_a_new_blob_creates_a_doc_and_flips_after_the_checkpoint() {
    for policy in BOTH {
        let rig = two_files(policy).await;
        make_other_branch(&rig, "# A\n\nother text\n");
        let old = rig.doc_id("a.qmd");
        rig.ctx
            .index()
            .set_capture(
                "a.qmd",
                &CaptureRef {
                    capture_doc_id: "cap".into(),
                    staleness: Some(false),
                    state: Some(CaptureState::Idle),
                    last_error: None,
                },
            )
            .unwrap();
        let fired = Arc::new(AtomicUsize::new(0));
        let f = fired.clone();
        rig.ctx.set_git_callback(Arc::new(move |_ctx, path| {
            assert!(path.ends_with("a.qmd"));
            f.fetch_add(1, Ordering::SeqCst);
        }));
        rig.ctx
            .git_epochs()
            .unwrap()
            .hooks
            .events
            .lock()
            .unwrap()
            .clear();

        rig.git(&["switch", "-q", "other"]);
        rig.settle().await;

        let new = rig.doc_id("a.qmd");
        assert_ne!(new, old, "{policy:?}");
        assert_eq!(rig.text("a.qmd").await, "# A\n\nother text\n");
        assert_eq!(rig.read("a.qmd"), "# A\n\nother text\n");
        let events = rig
            .ctx
            .git_epochs()
            .unwrap()
            .hooks
            .events
            .lock()
            .unwrap()
            .clone();
        let cp = events
            .iter()
            .position(|e| e == "checkpoint:a.qmd")
            .expect("checkpoint");
        let ix = events.iter().position(|e| e == "index").expect("index");
        assert!(cp < ix, "checkpoint before the index change: {events:?}");
        assert_eq!(
            rig.ctx.index().get_capture("a.qmd").unwrap().staleness,
            Some(true)
        );
        assert_eq!(fired.load(Ordering::SeqCst), 1);
        let blob = rig.blob_of("other", "a.qmd");
        let reg = rig.entry_for("a.qmd", &blob);
        assert_eq!(reg.len(), 1);
        assert_eq!(reg[0].doc, new);
        assert_eq!(reg[0].generation, 1, "max over the path plus one");
    }
}

#[tokio::test]
async fn switching_back_attaches_the_existing_canonical_doc() {
    for policy in BOTH {
        let rig = two_files(policy).await;
        make_other_branch(&rig, "# A\n\nother text\n");
        let original = rig.doc_id("a.qmd");
        rig.git(&["switch", "-q", "other"]);
        rig.settle().await;
        let entries_before = rig.view().entries.len();
        rig.git(&["switch", "-q", "main"]);
        rig.settle().await;
        assert_eq!(rig.doc_id("a.qmd"), original, "{policy:?}: no fork");
        assert_eq!(rig.read("a.qmd"), A);
        assert_eq!(rig.text("a.qmd").await, A, "no stray text");
        assert_eq!(rig.view().entries.len(), entries_before);
    }
}

#[tokio::test]
async fn a_late_edit_in_the_old_doc_never_reaches_the_new_doc_or_disk() {
    let rig = two_files(DiskWritePolicy::WriteBack).await;
    make_other_branch(&rig, "# A\n\nother text\n");
    let old = rig.doc_id("a.qmd");
    rig.git(&["switch", "-q", "other"]);
    rig.settle().await;
    rig.edit_doc(&old, "# A\n\nstranded edit\n").await;
    rig.ctx.sync_all().await;
    assert_eq!(rig.text("a.qmd").await, "# A\n\nother text\n");
    assert_eq!(rig.read("a.qmd"), "# A\n\nother text\n");
}

#[tokio::test]
async fn files_git_removes_are_unlisted_and_relisted_in_one_index_change() {
    for policy in BOTH {
        let rig = two_files(policy).await;
        rig.git(&["switch", "-q", "-c", "other"]);
        rig.write("a.qmd", "# A\n\nother text\n");
        rig.git(&["rm", "-q", "b.qmd"]);
        rig.write("c.qmd", "# C\n");
        rig.git(&["add", "c.qmd"]);
        rig.git(&["commit", "-qam", "flip a, drop b, add c"]);
        rig.git(&["switch", "-q", "main"]);
        let b_doc = rig.doc_id("b.qmd");

        let before = rig.index_heads();
        rig.git(&["switch", "-q", "other"]);
        rig.settle().await;
        assert_eq!(
            rig.index_changes_since(&before),
            1,
            "{policy:?}: one change per switch"
        );
        assert!(!rig.ctx.index().has_file("b.qmd"), "unlisted");
        assert!(rig.ctx.index().has_file("c.qmd"), "new file listed");
        assert!(
            rig.view().entries.iter().any(|e| e.doc == b_doc),
            "the doc stays in the registry"
        );

        let before = rig.index_heads();
        rig.git(&["switch", "-q", "main"]);
        rig.settle().await;
        assert_eq!(rig.index_changes_since(&before), 1);
        assert_eq!(rig.doc_id("b.qmd"), b_doc, "relisted at the registry's doc");
        assert_eq!(rig.read("b.qmd"), B);
    }
}

#[tokio::test]
async fn binaries_compare_by_byte_hash_and_switch_back_to_the_old_doc() {
    let rig = Rig::new(
        DiskWritePolicy::WriteBack,
        &[("a.qmd", A.as_bytes()), ("logo.png", &[1, 2, 3, 4, 5])],
    )
    .await;
    rig.git(&["switch", "-q", "-c", "other"]);
    std::fs::write(rig.dir.join("logo.png"), [9, 8, 7]).unwrap();
    rig.git(&["commit", "-qam", "new logo"]);
    rig.git(&["switch", "-q", "main"]);
    let original = rig.doc_id("logo.png");
    rig.git(&["switch", "-q", "other"]);
    rig.settle().await;
    let flipped = rig.doc_id("logo.png");
    assert_ne!(flipped, original);
    let h = rig.handle(&flipped).await;
    let bytes = h
        .with_document(|d| quarto_hub::resource::read_binary_content(d))
        .unwrap();
    assert_eq!(bytes, vec![9, 8, 7]);
    rig.git(&["switch", "-q", "main"]);
    rig.settle().await;
    assert_eq!(rig.doc_id("logo.png"), original);
    assert_eq!(
        std::fs::read(rig.dir.join("logo.png")).unwrap(),
        vec![1, 2, 3, 4, 5]
    );
}

// ---------------------------------------------------------------- arrival variants

/// Registers a doc under `other` carrying live edits that are not in git, then comes back to `other`.
async fn live_edits_on_other(rig: &Rig) -> String {
    rig.git(&["switch", "-q", "other"]);
    rig.settle().await;
    let doc = rig.doc_id("a.qmd");
    rig.edit_doc(&doc, "# A\n\nlive edit\n").await;
    rig.git(&["switch", "-q", "main"]);
    rig.settle().await;
    doc
}

#[tokio::test]
async fn writeback_joins_a_drifted_doc_and_writes_its_text_then_the_checkpoint() {
    let rig = two_files(DiskWritePolicy::WriteBack).await;
    make_other_branch(&rig, "# A\n\nother text\n");
    let doc = live_edits_on_other(&rig).await;
    rig.ctx
        .git_epochs()
        .unwrap()
        .hooks
        .events
        .lock()
        .unwrap()
        .clear();
    rig.git(&["switch", "-q", "other"]);
    rig.settle().await;
    assert_eq!(rig.doc_id("a.qmd"), doc, "joined the canonical doc");
    assert_eq!(
        rig.read("a.qmd"),
        "# A\n\nlive edit\n",
        "doc text written to disk"
    );
    let st = rig.ctx.git_status();
    assert!(
        st.decisions
            .iter()
            .any(|d| d.kind == "joined" && d.step.as_deref() == Some("4"))
    );
}

#[tokio::test]
async fn readonly_never_writes_the_disk_and_creates_a_doc_from_it() {
    let rig = two_files(DiskWritePolicy::ReadOnly).await;
    make_other_branch(&rig, "# A\n\nother text\n");
    let doc = live_edits_on_other(&rig).await;
    rig.git(&["switch", "-q", "other"]);
    rig.settle().await;
    assert_ne!(rig.doc_id("a.qmd"), doc);
    assert_eq!(rig.read("a.qmd"), "# A\n\nother text\n", "disk untouched");
    assert_eq!(rig.text("a.qmd").await, "# A\n\nother text\n");
}

#[tokio::test]
async fn colocated_jj_takes_the_step_5_fallback_and_registers_nothing_until_a_commit() {
    // The probe resolves `.jj/working_copy` once, at startup, so the marker exists from the first commit.
    let rig = Rig::new(
        DiskWritePolicy::WriteBack,
        &[
            ("a.qmd", A.as_bytes()),
            ("b.qmd", B.as_bytes()),
            (".jj/working_copy/marker", b""),
        ],
    )
    .await;
    make_other_branch(&rig, "# A\n\nother text\n");
    let blob = rig.blob_of("other", "a.qmd");
    rig.git(&["switch", "-q", "other"]);
    rig.settle().await;
    let seeded = rig.doc_id("a.qmd");
    assert_eq!(rig.text("a.qmd").await, "# A\n\nother text\n");
    assert!(rig.entry_for("a.qmd", &blob).is_empty(), "unregistered");
    assert!(
        rig.ctx
            .git_status()
            .decisions
            .iter()
            .any(|d| d.kind == "flipped")
    );

    rig.write("a.qmd", "# A\n\nedited after\n");
    rig.ctx.sync_all().await;
    rig.git(&["commit", "-qam", "commit after"]);
    rig.settle().await;
    let new_blob = rig.blob_of("HEAD", "a.qmd");
    let reg = rig.entry_for("a.qmd", &new_blob);
    assert_eq!(reg.len(), 1);
    assert_eq!(reg[0].doc, seeded);
    assert_eq!(
        reg[0].generation, 1,
        "max plus one, since the old entry was absent"
    );
}

#[tokio::test]
async fn a_dirty_file_that_differs_from_doc_and_blob_gets_a_new_unregistered_doc_until_a_commit() {
    let main_text = "t1\n\nmid\n\nbottom\n";
    let other_text = "T1\n\nmid\n\nbottom\n";
    let rig = Rig::new(
        DiskWritePolicy::WriteBack,
        &[("a.qmd", main_text.as_bytes())],
    )
    .await;
    rig.git(&["switch", "-q", "-c", "other"]);
    rig.write("a.qmd", other_text);
    rig.git(&["commit", "-qam", "other"]);
    // Visit `other` once so a canonical doc exists for its blob, then come back.
    rig.settle().await;
    rig.git(&["switch", "-q", "main"]);
    rig.settle().await;
    let blob = rig.blob_of("other", "a.qmd");
    let canonical = rig.entry_for("a.qmd", &blob)[0].doc.clone();

    // An uncommitted edit far from the change on `other`; `switch -m` merges it into the new tree.
    rig.write("a.qmd", "t1\n\nmid\n\nbottom edited\n");
    rig.git(&["switch", "-q", "-m", "other"]);
    rig.settle().await;
    let seeded = rig.doc_id("a.qmd");
    assert_ne!(
        seeded, canonical,
        "a new doc from disk, not the canonical one"
    );
    assert_eq!(rig.text("a.qmd").await, "T1\n\nmid\n\nbottom edited\n");
    assert_eq!(
        rig.entry_for("a.qmd", &blob).len(),
        1,
        "no new registration"
    );
    let st = rig.ctx.git_status();
    assert!(
        st.decisions
            .iter()
            .any(|d| d.kind == "seeded" && d.step.as_deref() == Some("5"))
    );

    // A commit-type decision registers it at max plus one.
    rig.ctx.sync_all().await;
    rig.git(&["commit", "-qam", "commit the merge"]);
    rig.settle().await;
    let committed = rig.blob_of("HEAD", "a.qmd");
    let reg = rig.entry_for("a.qmd", &committed);
    assert_eq!(reg.len(), 1);
    assert_eq!(reg[0].doc, seeded);
    assert!(reg[0].generation >= 2);
}

// ---------------------------------------------------------------- reset --hard and stash

#[tokio::test]
async fn reset_hard_retires_the_edited_doc_and_flips_to_a_fresh_one() {
    for use_stash in [false, true] {
        let rig = two_files(DiskWritePolicy::WriteBack).await;
        let d = rig.doc_id("a.qmd");
        let blob = rig.blob_of("HEAD", "a.qmd");
        rig.edit_doc(&d, "# A\n\nbrowser edit\n").await;
        rig.ctx.sync_all().await;
        assert_eq!(rig.read("a.qmd"), "# A\n\nbrowser edit\n");

        if use_stash {
            rig.git(&["stash"]);
        } else {
            rig.git(&["reset", "--hard"]);
        }
        // Held between the trigger and the decision: ordinary sync writes nothing.
        let held = rig.ctx.sync_all().await;
        assert!(held.skipped > 0);
        assert_eq!(rig.read("a.qmd"), A);
        rig.settle().await;

        let fresh = rig.doc_id("a.qmd");
        assert_ne!(fresh, d, "stash={use_stash}");
        assert_eq!(rig.text("a.qmd").await, A, "guests end on the HEAD text");
        assert_eq!(
            text_of(&rig.handle(&d).await),
            "# A\n\nbrowser edit\n",
            "dirty text stays"
        );
        let v = rig.view();
        assert!(
            v.retired
                .iter()
                .any(|e| e.path == "a.qmd" && e.blob == blob && e.doc == d)
        );
        let reg = rig.entry_for("a.qmd", &blob);
        assert!(reg.iter().any(|e| e.doc == fresh && e.generation == 1));
        let st = rig.ctx.git_status();
        let rec = st
            .decisions
            .iter()
            .find(|r| r.kind == "retired")
            .expect("retired");
        assert_eq!((rec.step.as_deref(), rec.discarded), (Some("2b"), Some(1)));

        if use_stash {
            rig.git(&["stash", "pop"]);
            // The periodic pass leaves a just-rewritten file for the next tick (G1).
            tokio::time::sleep(Duration::from_millis(700)).await;
            rig.ctx.sync_all().await;
            assert_eq!(
                rig.text("a.qmd").await,
                "# A\n\nbrowser edit\n",
                "an author edit in the new doc"
            );
        }
    }
}

// ---------------------------------------------------------------- startup, tracking off

#[tokio::test]
async fn startup_registers_clean_files_and_leaves_dirty_ones_alone() {
    let mut rig = Rig::build(
        DiskWritePolicy::WriteBack,
        &[
            ("a.qmd", A.as_bytes()),
            ("b.qmd", B.as_bytes()),
            ("logo.png", &[1, 2, 3]),
        ],
        GitTracking::Ignore,
        true,
    )
    .await;
    // Dirty b.qmd on disk, then start tracking.
    rig.write("b.qmd", "# B\n\ndirty\n");
    rig.restart_with(GitTracking::Track).await;

    let index = rig.ctx.index();
    let epochs = index.epochs_doc_id().expect("sidecar pointer");
    assert!(!epochs.is_empty());
    assert!(index.owner().is_none(), "the hub writes no owner key");
    let repo = index.repo_identity().expect("repo entry");
    assert_eq!(repo.roots.len(), 1);
    assert_eq!(repo.prefix, "");
    let view = rig.view();
    let a = rig.blob_of("HEAD", "a.qmd");
    let png = rig.blob_of("HEAD", "logo.png");
    assert_eq!(rig.entry_for("a.qmd", &a)[0].doc, rig.doc_id("a.qmd"));
    assert_eq!(
        rig.entry_for("logo.png", &png)[0].doc,
        rig.doc_id("logo.png")
    );
    assert!(
        view.entries.iter().all(|e| e.path != "b.qmd"),
        "dirty stays unregistered"
    );
    let count = view.entries.len();

    rig.restart().await;
    assert_eq!(
        rig.view().entries.len(),
        count,
        "a restart adds no duplicates"
    );
}

#[tokio::test]
async fn tracking_off_writes_no_git_keys() {
    let rig = Rig::build(
        DiskWritePolicy::WriteBack,
        &[("a.qmd", A.as_bytes())],
        GitTracking::Ignore,
        true,
    )
    .await;
    assert!(rig.ctx.index().epochs_doc_id().is_none());
    assert!(rig.ctx.index().repo_identity().is_none());
    assert!(rig.ctx.index().owner().is_none());
    assert!(!rig.ctx.git_status().tracking);
    // The gate never closes without tracking.
    std::fs::write(rig.dir.join(".git/index.lock"), "").unwrap();
    let r = rig.ctx.sync_all().await;
    assert_eq!(r.skipped, 0);
}

// ---------------------------------------------------------------- generation, fork, canonical selection

#[tokio::test]
async fn a_doc_listed_under_another_path_is_forked_not_reused() {
    let rig = two_files(DiskWritePolicy::WriteBack).await;
    make_other_branch(&rig, "# A\n\nother text\n");
    // Another peer registered a doc for (a.qmd, other-blob) that this hub also lists under c.qmd.
    let x = rig.ctx.git_epochs().unwrap();
    let _ = x;
    let repo = rig.ctx.repo();
    let mut doc = Automerge::new();
    doc.transact::<_, _, automerge::AutomergeError>(|tx| {
        let obj = tx.put_object(ROOT, "text", automerge::ObjType::Text)?;
        tx.update_text(&obj, "# A\n\nother text\n")?;
        Ok(())
    })
    .unwrap();
    let h = repo.create(doc).await.unwrap();
    let xid = h.document_id().to_string();
    rig.ctx.index().add_file("c.qmd", &xid).unwrap();
    let blob = rig.blob_of("other", "a.qmd");
    let ops = vec![RegistryOp::Put {
        key: registry::registry_key("a.qmd", &blob, &xid).unwrap(),
        generation: 5,
    }];
    rig.sidecar().with_document(|d| {
        d.transact::<_, _, automerge::AutomergeError>(|tx| {
            regdoc::apply_in(tx, &ops);
            Ok(())
        })
        .unwrap();
    });
    rig.git(&["switch", "-q", "other"]);
    rig.settle().await;
    let flipped = rig.doc_id("a.qmd");
    assert_ne!(flipped, xid, "forked");
    assert_eq!(rig.doc_id("c.qmd"), xid, "the other path keeps its doc");
    assert_eq!(rig.text("a.qmd").await, "# A\n\nother text\n");
    let reg = rig.entry_for("a.qmd", &blob);
    assert!(
        reg.iter().any(|e| e.doc == flipped && e.generation == 6),
        "{reg:?}"
    );
}

#[tokio::test]
async fn a_peer_registering_a_better_canonical_doc_re_attaches_and_a_retired_doc_stays() {
    let rig = two_files(DiskWritePolicy::WriteBack).await;
    let d = rig.doc_id("a.qmd");
    let blob = rig.blob_of("HEAD", "a.qmd");
    // Another peer's doc with the same text and a lower id at the same generation.
    let (xid, xh) = loop {
        let h = rig
            .ctx
            .repo()
            .create({
                let mut doc = Automerge::new();
                doc.transact::<_, _, automerge::AutomergeError>(|tx| {
                    let obj = tx.put_object(ROOT, "text", automerge::ObjType::Text)?;
                    tx.update_text(&obj, A)?;
                    Ok(())
                })
                .unwrap();
                doc
            })
            .await
            .unwrap();
        let id = h.document_id().to_string();
        if id < d {
            break (id, h);
        }
    };
    let _ = xh;
    let put = |doc: String| {
        vec![RegistryOp::Put {
            key: registry::registry_key("a.qmd", &blob, &doc).unwrap(),
            generation: 0,
        }]
    };
    rig.sidecar().with_document(|s| {
        s.transact::<_, _, automerge::AutomergeError>(|tx| {
            regdoc::apply_in(tx, &put(xid.clone()));
            Ok(())
        })
        .unwrap();
    });
    rig.settle().await;
    assert_eq!(rig.doc_id("a.qmd"), xid, "re-attached to the canonical doc");

    // Another peer retires the attached doc: this hub stays in it.
    let retire = vec![RegistryOp::Retire {
        key: registry::registry_key("a.qmd", &blob, &xid).unwrap(),
        generation: 0,
    }];
    rig.sidecar().with_document(|s| {
        s.transact::<_, _, automerge::AutomergeError>(|tx| {
            regdoc::apply_in(tx, &retire);
            Ok(())
        })
        .unwrap();
    });
    rig.settle().await;
    assert_eq!(
        rig.doc_id("a.qmd"),
        xid,
        "a doc another peer retired is left attached"
    );
}

// ---------------------------------------------------------------- failure, locks, status, paths

#[tokio::test]
async fn a_failed_attach_falls_back_to_ordinary_sync_for_that_file_only() {
    let rig = Rig::new(
        DiskWritePolicy::WriteBack,
        &[("a.qmd", A.as_bytes()), ("b.qmd", B.as_bytes())],
    )
    .await;
    rig.git(&["switch", "-q", "-c", "other"]);
    rig.write("a.qmd", "# A\n\nother a\n");
    rig.write("b.qmd", "# B\n\nother b\n");
    rig.git(&["commit", "-qam", "both"]);
    rig.git(&["switch", "-q", "main"]);
    // Both files have an unavailable canonical doc on `other`.
    let a_blob = rig.blob_of("other", "a.qmd");
    let b_blob = rig.blob_of("other", "b.qmd");
    let ids = [unreachable_doc_id("x").await, unreachable_doc_id("y").await];
    let ops = vec![
        RegistryOp::Put {
            key: registry::registry_key("a.qmd", &a_blob, &ids[0]).unwrap(),
            generation: 3,
        },
        RegistryOp::Put {
            key: registry::registry_key("b.qmd", &b_blob, &ids[1]).unwrap(),
            generation: 3,
        },
    ];
    rig.sidecar().with_document(|s| {
        s.transact::<_, _, automerge::AutomergeError>(|tx| {
            regdoc::apply_in(tx, &ops);
            Ok(())
        })
        .unwrap();
    });
    let hooks = &rig.ctx.git_epochs().unwrap().hooks;
    hooks.attach_timeout_ms.store(200, Ordering::SeqCst);
    *hooks.fail_attach.lock().unwrap() = Some(Box::new(|p| p == "a.qmd"));
    let a_before = rig.doc_id("a.qmd");
    rig.git(&["switch", "-q", "other"]);
    rig.settle().await;
    assert_eq!(
        rig.doc_id("a.qmd"),
        a_before,
        "a.qmd falls back to ordinary sync"
    );
    assert_ne!(
        rig.doc_id("b.qmd"),
        ids[1],
        "b.qmd is created after the unavailable wait"
    );
    assert_eq!(rig.text("b.qmd").await, "# B\n\nother b\n");
}

#[tokio::test]
async fn unreachable_canonical_docs_wait_one_cap_not_one_per_doc() {
    let rig = Rig::new(
        DiskWritePolicy::WriteBack,
        &[
            ("a.qmd", A.as_bytes()),
            ("b.qmd", B.as_bytes()),
            ("c.qmd", b"# C\n"),
        ],
    )
    .await;
    rig.git(&["switch", "-q", "-c", "other"]);
    for (f, t) in [
        ("a.qmd", "# A2\n"),
        ("b.qmd", "# B2\n"),
        ("c.qmd", "# C2\n"),
    ] {
        rig.write(f, t);
    }
    rig.git(&["commit", "-qam", "all"]);
    rig.git(&["switch", "-q", "main"]);
    let mut ops = Vec::new();
    for f in ["a.qmd", "b.qmd", "c.qmd"] {
        let blob = rig.blob_of("other", f);
        let id = unreachable_doc_id("z").await;
        ops.push(RegistryOp::Put {
            key: registry::registry_key(f, &blob, &id).unwrap(),
            generation: 1,
        });
    }
    rig.sidecar().with_document(|s| {
        s.transact::<_, _, automerge::AutomergeError>(|tx| {
            regdoc::apply_in(tx, &ops);
            Ok(())
        })
        .unwrap();
    });
    rig.ctx
        .git_epochs()
        .unwrap()
        .hooks
        .attach_timeout_ms
        .store(600, Ordering::SeqCst);
    rig.git(&["switch", "-q", "other"]);
    let t = now_ms();
    rig.ctx.git_tick_at(t).await;
    let started = Instant::now();
    rig.ctx.git_tick_at(t + 1_100).await;
    assert!(
        started.elapsed() < Duration::from_millis(1_500),
        "three unreachable docs took {:?}",
        started.elapsed()
    );
    assert_eq!(rig.text("c.qmd").await, "# C2\n");
}

#[tokio::test]
async fn the_status_accessor_reports_gate_state_and_keeps_twenty_decisions() {
    let rig = two_files(DiskWritePolicy::WriteBack).await;
    let st = rig.ctx.git_status();
    assert!(st.tracking && !st.paused && !st.pending && st.markers.is_empty());
    std::fs::write(rig.dir.join(".git/index.lock"), "").unwrap();
    let st = rig.ctx.git_status();
    assert!(st.paused && st.pending);
    assert_eq!(st.markers, vec!["index.lock".to_string()]);
    std::fs::remove_file(rig.dir.join(".git/index.lock")).unwrap();
    rig.settle().await;
    for i in 0..25 {
        rig.write("a.qmd", &format!("# A\n\nedit {i}\n"));
        rig.ctx.sync_all().await;
        rig.git(&["commit", "-qam", &format!("c{i}")]);
        rig.settle().await;
    }
    let st = rig.ctx.git_status();
    assert_eq!(st.decisions.len(), 20);
    assert!(st.decisions.iter().all(|d| d.kind == "moved-forward"));
}

#[tokio::test]
async fn a_held_decision_stays_pending_for_tests_to_inspect() {
    let rig = two_files(DiskWritePolicy::WriteBack).await;
    make_other_branch(&rig, "# A\n\nother text\n");
    let hooks = &rig.ctx.git_epochs().unwrap().hooks;
    hooks.hold_decisions.store(true, Ordering::SeqCst);
    let old = rig.doc_id("a.qmd");
    rig.git(&["switch", "-q", "other"]);
    rig.settle().await;
    assert_eq!(rig.doc_id("a.qmd"), old, "no decision while held");
    assert!(rig.ctx.git_status().pending);
    hooks.hold_decisions.store(false, Ordering::SeqCst);
    rig.settle().await;
    assert_ne!(rig.doc_id("a.qmd"), old);
}

#[tokio::test]
async fn the_hubs_own_writes_do_not_restart_the_quiet_window() {
    let rig = two_files(DiskWritePolicy::WriteBack).await;
    make_other_branch(&rig, "# A\n\nother text\n");
    let old = rig.doc_id("a.qmd");
    // The disk file equals the checkpoint hash: an event for it is the hub's own write.
    let own = rig.dir.join("b.qmd");
    rig.git(&["switch", "-q", "other"]);
    let t = now_ms();
    rig.ctx.git_tick_at(t).await;
    rig.ctx.git_note_activity_at(&own, t + 900).await;
    rig.ctx.git_tick_at(t + 1_100).await;
    assert_ne!(rig.doc_id("a.qmd"), old, "the decision was not postponed");

    // Control: an event for a file the hub did not write restarts the window.
    rig.git(&["switch", "-q", "main"]);
    rig.write("b.qmd", "# B\n\nsomeone else typed\n");
    let t = now_ms();
    rig.ctx.git_tick_at(t).await;
    rig.ctx.git_note_activity_at(&own, t + 900).await;
    rig.ctx.git_tick_at(t + 1_100).await;
    assert!(
        rig.ctx.git_status().pending,
        "an event for a file the hub did not write restarts the window"
    );
}

/// Links `link` to `target`; returns false where the platform has no cheap symlink (the junction case is a
/// Windows run, GE-15).
#[cfg(unix)]
fn link_dir(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[cfg(not(unix))]
fn link_dir(_target: &Path, _link: &Path) -> bool {
    false
}

#[tokio::test]
async fn hostile_index_keys_are_rejected_and_nothing_is_written_through_them() {
    let rig = two_files(DiskWritePolicy::WriteBack).await;
    let id = rig.doc_id("a.qmd");
    let outside = TempDir::new().unwrap();
    let outside_dir = outside.path().canonicalize().unwrap();
    let mut keys: Vec<String> = [
        "../x",
        ".git/hooks/h",
        ".quarto-hub/x",
        ".vscode/tasks.json",
        "a/../../x",
        "C:\\x",
        "a\\b",
        ".git./x",
    ]
    .iter()
    .map(|k| k.to_string())
    .collect();
    let linked = link_dir(&outside_dir, &rig.dir.join("link"));
    if linked {
        keys.push("link/x.qmd".to_string());
    }
    for k in &keys {
        rig.ctx.index().add_file(k, &id).unwrap();
    }
    let r = rig.ctx.sync_all().await;
    assert_eq!(
        r.rejected,
        keys.len(),
        "every hostile key is rejected: {keys:?}"
    );
    assert!(
        std::fs::read_dir(&outside_dir).unwrap().next().is_none(),
        "nothing written through the link"
    );
    assert!(!rig.dir.join(".git/hooks/h").exists());
    assert!(!rig.dir.join(".vscode").exists());
}

#[tokio::test]
async fn an_unreadable_file_is_skipped_and_the_others_sync() {
    let rig = two_files(DiskWritePolicy::WriteBack).await;
    *rig.ctx
        .git_epochs()
        .unwrap()
        .hooks
        .fail_read
        .lock()
        .unwrap() = Some(Box::new(|p| {
        (p == "a.qmd").then_some(std::io::ErrorKind::PermissionDenied)
    }));
    rig.write("a.qmd", "# A\n\nlocked edit\n");
    rig.write("b.qmd", "# B\n\nfree edit\n");
    let r = rig.ctx.sync_all().await;
    assert_eq!(r.errors.len(), 1);
    assert_eq!(r.errors[0].file_path, "a.qmd");
    assert_eq!(rig.text("a.qmd").await, A, "the doc is untouched");
    assert_eq!(
        rig.read("a.qmd"),
        "# A\n\nlocked edit\n",
        "the file is untouched"
    );
    assert_eq!(rig.text("b.qmd").await, "# B\n\nfree edit\n");
    assert!(rig.ctx.index().has_file("a.qmd"));
    let _ = blob_id(b"");
}
