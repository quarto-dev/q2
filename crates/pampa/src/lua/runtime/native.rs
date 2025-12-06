/*
 * lua/runtime/native.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * NativeRuntime implementation for Lua filters.
 *
 * This runtime provides full system access using std:
 * - std::fs for file operations
 * - std::process for command execution
 * - std::env for environment access
 *
 * This is the default runtime for native (non-WASM) targets.
 */

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::traits::{
    CommandOutput, LuaRuntime, PathKind, PathMetadata, RuntimeError, RuntimeResult, TempDir,
    XdgDirKind,
};

/// Native runtime with full system access.
///
/// This is the default runtime for trusted filters on native targets.
/// It provides unrestricted access to the filesystem, process execution,
/// environment variables, and network (if enabled).
#[derive(Debug, Default)]
pub struct NativeRuntime {
    // Future: could add HTTP client for network operations
    // #[cfg(feature = "network")]
    // http_client: Option<reqwest::blocking::Client>,
}

impl NativeRuntime {
    /// Create a new NativeRuntime with default settings.
    pub fn new() -> Self {
        Self::default()
    }
}

impl LuaRuntime for NativeRuntime {
    // ═══════════════════════════════════════════════════════════════════════
    // FILE OPERATIONS
    // ═══════════════════════════════════════════════════════════════════════

    fn file_read(&self, path: &Path) -> RuntimeResult<Vec<u8>> {
        fs::read(path).map_err(RuntimeError::from)
    }

