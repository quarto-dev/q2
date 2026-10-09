//! A [`SystemRuntime`] that answers reads of chosen files from memory and
//! forwards everything else (epic E11, P10-D1).
//!
//! The preview server's whole-book download renders the book on disk but the
//! active chapter from the editor's current text. Pass 1 and the chapter
//! renders both read chapters through the runtime, so one overlay serves both.
//!
//! Every trait method is written out and forwards to the inner runtime, so a
//! method the inner runtime overrides is never replaced by the trait default.
//! Five methods see the overlay: `file_read`, `file_read_string`, `is_file`,
//! `is_dir` and `path_metadata`. The trait defaults of `file_read_string`,
//! `is_file` and `is_dir` route through `self`, and `NativeRuntime` may
//! override them, so forwarding them plainly would bypass the overlay.
//! `tests::every_trait_method_is_written_out` checks that no method is missing
//! when the trait grows.

use std::collections::HashMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use quarto_system_runtime::{
    CacheEntryInfo, CommandOutput, FetchPolicy, PathKind, PathMetadata, RuntimeError,
    RuntimeResult, SassOutput, SystemRuntime, TempDir, XdgDirKind,
};

/// `inner`, with the bytes of some absolute paths replaced.
pub struct OverlayRuntime {
    inner: Arc<dyn SystemRuntime>,
    files: HashMap<PathBuf, Vec<u8>>,
}

/// Lexically normalise an absolute path: drop `.` and fold `..`. Overlay keys
/// and lookups both go through it, so `/p/./a/../b.qmd` finds `/p/b.qmd`.
pub fn normalize_overlay_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

impl OverlayRuntime {
    /// `files` maps absolute paths to the bytes they should read as.
    pub fn new(inner: Arc<dyn SystemRuntime>, files: HashMap<PathBuf, Vec<u8>>) -> Self {
        let files = files
            .into_iter()
            .map(|(path, bytes)| (normalize_overlay_path(&path), bytes))
            .collect();
        Self { inner, files }
    }

    fn overlaid(&self, path: &Path) -> Option<&Vec<u8>> {
        if self.files.is_empty() || !path.is_absolute() {
            return None;
        }
        self.files.get(&normalize_overlay_path(path))
    }
}

#[async_trait::async_trait]
impl SystemRuntime for OverlayRuntime {
    // The five methods that see the overlay.

    fn file_read(&self, path: &Path) -> RuntimeResult<Vec<u8>> {
        match self.overlaid(path) {
            Some(bytes) => Ok(bytes.clone()),
            None => self.inner.file_read(path),
        }
    }

    fn file_read_string(&self, path: &Path) -> RuntimeResult<String> {
        match self.overlaid(path) {
            Some(bytes) => String::from_utf8(bytes.clone()).map_err(|e| {
                RuntimeError::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid UTF-8 in file: {e}"),
                ))
            }),
            None => self.inner.file_read_string(path),
        }
    }

    fn is_file(&self, path: &Path) -> RuntimeResult<bool> {
        match self.overlaid(path) {
            Some(_) => Ok(true),
            None => self.inner.is_file(path),
        }
    }

    fn is_dir(&self, path: &Path) -> RuntimeResult<bool> {
        match self.overlaid(path) {
            Some(_) => Ok(false),
            None => self.inner.is_dir(path),
        }
    }

    fn path_metadata(&self, path: &Path) -> RuntimeResult<PathMetadata> {
        let Some(bytes) = self.overlaid(path) else {
            return self.inner.path_metadata(path);
        };
        let size = bytes.len() as u64;
        Ok(match self.inner.path_metadata(path) {
            Ok(meta) => PathMetadata { size, ..meta },
            Err(_) => PathMetadata {
                kind: PathKind::File,
                size,
                modified: None,
                accessed: None,
                readonly: false,
            },
        })
    }

    // Everything else forwards.

    fn file_write(&self, path: &Path, contents: &[u8]) -> RuntimeResult<()> {
        self.inner.file_write(path, contents)
    }

    fn path_exists(&self, path: &Path, kind: Option<PathKind>) -> RuntimeResult<bool> {
        self.inner.path_exists(path, kind)
    }

    fn canonicalize(&self, path: &Path) -> RuntimeResult<PathBuf> {
        self.inner.canonicalize(path)
    }

    fn file_copy(&self, src: &Path, dst: &Path) -> RuntimeResult<()> {
        self.inner.file_copy(src, dst)
    }

    fn path_rename(&self, old: &Path, new: &Path) -> RuntimeResult<()> {
        self.inner.path_rename(old, new)
    }

    fn file_remove(&self, path: &Path) -> RuntimeResult<()> {
        self.inner.file_remove(path)
    }

    fn dir_create(&self, path: &Path, recursive: bool) -> RuntimeResult<()> {
        self.inner.dir_create(path, recursive)
    }

    fn dir_remove(&self, path: &Path, recursive: bool) -> RuntimeResult<()> {
        self.inner.dir_remove(path, recursive)
    }

    fn dir_list(&self, path: &Path) -> RuntimeResult<Vec<PathBuf>> {
        self.inner.dir_list(path)
    }

    fn cwd(&self) -> RuntimeResult<PathBuf> {
        self.inner.cwd()
    }

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
mod tests {
    use super::*;
    use quarto_system_runtime::NativeRuntime;
    use std::collections::BTreeSet;

