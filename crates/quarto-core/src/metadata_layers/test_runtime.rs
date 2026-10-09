//! A native runtime that reports configurable runtime metadata, for tests of
//! the layer collector and the lookups built on it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use quarto_system_runtime::{
    CommandOutput, NativeRuntime, PathKind, PathMetadata, RuntimeResult, SassOutput, SystemRuntime,
    TempDir, XdgDirKind,
};

/// `NativeRuntime` plus a fixed `runtime_metadata()`.
pub(crate) struct MetaRuntime {
    inner: NativeRuntime,
    metadata: Option<serde_json::Value>,
}

impl MetaRuntime {
    pub(crate) fn new(metadata: Option<serde_json::Value>) -> Self {
        Self {
            inner: NativeRuntime::new(),
            metadata,
        }
    }
}

#[async_trait::async_trait]
impl SystemRuntime for MetaRuntime {
    fn file_read(&self, path: &Path) -> RuntimeResult<Vec<u8>> {
        self.inner.file_read(path)
    }
    fn file_write(&self, path: &Path, contents: &[u8]) -> RuntimeResult<()> {
        self.inner.file_write(path, contents)
    }
    fn path_exists(&self, path: &Path, kind: Option<PathKind>) -> RuntimeResult<bool> {
        self.inner.path_exists(path, kind)
    }
    fn canonicalize(&self, path: &Path) -> RuntimeResult<PathBuf> {
        self.inner.canonicalize(path)
    }
    fn path_metadata(&self, path: &Path) -> RuntimeResult<PathMetadata> {
        self.inner.path_metadata(path)
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
        self.metadata.clone()
    }
    async fn fetch_url(&self, url: &str) -> RuntimeResult<(Vec<u8>, String)> {
        self.inner.fetch_url(url).await
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
    fn xdg_dir(&self, kind: XdgDirKind, subpath: Option<&Path>) -> RuntimeResult<PathBuf> {
        self.inner.xdg_dir(kind, subpath)
    }
    fn stdout_write(&self, data: &[u8]) -> RuntimeResult<()> {
        self.inner.stdout_write(data)
    }
    fn stderr_write(&self, data: &[u8]) -> RuntimeResult<()> {
        self.inner.stderr_write(data)
    }
    async fn compile_sass(
        &self,
        scss: &str,
        load_paths: &[PathBuf],
        minified: bool,
    ) -> RuntimeResult<SassOutput> {
        self.inner.compile_sass(scss, load_paths, minified).await
    }
}
