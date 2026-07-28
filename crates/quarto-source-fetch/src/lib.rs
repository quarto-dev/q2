//! Resolving, fetching, and safely extracting Quarto source archives
//! (bd-1vlw8).
//!
//! Quarto 1 shares one module (`extension-host.ts`) between `use brand`,
//! `use template`, and `add`. This crate is its Rust counterpart, split
//! out of the `quarto` binary for the same reason: `q2 add` will need
//! identical machinery, and a copy in each command is how the two drift.
//!
//! Today it provides archive handling; target resolution and network
//! fetching follow. The extraction path is deliberately the first thing
//! here, because it is the part that processes attacker-controllable
//! input — see [`archive`] for the threat model it defends against.

mod archive;
mod error;
mod limits;

pub use archive::{ArchiveFormat, ExtractSummary, detect_format, extract_into};
pub use error::FetchError;
pub use limits::ExtractLimits;
