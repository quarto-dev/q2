//! Streaming JSON writer built on top of `serde_json::ser::CompactFormatter`.
//!
//! Used by the JSON AST writer (`super::json`) to emit bytes directly without
//! materializing a `serde_json::Value` tree. Introduced for bd-wgup; see
//! `claude-notes/plans/2026-04-22-serde-json-value-intermediate.md`.
//!
//! The API deliberately mirrors serde_json's compact output byte-for-byte: the
//! same escaping rules, the same number formatting, no whitespace. Callers
//! only need to track their own high-level state (which variant is this? what
//! fields belong in it?); comma and colon insertion is handled internally via
//! a small per-level "is next element first?" stack.
//!
//! Ergonomics:
//!
//! ```ignore
//! use std::io::Cursor;
//! let mut buf = Vec::new();
//! let mut w = JsonStreamWriter::new(&mut buf);
//! w.begin_object()?;
//!   w.key("t")?; w.str_value("Str")?;
//!   w.key("c")?; w.str_value("hello")?;
//!   w.key("s")?; w.u64_value(42)?;
//! w.end_object()?;
//! assert_eq!(buf, br#"{"t":"Str","c":"hello","s":42}"#);
//! ```

use serde_json::ser::{CharEscape, CompactFormatter, Formatter};
use std::io;

/// Streaming JSON writer: wrap `io::Write` and emit JSON bytes with correct
/// escaping, number formatting, and comma/colon placement.
///
/// Tracks container nesting with `levels`: each entry records whether the
/// *next* value/key at that level is the first one, so the Formatter's
/// `begin_object_key` / `begin_array_value` gets the correct `first` flag.
/// After the first write at a level the flag flips to false.
pub struct JsonStreamWriter<W: io::Write> {
    writer: W,
    formatter: CompactFormatter,
    // Stack of open container states. `Array` tracks whether its next element
    // is the first (for the leading-comma decision). `Object` tracks the same
    // for its next key AND whether we're currently between a key and its
    // value (so nested writes route through `end_object_value` correctly).
    levels: Vec<Level>,
}

#[derive(Debug)]
enum Level {
    Array { first: bool },
    Object { first: bool, in_value: bool },
}

impl<W: io::Write> JsonStreamWriter<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            formatter: CompactFormatter,
            levels: Vec::new(),
        }
    }

    /// Consume self and return the underlying writer. Errors if any container
    /// was left open (misuse by the caller).
    pub fn into_inner(self) -> io::Result<W> {
        if !self.levels.is_empty() {
            return Err(io::Error::other(format!(
                "JsonStreamWriter::into_inner with {} unclosed container(s)",
                self.levels.len()
            )));
        }
        Ok(self.writer)
    }

    // --- Container boundaries --------------------------------------------------

    pub fn begin_object(&mut self) -> io::Result<()> {
        self.before_value()?;
        self.formatter.begin_object(&mut self.writer)?;
        self.levels.push(Level::Object {
            first: true,
            in_value: false,
        });
        Ok(())
    }

    pub fn end_object(&mut self) -> io::Result<()> {
        match self.levels.pop() {
            Some(Level::Object {
                in_value: false, ..
            }) => {}
            other => panic!(
                "end_object called in wrong state: top-of-stack was {:?}",
                other
            ),
        }
        self.formatter.end_object(&mut self.writer)?;
        self.after_value()?;
        Ok(())
    }

    pub fn begin_array(&mut self) -> io::Result<()> {
        self.before_value()?;
        self.formatter.begin_array(&mut self.writer)?;
        self.levels.push(Level::Array { first: true });
        Ok(())
    }

    pub fn end_array(&mut self) -> io::Result<()> {
        match self.levels.pop() {
            Some(Level::Array { .. }) => {}
            other => panic!(
                "end_array called in wrong state: top-of-stack was {:?}",
                other
            ),
        }
        self.formatter.end_array(&mut self.writer)?;
        self.after_value()?;
        Ok(())
    }

    // --- Object keys -----------------------------------------------------------

    /// Begin an object entry by writing its key. Must be followed by exactly
    /// one value write (a primitive or a matched begin_/end_ container pair).
    pub fn key(&mut self, name: &str) -> io::Result<()> {
        let first_for_key = match self.levels.last() {
            Some(Level::Object {
                first,
                in_value: false,
            }) => *first,
            Some(other) => panic!("key() outside of an object, top-of-stack was {:?}", other),
            None => panic!("key() with no open container"),
        };
        self.formatter
            .begin_object_key(&mut self.writer, first_for_key)?;
        write_escaped_str(&mut self.formatter, &mut self.writer, name)?;
        self.formatter.end_object_key(&mut self.writer)?;
        self.formatter.begin_object_value(&mut self.writer)?;
        if let Some(Level::Object { first, in_value }) = self.levels.last_mut() {
            *first = false;
            *in_value = true;
        }
        Ok(())
    }

    // --- Primitive values ------------------------------------------------------

    pub fn null_value(&mut self) -> io::Result<()> {
        self.before_value()?;
        self.formatter.write_null(&mut self.writer)?;
        self.after_value()?;
        Ok(())
    }

    pub fn bool_value(&mut self, v: bool) -> io::Result<()> {
        self.before_value()?;
        self.formatter.write_bool(&mut self.writer, v)?;
        self.after_value()?;
        Ok(())
    }

    pub fn u64_value(&mut self, v: u64) -> io::Result<()> {
        self.before_value()?;
        self.formatter.write_u64(&mut self.writer, v)?;
        self.after_value()?;
        Ok(())
    }

    pub fn i64_value(&mut self, v: i64) -> io::Result<()> {
        self.before_value()?;
        self.formatter.write_i64(&mut self.writer, v)?;
        self.after_value()?;
        Ok(())
    }

    pub fn f64_value(&mut self, v: f64) -> io::Result<()> {
        self.before_value()?;
        self.formatter.write_f64(&mut self.writer, v)?;
        self.after_value()?;
        Ok(())
    }

    pub fn str_value(&mut self, s: &str) -> io::Result<()> {
        self.before_value()?;
        write_escaped_str(&mut self.formatter, &mut self.writer, s)?;
        self.after_value()?;
        Ok(())
    }

    // --- Per-value state transitions ------------------------------------------

    /// Called before writing any value (primitive or container begin).
    /// If the parent is an Array, emits `begin_array_value(first)`.
    /// If the parent is an Object's value slot (post-key()), nothing extra —
    /// key() already called begin_object_value.
    /// At the top level, nothing to do.
    fn before_value(&mut self) -> io::Result<()> {
        match self.levels.last() {
            Some(Level::Array { first }) => {
                let is_first = *first;
                self.formatter
                    .begin_array_value(&mut self.writer, is_first)?;
            }
            Some(Level::Object { in_value: true, .. }) => {}
            Some(Level::Object {
                in_value: false, ..
            }) => {
                panic!("value written in an Object without a preceding key()");
            }
            None => {}
        }
        Ok(())
    }

    /// Called after writing any value (primitive or container end).
    /// Flips Array first=false / emits end_array_value, or emits
    /// end_object_value + clears the in_value flag.
    fn after_value(&mut self) -> io::Result<()> {
        match self.levels.last_mut() {
            Some(Level::Array { first }) => {
                self.formatter.end_array_value(&mut self.writer)?;
                *first = false;
            }
            Some(Level::Object { in_value, .. }) => {
                debug_assert!(*in_value, "after_value in Object without in_value");
                self.formatter.end_object_value(&mut self.writer)?;
                *in_value = false;
            }
            None => {}
        }
        Ok(())
    }
}

