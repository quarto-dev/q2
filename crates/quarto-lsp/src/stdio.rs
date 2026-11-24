//! JSON-RPC over stdio, with stdout reserved for the protocol
//! (bd-foum4ql3; plan `2026-11-17-q2-vscode-extension.md`, D-E 5).
//!
//! `q2 lsp` hosts previews in-process, so renders run here: engines,
//! pandoc, typst, and user pre/post-render scripts are spawned as children,
//! and some of them inherit stdout. A single stray byte on stdout corrupts
//! the JSON-RPC stream. Rather than audit every spawn, this module takes
//! the process's stdout for itself and points the process-level stdout at
//! stderr:
//!
//! - **unix:** `dup(1)` keeps the protocol pipe, then `dup2(2, 1)`;
//! - **Windows:** the original `STD_OUTPUT_HANDLE` keeps the protocol, then
//!   `SetStdHandle(STD_OUTPUT_HANDLE, <stderr>)`. Rust's `stdout()` and
//!   `Command`'s inherited handles both read the std handle at use time.
//!
//! After [`protocol_stdio`], `println!` and inherited child stdout land on
//! stderr, which editors show as the server's log.
//!
//! The transport mirrors `lsp_server::Connection::stdio()`: a reader thread
//! that stops after `exit`, and a writer thread.

use std::fs::File;
use std::io::{self, BufReader};
use std::thread;

use crossbeam_channel::bounded;
use lsp_server::{Connection, Message};

/// The reader and writer threads of [`protocol_stdio`].
pub struct StdioThreads {
    reader: thread::JoinHandle<io::Result<()>>,
    writer: thread::JoinHandle<io::Result<()>>,
}

impl StdioThreads {
    pub fn join(self) -> io::Result<()> {
        match self.reader.join() {
            Ok(r) => r?,
            Err(e) => std::panic::resume_unwind(e),
        }
        match self.writer.join() {
            Ok(r) => r,
            Err(e) => std::panic::resume_unwind(e),
        }
    }
}

/// Take stdout for the protocol, redirect the process's stdout to stderr,
/// and return a connection over stdin and the protocol stream.
pub fn protocol_stdio() -> io::Result<(Connection, StdioThreads)> {
    let protocol = take_stdout()?;
    let (writer_tx, writer_rx) = bounded::<Message>(0);
    let writer = thread::Builder::new()
        .name("LspServerWriter".to_owned())
        .spawn(move || {
            let mut out = io::BufWriter::new(protocol);
            // `Message::write` flushes after each message.
            writer_rx.into_iter().try_for_each(|m| m.write(&mut out))
        })?;
    let (reader_tx, reader_rx) = bounded::<Message>(0);
    let reader = thread::Builder::new()
        .name("LspServerReader".to_owned())
        .spawn(move || {
            let mut stdin = BufReader::new(io::stdin().lock());
            while let Some(msg) = Message::read(&mut stdin)? {
                let is_exit = matches!(&msg, Message::Notification(n) if n.method == "exit");
                if reader_tx.send(msg).is_err() || is_exit {
                    break;
                }
            }
            Ok(())
        })?;
    Ok((
        Connection {
            sender: writer_tx,
            receiver: reader_rx,
        },
        StdioThreads { reader, writer },
    ))
}

#[cfg(unix)]
fn take_stdout() -> io::Result<File> {
    use std::io::Write;
    use std::os::fd::FromRawFd;
    // Nothing buffered in Rust's stdout may reach the protocol later.
    io::stdout().flush()?;
    // SAFETY: plain fd syscalls on the standard descriptors; `dup` returns a
    // fresh descriptor that the `File` then owns.
    unsafe {
        let protocol = libc::dup(libc::STDOUT_FILENO);
        if protocol < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::dup2(libc::STDERR_FILENO, libc::STDOUT_FILENO) < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(File::from_raw_fd(protocol))
    }
}

#[cfg(windows)]
fn take_stdout() -> io::Result<File> {
    use std::io::Write;
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
    };
    io::stdout().flush()?;
    // SAFETY: std-handle queries and replacement; the original stdout handle
    // is handed to the `File`, which becomes its only owner (nothing else
    // reaches it once the std handle points at stderr).
    unsafe {
        let protocol = GetStdHandle(STD_OUTPUT_HANDLE);
        let stderr = GetStdHandle(STD_ERROR_HANDLE);
        if protocol == INVALID_HANDLE_VALUE || protocol.is_null() {
            return Err(io::Error::other("q2 lsp has no stdout handle"));
        }
        if SetStdHandle(STD_OUTPUT_HANDLE, stderr) == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(File::from_raw_handle(protocol as _))
    }
}
