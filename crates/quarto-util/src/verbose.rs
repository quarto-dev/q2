//! Verbosity-to-`tracing-subscriber`-filter mapping shared by Quarto's
//! CLI binaries.
//!
//! The CLI exposes a `-v` / `--verbose` accumulator flag whose `u8`
//! count is converted to an `EnvFilter` directive string. Keeping the
//! mapping in one place ensures `q2`, `q2 hub`, and any future binary
//! agree on what each verbosity level means.
//!
//! `RUST_LOG`, when set, is expected to override this default at the
//! call site (see each binary's `main.rs`).

/// Convert a verbosity-flag count (`-v` repeated) into an `EnvFilter`
/// directive string for `tracing_subscriber::EnvFilter::new(...)`.
///
/// - `0` (no flag): warnings and errors only — clean terminal during
///   normal use.
/// - `1` (`-v`): the workspace's `info` lines (startup banners,
///   one-shot lifecycle events).
/// - `2` (`-vv`): workspace `debug` plus `samod` at `info` — useful
///   when debugging sync flow.
/// - `3+` (`-vvv` or more): workspace `trace` and `tower_http=debug`,
///   plus `samod=debug`. The deepest level we expose; anything beyond
///   is the user's job via `RUST_LOG`.
///
/// Returned strings are `&'static str` to make it explicit that the
/// directive set is a closed, well-known list — callers should not
/// post-process or concatenate beyond passing them straight into
/// `EnvFilter::new`.
/// The `quarto` directive prefix-matches every `quarto*` crate
/// (`quarto_core`, `quarto_preview`, …); the separate `q2` directive
/// covers the `q2` *binary* crate itself, whose tracing targets start
/// with `q2::` and never matched `quarto` (bd-fu16z22k found this
/// while adding the `-v` active-profile echo).
pub fn verbose_to_filter(count: u8) -> &'static str {
    match count {
        0 => "quarto=warn,q2=warn",
        1 => "quarto=info,q2=info",
        2 => "quarto=debug,q2=debug,samod=info",
        _ => "quarto=trace,q2=trace,samod=debug,tower_http=debug",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_zero_is_warn_floor() {
        assert_eq!(verbose_to_filter(0), "quarto=warn,q2=warn");
    }

    #[test]
    fn single_v_is_info() {
        assert_eq!(verbose_to_filter(1), "quarto=info,q2=info");
    }

    #[test]
    fn double_v_adds_samod_info() {
        assert_eq!(verbose_to_filter(2), "quarto=debug,q2=debug,samod=info");
    }

    #[test]
    fn triple_v_adds_tower_http_and_samod_debug() {
        assert_eq!(
            verbose_to_filter(3),
            "quarto=trace,q2=trace,samod=debug,tower_http=debug"
        );
    }

    #[test]
    fn counts_above_three_clamp_to_triple_v() {
        // The clap accumulator can return arbitrarily large values
        // (e.g. someone types `-vvvvvv`). The mapping must remain
        // total over u8 and saturate at the deepest level rather than
        // panic or fall through.
        assert_eq!(verbose_to_filter(4), verbose_to_filter(3));
        assert_eq!(verbose_to_filter(10), verbose_to_filter(3));
        assert_eq!(verbose_to_filter(u8::MAX), verbose_to_filter(3));
    }
}
