//! Following hub projects as a samod client peer, and reading their
//! comments.
//!
//! A followed project is its index document plus every file document the
//! index lists. Each followed document has a task reading
//! `DocHandle::changes()`, which reports one [`ChangeHint`] per change. A
//! change to the index also brings newly listed files into the project.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use automerge::{ROOT, ReadDoc};
use futures::StreamExt;
use quarto_hub::index::IndexDocument;
use quarto_hub::resource::{DocumentType, detect_document_type};
use quarto_hub_provider::dial_hub;
use samod::{DocHandle, DocumentId, Repo};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use crate::comments::{JsonComment, ParseFailure, comments_in_qmd};

/// Something changed in one followed document. Carries ids only: the
/// client asks again for whatever it needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeHint {
    /// The project's index document id.
    pub project: String,
    /// The changed document's id: the index itself, or one of its files.
    pub document: String,
}

/// The comments of every qmd file in one project.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectComments {
    /// The project's index document id.
    pub project: String,
    /// One entry per `.qmd` file, sorted by path.
    pub files: Vec<FileComments>,
}

/// The comments of one qmd file.
#[derive(Debug, Clone, PartialEq)]
pub struct FileComments {
    /// The file's path in the project.
    pub path: String,
    /// The file's document id.
    pub document: String,
    /// The file's comments in source order; empty when `error` is set.
    pub comments: Vec<JsonComment>,
    /// Why the file's comments could not be read, when they could not.
    /// A client must keep its previous view of such a file rather than
    /// conclude that its comments were resolved.
    pub error: Option<ParseFailure>,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("could not connect to the hub: {0}")]
    Connect(String),
    #[error("the hub has no index document {0}")]
    IndexNotFound(String),
    #[error("project {0} is not being followed")]
    UnknownProject(String),
    #[error("hub error: {0}")]
    Hub(String),
}

/// The projects this peer follows on one hub.
pub struct HubWorkspace {
    inner: Arc<Inner>,
}

struct Inner {
    repo: Repo,
    projects: Mutex<BTreeMap<String, Followed>>,
    hints: UnboundedSender<ChangeHint>,
}

struct Followed {
    index: Arc<IndexDocument>,
    /// File document id to handle, for every file the index has listed.
    files: BTreeMap<String, DocHandle>,
    /// The change-watching tasks, aborted on unfollow.
    watchers: Vec<JoinHandle<()>>,
}

impl HubWorkspace {
    /// Dial the hub with `dialer` and wait up to `connect_timeout` for the
    /// first connection. Returns the workspace and the receiving end of its
    /// change hints.
    pub async fn connect<D: samod::Dialer>(
        dialer: Arc<D>,
        connect_timeout: Duration,
    ) -> Result<(Self, UnboundedReceiver<ChangeHint>), WorkspaceError> {
        let repo = dial_hub(dialer, connect_timeout)
            .await
            .map_err(|e| WorkspaceError::Connect(e.to_string()))?;
        let (hints, receiver) = unbounded_channel();
        let inner = Arc::new(Inner {
            repo,
            projects: Mutex::new(BTreeMap::new()),
            hints,
        });
        Ok((Self { inner }, receiver))
    }

    /// Start following the project whose index document is `index_doc_id`.
    /// Following a project already followed does nothing.
    pub async fn follow(&self, index_doc_id: &str) -> Result<(), WorkspaceError> {
        if self.is_following(index_doc_id) {
            return Ok(());
        }
        let index = IndexDocument::load(&self.inner.repo, index_doc_id)
            .await
            .map_err(|e| WorkspaceError::Hub(e.to_string()))?
            .ok_or_else(|| WorkspaceError::IndexNotFound(index_doc_id.to_string()))?;
        let index = Arc::new(index);

        let watcher = watch(
            self.inner.clone(),
            index_doc_id.to_string(),
            index.handle().clone(),
            true,
        );
        {
            let mut projects = self.inner.projects.lock().unwrap();
            if projects.contains_key(index_doc_id) {
                // Another caller followed it while the index was loading.
                watcher.abort();
                return Ok(());
            }
            projects.insert(
                index_doc_id.to_string(),
                Followed {
                    index,
                    files: BTreeMap::new(),
                    watchers: vec![watcher],
                },
            );
        }
        self.inner.sync_files(index_doc_id).await;
        Ok(())
    }