    fn file_write(&self, path: &Path, contents: &[u8]) -> RuntimeResult<()> {
        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, contents).map_err(RuntimeError::from)
    }

    fn path_exists(&self, path: &Path, kind: Option<PathKind>) -> RuntimeResult<bool> {
        if !path.exists() {
            return Ok(false);
        }

        match kind {
            None => Ok(true),
            Some(PathKind::File) => Ok(path.is_file()),
            Some(PathKind::Directory) => Ok(path.is_dir()),
            Some(PathKind::Symlink) => Ok(path.symlink_metadata()?.file_type().is_symlink()),
        }
    }

    fn path_metadata(&self, path: &Path) -> RuntimeResult<PathMetadata> {
        let metadata = fs::metadata(path)?;
        let file_type = metadata.file_type();

        let kind = if file_type.is_file() {
            PathKind::File
        } else if file_type.is_dir() {
            PathKind::Directory
        } else {
            PathKind::Symlink
        };

        Ok(PathMetadata {
            kind,
            size: metadata.len(),
            modified: metadata.modified().ok(),
            accessed: metadata.accessed().ok(),
            readonly: metadata.permissions().readonly(),
        })
    }

    fn file_copy(&self, src: &Path, dst: &Path) -> RuntimeResult<()> {
        // Create parent directories if they don't exist
        if let Some(parent) = dst.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::copy(src, dst)?;
        Ok(())
    }

    fn path_rename(&self, old: &Path, new: &Path) -> RuntimeResult<()> {
        // Create parent directories if they don't exist
        if let Some(parent) = new.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::rename(old, new).map_err(RuntimeError::from)
    }

    fn file_remove(&self, path: &Path) -> RuntimeResult<()> {
        fs::remove_file(path).map_err(RuntimeError::from)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // DIRECTORY OPERATIONS
    // ═══════════════════════════════════════════════════════════════════════

    fn dir_create(&self, path: &Path, recursive: bool) -> RuntimeResult<()> {
        if recursive {
            fs::create_dir_all(path).map_err(RuntimeError::from)
        } else {
            fs::create_dir(path).map_err(RuntimeError::from)
        }
    }

    fn dir_remove(&self, path: &Path, recursive: bool) -> RuntimeResult<()> {
        if recursive {
            fs::remove_dir_all(path).map_err(RuntimeError::from)
        } else {
            fs::remove_dir(path).map_err(RuntimeError::from)
        }
    }

    fn dir_list(&self, path: &Path) -> RuntimeResult<Vec<PathBuf>> {
        let entries: Result<Vec<_>, _> = fs::read_dir(path)?
            .map(|entry| entry.map(|e| e.path()))
            .collect();
        entries.map_err(RuntimeError::from)
    }

    fn cwd(&self) -> RuntimeResult<PathBuf> {
        std::env::current_dir().map_err(RuntimeError::from)
    }

    fn temp_dir(&self, template: &str) -> RuntimeResult<TempDir> {
        // Create a unique temp directory in the system temp directory
        let base = std::env::temp_dir();
        let unique_name = format!(
            "{}_{}",
            template,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let path = base.join(unique_name);
        fs::create_dir_all(&path)?;
        Ok(TempDir::new(path))
    }

    // ═══════════════════════════════════════════════════════════════════════
    // PROCESS EXECUTION
    // ═══════════════════════════════════════════════════════════════════════

    fn exec_pipe(&self, command: &str, args: &[&str], stdin: &[u8]) -> RuntimeResult<Vec<u8>> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Write stdin
        if let Some(mut stdin_pipe) = child.stdin.take() {
            stdin_pipe.write_all(stdin)?;
        }

        let output = child.wait_with_output()?;

        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(RuntimeError::ProcessFailed {
                code: output.status.code().unwrap_or(-1),
                message: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        }
    }

    fn exec_command(
        &self,
        command: &str,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> RuntimeResult<CommandOutput> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Write stdin if provided
        if let Some(input) = stdin {
            if let Some(mut stdin_pipe) = child.stdin.take() {
                stdin_pipe.write_all(input)?;
            }
        }

        let output = child.wait_with_output()?;

        Ok(CommandOutput {
            code: output.status.code().unwrap_or(-1),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ENVIRONMENT
    // ═══════════════════════════════════════════════════════════════════════

    fn env_get(&self, name: &str) -> RuntimeResult<Option<String>> {
        Ok(std::env::var(name).ok())
    }

    fn env_all(&self) -> RuntimeResult<HashMap<String, String>> {
        Ok(std::env::vars().collect())
    }

    // ═══════════════════════════════════════════════════════════════════════
    // NETWORK
    // ═══════════════════════════════════════════════════════════════════════

    fn fetch_url(&self, _url: &str) -> RuntimeResult<(Vec<u8>, String)> {
        // Network support requires an HTTP client dependency
        // For now, return NotSupported until we add reqwest or similar
        Err(RuntimeError::NotSupported(
            "Network fetch not yet implemented. Consider using pandoc.mediabag.fetch instead."
                .to_string(),
        ))

        // Future implementation with reqwest:
        // let response = self.http_client.get(url).send()?;
        // let mime_type = response.headers()
        //     .get("content-type")
        //     .and_then(|v| v.to_str().ok())
        //     .unwrap_or("application/octet-stream")
        //     .to_string();
        // let content = response.bytes()?.to_vec();
        // Ok((content, mime_type))
    }

    // ═══════════════════════════════════════════════════════════════════════
    // SYSTEM INFO
    // ═══════════════════════════════════════════════════════════════════════

    fn os_name(&self) -> &'static str {
        std::env::consts::OS
    }

    fn arch(&self) -> &'static str {
        std::env::consts::ARCH
    }

    fn cpu_time(&self) -> RuntimeResult<u64> {
        // CPU time is platform-specific and not easily available in std
        // For now, return an approximation based on elapsed time
        // A more accurate implementation would use platform-specific APIs
        Err(RuntimeError::NotSupported(
            "CPU time measurement not available on this platform".to_string(),
        ))
    }

    fn xdg_dir(&self, kind: XdgDirKind, subpath: Option<&Path>) -> RuntimeResult<PathBuf> {
        let base = match kind {
            XdgDirKind::Config => {
                // $XDG_CONFIG_HOME or ~/.config
                std::env::var("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| {
                        dirs::home_dir()
                            .unwrap_or_else(|| PathBuf::from("~"))
                            .join(".config")
                    })
            }
            XdgDirKind::Data => {
                // $XDG_DATA_HOME or ~/.local/share
                std::env::var("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| {
                        dirs::home_dir()
                            .unwrap_or_else(|| PathBuf::from("~"))
                            .join(".local/share")
                    })
            }
            XdgDirKind::Cache => {
                // $XDG_CACHE_HOME or ~/.cache
                std::env::var("XDG_CACHE_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| {
                        dirs::home_dir()
                            .unwrap_or_else(|| PathBuf::from("~"))
                            .join(".cache")
                    })
            }
            XdgDirKind::State => {
                // $XDG_STATE_HOME or ~/.local/state
                std::env::var("XDG_STATE_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| {
                        dirs::home_dir()
                            .unwrap_or_else(|| PathBuf::from("~"))
                            .join(".local/state")
                    })
            }
        };

        match subpath {
            Some(sub) => Ok(base.join(sub)),
            None => Ok(base),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // OUTPUT
    // ═══════════════════════════════════════════════════════════════════════

    fn stdout_write(&self, data: &[u8]) -> RuntimeResult<()> {
        io::stdout().write_all(data).map_err(RuntimeError::from)
    }

    fn stderr_write(&self, data: &[u8]) -> RuntimeResult<()> {
        io::stderr().write_all(data).map_err(RuntimeError::from)
    }
}