/// Escape-aware string writer matching serde_json's compact format.
///
/// Equivalent to serde_json's internal `format_escaped_str`, but exposed as a
/// free function so we can use it for object keys as well as values.
fn write_escaped_str<W: io::Write, F: Formatter>(
    formatter: &mut F,
    writer: &mut W,
    s: &str,
) -> io::Result<()> {
    formatter.begin_string(writer)?;
    format_escaped_str_contents(formatter, writer, s)?;
    formatter.end_string(writer)?;
    Ok(())
}

// Lookup table matching serde_json's ESCAPE array: entries that require
// escaping map to a CharEscape; 0 means "emit as-is."
const __DQ: u8 = b'"';
const __BS: u8 = b'\\';
const BB: u8 = b'b'; // \b
const TT: u8 = b't'; // \t
const NN: u8 = b'n'; // \n
const FF: u8 = b'f'; // \f
const RR: u8 = b'r'; // \r
const UU: u8 = b'u'; // \u00XX
const __: u8 = 0;

static ESCAPE: [u8; 256] = [
    //   0   1   2   3   4   5   6   7   8   9   A   B   C   D   E   F
    UU, UU, UU, UU, UU, UU, UU, UU, BB, TT, NN, UU, FF, RR, UU, UU, // 0
    UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, // 1
    __, __, __DQ, __, __, __, __, __, __, __, __, __, __, __, __, __, // 2
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 3
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 4
    __, __, __, __, __, __, __, __, __, __, __, __, __BS, __, __, __, // 5
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 6
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 7
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 8
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 9
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // A
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // B
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // C
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // D
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // E
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // F
];

