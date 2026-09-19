//! The `QUARTO_FILTER_PARAMS` codec: the exact base64 variant Q1's
//! `init.lua` decoder expects, plus the Windows environment-block size
//! predicate.
//!
//! Upstream writers: `pandoc.ts:310` and `pandoc.ts:335`, both
//! `encodeBase64(JSON.stringify(...))` from Deno's `encoding/base64`, which
//! is standard-alphabet, padded base64. Upstream decoder: `init.lua:596`
//! `base64.decode(os.getenv("QUARTO_FILTER_PARAMS"))`, then
//! `init.lua:599-604` `function param(name, default)`.
//!
//! Tests: `#[cfg(test)]` below (T3.1-T3.3);
//! `crates/quarto-core/tests/integration/pandoc_transport.rs` (T3.4).

use base64::Engine as _;

/// Encodes `json` as `QUARTO_FILTER_PARAMS` expects: standard alphabet, with
/// padding. This must stay `general_purpose::STANDARD` — Q1's vendored
/// `_base64.lua` decoder is built against the standard alphabet (`+`, `/`,
/// `=` padding), not `URL_SAFE` (`-`, `_`, no padding).
pub fn encode_params_blob(json: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(json)
}

/// The Windows environment-block size cap, in characters: `CreateProcess`
/// rejects an environment block larger than 32,767 characters. Modelled as a
/// pure predicate (rather than inlined at the call site) because this
/// repo's CI has no Windows test leg
/// (`.github/workflows/test-suite.yml:28`, `os: [ubuntu-latest,
/// macos-latest]`) — the boundary must be exercisable on macOS/Linux CI.
///
/// Binds the predicate only; the fallback behaviour for a blob that exceeds
/// this limit is an open decision (deferred — see the Task 3 brief's
/// reference to Findings for Gordon, item 9).
pub fn params_blob_exceeds_platform_limit(len: usize) -> bool {
    const WINDOWS_ENV_BLOCK_LIMIT: usize = 32_767;
    len > WINDOWS_ENV_BLOCK_LIMIT
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T3.1: a payload whose byte length is `% 3 == 2` produces padded
    /// output ending in `=`. The fixture length is the discriminator, not
    /// the assertion text: a `% 3 == 0` payload would encode identically
    /// under `STANDARD` and `STANDARD_NO_PAD` and would survive the revert
    /// below undetected.
    ///
    /// Revert hunk: swapping `general_purpose::STANDARD` for
    /// `general_purpose::STANDARD_NO_PAD` in `encode_params_blob` makes this
    /// RED (the output would no longer end in `=`).
    #[test]
    fn test_encoder_is_standard_with_padding() {
        // 11 bytes; 11 % 3 == 2, so standard base64 padding is exactly one
        // trailing `=`.
        let payload = "hello world";
        assert_eq!(payload.len() % 3, 2, "fixture must satisfy len % 3 != 0");

        let out = encode_params_blob(payload);

        assert_eq!(out, "aGVsbG8gd29ybGQ=");
        assert!(out.ends_with('='), "expected padded output, got {out}");
    }

    /// T3.2: a payload whose base64 contains both `+` and `/` uses the
    /// standard alphabet, not the URL-safe one.
    ///
    /// Revert hunk: swapping `general_purpose::STANDARD` for
    /// `general_purpose::URL_SAFE` in `encode_params_blob` makes this RED
    /// (the output would contain `-`/`_` instead of `+`/`/`).
    #[test]
    fn test_encoder_is_standard_alphabet() {
        // Chosen (empirically, via a Python `base64.b64encode` scan) so the
        // encoded output contains both `+` and `/`: "???>>>" -> "Pz8/Pj4+".
        let payload = "???>>>";

        let out = encode_params_blob(payload);

        assert_eq!(out, "Pz8/Pj4+");
        assert!(out.contains('+'), "expected '+' in output, got {out}");
        assert!(out.contains('/'), "expected '/' in output, got {out}");
        assert!(!out.contains('-'), "expected no '-' in output, got {out}");
        assert!(!out.contains('_'), "expected no '_' in output, got {out}");
    }

    /// T3.3: the Windows environment-block boundary is exact at 32,767.
    ///
    /// Revert hunk: changing the `WINDOWS_ENV_BLOCK_LIMIT` constant to
    /// `32_768` makes this RED (`32_767` would then read as exceeding the
    /// limit, and `32_768` would not).
    #[test]
    fn test_platform_limit_boundary() {
        assert!(!params_blob_exceeds_platform_limit(32_766));
        assert!(!params_blob_exceeds_platform_limit(32_767));
        assert!(params_blob_exceeds_platform_limit(32_768));
    }
}
