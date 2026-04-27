//! JSON triple-array encoding for highlight spans.
//!
//! Format (see `claude-notes/plans/2026-04-19-syntax-highlighting-design.md`):
//! `[[start_byte, end_byte, capture_name], …]`. A fourth positional slot,
//! reserved for a future optional-extras object, can be added without a
//! version bump — decoders must tolerate extra entries per triple.
//!
//! Nesting is preserved: an enclosing span's triple may appear before or
//! after the triples it encloses; writers decide whether to nest or
//! flatten at emission time.
//!
//! This crate is deliberately minimal (no heavy deps) so both producers
//! (`quarto-highlight` on native) and consumers (`pampa`'s HTML writer,
//! which must compile to `wasm32-unknown-unknown`) can share the wire
//! format without cross-compiling tree-sitter / wasmtime.

use serde::{Deserialize, Serialize};

/// Attribute key used to carry highlight spans on `CodeBlock` and
/// inline `Code`.
pub const SPANS_ATTR_KEY: &str = "data-hl-spans";

/// A single highlight span. Byte offsets are into the code text; they
/// match tree-sitter's own positions and Rust `&str[start..end]` slicing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "RawSpan", into = "RawSpan")]
pub struct HighlightSpan {
    pub start: usize,
    pub end: usize,
    /// The capture name from the grammar's `highlights.scm`, verbatim
    /// (e.g. `"keyword"`, `"function.builtin"`, `"string.escape"`).
    pub capture: String,
}

/// Wire form: a JSON array `[start, end, capture, ...extras?]`.
///
/// A `Vec<serde_json::Value>` keeps the decoder forward-compatible: a
/// future fourth element (e.g. a metadata object) will deserialize, and
/// `HighlightSpan::from(raw)` will simply ignore anything past index 2.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawSpan(Vec<serde_json::Value>);

impl From<HighlightSpan> for RawSpan {
    fn from(s: HighlightSpan) -> Self {
        RawSpan(vec![
            serde_json::Value::from(s.start),
            serde_json::Value::from(s.end),
            serde_json::Value::from(s.capture),
        ])
    }
}

impl From<RawSpan> for HighlightSpan {
    fn from(raw: RawSpan) -> Self {
        // Tolerate malformed input by returning a zero-length "error" span.
        // Encoding is produced by us; decoding tolerance matters only for
        // filter-authored values.
        let start = raw
            .0
            .first()
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(0);
        let end = raw
            .0
            .get(1)
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(start);
        let capture = raw
            .0
            .get(2)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        HighlightSpan {
            start,
            end,
            capture,
        }
    }
}

/// Serialize a list of spans to the JSON triple-array string.
pub fn encode(spans: &[HighlightSpan]) -> Result<String, serde_json::Error> {
    serde_json::to_string(spans)
}

/// Parse a JSON triple-array string back into a list of spans.
pub fn decode(s: &str) -> Result<Vec<HighlightSpan>, serde_json::Error> {
    serde_json::from_str(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triples_roundtrip() {
        let spans = vec![
            HighlightSpan {
                start: 0,
                end: 3,
                capture: "keyword".into(),
            },
            HighlightSpan {
                start: 4,
                end: 7,
                capture: "function".into(),
            },
        ];
        let s = encode(&spans).unwrap();
        assert_eq!(s, r#"[[0,3,"keyword"],[4,7,"function"]]"#);
        assert_eq!(decode(&s).unwrap(), spans);
    }

    #[test]
    fn decoder_tolerates_extras_in_fourth_slot() {
        let s = r#"[[0,3,"keyword",{"confidence":0.9}]]"#;
        let decoded = decode(s).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].start, 0);
        assert_eq!(decoded[0].end, 3);
        assert_eq!(decoded[0].capture, "keyword");
    }

    #[test]
    fn empty_array_decodes_to_empty_vec() {
        assert!(decode("[]").unwrap().is_empty());
    }
}
