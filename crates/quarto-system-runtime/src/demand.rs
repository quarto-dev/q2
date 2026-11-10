/*
 * demand.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The demand-filled VFS (preview VFS contract D2, D11).
 */

//! A [`VirtualFileSystem`] filled on demand, and the read set of a render.
//!
//! In `q2 preview` the browser renders against a VFS that sync fills from
//! disk. When the VFS cannot answer a read, an existence check or a
//! directory listing for a path under the project root, [`Demand`] asks a
//! [`MissHandler`] (the render worker's synchronous XHR to the preview
//! server's miss routes) and caches the answer:
//!
//! - a pulled file is inserted with [`VirtualFileSystem::insert_pulled`],
//!   which does not move the generation;
//! - a negative answer (absent path) is cached until the VFS generation
//!   moves, so a probe is asked once per generation;
//! - a listing is pulled once per directory and generation; a directory the
//!   VFS holds only in part answers a listing through the handler, never
//!   from what it happens to hold.
//!
//! The VFS lock is never held across a handler call: on wasm32 without
//! atomics, contention aborts instead of waiting.
//!
//! [`Demand`] also records the read set of a render (contract D11): every
//! project path read, every path probed (hit or miss) and every directory
//! listed, between [`Demand::begin_read_set`] and [`Demand::take_read_set`].
//!
//! Target-independent so the logic is unit-tested natively; `WasmRuntime`
//! delegates to it.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use serde::Serialize;

use crate::traits::{PathKind, RuntimeResult};
use crate::vfs::VirtualFileSystem;

/// The server's answer to an existence check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MissStat {
    pub exists: bool,
    pub is_file: bool,
    pub is_dir: bool,
}

/// One entry of a pulled directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissEntry {
    pub name: String,
    pub is_dir: bool,
}

/// Answers VFS misses. Paths are project-relative with `/` separators; the
/// root is `""`. A refused path answers like an absent one.
pub trait MissHandler: Send + Sync {
    /// The file's bytes, or `None` when it is absent or refused.
    fn read(&self, rel: &str) -> Option<Vec<u8>>;
    /// Whether the path exists, and as what.
    fn stat(&self, rel: &str) -> MissStat;
    /// The directory's admitted entries, or `None` when it is absent or
    /// refused.
    fn list(&self, rel: &str) -> Option<Vec<MissEntry>>;
}

/// What a render read (contract D11). Paths are project-relative with `/`
/// separators; reads outside the project root (embedded resources,
/// artifacts) are not recorded.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ReadSet {
    /// Files read.
    pub reads: BTreeSet<String>,
    /// Paths whose existence was checked, found or not.
    pub probes: BTreeSet<String>,
    /// Directories listed.
    pub listings: BTreeSet<String>,
}

