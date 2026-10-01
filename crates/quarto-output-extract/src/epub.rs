//! Semantic extraction of an epub package, for comparing two renders of the
//! same document (native pandoc vs pandoc.wasm, or two native runs).
//!
//! What it normalizes away, because it differs between otherwise identical
//! runs:
//! - UUIDs: pandoc gives an epub a random `urn:uuid:` identifier unless the
//!   document supplies one. Every `8-4-4-4-12` hex UUID in a text entry
//!   becomes `UUID`.
//! - Timestamps: `dcterms:modified` and friends (`YYYY-MM-DDThh:mm:ssZ`)
//!   become `TIMESTAMP`.
//! - Attribute order in xhtml start tags: the Lua filters build attribute
//!   tables whose iteration order is random per run (`data-icon` before or
//!   after `data-appearance`), so attributes within a tag are sorted.
//! - Zip metadata: entry order, compression and zip timestamps are ignored;
//!   entries are compared by name, sorted.
//! - Embedded bytes (images, fonts): compared by length and a stable FNV-1a
//!   hash, not diffed.
//!
//! Text entries (xhtml, opf, ncx, css, ...) are otherwise compared verbatim.

use std::fmt;
use std::io::Read;

use crate::{ExtractError, Result, open_zip};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryContent {
    /// UTF-8 entry with UUIDs and timestamps normalized.
    Text(String),
    Binary {
        len: usize,
        fnv1a: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EpubExtraction {
    /// Sorted by entry name.
    pub entries: Vec<(String, EntryContent)>,
}

pub fn extract_epub(bytes: &[u8]) -> Result<EpubExtraction> {
    let mut archive = open_zip(bytes)?;
    let mut entries = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| ExtractError::NotAZip(e.to_string()))?;
        if file.is_dir() {
            continue;
        }
        let name = file.name().to_string();
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(|e| ExtractError::NotAZip(e.to_string()))?;
        let content = match String::from_utf8(buf) {
            Ok(text) => {
                let text = if name.ends_with(".xhtml") || name.ends_with(".html") {
                    sort_tag_attributes(&text)
                } else {
                    text
                };
                EntryContent::Text(normalize_text(&text))
            }
            Err(e) => {
                let raw = e.into_bytes();
                EntryContent::Binary {
                    len: raw.len(),
                    fnv1a: fnv1a(&raw),
                }
            }
        };
        entries.push((name, content));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(EpubExtraction { entries })
}

impl fmt::Display for EpubExtraction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (name, content) in &self.entries {
            match content {
                EntryContent::Text(text) => writeln!(f, "=== {name}\n{text}")?,
                EntryContent::Binary { len, fnv1a } => {
                    writeln!(f, "=== {name}\n<binary {len} bytes fnv1a {fnv1a:016x}>")?
                }
            }
        }
        Ok(())
    }
}

