//! The exposure policy: which project paths may leave the disk.
//!
//! One predicate decides whether a path may be exposed to the browser, and
//! therefore to the sync server, to `--share` guests and to write-back. The
//! discovery walk, the preview watcher and the preview's single-file
//! resolver all call it; nothing keeps a private ignore list. The normative
//! description is decision D4 of `claude-notes/designs/preview-vfs-contract.md`.
//!
//! The checks, in the contract's order:
//!
//! 1. Lexical containment: the request is relative to the project root (a
//!    `/project/` prefix is stripped), never absolute, never `..` above the
//!    root.
//! 2. Canonical containment: the canonical path stays under the canonical
//!    root, so a symlink that points outside is refused. Regular files only.
//! 3. No hidden component below the root. The root itself may live under a
//!    hidden directory.
//! 4. Not inside a generated directory. Generated means provenance, not a
//!    name suffix: see [`in_generated_dir`].
//! 5. An extension allow-list per [`Class`].
//! 6. A size cap, for pulled contents only ([`ExposurePolicy::admit_pull`]).
//!
//! The lexical checks (1, 3, 4, 5) run before anything touches the disk, so
//! a refused path is refused whether or not it exists.

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

/// The VFS mount point the browser addresses project files under.
const VFS_PROJECT_PREFIX: &str = "/project";

/// Directories Quarto, an engine or a tool owns outright, at any depth.
/// `_site` and `_book` are the website and book default output directories;
/// a configured `output-dir` is added per project (see
/// [`ExposurePolicy::with_output_dir`]).
const OWNED_DIRS: &[&str] = &[
    "node_modules",
    "__pycache__",
    "_freeze",
    "site_libs",
    "_site",
    "_book",
    "renv",
    "venv",
    "target",
];

/// Suffixes of engine sidecar directories (`<stem>_files/`, `<stem>_cache/`).
/// A sidecar is generated only beside its `<stem>` document; the suffix
/// alone is not provenance (`static_files/brand.scss` is an author's file).
const SIDECAR_SUFFIXES: &[&str] = &["_files", "_cache"];

/// Extensions of the documents a sidecar directory can belong to.
const SIDECAR_SOURCE_EXTENSIONS: &[&str] = &["qmd", "md", "ipynb", "Rmd", "rmd"];

/// Extensions never exposed, even inside `_extensions/`: key material.
const SECRET_EXTENSIONS: &[&str] = &["pem", "key", "p12", "pfx", "keystore", "jks"];

/// The per-file limit for pulled contents: the request builder's limit on a
/// collected file, from `resources/pandoc-wasm.json`.
static PULL_SIZE_CAP: LazyLock<u64> = LazyLock::new(|| {
    let raw = include_str!("../../../resources/pandoc-wasm.json");
    let json: serde_json::Value =
        serde_json::from_str(raw).expect("resources/pandoc-wasm.json is valid JSON");
    json["limits"]["collected_file_bytes"]
        .as_u64()
        .expect("resources/pandoc-wasm.json has limits.collected_file_bytes")
});

/// What kind of file a path is, as far as exposure is concerned. A path
/// with no class is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    /// Documents: `.qmd`, `.md`, `.ipynb`, `.Rmd`.
    Source,
    /// Project and directory config by name: `_quarto*.yml`,
    /// `_metadata.yml`, `_brand.yml`, `_variables.yml` (and `.yaml`).
    Config,
    /// `.css`, `.scss`, `.sass`.
    Stylesheet,
    /// Custom React components: `.tsx`.
    Component,
    /// Bibliographies and citation styles: `.bib`, `.bibtex`, `.ris`, `.csl`.
    Bibliography,
    /// Data and structured text: `.yml`, `.yaml`, `.json`, `.csv`, `.tsv`,
    /// `.txt`, `.xml`, `.geojson`.
    Data,
    /// Include targets, partials, filters and scripts: `.html`, `.htm`,
    /// `.tex`, `.typ`, `.lua`, `.js`, `.mjs`.
    Template,
    /// The binary resource list in [`crate::resource`].
    Binary,
    /// Anything inside an `_extensions/` directory except key material.
    Extension,
}

