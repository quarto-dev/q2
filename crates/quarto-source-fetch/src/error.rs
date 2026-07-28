//! Errors from resolving, fetching, and extracting a source archive.

use std::fmt;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "{path} is not a recognized archive (expected a .tar.gz or .zip; \
         the file starts with {leading_bytes})"
    )]
    UnknownArchiveFormat {
        path: PathBuf,
        leading_bytes: String,
    },

    /// An entry whose name would write outside the destination, or is
    /// otherwise not a plain relative path.
    #[error("archive entry {name:?} is unsafe: {reason}")]
    UnsafeEntryPath { name: String, reason: &'static str },

    /// An entry that is neither a regular file nor a directory.
    ///
    /// Symlinks and hardlinks are the security-relevant cases (either
    /// can redirect a later write outside the destination), but a brand
    /// needs neither, nor a device node or fifo — so every non-plain
    /// entry is refused rather than skipped. Skipping would let an
    /// archive quietly differ from what was extracted.
    #[error("archive entry {name:?} is a {kind}, which is not allowed in a brand source")]
    UnsupportedEntryType { name: String, kind: &'static str },

    #[error("archive has more than {limit} entries")]
    TooManyEntries { limit: usize },

    #[error("archive expands to more than {limit} bytes")]
    TooLarge { limit: u64 },

    /// An entry whose declared size disagrees with the bytes actually
    /// read. Well-formed archives do not do this; a mismatch means the
    /// metadata cannot be trusted.
    #[error("archive entry {name:?} declares {declared} bytes but contains {actual}")]
    SizeMismatch {
        name: String,
        declared: u64,
        actual: u64,
    },

    #[error("could not read archive: {0}")]
    Archive(String),

    /// The user's target string is not a path, a URL, or `org/repo`.
    #[error("{target:?} is not a source we recognize: {reason}")]
    UnrecognizedTarget {
        target: String,
        reason: &'static str,
    },

    /// Every candidate URL was tried and none served an archive.
    #[error("could not download {description}: {detail}")]
    NotFound { description: String, detail: String },

    /// The response body exceeded the download ceiling.
    #[error("download exceeded {limit} bytes")]
    DownloadTooLarge { limit: u64 },

    #[error("network error fetching {url}: {message}")]
    Network { url: String, message: String },

    /// A `org/repo/<subdir>` target named a directory the archive does
    /// not contain.
    #[error("the archive has no {subdir:?} directory ({available})")]
    SubdirectoryNotFound { subdir: String, available: String },
}

impl FetchError {
    pub(crate) fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        FetchError::Io {
            context: context.into(),
            source,
        }
    }
}

/// Render the first few bytes of a file for the unknown-format error.
pub(crate) fn describe_leading_bytes(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "no bytes (the file is empty)".to_string();
    }
    let hex: Vec<String> = bytes.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("{}", HexList(hex))
}

struct HexList(Vec<String>);

impl fmt::Display for HexList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.join(" "))
    }
}