/// Sorts the attributes of every start tag (`<name a="1" b="2">`); quoted
/// values are kept intact and comments, end tags and declarations are untouched.
pub fn sort_tag_attributes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        rest = &rest[lt..];
        let is_start_tag = rest[1..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic());
        match is_start_tag.then(|| tag_end(rest)).flatten() {
            Some(end) => {
                out.push_str(&sort_one_tag(&rest[..=end]));
                rest = &rest[end + 1..];
            }
            None => {
                out.push('<');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Index of the `>` closing the tag at the start of `s`, skipping quoted values.
fn tag_end(s: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

fn sort_one_tag(tag: &str) -> String {
    let inner = &tag[1..tag.len() - 1];
    let (inner, self_closing) = match inner.strip_suffix('/') {
        Some(i) => (i, true),
        None => (inner, false),
    };
    // Split on whitespace outside quotes.
    let mut parts: Vec<&str> = Vec::new();
    let (mut start, mut quote) = (None, None::<char>);
    for (i, c) in inner.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => {
                quote = Some(c);
                start.get_or_insert(i);
            }
            (None, c) if c.is_whitespace() => {
                if let Some(s0) = start.take() {
                    parts.push(&inner[s0..i]);
                }
            }
            _ => {
                start.get_or_insert(i);
            }
        }
    }
    if let Some(s0) = start {
        parts.push(&inner[s0..]);
    }
    let Some((name, attrs)) = parts.split_first() else {
        return tag.to_string();
    };
    let mut attrs = attrs.to_vec();
    attrs.sort_unstable();
    let mut out = format!("<{name}");
    for a in attrs {
        out.push(' ');
        out.push_str(a);
    }
    out.push_str(if self_closing { "/>" } else { ">" });
    out
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Replaces UUIDs with `UUID` and `YYYY-MM-DDThh:mm:ssZ` timestamps with
/// `TIMESTAMP`.
pub fn normalize_text(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if let Some(n) = match_at(bytes, i, 36, is_uuid_byte_at) {
            out.push_str("UUID");
            i += n;
        } else if let Some(n) = match_at(bytes, i, 20, is_timestamp_byte_at) {
            out.push_str("TIMESTAMP");
            i += n;
        } else {
            // Copy one whole char (ASCII or multi-byte).
            let ch = s[i..].chars().next().expect("in bounds");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn match_at(bytes: &[u8], at: usize, len: usize, pat: fn(usize, u8) -> bool) -> Option<usize> {
    let window = bytes.get(at..at + len)?;
    window
        .iter()
        .enumerate()
        .all(|(k, b)| pat(k, *b))
        .then_some(len)
}

fn is_uuid_byte_at(k: usize, b: u8) -> bool {
    match k {
        8 | 13 | 18 | 23 => b == b'-',
        _ => b.is_ascii_hexdigit(),
    }
}

/// `2023-11-14T22:13:20Z`
fn is_timestamp_byte_at(k: usize, b: u8) -> bool {
    match k {
        4 | 7 => b == b'-',
        10 => b == b'T',
        13 | 16 => b == b':',
        19 => b == b'Z',
        _ => b.is_ascii_digit(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn uuids_and_timestamps_are_normalized() {
        let s = r#"<dc:identifier id="x">urn:uuid:3f2504e0-4f89-11d3-9a0c-0305e82c3301</dc:identifier><meta property="dcterms:modified">2023-11-14T22:13:20Z</meta> é"#;
        assert_eq!(
            normalize_text(s),
            r#"<dc:identifier id="x">urn:uuid:UUID</dc:identifier><meta property="dcterms:modified">TIMESTAMP</meta> é"#
        );
    }

    #[test]
    fn attribute_order_is_normalized() {
        let a = r#"<div class="c" data-icon="false" data-appearance="default">x > y</div><br/>"#;
        let b = r#"<div data-appearance="default" class="c" data-icon="false">x > y</div><br/>"#;
        assert_eq!(sort_tag_attributes(a), sort_tag_attributes(b));
        assert_eq!(
            sort_tag_attributes(r#"<a title="a b>c" href="x"/>"#),
            r#"<a href="x" title="a b>c"/>"#
        );
        assert_eq!(
            sort_tag_attributes("a < b </p><!-- c -->"),
            "a < b </p><!-- c -->"
        );
    }

    #[test]
    fn near_misses_are_left_alone() {
        let s = "2023-11-14 22:13:20 and 3f2504e0-4f89-11d3-9a0c";
        assert_eq!(normalize_text(s), s);
    }

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, data) in entries {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn runs_differing_only_in_uuid_and_order_are_equal() {
        let a = zip_of(&[
            (
                "EPUB/content.opf",
                b"urn:uuid:3f2504e0-4f89-11d3-9a0c-0305e82c3301",
            ),
            ("EPUB/media/a.png", &[0xff, 0x00, 0x01]),
        ]);
        let b = zip_of(&[
            ("EPUB/media/a.png", &[0xff, 0x00, 0x01]),
            (
                "EPUB/content.opf",
                b"urn:uuid:aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            ),
        ]);
        assert_eq!(extract_epub(&a).unwrap(), extract_epub(&b).unwrap());
    }

    #[test]
    fn differing_media_bytes_are_detected() {
        let a = zip_of(&[("m.png", &[0xff, 0x00])]);
        let b = zip_of(&[("m.png", &[0xff, 0x01])]);
        assert_ne!(extract_epub(&a).unwrap(), extract_epub(&b).unwrap());
    }

    #[test]
    fn differing_text_is_detected() {
        let a = zip_of(&[("a.xhtml", b"<p>one</p>")]);
        let b = zip_of(&[("a.xhtml", b"<p>two</p>")]);
        assert_ne!(extract_epub(&a).unwrap(), extract_epub(&b).unwrap());
    }
}
