//! Resource ceilings for fetching and extracting an untrusted archive.
//!
//! Every field exists because the input is attacker-controllable: a
//! brand source can be any GitHub repository, and the archive it serves
//! is parsed and written to the user's project directory. Without caps,
//! a few kilobytes of crafted zip expand into an unbounded write.
//!
//! The defaults are sized for the realistic upper end of a brand — a
//! `_brand.yml` plus logos and several webfont families — with generous
//! headroom, not for arbitrary repositories.

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ExtractLimits {
    /// Maximum bytes accepted from the network for one archive.
    pub max_download_bytes: u64,
    /// Maximum total bytes written across all entries.
    ///
    /// This is the decompression-bomb ceiling. It is enforced while
    /// copying, not from the archive's declared sizes, so an entry that
    /// lies about its length still trips it.
    pub max_uncompressed_bytes: u64,
    /// Maximum number of entries in one archive.
    pub max_entries: usize,
    /// Per-request network timeout.
    pub request_timeout: Duration,
}

impl Default for ExtractLimits {
    fn default() -> Self {
        Self {
            max_download_bytes: 50 * 1024 * 1024,
            max_uncompressed_bytes: 200 * 1024 * 1024,
            max_entries: 10_000,
            request_timeout: Duration::from_secs(30),
        }
    }
}
