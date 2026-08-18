//! Shared utilities for Quarto

pub mod data_dir;
pub mod path;
pub mod runtime_dir;
pub mod user_status;
pub mod verbose;
pub mod version;

pub use data_dir::quarto_data_dir;
pub use path::{is_external_url, is_rooted, to_forward_slashes};
pub use runtime_dir::quarto_runtime_dir;
pub use verbose::verbose_to_filter;
pub use version::*;
// `user_status!` is `#[macro_export]`, so it lands at the crate root —
// the `pub mod user_status` above is for the doc comment, not the
// macro itself.