impl ReadSet {
    /// Add everything in `other`.
    pub fn extend(&mut self, other: &ReadSet) {
        self.reads.extend(other.reads.iter().cloned());
        self.probes.extend(other.probes.iter().cloned());
        self.listings.extend(other.listings.iter().cloned());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Known {
    Absent,
    File,
    Dir,
}

/// Answers cached for one VFS generation.
#[derive(Default)]
struct Caches {
    generation: u64,
    known: HashMap<PathBuf, Known>,
    /// Directories whose complete listing has been pulled.
    listed: HashMap<PathBuf, Vec<MissEntry>>,
}

/// The demand layer over a [`VirtualFileSystem`]. See the module docs.
#[derive(Default)]
pub struct Demand {
    handler: RwLock<Option<Arc<dyn MissHandler>>>,
    caches: Mutex<Caches>,
    read_set: Mutex<Option<ReadSet>>,
}

impl Demand {
    /// Install (or with `None`, remove) the miss handler.
    pub fn set_handler(&self, handler: Option<Arc<dyn MissHandler>>) {
        *self.handler.write().unwrap() = handler;
        *self.caches.lock().unwrap() = Caches::default();
    }

    /// Start recording a read set, discarding any in progress.
    pub fn begin_read_set(&self) {
        *self.read_set.lock().unwrap() = Some(ReadSet::default());
    }

    /// Stop recording and return what was recorded since
    /// [`begin_read_set`](Self::begin_read_set).
    pub fn take_read_set(&self) -> Option<ReadSet> {
        self.read_set.lock().unwrap().take()
    }

    /// Record reads made earlier on behalf of the current render (a cache
    /// entry's discovery reads). No-op when nothing is recording.
    pub fn record(&self, reads: &ReadSet) {
        if let Some(set) = self.read_set.lock().unwrap().as_mut() {
            set.extend(reads);
        }
    }

    /// Read a file, pulling it on a miss.
    pub fn read_file(
        &self,
        vfs: &RwLock<VirtualFileSystem>,
        path: &Path,
    ) -> RuntimeResult<Vec<u8>> {
        let (normalized, rel, local, generation) = {
            let vfs = vfs.read().unwrap();
            let normalized = vfs.normalize_path(path);
            let rel = project_rel(&vfs, &normalized);
            (
                normalized.clone(),
                rel,
                vfs.read_file(&normalized),
                vfs.generation(),
            )
        };
        self.note(&rel, |s| &mut s.reads);
        if local.is_ok() {
            return local;
        }
        let (Some(rel), Some(handler)) = (rel, self.handler()) else {
            return local;
        };
        if matches!(
            self.cached(generation, &normalized),
            Some(Known::Absent | Known::Dir)
        ) {
            // Known absent, or a directory: either way not a readable file.
            return local;
        }
        match handler.read(&rel) {
            Some(bytes) => {
                vfs.write()
                    .unwrap()
                    .insert_pulled(&normalized, bytes.clone());
                Ok(bytes)
            }
            None => {
                self.remember(generation, normalized, Known::Absent);
                local
            }
        }
    }

    /// Whether `path` exists (as `kind`), asking on a miss.
    pub fn exists(
        &self,
        vfs: &RwLock<VirtualFileSystem>,
        path: &Path,
        kind: Option<PathKind>,
    ) -> bool {
        let (normalized, rel, local, generation) = {
            let vfs = vfs.read().unwrap();
            let normalized = vfs.normalize_path(path);
            let rel = project_rel(&vfs, &normalized);
            let local = if vfs.is_file(&normalized) {
                Some(Known::File)
            } else if vfs.is_directory(&normalized) {
                Some(Known::Dir)
            } else {
                None
            };
            (normalized, rel, local, vfs.generation())
        };
        self.note(&rel, |s| &mut s.probes);
        let known = match local {
            Some(known) => known,
            None => {
                let (Some(rel), Some(handler)) = (rel, self.handler()) else {
                    return false;
                };
                if self.cached(generation, &normalized).is_none() {
                    self.list_parent(&*handler, generation, &normalized, &rel);
                }
                match self.cached(generation, &normalized) {
                    Some(known) => known,
                    None => {
                        let stat = handler.stat(&rel);
                        let known = if stat.is_dir {
                            Known::Dir
                        } else if stat.exists {
                            Known::File
                        } else {
                            Known::Absent
                        };
                        self.remember(generation, normalized, known);
                        known
                    }
                }
            }
        };
        match kind {
            None => known != Known::Absent,
            Some(PathKind::File) => known == Known::File,
            Some(PathKind::Directory) => known == Known::Dir,
            Some(PathKind::Symlink) => false,
        }
    }

    /// List a directory's direct children, pulling the listing once per
    /// directory and generation.
    pub fn list_dir(
        &self,
        vfs: &RwLock<VirtualFileSystem>,
        path: &Path,
    ) -> RuntimeResult<Vec<PathBuf>> {
        let (normalized, rel, local, generation) = {
            let vfs = vfs.read().unwrap();
            let normalized = vfs.normalize_path(path);
            let rel = project_rel(&vfs, &normalized);
            (
                normalized.clone(),
                rel,
                vfs.list_directory(&normalized),
                vfs.generation(),
            )
        };
        self.note(&rel, |s| &mut s.listings);
        let (Some(rel), Some(handler)) = (rel, self.handler()) else {
            return local;
        };
        let pulled = {
            let caches = self.caches_for(generation);
            caches.listed.get(&normalized).cloned()
        };
        let pulled = match pulled {
            Some(entries) => entries,
            None => match handler.list(&rel) {
                Some(entries) => {
                    let mut caches = self.caches_for(generation);
                    for entry in &entries {
                        let known = if entry.is_dir {
                            Known::Dir
                        } else {
                            Known::File
                        };
                        caches
                            .known
                            .entry(normalized.join(&entry.name))
                            .or_insert(known);
                    }
                    caches.listed.insert(normalized.clone(), entries.clone());
                    entries
                }
                None => return local,
            },
        };
        let mut out: BTreeSet<PathBuf> = local.unwrap_or_default().into_iter().collect();
        out.extend(pulled.iter().map(|e| normalized.join(&e.name)));
        Ok(out.into_iter().collect())
    }

    fn handler(&self) -> Option<Arc<dyn MissHandler>> {
        self.handler.read().unwrap().clone()
    }

    /// The caches, emptied first if the generation moved.
    fn caches_for(&self, generation: u64) -> std::sync::MutexGuard<'_, Caches> {
        let mut caches = self.caches.lock().unwrap();
        if caches.generation != generation {
            *caches = Caches {
                generation,
                ..Caches::default()
            };
        }
        caches
    }

    /// The prediction rule (contract D2): a probe miss asks for its parent
    /// directory's listing instead of a single `stat`, once per directory
    /// and generation, so the other candidates dart-sass tries there are
    /// answered locally. A parent that does not exist is remembered absent,
    /// which answers every path beneath it.
    fn list_parent(
        &self,
        handler: &dyn MissHandler,
        generation: u64,
        normalized: &Path,
        rel: &str,
    ) {
        let (Some(parent), Some(parent_rel)) = (normalized.parent(), parent_rel(rel)) else {
            return;
        };
        if self.cached(generation, parent) == Some(Known::Absent)
            || self.caches_for(generation).listed.contains_key(parent)
        {
            return;
        }
        match handler.list(&parent_rel) {
            Some(entries) => {
                let mut caches = self.caches_for(generation);
                for entry in &entries {
                    let known = if entry.is_dir {
                        Known::Dir
                    } else {
                        Known::File
                    };
                    caches
                        .known
                        .entry(parent.join(&entry.name))
                        .or_insert(known);
                }
                caches.known.insert(parent.to_path_buf(), Known::Dir);
                caches.listed.insert(parent.to_path_buf(), entries);
            }
            None => self.remember(generation, parent.to_path_buf(), Known::Absent),
        }
    }

    /// A cached answer for `normalized`: remembered directly, implied by a
    /// pulled listing of its parent (a name not in it is absent), or by an
    /// ancestor known to be absent or a file.
    fn cached(&self, generation: u64, normalized: &Path) -> Option<Known> {
        let caches = self.caches_for(generation);
        if let Some(known) = implied(&caches, normalized) {
            return Some(known);
        }
        normalized
            .ancestors()
            .skip(1)
            .any(|a| matches!(implied(&caches, a), Some(Known::Absent | Known::File)))
            .then_some(Known::Absent)
    }

    fn remember(&self, generation: u64, normalized: PathBuf, known: Known) {
        self.caches_for(generation).known.insert(normalized, known);
    }

    fn note(
        &self,
        rel: &Option<String>,
        field: impl FnOnce(&mut ReadSet) -> &mut BTreeSet<String>,
    ) {
        if let Some(rel) = rel
            && let Some(set) = self.read_set.lock().unwrap().as_mut()
        {
            field(set).insert(rel.clone());
        }
    }
}

/// What the caches say about `path` itself: remembered directly, or implied
/// by a pulled listing of its parent (a name not in it is absent).
fn implied(caches: &Caches, path: &Path) -> Option<Known> {
    if let Some(known) = caches.known.get(path) {
        return Some(*known);
    }
    let listing = caches.listed.get(path.parent()?)?;
    let name = path.file_name()?.to_str()?;
    Some(match listing.iter().find(|e| e.name == name) {
        Some(e) if e.is_dir => Known::Dir,
        Some(_) => Known::File,
        None => Known::Absent,
    })
}

/// The parent of a project-relative path (`""` for the root's children),
/// or `None` for the root itself.
fn parent_rel(rel: &str) -> Option<String> {
    if rel.is_empty() {
        return None;
    }
    Some(
        rel.rsplit_once('/')
            .map_or(String::new(), |(p, _)| p.to_string()),
    )
}

/// `normalized` relative to the project root, `/`-separated, or `None` when
/// it lies outside it.
fn project_rel(vfs: &VirtualFileSystem, normalized: &Path) -> Option<String> {
    let rel = normalized.strip_prefix(vfs.project_root()).ok()?;
    let parts: Vec<&str> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A disk stand-in: files and directories by project-relative path,
    /// with call counters per arm.
    #[derive(Default)]
    struct Disk {
        files: Mutex<HashMap<String, Vec<u8>>>,
        dirs: Vec<String>,
        reads: AtomicUsize,
        stats: AtomicUsize,
        lists: AtomicUsize,
        /// When set, a read re-enters the VFS through this callback.
        reenter: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    }

    impl MissHandler for Disk {
        fn read(&self, rel: &str) -> Option<Vec<u8>> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            if let Some(f) = self.reenter.lock().unwrap().as_ref() {
                f();
            }
            self.files.lock().unwrap().get(rel).cloned()
        }
        fn stat(&self, rel: &str) -> MissStat {
            self.stats.fetch_add(1, Ordering::SeqCst);
            let is_file = self.files.lock().unwrap().contains_key(rel);
            let is_dir = self.dirs.iter().any(|d| d == rel);
            MissStat {
                exists: is_file || is_dir,
                is_file,
                is_dir,
            }
        }
        fn list(&self, rel: &str) -> Option<Vec<MissEntry>> {
            self.lists.fetch_add(1, Ordering::SeqCst);
            let prefix = if rel.is_empty() {
                String::new()
            } else {
                format!("{rel}/")
            };
            let files = self.files.lock().unwrap();
            let mut out: Vec<MissEntry> = files
                .keys()
                .filter_map(|p| p.strip_prefix(&prefix))
                .filter(|rest| !rest.contains('/'))
                .map(|name| MissEntry {
                    name: name.to_string(),
                    is_dir: false,
                })
                .collect();
            out.extend(
                self.dirs
                    .iter()
                    .filter_map(|d| d.strip_prefix(&prefix))
                    .filter(|rest| !rest.is_empty() && !rest.contains('/'))
                    .map(|name| MissEntry {
                        name: name.to_string(),
                        is_dir: true,
                    }),
            );
            (rel.is_empty() || self.dirs.iter().any(|d| d == rel)).then_some(out)
        }
    }

