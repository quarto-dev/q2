//! `hub-lsp` — answer document-structure questions about hub projects over
//! LSP.
//!
//! Joins a hub's sync websocket as a samod client peer and serves LSP on
//! stdio: the client names projects as workspace folders and asks with
//! custom requests such as `quarto/projectComments`. See
//! `claude-notes/plans/2026-11-09-hub-lsp-analysis-peer.md`.
//!
//! Handshake header values come from files (`--header-file NAME=PATH`), not
//! from the command line or the environment, where other processes running
//! as the same user can read them.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use quarto_hub_lsp::server::serve_stdio;
use quarto_hub_lsp::workspace::HubWorkspace;
use quarto_hub_provider::HeaderDialer;

/// How long to wait for the first connection to the hub.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Arguments for `q2 hub-lsp`.
pub struct HubLspArgs {
    /// The hub's sync websocket URL, e.g. `ws://127.0.0.1:40404/ws`.
    pub server: String,
    /// `NAME=PATH` pairs: send header `NAME` with the contents of `PATH`.
    pub header_files: Vec<String>,
}

pub fn execute(args: HubLspArgs) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(run(args));
    // tokio reads stdin on a blocking thread, and that read returns only
    // when the client closes its end of the pipe. Dropping the runtime
    // would wait for it, so exit without waiting.
    runtime.shutdown_background();
    result
}

async fn run(args: HubLspArgs) -> Result<()> {
    let server = url::Url::parse(&args.server)
        .with_context(|| format!("--server '{}' is not a URL", args.server))?;
    let headers = read_header_files(&args.header_files)?;
    let dialer = HeaderDialer::new(server.clone(), headers)?;
    let (workspace, hints) = HubWorkspace::connect(Arc::new(dialer), CONNECT_TIMEOUT)
        .await
        .with_context(|| format!("joining the hub at {server}"))?;
    serve_stdio(workspace, hints).await;
    Ok(())
}

/// Split a `--header-file` argument at its first `=`.
fn parse_header_file_spec(spec: &str) -> Result<(String, PathBuf)> {
    match spec.split_once('=') {
        Some((name, path)) if !name.is_empty() && !path.is_empty() => {
            Ok((name.to_string(), PathBuf::from(path)))
        }
        _ => bail!("--header-file '{spec}' must be NAME=PATH"),
    }
}

/// Each header's name and the contents of its file, without a trailing
/// newline.
fn read_header_files(specs: &[String]) -> Result<Vec<(String, String)>> {
    specs
        .iter()
        .map(|spec| {
            let (name, path) = parse_header_file_spec(spec)?;
            let value = std::fs::read_to_string(&path).with_context(|| {
                format!("reading the value of header {name} from {}", path.display())
            })?;
            let value = value.trim_end_matches(['\r', '\n']).to_string();
            Ok((name, value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_file_spec_splits_at_the_first_equals_sign() {
        let (name, path) = parse_header_file_spec("X-Relay-Key=/data/relay=secret").unwrap();
        assert_eq!(name, "X-Relay-Key");
        assert_eq!(path, PathBuf::from("/data/relay=secret"));
    }

    #[test]
    fn a_header_file_spec_needs_a_name_and_a_path() {
        for spec in ["X-Relay-Key", "=/data/relay-secret", "X-Relay-Key="] {
            let err = parse_header_file_spec(spec).unwrap_err().to_string();
            assert!(err.contains("NAME=PATH"), "{spec}: {err}");
        }
    }

    #[test]
    fn a_header_value_is_read_from_its_file_without_the_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("relay-secret");
        std::fs::write(&path, "s3cret\r\n").unwrap();
        let headers = read_header_files(&[format!("X-Relay-Key={}", path.display())]).unwrap();
        assert_eq!(
            headers,
            vec![("X-Relay-Key".to_string(), "s3cret".to_string())]
        );
    }

    #[test]
    fn an_unreadable_header_file_is_an_error_naming_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        let err = read_header_files(&[format!("X-Relay-Key={}", missing.display())]).unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("missing"), "{message}");
        assert!(message.contains("X-Relay-Key"), "{message}");
    }
}