    /// Method names declared inside `pub trait SystemRuntime { .. }`.
    fn trait_methods(src: &str) -> BTreeSet<String> {
        let start = src.find("pub trait SystemRuntime").expect("trait");
        let body = &src[start..];
        let end = body.find("\n}\n").expect("end of trait");
        method_names(&body[..end])
    }

    /// Method names declared at four-space indent (`fn` or `async fn`).
    fn method_names(body: &str) -> BTreeSet<String> {
        body.lines()
            .filter_map(|line| {
                let rest = line.strip_prefix("    ")?;
                if rest.starts_with(' ') {
                    return None;
                }
                let rest = rest.strip_prefix("async ").unwrap_or(rest);
                let rest = rest.strip_prefix("fn ")?;
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                Some(name)
            })
            .collect()
    }

    /// The `impl` block of this file, up to the tests module.
    fn impl_methods() -> BTreeSet<String> {
        let src = include_str!("overlay_runtime.rs");
        let start = src.find("impl SystemRuntime for OverlayRuntime").unwrap();
        let end = src.find("#[cfg(test)]").unwrap();
        method_names(&src[start..end])
    }

    #[test]
    fn every_trait_method_is_written_out() {
        let traits = include_str!("../../quarto-system-runtime/src/traits.rs");
        let in_trait = trait_methods(traits);
        assert!(in_trait.len() > 40, "parsed {} methods", in_trait.len());
        let in_impl = impl_methods();
        let missing: Vec<_> = in_trait.difference(&in_impl).collect();
        assert!(
            missing.is_empty(),
            "OverlayRuntime does not forward {missing:?}; a trait default would replace the \
             inner runtime's override"
        );
        let extra: Vec<_> = in_impl.difference(&in_trait).collect();
        assert!(extra.is_empty(), "not trait methods: {extra:?}");
    }

    fn overlay(dir: &Path, chapter: &str, text: &str) -> OverlayRuntime {
        let mut files = HashMap::new();
        files.insert(dir.join(chapter), text.as_bytes().to_vec());
        OverlayRuntime::new(Arc::new(NativeRuntime::new()), files)
    }

    #[test]
    fn overlay_bytes_reach_every_read_path() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        std::fs::write(dir.join("a.qmd"), "on disk").unwrap();
        std::fs::write(dir.join("b.qmd"), "other").unwrap();
        let rt = overlay(&dir, "a.qmd", "edited");

        let a = dir.join("a.qmd");
        assert_eq!(rt.file_read(&a).unwrap(), b"edited");
        assert_eq!(rt.file_read_string(&a).unwrap(), "edited");
        assert!(rt.is_file(&a).unwrap());
        assert!(!rt.is_dir(&a).unwrap());
        assert_eq!(rt.path_metadata(&a).unwrap().size, 6);
        // An un-normalised spelling of the same file.
        let odd = dir.join("sub").join("..").join(".").join("a.qmd");
        assert_eq!(rt.file_read_string(&odd).unwrap(), "edited");

        // Every other path forwards, including the sibling chapter.
        let b = dir.join("b.qmd");
        assert_eq!(rt.file_read_string(&b).unwrap(), "other");
        assert_eq!(rt.path_metadata(&b).unwrap().size, 5);
        assert!(rt.is_dir(&dir).unwrap());
        assert!(!rt.is_file(&dir.join("missing.qmd")).unwrap());
        assert!(rt.file_read(&dir.join("missing.qmd")).is_err());
    }

    #[test]
    fn a_non_utf8_overlay_is_an_error_from_the_string_read_only() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        let mut files = HashMap::new();
        files.insert(dir.join("a.qmd"), vec![0xff, 0xfe]);
        let rt = OverlayRuntime::new(Arc::new(NativeRuntime::new()), files);
        assert!(rt.file_read_string(&dir.join("a.qmd")).is_err());
        assert_eq!(rt.file_read(&dir.join("a.qmd")).unwrap(), vec![0xff, 0xfe]);
    }

    #[test]
    fn writes_and_listings_forward() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        let rt = overlay(&dir, "a.qmd", "x");
        rt.file_write(&dir.join("w.txt"), b"w").unwrap();
        assert_eq!(std::fs::read(dir.join("w.txt")).unwrap(), b"w");
        assert!(
            rt.dir_list(&dir)
                .unwrap()
                .iter()
                .any(|p| p.ends_with("w.txt"))
        );
    }
}
