/*
 * session_runtime/layers.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! The two project-file layers a [`super::SessionRuntime`] reads through:
//! [`EditorLayer`] (open buffers) over a [`BaseLayer`] (the project's files
//! as its feeder last reported them).
//!
//! Both are keyed by **project-relative path**, the key the hub's index
//! document uses too (`files: path → docId`), so a hub feeder needs no
//! translation. Absolute paths exist only at the runtime boundary.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use quarto_system_runtime::VirtualFileSystem;

use crate::document_profile::IncludeEntry;

/// What a cached analysis result depended on, per file.
///
/// Recorded by the runtime on every project-file lookup (see
/// [`super::SessionRuntime::reads`]) so a session can tell whether a cached
/// Pass-1 result is still current: it is, iff every recorded key still
/// matches.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VersionKey {
    /// An open editor buffer, at this LSP document version.
    Buffer(i32),
    /// SHA-256 of the bytes that were read. Content, not mtime: a content
    /// processor's membership flips on a plain edit with no path change
    /// (`document-profile-contract.md`, "Content-processor membership").
    Content([u8; 32]),
    /// A file known by its metadata only, whose bytes are read on demand
    /// (binaries; decision D2). Its bytes do not feed Pass 1, only its
    /// presence does.
    Stat {
        size: u64,
        modified: Option<SystemTime>,
    },
    /// A version a feeder supplies directly, e.g. a hub document's
    /// automerge heads.
    Opaque(String),
    /// The path was looked up and does not exist. Recorded because a file
    /// that appears later must invalidate the result too.
    Absent,
}

impl VersionKey {
    pub fn content_of(bytes: &[u8]) -> Self {
        VersionKey::Content(IncludeEntry::hash_bytes(bytes))
    }
}

/// One open editor buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Buffer {
    pub bytes: Arc<[u8]>,
    /// The LSP `textDocument` version.
    pub version: i32,
}

/// Open editor buffers, keyed by project-relative path.
///
/// Kept apart from the [`BaseLayer`] and never written into it: the base
/// always reflects the feeder's truth, so a buffer can be dropped on close,
/// diffed against its base to publish it to a hub (buffers-to-preview, F4),
/// and told apart from a change on disk. One per client connection, not per
/// session (plan: "The layers").
#[derive(Debug, Clone, Default)]
pub struct EditorLayer {
    buffers: HashMap<PathBuf, Buffer>,
}

impl EditorLayer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, rel: impl Into<PathBuf>, bytes: impl Into<Arc<[u8]>>, version: i32) {
        self.buffers.insert(
            rel.into(),
            Buffer {
                bytes: bytes.into(),
                version,
            },
        );
    }

    /// Drop a buffer (the client closed it). Returns whether one was open.
    pub fn remove(&mut self, rel: &Path) -> bool {
        self.buffers.remove(rel).is_some()
    }

    pub fn get(&self, rel: &Path) -> Option<&Buffer> {
        self.buffers.get(rel)
    }

    pub fn is_empty(&self) -> bool {
        self.buffers.is_empty()
    }

    /// Whether `rel` is a directory that holds an open buffer.
    pub(crate) fn has_dir(&self, rel: &Path) -> bool {
        self.buffers
            .keys()
            .any(|p| p.starts_with(rel) && p.as_path() != rel)
    }

    /// Direct children of directory `rel` that hold or are buffers.
    pub(crate) fn children(&self, rel: &Path) -> BTreeSet<PathBuf> {
        self.buffers
            .keys()
            .filter_map(|p| {
                let rest = p.strip_prefix(rel).ok()?;
                let first = rest.components().next()?;
                Some(rel.join(first))
            })
            .collect()
    }
}

/// What the base layer knows about one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaseEntry {
    File {
        size: u64,
        modified: Option<SystemTime>,
        version: VersionKey,
    },
    Dir,
}

/// How to get a base file's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaseRead {
    /// The layer holds them.
    Bytes(Vec<u8>),
    /// Present by metadata only: read these bytes on demand from this
    /// absolute path, through the runtime's inner runtime (decision D2).
    OnDemand(PathBuf),
}

/// The project's files as a feeder last reported them.
///
/// Implementations: [`VfsBaseLayer`] (disk mode, and the browser), and in
/// attached mode a read-through view over a hub project's automerge
/// documents (F3). Paths are project-relative; `""` is the project root.
pub trait BaseLayer: Send + Sync {
    /// The entry at `rel`, if any.
    fn entry(&self, rel: &Path) -> Option<BaseEntry>;
    /// The bytes of the file at `rel`; `None` if there is no file there.
    fn read(&self, rel: &Path) -> Option<BaseRead>;
    /// The direct children of directory `rel`, as project-relative paths;
    /// `None` if there is no directory there.
    fn list(&self, rel: &Path) -> Option<Vec<PathBuf>>;
}

/// A file present by metadata only (see [`BaseRead::OnDemand`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnDemandFile {
    /// Absolute path the bytes are read from.
    pub source: PathBuf,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// A [`BaseLayer`] backed by a [`VirtualFileSystem`], the same in-memory
/// store the browser analyses from. Text files hold their bytes; binaries
/// can be entered by metadata only and read on demand (D2).
#[derive(Debug)]
pub struct VfsBaseLayer {
    /// Holds the loaded files under [`VFS_ROOT`], at their relative paths.
    vfs: VirtualFileSystem,
    hashes: HashMap<PathBuf, [u8; 32]>,
    on_demand: HashMap<PathBuf, OnDemandFile>,
}

/// Where [`VfsBaseLayer`] mounts the project inside its VFS. Private to the
/// layer: callers only ever see project-relative paths.
const VFS_ROOT: &str = "/project";

impl Default for VfsBaseLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl VfsBaseLayer {
    pub fn new() -> Self {
        Self {
            vfs: VirtualFileSystem::with_project_root(PathBuf::from(VFS_ROOT)),
            hashes: HashMap::new(),
            on_demand: HashMap::new(),
        }
    }

    /// Add or replace a file whose bytes the layer holds.
    pub fn insert_file(&mut self, rel: impl Into<PathBuf>, bytes: Vec<u8>) {
        let rel = rel.into();
        self.on_demand.remove(&rel);
        self.hashes
            .insert(rel.clone(), IncludeEntry::hash_bytes(&bytes));
        self.vfs.add_file(&rel, bytes);
    }

    /// Add or replace a file known by metadata only.
    pub fn insert_on_demand(&mut self, rel: impl Into<PathBuf>, file: OnDemandFile) {
        let rel = rel.into();
        self.vfs.remove_file(&rel);
        self.hashes.remove(&rel);
        if let Some(parent) = rel.parent() {
            self.vfs.add_directory(parent);
        }
        self.on_demand.insert(rel, file);
    }

    /// Remove a file. Returns whether it was present. Directories it leaves
    /// empty stay listed until the layer is rebuilt; a feeder that removes
    /// whole directories reloads.
    pub fn remove(&mut self, rel: &Path) -> bool {
        let held = self.vfs.remove_file(rel);
        self.hashes.remove(rel);
        let lazy = self.on_demand.remove(rel).is_some();
        held || lazy
    }

    fn vfs_to_rel(path: &Path) -> Option<PathBuf> {
        path.strip_prefix(VFS_ROOT).ok().map(Path::to_path_buf)
    }
}

impl BaseLayer for VfsBaseLayer {
    fn entry(&self, rel: &Path) -> Option<BaseEntry> {
        if let Some(file) = self.on_demand.get(rel) {
            return Some(BaseEntry::File {
                size: file.size,
                modified: file.modified,
                version: VersionKey::Stat {
                    size: file.size,
                    modified: file.modified,
                },
            });
        }
        if self.vfs.is_file(rel) {
            return Some(BaseEntry::File {
                size: self.vfs.file_size(rel).unwrap_or(0),
                modified: None,
                version: VersionKey::Content(self.hashes.get(rel).copied().unwrap_or_default()),
            });
        }
        self.vfs.is_directory(rel).then_some(BaseEntry::Dir)
    }

    fn read(&self, rel: &Path) -> Option<BaseRead> {
        if let Some(file) = self.on_demand.get(rel) {
            return Some(BaseRead::OnDemand(file.source.clone()));
        }
        self.vfs.read_file(rel).ok().map(BaseRead::Bytes)
    }

    fn list(&self, rel: &Path) -> Option<Vec<PathBuf>> {
        let mut children: BTreeSet<PathBuf> = self
            .vfs
            .list_directory(rel)
            .ok()?
            .iter()
            .filter_map(|p| Self::vfs_to_rel(p))
            .collect();
        children.extend(
            self.on_demand
                .keys()
                .filter(|p| p.parent() == Some(rel))
                .cloned(),
        );
        Some(children.into_iter().collect())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl VfsBaseLayer {
    /// The disk feeder's initial load: walk `root` with the same
    /// classification the hub uses to sync a project
    /// ([`quarto_project_files::ProjectFiles`], decision D3), so the layer
    /// holds exactly the file set a preview of this directory would. Text
    /// files are loaded; binaries are entered by metadata only.
    ///
    /// Files the classification leaves out (a `.bib`, a data file) are not
    /// in the layer; in disk mode the runtime reads them through to disk.
    pub fn load_from_disk(root: &Path) -> std::io::Result<Self> {
        let files = quarto_project_files::ProjectFiles::discover(root);
        let mut layer = Self::new();
        let text = files
            .qmd_files
            .iter()
            .chain(&files.config_files)
            .chain(&files.extension_files)
            .chain(&files.source_files)
            .chain(&files.resource_files)
            .chain(&files.text_dep_files);
        for rel in text {
            layer.insert_file(rel.clone(), std::fs::read(root.join(rel))?);
        }
        for rel in &files.binary_files {
            let source = root.join(rel);
            let meta = std::fs::metadata(&source)?;
            layer.insert_on_demand(
                rel.clone(),
                OnDemandFile {
                    source,
                    size: meta.len(),
                    modified: meta.modified().ok(),
                },
            );
        }
        Ok(layer)
    }
}
