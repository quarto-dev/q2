//! Shared utilities for Quarto

pub mod path;
pub mod user_status;
pub mod verbose;
pub mod version;

pub use path::{is_external_url, is_rooted, to_forward_slashes};
pub use verbose::verbose_to_filter;
pub use version::*;
// `user_status!` is `#[macro_export]`, so it lands at the crate root —
// the `pub mod user_status` above is for the doc comment, not the
// macro itself.