    /// Stop following a project. Returns whether it was being followed.
    pub fn unfollow(&self, index_doc_id: &str) -> bool {
        let removed = self.inner.projects.lock().unwrap().remove(index_doc_id);
        match removed {
            Some(followed) => {
                for watcher in followed.watchers {
                    watcher.abort();
                }
                true
            }
            None => false,
        }
    }

    pub fn is_following(&self, index_doc_id: &str) -> bool {
        self.inner
            .projects
            .lock()
            .unwrap()
            .contains_key(index_doc_id)
    }

    /// The comments of every `.qmd` file the project's index lists now.
    pub async fn project_comments(
        &self,
        index_doc_id: &str,
    ) -> Result<ProjectComments, WorkspaceError> {
        if !self.is_following(index_doc_id) {
            return Err(WorkspaceError::UnknownProject(index_doc_id.to_string()));
        }
        // A file the index gained since the last change notification is
        // picked up here rather than missed.
        self.inner.sync_files(index_doc_id).await;

        let (index, handles) = {
            let projects = self.inner.projects.lock().unwrap();
            let followed = projects
                .get(index_doc_id)
                .ok_or_else(|| WorkspaceError::UnknownProject(index_doc_id.to_string()))?;
            (followed.index.clone(), followed.files.clone())
        };

        let listed: BTreeMap<String, String> = index.get_all_files().into_iter().collect();
        let files = listed
            .into_iter()
            .filter(|(path, _)| path.ends_with(".qmd"))
            .map(|(path, document)| {
                let read = match handles.get(&document) {
                    Some(handle) => match read_text(handle) {
                        Some(text) => comments_in_qmd(&path, &text),
                        None => Err(ParseFailure {
                            messages: vec!["the document is not a text document".to_string()],
                        }),
                    },
                    None => Err(ParseFailure {
                        messages: vec!["the hub does not have this document".to_string()],
                    }),
                };
                let (comments, error) = match read {
                    Ok(comments) => (comments, None),
                    Err(failure) => (Vec::new(), Some(failure)),
                };
                FileComments {
                    path,
                    document,
                    comments,
                    error,
                }
            })
            .collect();

        Ok(ProjectComments {
            project: index_doc_id.to_string(),
            files,
        })
    }
}

impl Inner {
    /// Find and watch every file the project's index lists that is not
    /// watched yet.
    async fn sync_files(self: &Arc<Self>, project: &str) {
        let (index, known) = {
            let projects = self.projects.lock().unwrap();
            let Some(followed) = projects.get(project) else {
                return;
            };
            (
                followed.index.clone(),
                followed.files.keys().cloned().collect::<Vec<_>>(),
            )
        };
        let listed: BTreeMap<String, String> = index.get_all_files().into_iter().collect();
        for document in listed.into_values() {
            if known.contains(&document) {
                continue;
            }
            let Ok(id) = document.parse::<DocumentId>() else {
                continue;
            };
            let Ok(Some(handle)) = self.repo.find(id).await else {
                continue;
            };
            let mut projects = self.projects.lock().unwrap();
            // The project may have been unfollowed, or the file found by a
            // concurrent call, while `find` was waiting on the hub.
            let Some(followed) = projects.get_mut(project) else {
                return;
            };
            if followed.files.contains_key(&document) {
                continue;
            }
            let watcher = watch(self.clone(), project.to_string(), handle.clone(), false);
            followed.files.insert(document, handle);
            followed.watchers.push(watcher);
        }
    }
}

/// Report a [`ChangeHint`] for every change to `handle`. For the index
/// document, also bring newly listed files into the project.
fn watch(inner: Arc<Inner>, project: String, handle: DocHandle, is_index: bool) -> JoinHandle<()> {
    tokio::spawn(async move {
        let document = handle.document_id().to_string();
        let mut changes = handle.changes();
        while changes.next().await.is_some() {
            // A dropped receiver means nobody is listening; keep following
            // so queries still see the latest text.
            let _ = inner.hints.send(ChangeHint {
                project: project.clone(),
                document: document.clone(),
            });
            if is_index {
                inner.sync_files(&project).await;
            }
        }
    })
}

/// The text of a text file document, or `None` for any other kind.
fn read_text(handle: &DocHandle) -> Option<String> {
    handle.with_document(|doc| match detect_document_type(doc) {
        DocumentType::Text => {
            let (_, text) = doc.get(ROOT, "text").ok().flatten()?;
            doc.text(&text).ok()
        }
        DocumentType::Binary | DocumentType::Invalid => None,
    })
}
