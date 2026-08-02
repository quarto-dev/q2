//! Resolving, fetching, and safely extracting Quarto source archives
//! (bd-1vlw8).
//!
//! Quarto 1 shares one module (`extension-host.ts`) between `use brand`,
//! `use template`, and `add`. This crate is its Rust counterpart, split
//! out of the `quarto` binary for the same reason: `q2 add` will need
//! identical machinery, and a copy in each command is how the two drift.
//!
//! Three pieces, in the order a request flows through them:
//!
//! - [`resolve_target`] turns what the user typed into something
//!   fetchable — a local path, a URL, or a GitHub `org/repo`.
//! - [`fetch_into`] downloads it (through the [`SourceFetch`] seam) and
//!   materializes it as a directory.
//! - `archive` extracts it. That module landed first and alone, because
//!   it is the part that processes attacker-controllable input; see its
//!   documentation for the threat model it defends against.

mod archive;
mod error;
mod fetch;
mod limits;
mod target;

pub use archive::{ArchiveFormat, ExtractSummary, detect_format, extract_into};
pub use error::FetchError;
pub use fetch::{SourceFetch, UreqFetch, derive_archive_root, fetch_into};
pub use limits::ExtractLimits;
pub use target::{RemoteTarget, Target, resolve_target};