    fn setup(
        files: &[(&str, &str)],
        dirs: &[&str],
    ) -> (Arc<Disk>, Arc<RwLock<VirtualFileSystem>>, Arc<Demand>) {
        let disk = Arc::new(Disk {
            files: Mutex::new(
                files
                    .iter()
                    .map(|(p, c)| (p.to_string(), c.as_bytes().to_vec()))
                    .collect(),
            ),
            dirs: dirs.iter().map(|d| d.to_string()).collect(),
            ..Disk::default()
        });
        let demand = Arc::new(Demand::default());
        demand.set_handler(Some(disk.clone()));
        (
            disk,
            Arc::new(RwLock::new(VirtualFileSystem::new())),
            demand,
        )
    }

    #[test]
    fn a_read_miss_pulls_once_and_then_reads_locally() {
        let (disk, vfs, demand) = setup(&[("refs.bib", "@a")], &[]);
        let before = vfs.read().unwrap().generation();
        assert_eq!(
            demand.read_file(&vfs, Path::new("refs.bib")).unwrap(),
            b"@a"
        );
        assert_eq!(
            demand
                .read_file(&vfs, Path::new("/project/refs.bib"))
                .unwrap(),
            b"@a"
        );
        assert_eq!(disk.reads.load(Ordering::SeqCst), 1);
        assert_eq!(
            vfs.read().unwrap().generation(),
            before,
            "a pull is generation-neutral"
        );
    }