/// Classify a project-relative path by name and extension. `None` means the
/// policy refuses it whatever else holds.
pub fn classify(rel: &Path) -> Option<Class> {
    let name = rel.file_name()?.to_str()?;
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if ext
        .as_deref()
        .is_some_and(|e| SECRET_EXTENSIONS.contains(&e))
    {
        return None;
    }
    if rel.components().any(|c| c.as_os_str() == "_extensions") {
        return Some(Class::Extension);
    }
    let ext = ext?;
    if matches!(ext.as_str(), "yml" | "yaml") {
        let stem = &name[..name.len() - ext.len() - 1];
        if stem.starts_with("_quarto") || matches!(stem, "_metadata" | "_brand" | "_variables") {
            return Some(Class::Config);
        }
    }
    let class = match ext.as_str() {
        "qmd" | "md" | "ipynb" | "rmd" => Class::Source,
        "css" | "scss" | "sass" => Class::Stylesheet,
        "tsx" => Class::Component,
        "bib" | "bibtex" | "ris" | "csl" => Class::Bibliography,
        "yml" | "yaml" | "json" | "csv" | "tsv" | "txt" | "xml" | "geojson" => Class::Data,
        "html" | "htm" | "tex" | "typ" | "lua" | "js" | "mjs" => Class::Template,
        e if crate::resource::is_binary_extension(e) => Class::Binary,
        _ => return None,
    };
    Some(class)
}

/// Why the policy refused a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// An absolute path, or one with a drive or UNC prefix.
    Absolute,
    /// `..` climbs above the project root.
    Traversal,
    /// The canonical path leaves the canonical root (a symlink escape).
    OutsideRoot,
    /// A component below the root starts with `.`.
    Hidden,
    /// Inside a directory Quarto, an engine or a tool generated.
    Generated,
    /// The name or extension is not on the allow-list.
    Extension,
    /// The path does not exist.
    NotFound,
    /// The path exists but is not a regular file (or, for
    /// [`ExposurePolicy::admit_dir`], not a directory).
    WrongType,
    /// The file is larger than the pull cap.
    TooLarge {
        /// The file's size in bytes.
        size: u64,
        /// The cap in bytes.
        cap: u64,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absolute => write!(f, "absolute paths are not served"),
            Self::Traversal => write!(f, "the path climbs above the project root"),
            Self::OutsideRoot => write!(f, "the path resolves outside the project root"),
            Self::Hidden => write!(f, "hidden paths are not served"),
            Self::Generated => write!(f, "the path is inside a generated directory"),
            Self::Extension => write!(f, "this file type is not served"),
            Self::NotFound => write!(f, "no such file"),
            Self::WrongType => write!(f, "the path is not of the expected type"),
            Self::TooLarge { size, cap } => {
                write!(f, "the file is {size} bytes; the limit is {cap} bytes")
            }
        }
    }
}

impl std::error::Error for Refusal {}

/// A path the policy admitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admitted {
    /// The normalized project-relative path.
    pub rel: PathBuf,
    /// The canonical absolute path on disk.
    pub abs: PathBuf,
    /// The file's class.
    pub class: Class,
    /// The file's size in bytes.
    pub size: u64,
}

/// The exposure policy for one project root.
#[derive(Debug, Clone)]
pub struct ExposurePolicy {
    root: PathBuf,
    canonical_root: PathBuf,
    /// The configured output directory, project-relative. `None` when it is
    /// the root itself or outside it.
    output_dir: Option<PathBuf>,
    size_cap: u64,
}

