//! Path-root rewriting shared by capture (real roots -> placeholders) and
//! replay (placeholders -> real roots).

use anyhow::{Context, Result};
use base64::Engine;

/// The placeholder roots a recording uses in place of the capture machine's
/// temp, share, document and output directories.
pub const TMP_ROOT: &str = "/__q2_tmp__";
/// Matches `share_root` in `resources/pandoc-wasm.json`.
pub const SHARE_ROOT: &str = "/__q2_share__";
pub const DOC_ROOT: &str = "/__q2_doc__";
pub const OUT_ROOT: &str = "/__q2_out__";

/// The env var that carries the base64-encoded filter params JSON.
pub const PARAMS_VAR: &str = "QUARTO_FILTER_PARAMS";

/// An ordered set of `from -> to` string substitutions applied in a single
/// left-to-right pass, longest `from` first at each position, so a `to` that
/// contains another pair's `from` is never rewritten twice.
#[derive(Debug, Clone)]
pub struct PathMap {
    pairs: Vec<(String, String)>,
}

impl PathMap {
    pub fn new(mut pairs: Vec<(String, String)>) -> Self {
        pairs.retain(|(from, _)| !from.is_empty());
        pairs.sort_by_key(|p| std::cmp::Reverse(p.0.len()));
        Self { pairs }
    }

    pub fn apply(&self, s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        'outer: while !rest.is_empty() {
            for (from, to) in &self.pairs {
                if let Some(tail) = rest.strip_prefix(from.as_str()) {
                    out.push_str(to);
                    rest = tail;
                    continue 'outer;
                }
            }
            let ch = rest.chars().next().expect("non-empty");
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
        out
    }

    /// Applies the map to UTF-8 content; `None` if `bytes` is not UTF-8
    /// (binary files are copied verbatim).
    pub fn apply_bytes(&self, bytes: &[u8]) -> Option<Vec<u8>> {
        std::str::from_utf8(bytes)
            .ok()
            .map(|s| self.apply(s).into_bytes())
    }

    /// Applies the map inside a base64-encoded text payload (the params
    /// blob), re-encoding with the same standard alphabet and padding.
    pub fn apply_blob(&self, blob: &str) -> Result<String> {
        let raw = base64::engine::general_purpose::STANDARD
            .decode(blob)
            .context("params blob is not base64")?;
        let text = String::from_utf8(raw).context("params blob is not UTF-8")?;
        Ok(base64::engine::general_purpose::STANDARD.encode(self.apply(&text)))
    }

    /// Applies the map to an env value, descending into the params blob.
    pub fn apply_env(&self, name: &str, value: &str) -> Result<String> {
        if name == PARAMS_VAR {
            self.apply_blob(value)
        } else {
            Ok(self.apply(value))
        }
    }

    /// True if any `from` still occurs in `s` (a leak check after rewriting).
    pub fn leaks_in(&self, s: &str) -> Option<&str> {
        self.pairs
            .iter()
            .map(|(from, _)| from.as_str())
            .find(|from| s.contains(from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> PathMap {
        PathMap::new(vec![
            ("/var/T/pipe_X".into(), TMP_ROOT.into()),
            ("/var/T/pipe_X/pandoc-share".into(), SHARE_ROOT.into()),
        ])
    }

    #[test]
    fn longest_root_wins() {
        assert_eq!(
            map().apply("-L /var/T/pipe_X/pandoc-share/filters/main.lua /var/T/pipe_X/in.json"),
            "-L /__q2_share__/filters/main.lua /__q2_tmp__/in.json"
        );
    }

    #[test]
    fn single_pass_never_rewrites_a_replacement() {
        let m = PathMap::new(vec![("/a".into(), "/a/b".into())]);
        assert_eq!(m.apply("/a/x"), "/a/b/x");
    }

    #[test]
    fn non_ascii_is_preserved() {
        assert_eq!(map().apply("é /var/T/pipe_X ü"), "é /__q2_tmp__ ü");
    }

    #[test]
    fn blob_round_trips_with_padding() {
        let m = map();
        let enc = base64::engine::general_purpose::STANDARD.encode(r#"{"p":"/var/T/pipe_X/x"}"#);
        let out = m.apply_blob(&enc).unwrap();
        let dec = base64::engine::general_purpose::STANDARD
            .decode(out)
            .unwrap();
        assert_eq!(dec, br#"{"p":"/__q2_tmp__/x"}"#);
    }

    #[test]
    fn env_only_decodes_the_params_var() {
        let m = map();
        assert_eq!(
            m.apply_env("QUARTO_SHARE_PATH", "/var/T/pipe_X/pandoc-share")
                .unwrap(),
            SHARE_ROOT
        );
    }

    #[test]
    fn binary_is_left_alone() {
        assert!(map().apply_bytes(&[0xff, 0xfe, 0x00]).is_none());
    }

    #[test]
    fn leak_check_finds_remaining_roots() {
        let m = map();
        assert!(m.leaks_in("/var/T/pipe_X").is_some());
        assert!(m.leaks_in("/__q2_tmp__").is_none());
    }
}