// Fallback for xdg_dir when dirs crate is not available
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map(PathBuf::from)
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir as TempFileTempDir;

    fn runtime() -> NativeRuntime {
        NativeRuntime::new()
    }

    #[test]
    fn test_file_read_write() {
        let temp = TempFileTempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        let rt = runtime();

        // Write
        rt.file_write(&path, b"hello world").unwrap();
        assert!(path.exists());

        // Read
        let content = rt.file_read(&path).unwrap();
        assert_eq!(content, b"hello world");

        // Read as string
        let content_str = rt.file_read_string(&path).unwrap();
        assert_eq!(content_str, "hello world");
    }

    #[test]
    fn test_file_write_creates_parent_dirs() {
        let temp = TempFileTempDir::new().unwrap();
        let path = temp.path().join("nested/dir/test.txt");
        let rt = runtime();

        rt.file_write(&path, b"content").unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_file_read_nonexistent() {
        let rt = runtime();
        let result = rt.file_read(Path::new("/nonexistent/path/file.txt"));
        assert!(result.is_err());
    }

    #[test]
    fn test_path_exists() {
        let temp = TempFileTempDir::new().unwrap();
        let file_path = temp.path().join("file.txt");
        let dir_path = temp.path().join("subdir");
        let rt = runtime();

        // Create file and directory
        fs::write(&file_path, "content").unwrap();
        fs::create_dir(&dir_path).unwrap();

        // Test exists without type filter
        assert!(rt.path_exists(&file_path, None).unwrap());
        assert!(rt.path_exists(&dir_path, None).unwrap());
        assert!(
            !rt.path_exists(&temp.path().join("nonexistent"), None)
                .unwrap()
        );

        // Test with type filter
        assert!(rt.path_exists(&file_path, Some(PathKind::File)).unwrap());
        assert!(
            !rt.path_exists(&file_path, Some(PathKind::Directory))
                .unwrap()
        );
        assert!(
            rt.path_exists(&dir_path, Some(PathKind::Directory))
                .unwrap()
        );
        assert!(!rt.path_exists(&dir_path, Some(PathKind::File)).unwrap());
    }

    #[test]
    fn test_path_metadata() {
        let temp = TempFileTempDir::new().unwrap();
        let file_path = temp.path().join("file.txt");
        let rt = runtime();

        fs::write(&file_path, "hello").unwrap();

        let metadata = rt.path_metadata(&file_path).unwrap();
        assert_eq!(metadata.kind, PathKind::File);
        assert_eq!(metadata.size, 5);
        assert!(metadata.modified.is_some());
    }

    #[test]
    fn test_file_copy() {
        let temp = TempFileTempDir::new().unwrap();
        let src = temp.path().join("src.txt");
        let dst = temp.path().join("dst.txt");
        let rt = runtime();

        fs::write(&src, "original content").unwrap();
        rt.file_copy(&src, &dst).unwrap();

        assert!(dst.exists());
        assert_eq!(fs::read_to_string(&dst).unwrap(), "original content");
    }

    #[test]
    fn test_path_rename() {
        let temp = TempFileTempDir::new().unwrap();
        let old = temp.path().join("old.txt");
        let new = temp.path().join("new.txt");
        let rt = runtime();

        fs::write(&old, "content").unwrap();
        rt.path_rename(&old, &new).unwrap();

        assert!(!old.exists());
        assert!(new.exists());
        assert_eq!(fs::read_to_string(&new).unwrap(), "content");
    }

    #[test]
    fn test_file_remove() {
        let temp = TempFileTempDir::new().unwrap();
        let path = temp.path().join("to_remove.txt");
        let rt = runtime();

        fs::write(&path, "content").unwrap();
        assert!(path.exists());

        rt.file_remove(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn test_dir_create_and_remove() {
        let temp = TempFileTempDir::new().unwrap();
        let dir = temp.path().join("newdir");
        let rt = runtime();

        // Create
        rt.dir_create(&dir, false).unwrap();
        assert!(dir.is_dir());

        // Remove
        rt.dir_remove(&dir, false).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn test_dir_create_recursive() {
        let temp = TempFileTempDir::new().unwrap();
        let dir = temp.path().join("a/b/c/d");
        let rt = runtime();

        rt.dir_create(&dir, true).unwrap();
        assert!(dir.is_dir());
    }

    #[test]
    fn test_dir_remove_recursive() {
        let temp = TempFileTempDir::new().unwrap();
        let dir = temp.path().join("parent");
        let rt = runtime();

        fs::create_dir_all(dir.join("child")).unwrap();
        fs::write(dir.join("child/file.txt"), "content").unwrap();

        rt.dir_remove(&dir, true).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn test_dir_list() {
        let temp = TempFileTempDir::new().unwrap();
        let rt = runtime();

        fs::write(temp.path().join("a.txt"), "").unwrap();
        fs::write(temp.path().join("b.txt"), "").unwrap();
        fs::create_dir(temp.path().join("subdir")).unwrap();

        let entries = rt.dir_list(temp.path()).unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn test_cwd() {
        let rt = runtime();
        let cwd = rt.cwd().unwrap();
        assert!(cwd.is_absolute());
        assert!(cwd.exists());
    }

    #[test]
    fn test_temp_dir() {
        let rt = runtime();
        let temp = rt.temp_dir("test_prefix").unwrap();

        assert!(temp.path().exists());
        assert!(temp.path().is_dir());
        assert!(temp.path().to_string_lossy().contains("test_prefix"));

        let path = temp.path().to_path_buf();
        drop(temp);

        // Should be cleaned up
        assert!(!path.exists());
    }

    #[test]
    fn test_exec_command_success() {
        let rt = runtime();

        let output = rt.exec_command("echo", &["hello"], None).unwrap();

        assert!(output.success());
        assert!(output.stdout_string().contains("hello"));
    }

    #[test]
    fn test_exec_command_failure() {
        let rt = runtime();

        let output = rt.exec_command("false", &[], None).unwrap();

        assert!(!output.success());
    }

    #[test]
    fn test_exec_command_with_stdin() {
        let rt = runtime();

        let output = rt.exec_command("cat", &[], Some(b"input data")).unwrap();

        assert!(output.success());
        assert_eq!(output.stdout_string(), "input data");
    }

    #[test]
    fn test_exec_pipe_success() {
        let rt = runtime();

        let output = rt.exec_pipe("cat", &[], b"pipe input").unwrap();

        assert_eq!(output, b"pipe input");
    }

    #[test]
    fn test_exec_pipe_failure() {
        let rt = runtime();

        let result = rt.exec_pipe("false", &[], b"");

        assert!(matches!(result, Err(RuntimeError::ProcessFailed { .. })));
    }

    #[test]
    fn test_env_get() {
        let rt = runtime();

        // PATH should exist on all systems
        let path = rt.env_get("PATH").unwrap();
        assert!(path.is_some());

        // Nonexistent variable
        let none = rt.env_get("DEFINITELY_NOT_A_REAL_VAR_12345").unwrap();
        assert!(none.is_none());
    }

    #[test]
    fn test_env_all() {
        let rt = runtime();

        let all = rt.env_all().unwrap();
        assert!(!all.is_empty());
        assert!(all.contains_key("PATH") || all.contains_key("Path"));
    }

    #[test]
    fn test_os_name() {
        let rt = runtime();
        let os = rt.os_name();

        // Should be a known OS
        assert!(
            [
                "linux",
                "macos",
                "windows",
                "freebsd",
                "openbsd",
                "netbsd",
                "dragonfly",
                "ios",
                "android"
            ]
            .contains(&os)
                || os == "darwin" // macOS reports as "darwin"
        );
    }

    #[test]
    fn test_arch() {
        let rt = runtime();
        let arch = rt.arch();

        // Should be a known architecture
        assert!(
            [
                "x86",
                "x86_64",
                "arm",
                "aarch64",
                "mips",
                "mips64",
                "powerpc",
                "powerpc64",
                "riscv64",
                "s390x",
                "wasm32"
            ]
            .contains(&arch)
        );
    }

    #[test]
    fn test_xdg_dir() {
        let rt = runtime();

        // These should return valid paths (though they may not exist)
        let config = rt.xdg_dir(XdgDirKind::Config, None).unwrap();
        assert!(
            config.to_string_lossy().contains("config")
                || config.to_string_lossy().contains("Config")
        );

        let data = rt.xdg_dir(XdgDirKind::Data, None).unwrap();
        assert!(!data.as_os_str().is_empty());

        // Test with subpath
        let config_sub = rt
            .xdg_dir(XdgDirKind::Config, Some(Path::new("myapp")))
            .unwrap();
        assert!(config_sub.ends_with("myapp"));
    }

    #[test]
    fn test_stdout_stderr_write() {
        let rt = runtime();

        // These should not error (though we can't easily verify the output)
        rt.stdout_write(b"test stdout\n").unwrap();
        rt.stderr_write(b"test stderr\n").unwrap();
    }
}