impl ExposurePolicy {
    /// The policy for `root`. A root that cannot be canonicalized keeps its
    /// lexical spelling; nothing under it can be canonicalized either, so
    /// nothing is admitted.
    pub fn new(root: &Path) -> Self {
        let canonical_root =
            quarto_system_runtime::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        Self {
            root: root.to_path_buf(),
            canonical_root,
            output_dir: None,
            size_cap: *PULL_SIZE_CAP,
        }
    }

    /// Refuse everything under the project's `output-dir`. Accepts an
    /// absolute path (under the root or the canonical root) or a
    /// project-relative one; an output directory that is the root itself or
    /// lies outside it adds nothing.
    pub fn with_output_dir(mut self, output_dir: &Path) -> Self {
        let rel = if output_dir.is_absolute() {
            output_dir
                .strip_prefix(&self.root)
                .or_else(|_| output_dir.strip_prefix(&self.canonical_root))
                .ok()
                .map(Path::to_path_buf)
        } else {
            Some(output_dir.to_path_buf())
        };
        self.output_dir = rel
            .and_then(|r| normalize_relative(&r).ok())
            .filter(|r| !r.as_os_str().is_empty());
        self
    }

    /// Override the pull size cap (tests).
    pub fn with_size_cap(mut self, cap: u64) -> Self {
        self.size_cap = cap;
        self
    }

    /// The project root this policy was built for.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The pull size cap in bytes.
    pub fn size_cap(&self) -> u64 {
        self.size_cap
    }

    /// Normalize a request path: strip a `/project/` prefix, refuse other
    /// absolute paths, resolve `.` and `..` lexically, refuse a climb above
    /// the root. The empty path is the root.
    pub fn normalize(&self, request: &Path) -> Result<PathBuf, Refusal> {
        let request = request.strip_prefix(VFS_PROJECT_PREFIX).unwrap_or(request);
        normalize_relative(request)
    }

    /// Admit a file for eager sync: checks 1 to 5, no size cap.
    pub fn admit(&self, request: &Path) -> Result<Admitted, Refusal> {
        let rel = self.normalize(request)?;
        let class = self.admit_lexically(&rel)?;
        let abs = self.contain(&rel)?;
        let meta = std::fs::metadata(&abs).map_err(|_| Refusal::NotFound)?;
        if !meta.is_file() {
            return Err(Refusal::WrongType);
        }
        Ok(Admitted {
            rel,
            abs,
            class,
            size: meta.len(),
        })
    }

    /// Admit a file for a pull: [`admit`](Self::admit) plus the size cap.
    pub fn admit_pull(&self, request: &Path) -> Result<Admitted, Refusal> {
        let admitted = self.admit(request)?;
        if admitted.size > self.size_cap {
            return Err(Refusal::TooLarge {
                size: admitted.size,
                cap: self.size_cap,
            });
        }
        Ok(admitted)
    }

    /// The checks that need no disk access beyond the provenance probe for
    /// sidecar directories: hidden, generated, class. `rel` must already be
    /// normalized. Callers that hold an event path rather than a request
    /// (the watcher) use this to filter without canonicalizing.
    pub fn admit_lexically(&self, rel: &Path) -> Result<Class, Refusal> {
        if is_hidden(rel) {
            return Err(Refusal::Hidden);
        }
        if let Some(parent) = rel.parent()
            && self.dir_is_generated(parent)
        {
            return Err(Refusal::Generated);
        }
        classify(rel).ok_or(Refusal::Extension)
    }

    /// Admit a directory, for a listing or an existence check: checks 1 to
    /// 4. Returns the normalized relative path and the canonical absolute
    /// path. The root itself is admitted.
    pub fn admit_dir(&self, request: &Path) -> Result<(PathBuf, PathBuf), Refusal> {
        let rel = self.normalize(request)?;
        self.admit_dir_lexically(&rel)?;
        let abs = self.contain(&rel)?;
        if !abs.is_dir() {
            return Err(Refusal::WrongType);
        }
        Ok((rel, abs))
    }