    #[test]
    fn a_negative_answer_is_cached_until_the_generation_moves() {
        let (disk, vfs, demand) = setup(&[], &[]);
        for _ in 0..3 {
            assert!(demand.read_file(&vfs, Path::new("missing.scss")).is_err());
            assert!(!demand.exists(&vfs, Path::new("missing.scss"), Some(PathKind::File)));
        }
        assert_eq!(disk.reads.load(Ordering::SeqCst), 1);
        assert_eq!(
            disk.stats.load(Ordering::SeqCst),
            0,
            "the read's miss answers the probe"
        );

        // The file is created and arrives through sync: the generation moves.
        disk.files
            .lock()
            .unwrap()
            .insert("missing.scss".into(), b"$x: 1;".to_vec());
        vfs.write()
            .unwrap()
            .add_file(Path::new("other.qmd"), vec![]);
        assert_eq!(
            demand.read_file(&vfs, Path::new("missing.scss")).unwrap(),
            b"$x: 1;"
        );
        assert_eq!(disk.reads.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_probe_asks_once_per_generation() {
        let (disk, vfs, demand) = setup(&[("a.scss", "x")], &["partials"]);
        for _ in 0..3 {
            assert!(demand.exists(&vfs, Path::new("a.scss"), Some(PathKind::File)));
            assert!(!demand.exists(&vfs, Path::new("a.scss"), Some(PathKind::Directory)));
            assert!(demand.exists(&vfs, Path::new("partials"), Some(PathKind::Directory)));
            assert!(!demand.exists(&vfs, Path::new("_b.scss"), None));
        }
        // All four probes sit in the root: one listing answers them.
        assert_eq!(disk.lists.load(Ordering::SeqCst), 1);
        assert_eq!(disk.stats.load(Ordering::SeqCst), 0);
    }

    /// dart-sass probes about fourteen candidate names per `@import`, in
    /// directories that often do not exist (Bootstrap's imports are tried
    /// against the theme's directory first). A probe asks for its parent's
    /// listing once per generation; an absent parent answers every probe
    /// beneath it (contract D2, the prediction rule).
    #[test]
    fn probes_cost_one_listing_per_directory_and_none_under_an_absent_one() {
        let (disk, vfs, demand) = setup(
            &[("styles/partials/_vars.scss", "$v: 1;")],
            &["styles", "styles/partials"],
        );
        for name in [
            "vars.scss",
            "vars.sass",
            "vars.css",
            "_vars.sass",
            "vars/_index.scss",
        ] {
            assert!(
                !demand.exists(
                    &vfs,
                    &Path::new("styles/partials").join(name),
                    Some(PathKind::File)
                ),
                "{name}"
            );
        }
        assert!(demand.exists(
            &vfs,
            Path::new("styles/partials/_vars.scss"),
            Some(PathKind::File)
        ));
        for name in [
            "rfs.scss",
            "_rfs.scss",
            "rfs/_index.scss",
            "rfs/index.sass",
            "deep/er/x.scss",
        ] {
            assert!(
                !demand.exists(&vfs, &Path::new("styles/vendor").join(name), None),
                "{name}"
            );
        }
        // Two listings: styles/partials, and styles/vendor, which is absent
        // and so answers everything beneath it. `vars/_index.scss` needs
        // none: styles/partials's listing has no `vars`.
        assert_eq!(disk.lists.load(Ordering::SeqCst), 2);
        assert_eq!(disk.stats.load(Ordering::SeqCst), 0);
        assert_eq!(disk.reads.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_partly_held_directory_lists_through_the_handler_once() {
        let (disk, vfs, demand) = setup(
            &[("dir/a.scss", "a"), ("dir/b.scss", "b")],
            &["dir", "dir/sub"],
        );
        // The VFS holds one of the files already (sync pushed it).
        vfs.write()
            .unwrap()
            .add_file(Path::new("dir/a.scss"), b"a".to_vec());
        let names = |v: Vec<PathBuf>| -> Vec<String> {
            v.into_iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        };
        for _ in 0..2 {
            let listed = demand.list_dir(&vfs, Path::new("dir")).unwrap();
            assert_eq!(names(listed), vec!["a.scss", "b.scss", "sub"]);
        }
        assert_eq!(disk.lists.load(Ordering::SeqCst), 1);
        // The listing answers probes in that directory without asking.
        assert!(demand.exists(&vfs, Path::new("dir/b.scss"), Some(PathKind::File)));
        assert!(!demand.exists(&vfs, Path::new("dir/zzz.scss"), None));
        assert_eq!(disk.stats.load(Ordering::SeqCst), 0);
        // ... and reads of a listed file still pull its contents.
        assert_eq!(
            demand.read_file(&vfs, Path::new("dir/b.scss")).unwrap(),
            b"b"
        );
        // After the generation moves, the listing is asked again.
        vfs.write().unwrap().add_file(Path::new("x.qmd"), vec![]);
        demand.list_dir(&vfs, Path::new("dir")).unwrap();
        assert_eq!(disk.lists.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn paths_outside_the_project_never_ask() {
        let (disk, vfs, demand) = setup(&[], &[]);
        assert!(
            demand
                .read_file(&vfs, Path::new("/__quarto_resources__/x.scss"))
                .is_err()
        );
        assert!(!demand.exists(&vfs, Path::new("/.quarto/a.css"), None));
        assert!(demand.list_dir(&vfs, Path::new("/tmp/nope")).is_err());
        assert_eq!(disk.reads.load(Ordering::SeqCst), 0);
        assert_eq!(disk.stats.load(Ordering::SeqCst), 0);
        assert_eq!(disk.lists.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn no_handler_means_local_answers_only() {
        let demand = Demand::default();
        let vfs = RwLock::new(VirtualFileSystem::new());
        assert!(demand.read_file(&vfs, Path::new("a.qmd")).is_err());
        assert!(!demand.exists(&vfs, Path::new("a.qmd"), None));
    }

    /// The handler may call back into the VFS (it is JS on wasm32): the VFS
    /// lock must not be held across the call, or wasm32 aborts.
    #[test]
    fn the_vfs_lock_is_not_held_across_the_handler_call() {
        let (disk, vfs, demand) = setup(&[("a.bib", "@a"), ("b.bib", "@b")], &[]);
        let (vfs2, demand2) = (vfs.clone(), demand.clone());
        *disk.reenter.lock().unwrap() = Some(Box::new(move || {
            // Would deadlock (natively) or abort (wasm32) if a guard were held.
            let mut guard = vfs2.try_write().expect("VFS lock is free during the call");
            guard.add_file(Path::new("/tmp/side-effect"), vec![]);
            drop(guard);
            let _ = demand2.exists(&vfs2, Path::new("whatever"), None);
        }));
        assert_eq!(demand.read_file(&vfs, Path::new("a.bib")).unwrap(), b"@a");
    }

    #[test]
    fn the_read_set_holds_reads_probes_and_listings_of_one_render() {
        let (_disk, vfs, demand) = setup(&[("refs.bib", "@a")], &["partials"]);
        vfs.write()
            .unwrap()
            .add_file(Path::new("index.qmd"), b"# Hi".to_vec());
        // Before the scope: not recorded.
        demand.read_file(&vfs, Path::new("index.qmd")).unwrap();

        demand.begin_read_set();
        demand.read_file(&vfs, Path::new("index.qmd")).unwrap();
        demand.read_file(&vfs, Path::new("refs.bib")).unwrap();
        demand.exists(&vfs, Path::new("partials/_vars.scss"), Some(PathKind::File));
        demand.exists(&vfs, Path::new("index.qmd"), None);
        demand.list_dir(&vfs, Path::new("partials")).unwrap();
        demand
            .read_file(&vfs, Path::new("/__quarto_resources__/x.scss"))
            .ok();
        demand.record(&ReadSet {
            reads: ["_quarto.yml".to_string()].into(),
            ..ReadSet::default()
        });
        let set = demand.take_read_set().unwrap();
        assert_eq!(
            set.reads,
            ["_quarto.yml", "index.qmd", "refs.bib"]
                .map(String::from)
                .into()
        );
        assert_eq!(
            set.probes,
            ["index.qmd", "partials/_vars.scss"]
                .map(String::from)
                .into()
        );
        assert_eq!(set.listings, ["partials".to_string()].into());
        assert!(demand.take_read_set().is_none(), "the scope ended");
    }
}
