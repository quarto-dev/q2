/*
 * session_runtime/mod.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! The runtime a long-lived analysis session reads a project through
//! (epic bd-3f6ijnw9, child C2).
//!
//! A [`SessionRuntime`] answers every [`SystemRuntime`] call for one project
//! from three layers, top to bottom:
//!
//! 1. [`EditorLayer`]: open editor buffers (unsaved text).
//! 2. [`BaseLayer`]: the project's files as the session's feeder last
//!    reported them. [`VfsBaseLayer`] backs it with the same
//!    `VirtualFileSystem` the browser analyses from; in attached mode (F3) a
//!    read-through view over a hub project's automerge documents does.
//! 3. The **inner** runtime (native, or the browser's): everything that is
//!    not a project file — environment, temp dirs, XDG and user extension
//!    directories, `exec`, Sass, the cache.
//!
//! [`ProjectSource`] decides what happens to a *project* path that neither
//! layer has. With [`ProjectSource::Disk`] it falls through to the inner
//! runtime: the base holds the files the hub's classification knows about,
//! and anything else in the directory (a `.bib`, a data file) is read from
//! disk. With [`ProjectSource::Attached`] it does not exist: the sync
//! server is the only truth for project files, as it is for a preview in the
//! browser. Paths outside the project root always go to the inner runtime.
//!
//! The runtime is **read-only for project files**: writes, copies onto,
//! renames and removals inside the root are refused. Pass 1 writes nothing
//! there, and analysis must not.
//!
//! Every project-file lookup is recorded with the [`VersionKey`] it saw
//! ([`SessionRuntime::reads`]), so a session can keep a cached result exactly
//! as long as every file it read is unchanged — includes and `_metadata.yml`
//! layers included. Directory listings are not recorded; a session rebuilds
//! when its feeder reports files added or removed.
//!
//! A `SessionRuntime` is a snapshot: it holds `Arc`s of the layers as they
//! were when it was built, and the session builds a new one per analysis run.
//! It generalizes [`crate::overlay_runtime::OverlayRuntime`] (a map of paths
//! over an inner runtime), and like it, writes out every trait method so that
//! an inner runtime's overrides are never replaced by trait defaults.

mod layers;

pub use layers::{
    BaseEntry, BaseLayer, BaseRead, Buffer, EditorLayer, OnDemandFile, VersionKey, VfsBaseLayer,
};

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use quarto_system_runtime::{
    CacheEntryInfo, CommandOptions, CommandOutput, FetchPolicy, PathKind, PathMetadata,
    RuntimeError, RuntimeResult, SassOutput, SystemRuntime, TempDir, XdgDirKind,
};

use crate::overlay_runtime::normalize_overlay_path;

/// Where a session's project files really live. See the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSource {
    /// A directory on disk. Project paths no layer has are read from disk.
    Disk,
    /// A sync server. Project paths no layer has do not exist.
    Attached,
}

/// A [`SystemRuntime`] over editor buffers, a base layer, and an inner
/// runtime. See the module docs.
pub struct SessionRuntime {
    root: PathBuf,
    source: ProjectSource,
    editor: Arc<EditorLayer>,
    base: Arc<dyn BaseLayer>,
    inner: Arc<dyn SystemRuntime>,
    reads: Mutex<BTreeMap<PathBuf, VersionKey>>,
}

/// Where a path resolves.
enum Located {
    Buffer(PathBuf),
    Base(PathBuf, BaseEntry),
    /// A project path neither layer has.
    Unknown(PathBuf),
    /// Outside the project root.
    Outside,
}

fn not_found(path: &Path) -> RuntimeError {
    RuntimeError::Io(io::Error::new(
        io::ErrorKind::NotFound,
        format!("Path not found: {}", path.display()),
    ))
}

fn read_only(path: &Path) -> RuntimeError {
    RuntimeError::PermissionDenied(format!(
        "the analysis session is read-only for project files: {}",
        path.display()
    ))
}

fn is_not_found(e: &RuntimeError) -> bool {
    matches!(e, RuntimeError::Io(io) if io.kind() == io::ErrorKind::NotFound)
}

fn utf8(bytes: Vec<u8>) -> RuntimeResult<String> {
    String::from_utf8(bytes).map_err(|e| {
        RuntimeError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid UTF-8 in file: {e}"),
        ))
    })
}

impl SessionRuntime {
    /// `root` is the project directory, absolute. In disk mode it should be
    /// canonical (the session gets it from project discovery), so that
    /// paths the pipeline canonicalizes stay under it.
    pub fn new(
        root: impl Into<PathBuf>,
        source: ProjectSource,
        editor: Arc<EditorLayer>,
        base: Arc<dyn BaseLayer>,
        inner: Arc<dyn SystemRuntime>,
    ) -> Self {
        Self {
            root: normalize_overlay_path(&root.into()),
            source,
            editor,
            base,
            inner,
            reads: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every project file looked up so far, by project-relative path, with
    /// the version seen. A read overrides an earlier existence check.
    pub fn reads(&self) -> BTreeMap<PathBuf, VersionKey> {
        self.reads.lock().map(|r| r.clone()).unwrap_or_default()
    }

    /// Whether a lookup of project file `rel` would still see `key`.
    /// Records nothing.
    ///
    /// This is pull validation: a cached result is current iff every entry
    /// of the [`reads`](Self::reads) log of the run that produced it still
    /// matches in a runtime built over the *current* layers. An open buffer
    /// shadows everything below it, so a buffer that appeared, closed or
    /// moved to another version invalidates; so does a change in the base
    /// layer's content hash, a file appearing where one was absent, and (in
    /// disk mode) a change on disk to a file no layer holds, which is
    /// re-hashed or re-stat'ed here.
    pub fn still_matches(&self, rel: &Path, key: &VersionKey) -> bool {
        let buffer = self.buffer_key(rel);
        if let VersionKey::Buffer(_) = key {
            return buffer.as_ref() == Some(key);
        }
        if buffer.is_some() {
            return false;
        }
        if let Some(entry) = self.base.entry(rel) {
            return match entry {
                BaseEntry::File { version, .. } => &version == key,
                BaseEntry::Dir => false,
            };
        }
        let path = self.root.join(rel);
        match (self.source, key) {
            (ProjectSource::Attached, VersionKey::Absent) => true,
            (ProjectSource::Attached, _) => false,
            (ProjectSource::Disk, VersionKey::Absent) => {
                !self.inner.path_exists(&path, None).unwrap_or(false)
            }
            (ProjectSource::Disk, VersionKey::Content(_)) => self
                .inner
                .file_read(&path)
                .is_ok_and(|bytes| &VersionKey::content_of(&bytes) == key),
            (ProjectSource::Disk, VersionKey::Stat { size, modified }) => {
                self.inner.path_metadata(&path).is_ok_and(|m| {
                    m.kind == PathKind::File && m.size == *size && m.modified == *modified
                })
            }
            (ProjectSource::Disk, _) => false,
        }
    }

    /// The project-relative form of `path`, if it is inside the root.
    fn project_rel(&self, path: &Path) -> Option<PathBuf> {
        let absolute = if path.is_absolute() {
            normalize_overlay_path(path)
        } else {
            normalize_overlay_path(&self.cwd().ok()?.join(path))
        };
        absolute
            .strip_prefix(&self.root)
            .ok()
            .map(Path::to_path_buf)
    }

    fn locate(&self, path: &Path) -> Located {
        let Some(rel) = self.project_rel(path) else {
            return Located::Outside;
        };
        if self.editor.get(&rel).is_some() {
            return Located::Buffer(rel);
        }
        match self.base.entry(&rel) {
            Some(entry) => Located::Base(rel, entry),
            None => Located::Unknown(rel),
        }
    }

    /// Record what a read saw (overrides an existence check).
    fn record(&self, rel: PathBuf, key: VersionKey) {
        if let Ok(mut reads) = self.reads.lock() {
            reads.insert(rel, key);
        }
    }

    /// Record what an existence check saw, without overriding a read.
    fn record_lookup(&self, rel: &Path, key: VersionKey) {
        if let Ok(mut reads) = self.reads.lock() {
            reads.entry(rel.to_path_buf()).or_insert(key);
        }
    }

    fn buffer_key(&self, rel: &Path) -> Option<VersionKey> {
        self.editor.get(rel).map(|b| VersionKey::Buffer(b.version))
    }

    fn read_bytes(&self, path: &Path) -> RuntimeResult<Vec<u8>> {
        match self.locate(path) {
            Located::Outside => self.inner.file_read(path),
            Located::Buffer(rel) => {
                let buffer = self.editor.get(&rel).expect("located as a buffer");
                self.record(rel, VersionKey::Buffer(buffer.version));
                Ok(buffer.bytes.to_vec())
            }
            Located::Base(rel, entry) => match self.base.read(&rel) {
                Some(BaseRead::Bytes(bytes)) => {
                    if let BaseEntry::File { version, .. } = entry {
                        self.record(rel, version);
                    }
                    Ok(bytes)
                }
                Some(BaseRead::OnDemand(source)) => {
                    let bytes = self.inner.file_read(&source)?;
                    if let BaseEntry::File { version, .. } = entry {
                        self.record(rel, version);
                    }
                    Ok(bytes)
                }
                // A directory.
                None => Err(RuntimeError::Io(io::Error::new(
                    io::ErrorKind::IsADirectory,
                    format!("Is a directory: {}", path.display()),
                ))),
            },
            Located::Unknown(rel) => match self.source {
                ProjectSource::Attached => {
                    self.record(rel, VersionKey::Absent);
                    Err(not_found(path))
                }
                ProjectSource::Disk => match self.inner.file_read(path) {
                    Ok(bytes) => {
                        self.record(rel, VersionKey::content_of(&bytes));
                        Ok(bytes)
                    }
                    Err(e) => {
                        if is_not_found(&e) {
                            self.record(rel, VersionKey::Absent);
                        }
                        Err(e)
                    }
                },
            },
        }
    }

    /// File / directory / absent, for a project path, with lookup recording.
    fn project_kind(&self, path: &Path) -> Option<Option<PathKind>> {
        match self.locate(path) {
            Located::Outside => None,
            Located::Buffer(rel) => {
                if let Some(key) = self.buffer_key(&rel) {
                    self.record_lookup(&rel, key);
                }
                Some(Some(PathKind::File))
            }
            Located::Base(rel, BaseEntry::File { version, .. }) => {
                self.record_lookup(&rel, version);
                Some(Some(PathKind::File))
            }
            Located::Base(_, BaseEntry::Dir) => Some(Some(PathKind::Directory)),
            Located::Unknown(rel) => {
                if rel.as_os_str().is_empty() || self.editor.has_dir(&rel) {
                    return Some(Some(PathKind::Directory));
                }
                let kind = match self.source {
                    ProjectSource::Attached => None,
                    ProjectSource::Disk => self.inner.path_metadata(path).ok().map(|m| m.kind),
                };
                match kind {
                    None => self.record_lookup(&rel, VersionKey::Absent),
                    Some(PathKind::File) => {
                        if let Ok(meta) = self.inner.path_metadata(path) {
                            self.record_lookup(
                                &rel,
                                VersionKey::Stat {
                                    size: meta.size,
                                    modified: meta.modified,
                                },
                            );
                        }
                    }
                    Some(_) => {}
                }
                Some(kind)
            }
        }
    }

    fn project_mutation(&self, path: &Path) -> Option<RuntimeError> {
        self.project_rel(path).map(|_| read_only(path))
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl SystemRuntime for SessionRuntime {
    // Reads and lookups: answered from the layers for project paths.

    fn file_read(&self, path: &Path) -> RuntimeResult<Vec<u8>> {
        self.read_bytes(path)
    }

    fn file_read_string(&self, path: &Path) -> RuntimeResult<String> {
        match self.locate(path) {
            Located::Outside => self.inner.file_read_string(path),
            _ => utf8(self.read_bytes(path)?),
        }
    }

    fn path_exists(&self, path: &Path, kind: Option<PathKind>) -> RuntimeResult<bool> {
        match self.project_kind(path) {
            None => self.inner.path_exists(path, kind),
            Some(found) => Ok(match (found, kind) {
                (None, _) => false,
                (Some(_), None) => true,
                (Some(found), Some(wanted)) => found == wanted,
            }),
        }
    }

    fn is_file(&self, path: &Path) -> RuntimeResult<bool> {
        match self.project_kind(path) {
            None => self.inner.is_file(path),
            Some(kind) => Ok(kind == Some(PathKind::File)),
        }
    }

    fn is_dir(&self, path: &Path) -> RuntimeResult<bool> {
        match self.project_kind(path) {
            None => self.inner.is_dir(path),
            Some(kind) => Ok(kind == Some(PathKind::Directory)),
        }
    }

    fn canonicalize(&self, path: &Path) -> RuntimeResult<PathBuf> {
        let Some(rel) = self.project_rel(path) else {
            return self.inner.canonicalize(path);
        };
        let normalized = self.root.join(&rel);
        match self.source {
            // Prefer the disk's answer (symlinks); a buffer or base entry
            // that has no disk file yet keeps its lexical form.
            ProjectSource::Disk => match self.inner.canonicalize(path) {
                Ok(p) => Ok(p),
                Err(e) => match self.project_kind(path) {
                    Some(Some(_)) => Ok(normalized),
                    _ => Err(e),
                },
            },
            ProjectSource::Attached => match self.project_kind(path) {
                Some(Some(_)) => Ok(normalized),
                _ => Err(not_found(path)),
            },
        }
    }

    fn path_metadata(&self, path: &Path) -> RuntimeResult<PathMetadata> {
        let dir = |modified| PathMetadata {
            kind: PathKind::Directory,
            size: 0,
            modified,
            accessed: None,
            readonly: true,
        };
        match self.locate(path) {
            Located::Outside => self.inner.path_metadata(path),
            Located::Buffer(rel) => {
                let buffer = self.editor.get(&rel).expect("located as a buffer");
                self.record_lookup(&rel, VersionKey::Buffer(buffer.version));
                // An open buffer reports its own size; mtime is the disk's,
                // if there is a disk file (listing autofill reads it).
                let modified = match self.source {
                    ProjectSource::Disk => {
                        self.inner.path_metadata(path).ok().and_then(|m| m.modified)
                    }
                    ProjectSource::Attached => None,
                };
                Ok(PathMetadata {
                    kind: PathKind::File,
                    size: buffer.bytes.len() as u64,
                    modified,
                    accessed: None,
                    readonly: true,
                })
            }
            Located::Base(
                rel,
                BaseEntry::File {
                    size,
                    modified,
                    version,
                },
            ) => {
                self.record_lookup(&rel, version);
                let modified = modified.or_else(|| match self.source {
                    ProjectSource::Disk => {
                        self.inner.path_metadata(path).ok().and_then(|m| m.modified)
                    }
                    ProjectSource::Attached => None,
                });
                Ok(PathMetadata {
                    kind: PathKind::File,
                    size,
                    modified,
                    accessed: None,
                    readonly: true,
                })
            }
            Located::Base(_, BaseEntry::Dir) => Ok(dir(None)),
            Located::Unknown(rel) => {
                if rel.as_os_str().is_empty() || self.editor.has_dir(&rel) {
                    return Ok(dir(None));
                }
                match self.source {
                    ProjectSource::Attached => {
                        self.record_lookup(&rel, VersionKey::Absent);
                        Err(not_found(path))
                    }
                    ProjectSource::Disk => {
                        let result = self.inner.path_metadata(path);
                        match &result {
                            Ok(meta) if meta.kind == PathKind::File => self.record_lookup(
                                &rel,
                                VersionKey::Stat {
                                    size: meta.size,
                                    modified: meta.modified,
                                },
                            ),
                            Err(e) if is_not_found(e) => {
                                self.record_lookup(&rel, VersionKey::Absent)
                            }
                            _ => {}
                        }
                        result
                    }
                }
            }
        }
    }

    fn dir_list(&self, path: &Path) -> RuntimeResult<Vec<PathBuf>> {
        let Some(rel) = self.project_rel(path) else {
            return self.inner.dir_list(path);
        };
        let mut found = false;
        let mut children: BTreeSet<PathBuf> = BTreeSet::new();
        if let Some(base) = self.base.list(&rel) {
            found = true;
            children.extend(base.into_iter().map(|c| self.root.join(c)));
        }
        if rel.as_os_str().is_empty() || self.editor.has_dir(&rel) {
            found = true;
            children.extend(
                self.editor
                    .children(&rel)
                    .into_iter()
                    .map(|c| self.root.join(c)),
            );
        }
        if self.source == ProjectSource::Disk {
            match self.inner.dir_list(path) {
                Ok(disk) => {
                    found = true;
                    children.extend(disk.into_iter().map(|p| normalize_overlay_path(&p)));
                }
                Err(e) if !is_not_found(&e) => return Err(e),
                Err(_) => {}
            }
        }
        if found {
            Ok(children.into_iter().collect())
        } else {
            Err(not_found(path))
        }
    }

    // Mutations: refused inside the project root, forwarded outside it.

    fn file_write(&self, path: &Path, contents: &[u8]) -> RuntimeResult<()> {
        match self.project_mutation(path) {
            Some(e) => Err(e),
            None => self.inner.file_write(path, contents),
        }
    }

    fn file_copy(&self, src: &Path, dst: &Path) -> RuntimeResult<()> {
        if let Some(e) = self.project_mutation(dst) {
            return Err(e);
        }
        match self.locate(src) {
            Located::Outside => self.inner.file_copy(src, dst),
            _ => self.inner.file_write(dst, &self.read_bytes(src)?),
        }
    }

    fn path_rename(&self, old: &Path, new: &Path) -> RuntimeResult<()> {
        if let Some(e) = self
            .project_mutation(old)
            .or_else(|| self.project_mutation(new))
        {
            return Err(e);
        }
        self.inner.path_rename(old, new)
    }

    fn file_remove(&self, path: &Path) -> RuntimeResult<()> {
        match self.project_mutation(path) {
            Some(e) => Err(e),
            None => self.inner.file_remove(path),
        }
    }

    fn dir_create(&self, path: &Path, recursive: bool) -> RuntimeResult<()> {
        match self.project_mutation(path) {
            Some(e) => Err(e),
            None => self.inner.dir_create(path, recursive),
        }
    }

    fn dir_remove(&self, path: &Path, recursive: bool) -> RuntimeResult<()> {
        match self.project_mutation(path) {
            Some(e) => Err(e),
            None => self.inner.dir_remove(path, recursive),
        }
    }

    fn cwd(&self) -> RuntimeResult<PathBuf> {
        match self.source {
            ProjectSource::Disk => self.inner.cwd(),
            ProjectSource::Attached => Ok(self.root.clone()),
        }
    }

    // Everything else forwards to the inner runtime.

    fn temp_dir(&self, template: &str) -> RuntimeResult<TempDir> {
        self.inner.temp_dir(template)
    }

    fn exec_pipe(&self, command: &str, args: &[&str], stdin: &[u8]) -> RuntimeResult<Vec<u8>> {
        self.inner.exec_pipe(command, args, stdin)
    }

    fn exec_command(
        &self,
        command: &str,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> RuntimeResult<CommandOutput> {
        self.inner.exec_command(command, args, stdin)
    }

    fn exec_command_with_options(
        &self,
        command: &str,
        args: &[&str],
        stdin: Option<&[u8]>,
        options: &CommandOptions<'_>,
    ) -> RuntimeResult<CommandOutput> {
        self.inner
            .exec_command_with_options(command, args, stdin, options)
    }

    fn env_get(&self, name: &str) -> RuntimeResult<Option<String>> {
        self.inner.env_get(name)
    }

    fn env_all(&self) -> RuntimeResult<HashMap<String, String>> {
        self.inner.env_all()
    }

    fn runtime_metadata(&self) -> Option<serde_json::Value> {
        self.inner.runtime_metadata()
    }

    fn is_interactive(&self) -> bool {
        self.inner.is_interactive()
    }

    fn running_in_ci(&self) -> bool {
        self.inner.running_in_ci()
    }

    async fn fetch_url(&self, url: &str) -> RuntimeResult<(Vec<u8>, String)> {
        self.inner.fetch_url(url).await
    }

    async fn fetch_url_hardened(
        &self,
        url: &str,
        policy: &FetchPolicy,
    ) -> RuntimeResult<(Vec<u8>, String)> {
        self.inner.fetch_url_hardened(url, policy).await
    }

    fn can_rasterize_svg(&self) -> bool {
        self.inner.can_rasterize_svg()
    }

    async fn rasterize_svg(&self, svg: &[u8], max_side: u32) -> RuntimeResult<Vec<u8>> {
        self.inner.rasterize_svg(svg, max_side).await
    }

    fn os_name(&self) -> &'static str {
        self.inner.os_name()
    }

    fn arch(&self) -> &'static str {
        self.inner.arch()
    }

    fn cpu_time(&self) -> RuntimeResult<u64> {
        self.inner.cpu_time()
    }

    fn unix_timestamp(&self) -> RuntimeResult<u64> {
        self.inner.unix_timestamp()
    }

    fn local_utc_offset_seconds(&self, unix_timestamp: i64) -> i32 {
        self.inner.local_utc_offset_seconds(unix_timestamp)
    }

    fn monotonic_now_nanos(&self) -> u64 {
        self.inner.monotonic_now_nanos()
    }

    fn xdg_dir(&self, kind: XdgDirKind, subpath: Option<&Path>) -> RuntimeResult<PathBuf> {
        self.inner.xdg_dir(kind, subpath)
    }

    fn find_binary(&self, name: &str, env_var: &str) -> Option<PathBuf> {
        self.inner.find_binary(name, env_var)
    }

    fn print(&self, message: &str) {
        self.inner.print(message)
    }

    fn stdout_write(&self, data: &[u8]) -> RuntimeResult<()> {
        self.inner.stdout_write(data)
    }

    fn stderr_write(&self, data: &[u8]) -> RuntimeResult<()> {
        self.inner.stderr_write(data)
    }

    fn sass_available(&self) -> bool {
        self.inner.sass_available()
    }

    fn sass_compiler_name(&self) -> Option<&'static str> {
        self.inner.sass_compiler_name()
    }

    async fn compile_sass(
        &self,
        scss: &str,
        load_paths: &[PathBuf],
        minified: bool,
    ) -> RuntimeResult<SassOutput> {
        self.inner.compile_sass(scss, load_paths, minified).await
    }

    async fn cache_get(&self, namespace: &str, key: &str) -> RuntimeResult<Option<Vec<u8>>> {
        self.inner.cache_get(namespace, key).await
    }

    async fn cache_set(&self, namespace: &str, key: &str, value: &[u8]) -> RuntimeResult<()> {
        self.inner.cache_set(namespace, key, value).await
    }

    async fn cache_delete(&self, namespace: &str, key: &str) -> RuntimeResult<()> {
        self.inner.cache_delete(namespace, key).await
    }

    async fn cache_clear_namespace(&self, namespace: &str) -> RuntimeResult<()> {
        self.inner.cache_clear_namespace(namespace).await
    }

    async fn cache_list(&self, namespace: &str) -> RuntimeResult<Option<Vec<CacheEntryInfo>>> {
        self.inner.cache_list(namespace).await
    }
}

#[cfg(test)]
mod tests;