    /// The directory checks that need no canonicalization: hidden and
    /// generated. `rel` must already be normalized. The discovery walk uses
    /// this to prune directories before descending.
    pub fn admit_dir_lexically(&self, rel: &Path) -> Result<(), Refusal> {
        if is_hidden(rel) {
            return Err(Refusal::Hidden);
        }
        if self.dir_is_generated(rel) {
            return Err(Refusal::Generated);
        }
        Ok(())
    }

    /// Whether a project-relative directory is, or is inside, a generated
    /// directory under this policy (including the configured output
    /// directory).
    pub fn dir_is_generated(&self, rel_dir: &Path) -> bool {
        if let Some(out) = &self.output_dir
            && rel_dir.starts_with(out)
        {
            return true;
        }
        in_generated_dir(&self.root, rel_dir)
    }

    /// Whether an absolute event path (as the watcher reports it, under the
    /// root or the canonical root) sits inside a generated directory. A path
    /// outside the root is not this policy's to judge and returns `false`.
    pub fn event_in_generated_dir(&self, abs: &Path) -> bool {
        let Some(rel) = self.relative(abs) else {
            return false;
        };
        rel.parent().is_some_and(|dir| self.dir_is_generated(dir))
    }

    /// Whether an absolute event path passes the checks that need no
    /// canonicalization ([`admit_lexically`](Self::admit_lexically)). A
    /// path outside the root does not.
    pub fn admits_event(&self, abs: &Path) -> bool {
        self.relative(abs)
            .is_some_and(|rel| self.admit_lexically(rel).is_ok())
    }

    /// `abs` relative to the root, accepting either the root's own
    /// spelling or its canonical one (watchers report canonical paths;
    /// macOS spells temp directories both ways).
    pub fn relative<'a>(&self, abs: &'a Path) -> Option<&'a Path> {
        abs.strip_prefix(&self.canonical_root)
            .or_else(|_| abs.strip_prefix(&self.root))
            .ok()
    }

    /// Canonical containment (check 2): the canonical form of `rel` must
    /// stay under the canonical root.
    fn contain(&self, rel: &Path) -> Result<PathBuf, Refusal> {
        let abs = quarto_system_runtime::canonicalize(&self.root.join(rel))
            .map_err(|_| Refusal::NotFound)?;
        if !abs.starts_with(&self.canonical_root) {
            return Err(Refusal::OutsideRoot);
        }
        Ok(abs)
    }
}

/// Whether the project-relative path `rel` is, or is inside, a generated
/// directory: a name Quarto or a tool owns at any depth (`node_modules`,
/// `__pycache__`, `_freeze`, `site_libs`, `_site`, `_book`, `renv`, `venv`,
/// `target`), or a sidecar (`<stem>_files/`, `<stem>_cache/`) beside its
/// `<stem>.qmd`, `.md`, `.ipynb` or `.Rmd` on disk. Every component of
/// `rel` is tested, the last included; pass a file's parent to test the
/// directories a file sits in.
///
/// The configured `output-dir` is not known here; see
/// [`ExposurePolicy::dir_is_generated`].
pub fn in_generated_dir(root: &Path, rel: &Path) -> bool {
    let mut dir = root.to_path_buf();
    for component in rel.components() {
        let Component::Normal(os_name) = component else {
            continue;
        };
        if let Some(name) = os_name.to_str() {
            if OWNED_DIRS.contains(&name) {
                return true;
            }
            if let Some(stem) = sidecar_stem(name)
                && SIDECAR_SOURCE_EXTENSIONS
                    .iter()
                    .any(|ext| dir.join(format!("{stem}.{ext}")).is_file())
            {
                return true;
            }
        }
        dir.push(os_name);
    }
    false
}

