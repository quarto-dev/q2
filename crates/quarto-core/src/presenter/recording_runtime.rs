/*
 * presenter/recording_runtime.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Test support: a SystemRuntime that records writes and touches no files.
 */

//! A [`SystemRuntime`] that records `dir_create` and `file_write` calls and performs
//! no I/O, for proving that figures are written through the runtime.

use std::path::PathBuf;

use quarto_system_runtime::SystemRuntime;

#[derive(Debug, PartialEq)]
pub(crate) enum Call {
    DirCreate(PathBuf, bool),
    FileWrite(PathBuf, Vec<u8>),
}

/// A runtime that only records `dir_create` and `file_write`; it touches no files.
#[derive(Default)]
pub(crate) struct RecordingRuntime {
    pub(crate) calls: std::sync::Mutex<Vec<Call>>,
}

#[async_trait::async_trait]
impl SystemRuntime for RecordingRuntime {
    fn file_read(&self, _path: &std::path::Path) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
        Ok(vec![])
    }

    fn file_write(
        &self,
        path: &std::path::Path,
        contents: &[u8],
    ) -> quarto_system_runtime::RuntimeResult<()> {
        self.calls
            .lock()
            .unwrap()
            .push(Call::FileWrite(path.to_path_buf(), contents.to_vec()));
        Ok(())
    }

    fn path_exists(
        &self,
        _path: &std::path::Path,
        _kind: Option<quarto_system_runtime::PathKind>,
    ) -> quarto_system_runtime::RuntimeResult<bool> {
        Ok(true)
    }

    fn canonicalize(
        &self,
        path: &std::path::Path,
    ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
        Ok(path.to_path_buf())
    }

    fn path_metadata(
        &self,
        _path: &std::path::Path,
    ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata> {
        unimplemented!()
    }

    fn file_copy(
        &self,
        _src: &std::path::Path,
        _dst: &std::path::Path,
    ) -> quarto_system_runtime::RuntimeResult<()> {
        Ok(())
    }

    fn path_rename(
        &self,
        _old: &std::path::Path,
        _new: &std::path::Path,
    ) -> quarto_system_runtime::RuntimeResult<()> {
        Ok(())
    }

    fn file_remove(&self, _path: &std::path::Path) -> quarto_system_runtime::RuntimeResult<()> {
        Ok(())
    }

    fn dir_create(
        &self,
        path: &std::path::Path,
        recursive: bool,
    ) -> quarto_system_runtime::RuntimeResult<()> {
        self.calls
            .lock()
            .unwrap()
            .push(Call::DirCreate(path.to_path_buf(), recursive));
        Ok(())
    }

    fn dir_remove(
        &self,
        _path: &std::path::Path,
        _recursive: bool,
    ) -> quarto_system_runtime::RuntimeResult<()> {
        Ok(())
    }

    fn dir_list(
        &self,
        _path: &std::path::Path,
    ) -> quarto_system_runtime::RuntimeResult<Vec<PathBuf>> {
        Ok(vec![])
    }

    fn cwd(&self) -> quarto_system_runtime::RuntimeResult<PathBuf> {
        Ok(PathBuf::from("/"))
    }

    fn temp_dir(
        &self,
        _template: &str,
    ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::TempDir> {
        Err(quarto_system_runtime::RuntimeError::NotSupported(
            "temp_dir".to_string(),
        ))
    }

    fn exec_pipe(
        &self,
        _command: &str,
        _args: &[&str],
        _stdin: &[u8],
    ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
        Ok(vec![])
    }

    fn exec_command(
        &self,
        _command: &str,
        _args: &[&str],
        _stdin: Option<&[u8]>,
    ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput> {
        Ok(quarto_system_runtime::CommandOutput {
            code: 0,
            stdout: vec![],
            stderr: vec![],
        })
    }

    fn env_get(&self, _name: &str) -> quarto_system_runtime::RuntimeResult<Option<String>> {
        Ok(None)
    }

    fn env_all(
        &self,
    ) -> quarto_system_runtime::RuntimeResult<std::collections::HashMap<String, String>> {
        Ok(std::collections::HashMap::new())
    }

    async fn fetch_url(
        &self,
        _url: &str,
    ) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
        Err(quarto_system_runtime::RuntimeError::NotSupported(
            "fetch not implemented".to_string(),
        ))
    }

    fn os_name(&self) -> &'static str {
        "mock"
    }

    fn arch(&self) -> &'static str {
        "mock"
    }

    fn cpu_time(&self) -> quarto_system_runtime::RuntimeResult<u64> {
        Ok(0)
    }

    fn xdg_dir(
        &self,
        _kind: quarto_system_runtime::XdgDirKind,
        _subpath: Option<&std::path::Path>,
    ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
        Ok(PathBuf::from("/xdg"))
    }

    fn stdout_write(&self, _data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
        Ok(())
    }

    fn stderr_write(&self, _data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
        Ok(())
    }
}