/// Write the string contents (between opening and closing quotes) with the
/// same escaping rules serde_json uses: control chars and `"` / `\` via
/// `CharEscape`, non-ASCII and printable ASCII as raw fragments.
fn format_escaped_str_contents<W: io::Write, F: Formatter>(
    formatter: &mut F,
    writer: &mut W,
    s: &str,
) -> io::Result<()> {
    let bytes = s.as_bytes();
    let mut start = 0usize;

    for (i, &b) in bytes.iter().enumerate() {
        let esc = ESCAPE[b as usize];
        if esc == 0 {
            continue;
        }
        if start < i {
            formatter.write_string_fragment(writer, &s[start..i])?;
        }
        let char_escape = match esc {
            BB => CharEscape::Backspace,
            TT => CharEscape::Tab,
            NN => CharEscape::LineFeed,
            FF => CharEscape::FormFeed,
            RR => CharEscape::CarriageReturn,
            __DQ => CharEscape::Quote,
            __BS => CharEscape::ReverseSolidus,
            UU => CharEscape::AsciiControl(b),
            _ => unreachable!("ESCAPE table corrupt"),
        };
        formatter.write_char_escape(writer, char_escape)?;
        start = i + 1;
    }
    if start < bytes.len() {
        formatter.write_string_fragment(writer, &s[start..])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// Round-trip: build the same structure via JsonStreamWriter and via
    /// serde_json::to_writer(Value). Parse both back and assert deep equality.
    /// Byte-for-byte equality isn't required (key-order choices are ours to
    /// make), but JSON-value equality is a hard requirement of bd-wgup.
    fn assert_json_equivalent(stream_bytes: &[u8], value_bytes: &[u8]) {
        let a: Value = serde_json::from_slice(stream_bytes).unwrap_or_else(|e| {
            panic!(
                "stream bytes not valid JSON: {e}\n{}",
                String::from_utf8_lossy(stream_bytes)
            )
        });
        let b: Value = serde_json::from_slice(value_bytes)
            .unwrap_or_else(|e| panic!("value bytes not valid JSON: {e}"));
        assert_eq!(
            a,
            b,
            "JSON values differ\n stream: {}\n value:  {}",
            String::from_utf8_lossy(stream_bytes),
            String::from_utf8_lossy(value_bytes)
        );
    }

    #[test]
    fn simple_object() {
        let mut buf = Vec::new();
        let mut w = JsonStreamWriter::new(&mut buf);
        w.begin_object().unwrap();
        w.key("t").unwrap();
        w.str_value("Str").unwrap();
        w.key("c").unwrap();
        w.str_value("hello").unwrap();
        w.key("s").unwrap();
        w.u64_value(42).unwrap();
        w.end_object().unwrap();
        drop(w);

        let reference = serde_json::to_vec(&serde_json::json!({
            "t": "Str", "c": "hello", "s": 42
        }))
        .unwrap();
        assert_json_equivalent(&buf, &reference);
    }

    #[test]
    fn nested_object_and_array() {
        let mut buf = Vec::new();
        let mut w = JsonStreamWriter::new(&mut buf);
        w.begin_object().unwrap();
        w.key("blocks").unwrap();
        w.begin_array().unwrap();
        w.begin_object().unwrap();
        w.key("t").unwrap();
        w.str_value("Para").unwrap();
        w.key("c").unwrap();
        w.begin_array().unwrap();
        w.str_value("hello").unwrap();
        w.end_array().unwrap();
        w.end_object().unwrap();
        w.end_array().unwrap();
        w.key("meta").unwrap();
        w.begin_object().unwrap();
        w.end_object().unwrap();
        w.end_object().unwrap();
        drop(w);

        let reference = serde_json::to_vec(&serde_json::json!({
            "blocks": [{"t": "Para", "c": ["hello"]}],
            "meta": {}
        }))
        .unwrap();
        assert_json_equivalent(&buf, &reference);
    }

    #[test]
    fn escaping_matches_serde_json() {
        let tricky = "line1\nquote\"backslash\\tab\t\x01control";
        let mut buf = Vec::new();
        let mut w = JsonStreamWriter::new(&mut buf);
        w.str_value(tricky).unwrap();
        drop(w);

        let reference = serde_json::to_vec(&Value::String(tricky.to_string())).unwrap();
        assert_eq!(buf, reference, "escape bytes must match serde_json exactly");
    }

    #[test]
    fn numbers() {
        let mut buf = Vec::new();
        let mut w = JsonStreamWriter::new(&mut buf);
        w.begin_array().unwrap();
        w.u64_value(0).unwrap();
        w.u64_value(u64::MAX).unwrap();
        w.i64_value(-1).unwrap();
        w.i64_value(i64::MIN).unwrap();
        w.f64_value(1.5).unwrap();
        w.bool_value(true).unwrap();
        w.bool_value(false).unwrap();
        w.null_value().unwrap();
        w.end_array().unwrap();
        drop(w);

        let reference = serde_json::to_vec(&serde_json::json!([
            0,
            u64::MAX,
            -1,
            i64::MIN,
            1.5,
            true,
            false,
            null
        ]))
        .unwrap();
        assert_json_equivalent(&buf, &reference);
    }

    #[test]
    fn empty_containers() {
        let mut buf = Vec::new();
        let mut w = JsonStreamWriter::new(&mut buf);
        w.begin_object().unwrap();
        w.key("a").unwrap();
        w.begin_array().unwrap();
        w.end_array().unwrap();
        w.key("b").unwrap();
        w.begin_object().unwrap();
        w.end_object().unwrap();
        w.end_object().unwrap();
        drop(w);

        let reference = serde_json::to_vec(&serde_json::json!({"a": [], "b": {}})).unwrap();
        assert_json_equivalent(&buf, &reference);
    }

    #[test]
    fn into_inner_errors_on_unclosed() {
        let buf: Vec<u8> = Vec::new();
        let mut w = JsonStreamWriter::new(buf);
        w.begin_object().unwrap();
        let err = w.into_inner().unwrap_err();
        assert!(err.to_string().contains("unclosed"));
    }
}