/// The `<stem>` of a sidecar directory name, if `name` has that shape.
fn sidecar_stem(name: &str) -> Option<&str> {
    SIDECAR_SUFFIXES
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .filter(|stem| !stem.is_empty())
}

/// Whether any component of a normalized relative path starts with `.`.
fn is_hidden(rel: &Path) -> bool {
    rel.components().any(|c| match c {
        Component::Normal(name) => name.to_string_lossy().starts_with('.'),
        _ => false,
    })
}

/// Lexically normalize a path that must be relative: drop `.`, resolve
/// `..`, refuse a root, prefix or climb above the start.
fn normalize_relative(path: &Path) -> Result<PathBuf, Refusal> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => return Err(Refusal::Absolute),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return Err(Refusal::Traversal);
                }
            }
            Component::Normal(name) => out.push(name),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// A project root with a few admitted files.
    fn project() -> TempDir {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::write(root.join("index.qmd"), "# Hi").unwrap();
        fs::create_dir_all(root.join("styles")).unwrap();
        fs::write(root.join("styles/deck.scss"), "$x: 1;").unwrap();
        fs::write(root.join("refs.bib"), "@book{a,}").unwrap();
        temp
    }

    fn policy(temp: &TempDir) -> ExposurePolicy {
        ExposurePolicy::new(temp.path())
    }

    // ── 1. Lexical containment ──────────────────────────────────────

    #[test]
    fn lexical_containment() {
        let temp = project();
        let p = policy(&temp);
        assert_eq!(
            p.admit(Path::new("../outside.qmd")),
            Err(Refusal::Traversal)
        );
        assert_eq!(
            p.admit(Path::new("styles/../../outside.qmd")),
            Err(Refusal::Traversal)
        );
        assert_eq!(p.admit(Path::new("/etc/passwd")), Err(Refusal::Absolute));
        // `a/../b` inside the root is admitted, normalized.
        let a = p.admit(Path::new("styles/../index.qmd")).unwrap();
        assert_eq!(a.rel, PathBuf::from("index.qmd"));
        // The VFS `/project/` prefix is stripped.
        let a = p.admit(Path::new("/project/styles/deck.scss")).unwrap();
        assert_eq!(a.rel, PathBuf::from("styles/deck.scss"));
        assert_eq!(a.class, Class::Stylesheet);
        // ... but `/project/..` still cannot climb out.
        assert_eq!(
            p.admit(Path::new("/project/../x.qmd")),
            Err(Refusal::Traversal)
        );
        // `./` is harmless.
        assert!(p.admit(Path::new("./index.qmd")).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn windows_absolute_forms_are_refused() {
        let temp = project();
        let p = policy(&temp);
        assert_eq!(
            p.admit(Path::new(r"C:\Windows\win.ini")),
            Err(Refusal::Absolute)
        );
        assert_eq!(
            p.admit(Path::new(r"\\server\share\x.qmd")),
            Err(Refusal::Absolute)
        );
    }

    #[test]
    fn absent_file_is_not_found() {
        let temp = project();
        assert_eq!(
            policy(&temp).admit(Path::new("missing.qmd")),
            Err(Refusal::NotFound)
        );
    }

    #[test]
    fn a_directory_is_not_a_file() {
        let temp = project();
        fs::create_dir(temp.path().join("dir.qmd")).unwrap();
        assert_eq!(
            policy(&temp).admit(Path::new("dir.qmd")),
            Err(Refusal::WrongType)
        );
    }

    // ── 2. Canonical containment ────────────────────────────────────

    /// Create a symlink at `link` pointing to `target` (file or directory).
    #[cfg(unix)]
    fn symlink(target: &Path, link: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).unwrap();
        true
    }

    /// Symlink creation needs a privilege on Windows that CI and most
    /// developers lack; the tests that use it skip there.
    #[cfg(not(unix))]
    fn symlink(_target: &Path, _link: &Path) -> bool {
        false
    }

    #[test]
    fn a_symlink_pointing_outside_is_refused() {
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("secret.qmd"), "secret").unwrap();
        fs::create_dir(outside.path().join("dir")).unwrap();
        fs::write(outside.path().join("dir/inner.qmd"), "inner").unwrap();
        let temp = project();
        if !symlink(
            &outside.path().join("secret.qmd"),
            &temp.path().join("link.qmd"),
        ) {
            return;
        }
        symlink(&outside.path().join("dir"), &temp.path().join("linked"));
        let p = policy(&temp);
        assert_eq!(p.admit(Path::new("link.qmd")), Err(Refusal::OutsideRoot));
        assert_eq!(
            p.admit(Path::new("linked/inner.qmd")),
            Err(Refusal::OutsideRoot)
        );
        assert_eq!(p.admit_dir(Path::new("linked")), Err(Refusal::OutsideRoot));
    }

    #[test]
    fn a_symlink_pointing_inside_is_admitted() {
        let temp = project();
        if !symlink(
            &temp.path().join("index.qmd"),
            &temp.path().join("alias.qmd"),
        ) {
            return;
        }
        let a = policy(&temp).admit(Path::new("alias.qmd")).unwrap();
        assert_eq!(a.rel, PathBuf::from("alias.qmd"));
        assert!(a.abs.ends_with("index.qmd"));
    }

    // ── 3. Hidden components ────────────────────────────────────────

    #[test]
    fn hidden_components_below_the_root_are_refused() {
        let temp = project();
        fs::create_dir(temp.path().join(".quarto")).unwrap();
        fs::write(temp.path().join(".quarto/x.qmd"), "x").unwrap();
        fs::write(temp.path().join(".hidden.qmd"), "x").unwrap();
        fs::write(temp.path().join(".env"), "TOKEN=x").unwrap();
        let p = policy(&temp);
        assert_eq!(p.admit(Path::new(".quarto/x.qmd")), Err(Refusal::Hidden));
        assert_eq!(p.admit(Path::new(".hidden.qmd")), Err(Refusal::Hidden));
        assert_eq!(p.admit(Path::new(".env")), Err(Refusal::Hidden));
        assert_eq!(p.admit_dir(Path::new(".quarto")), Err(Refusal::Hidden));
        // A missing hidden path is refused, not reported as absent.
        assert_eq!(p.admit(Path::new(".git/config")), Err(Refusal::Hidden));
    }

    #[test]
    fn a_root_under_a_hidden_directory_is_fine() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join(".worktrees/proj");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("index.qmd"), "# Hi").unwrap();
        let p = ExposurePolicy::new(&root);
        assert!(p.admit(Path::new("index.qmd")).is_ok());
        assert!(p.admit_dir(Path::new("")).is_ok());
    }

    // ── 4. Generated by provenance ──────────────────────────────────

    #[test]
    fn a_sidecar_is_generated_only_beside_its_source() {
        let temp = project();
        let root = temp.path();
        fs::create_dir_all(root.join("index_files/libs")).unwrap();
        fs::write(root.join("index_files/x.css"), "a{}").unwrap();
        fs::create_dir_all(root.join("static_files")).unwrap();
        fs::write(root.join("static_files/brand.scss"), "$y: 2;").unwrap();
        fs::create_dir_all(root.join("posts/one_cache")).unwrap();
        fs::write(root.join("posts/one.ipynb"), "{}").unwrap();
        fs::write(root.join("posts/one_cache/x.json"), "{}").unwrap();
        let p = policy(&temp);
        // `index.qmd` exists beside `index_files/`.
        assert_eq!(
            p.admit(Path::new("index_files/x.css")),
            Err(Refusal::Generated)
        );
        assert_eq!(
            p.admit_dir(Path::new("index_files/libs")),
            Err(Refusal::Generated)
        );
        // No `static.qmd`: an author's directory.
        assert_eq!(
            p.admit(Path::new("static_files/brand.scss")).unwrap().class,
            Class::Stylesheet
        );
        // Provenance from a notebook, nested.
        assert_eq!(
            p.admit(Path::new("posts/one_cache/x.json")),
            Err(Refusal::Generated)
        );
        // Without its source, the same name is admitted.
        fs::remove_file(root.join("index.qmd")).unwrap();
        assert!(p.admit(Path::new("index_files/x.css")).is_ok());
    }

    #[test]
    fn owned_dirs_are_generated_at_any_depth() {
        let temp = project();
        let root = temp.path();
        for rel in [
            "site_libs/quarto.css",
            "deep/er/site_libs/x.css",
            "node_modules/pkg/README.md",
            "_freeze/index/x.json",
            "__pycache__/m.txt",
            "_site/index.html",
        ] {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "x").unwrap();
            assert_eq!(
                policy(&temp).admit(Path::new(rel)),
                Err(Refusal::Generated),
                "{rel}"
            );
        }
    }

    #[test]
    fn the_configured_output_dir_is_generated() {
        let temp = project();
        let root = temp.path();
        fs::create_dir_all(root.join("public/css")).unwrap();
        fs::write(root.join("public/css/x.css"), "a{}").unwrap();
        assert!(policy(&temp).admit(Path::new("public/css/x.css")).is_ok());
        for out in [PathBuf::from("public"), root.join("public")] {
            let p = policy(&temp).with_output_dir(&out);
            assert_eq!(
                p.admit(Path::new("public/css/x.css")),
                Err(Refusal::Generated),
                "{}",
                out.display()
            );
            assert_eq!(p.admit_dir(Path::new("public")), Err(Refusal::Generated));
            // Siblings are untouched.
            assert!(p.admit(Path::new("index.qmd")).is_ok());
        }
        // An output-dir equal to the root refuses nothing.
        let p = policy(&temp).with_output_dir(root);
        assert!(p.admit(Path::new("index.qmd")).is_ok());
    }

    #[test]
    fn event_paths_are_judged_relative_to_either_root_spelling() {
        let temp = project();
        let root = temp.path();
        fs::create_dir_all(root.join("index_files")).unwrap();
        let p = policy(&temp).with_output_dir(Path::new("public"));
        let canonical = quarto_system_runtime::canonicalize(root).unwrap();
        for base in [root.to_path_buf(), canonical] {
            assert!(p.event_in_generated_dir(&base.join("index_files/a.png")));
            assert!(p.event_in_generated_dir(&base.join("public/a.png")));
            assert!(p.event_in_generated_dir(&base.join("x/site_libs/a.css")));
            assert!(!p.event_in_generated_dir(&base.join("img/a.png")));
            assert!(!p.event_in_generated_dir(&base.join("index.qmd")));
        }
        assert!(!p.event_in_generated_dir(Path::new("/elsewhere/site_libs/a.css")));
    }

    #[test]
    fn admits_event_applies_the_lexical_checks() {
        let temp = project();
        let root = temp.path();
        fs::create_dir_all(root.join("index_files")).unwrap();
        let p = policy(&temp);
        assert!(p.admits_event(&root.join("refs.bib")));
        assert!(p.admits_event(&root.join("new/never-existed.scss")));
        assert!(!p.admits_event(&root.join(".quarto/preview-out/x.png")));
        assert!(!p.admits_event(&root.join("index_files/a.png")));
        assert!(!p.admits_event(&root.join("a.exe")));
        assert!(!p.admits_event(root));
        assert!(!p.admits_event(Path::new("/elsewhere/a.qmd")));
    }

    // ── 5. Extension allow-list ─────────────────────────────────────

    #[test]
    fn classify_by_class() {
        let cases: &[(&str, Option<Class>)] = &[
            ("a.qmd", Some(Class::Source)),
            ("a.md", Some(Class::Source)),
            ("a.ipynb", Some(Class::Source)),
            ("a.Rmd", Some(Class::Source)),
            ("_quarto.yml", Some(Class::Config)),
            ("_quarto-preview.yml", Some(Class::Config)),
            ("sub/_metadata.yaml", Some(Class::Config)),
            ("_brand.yml", Some(Class::Config)),
            ("_variables.yml", Some(Class::Config)),
            ("a.css", Some(Class::Stylesheet)),
            ("a.SCSS", Some(Class::Stylesheet)),
            ("a.sass", Some(Class::Stylesheet)),
            ("a.tsx", Some(Class::Component)),
            ("refs.bib", Some(Class::Bibliography)),
            ("apa.csl", Some(Class::Bibliography)),
            ("data.csv", Some(Class::Data)),
            ("refs.json", Some(Class::Data)),
            ("other.yml", Some(Class::Data)),
            ("header.html", Some(Class::Template)),
            ("filter.lua", Some(Class::Template)),
            ("logo.png", Some(Class::Binary)),
            ("font.woff2", Some(Class::Binary)),
            ("_extensions/acme/filter.lua", Some(Class::Extension)),
            ("_extensions/acme/anything.weird", Some(Class::Extension)),
            ("_extensions/acme/server.pem", None),
            ("id_rsa", None),
            ("cert.key", None),
            ("a.exe", None),
            ("a.ts", None),
            ("Makefile", None),
        ];
        for (rel, want) in cases {
            assert_eq!(classify(Path::new(rel)), *want, "{rel}");
        }
    }

    #[test]
    fn unknown_extensions_and_dotenv_are_refused() {
        let temp = project();
        let root = temp.path();
        fs::write(root.join("a.exe"), "x").unwrap();
        fs::write(root.join("x.env"), "x").unwrap();
        fs::write(root.join("x.sass"), "x").unwrap();
        let p = policy(&temp);
        assert_eq!(p.admit(Path::new("a.exe")), Err(Refusal::Extension));
        assert_eq!(p.admit(Path::new("x.env")), Err(Refusal::Extension));
        assert_eq!(
            p.admit(Path::new("x.sass")).unwrap().class,
            Class::Stylesheet
        );
        // Refused before existence is checked.
        assert_eq!(p.admit(Path::new("missing.exe")), Err(Refusal::Extension));
    }

    // ── 6. Size cap ─────────────────────────────────────────────────

    #[test]
    fn the_pull_cap_refuses_with_a_typed_reason() {
        let temp = project();
        fs::write(temp.path().join("big.png"), vec![0u8; 64]).unwrap();
        let p = policy(&temp).with_size_cap(16);
        assert_eq!(
            p.admit_pull(Path::new("big.png")),
            Err(Refusal::TooLarge { size: 64, cap: 16 })
        );
        // Eager sync has no cap.
        assert_eq!(p.admit(Path::new("big.png")).unwrap().size, 64);
        assert!(p.admit_pull(Path::new("refs.bib")).is_ok());
    }

    #[test]
    fn the_default_cap_is_the_request_builders_per_file_limit() {
        let temp = project();
        assert_eq!(policy(&temp).size_cap(), 25 * 1024 * 1024);
    }

    // ── Directories ─────────────────────────────────────────────────

    #[test]
    fn admit_dir_checks_type_and_existence() {
        let temp = project();
        let p = policy(&temp);
        let (rel, abs) = p.admit_dir(Path::new("/project/styles")).unwrap();
        assert_eq!(rel, PathBuf::from("styles"));
        assert!(abs.ends_with("styles"));
        assert_eq!(p.admit_dir(Path::new("nope")), Err(Refusal::NotFound));
        assert_eq!(p.admit_dir(Path::new("index.qmd")), Err(Refusal::WrongType));
        assert_eq!(p.admit_dir(Path::new("..")), Err(Refusal::Traversal));
    }
}
