//! The import format table (epic interface 4): the single source for the picker's `accept`
//! filter, drop interception, labels, argv and the source size cap. TS keeps no copy.

use serde::Serialize;

/// Source files over this many bytes are refused before pandoc loads (I19). P1's T8
/// measurement left the 25 MB cap unchanged.
pub const MAX_SOURCE_BYTES: u64 = 26_214_400;

/// One importable format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportFormat {
    /// The pandoc reader name (`-f <id>`).
    pub id: &'static str,
    pub label: &'static str,
    /// Lowercase, with the leading dot.
    pub extensions: &'static [&'static str],
    pub mime_types: &'static [&'static str],
    /// Whether the reader takes `--track-changes=all` (docx only).
    pub track_changes: bool,
}

/// Every I1 format, in picker order.
pub const IMPORT_FORMATS: &[ImportFormat] = &[
    ImportFormat {
        id: "docx",
        label: "Word document",
        extensions: &[".docx"],
        mime_types: &["application/vnd.openxmlformats-officedocument.wordprocessingml.document"],
        track_changes: true,
    },
    ImportFormat {
        id: "odt",
        label: "OpenDocument text",
        extensions: &[".odt"],
        mime_types: &["application/vnd.oasis.opendocument.text"],
        track_changes: false,
    },
    ImportFormat {
        id: "rtf",
        label: "Rich Text Format",
        extensions: &[".rtf"],
        mime_types: &["application/rtf", "text/rtf"],
        track_changes: false,
    },
    ImportFormat {
        id: "epub",
        label: "EPUB book",
        extensions: &[".epub"],
        mime_types: &["application/epub+zip"],
        track_changes: false,
    },
    ImportFormat {
        id: "pptx",
        label: "PowerPoint presentation",
        extensions: &[".pptx"],
        mime_types: &["application/vnd.openxmlformats-officedocument.presentationml.presentation"],
        track_changes: false,
    },
];

/// The format whose extension ends `file_name` (case-insensitive), if any.
pub fn format_for_file_name(file_name: &str) -> Option<&'static ImportFormat> {
    let lower = file_name.to_lowercase();
    IMPORT_FORMATS
        .iter()
        .find(|f| f.extensions.iter().any(|ext| lower.ends_with(ext)))
}

/// The extension of `file_name` as the user typed it, without the dot (`""` if none).
pub fn extension_of(file_name: &str) -> &str {
    match file_name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.contains(['/', '\\']) => ext,
        _ => "",
    }
}

#[derive(Serialize)]
struct FormatRow {
    id: &'static str,
    label: &'static str,
    extensions: &'static [&'static str],
    mime_types: &'static [&'static str],
}

/// `get_import_formats()`'s JSON value:
/// `{ formats: [{ id, label, extensions, mime_types }], max_source_bytes }`.
pub fn format_table_json() -> serde_json::Value {
    let rows: Vec<FormatRow> = IMPORT_FORMATS
        .iter()
        .map(|f| FormatRow {
            id: f.id,
            label: f.label,
            extensions: f.extensions,
            mime_types: f.mime_types,
        })
        .collect();
    serde_json::json!({ "formats": rows, "max_source_bytes": MAX_SOURCE_BYTES })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_the_five_i1_formats_in_order() {
        let ids: Vec<_> = IMPORT_FORMATS.iter().map(|f| f.id).collect();
        assert_eq!(ids, ["docx", "odt", "rtf", "epub", "pptx"]);
    }

    #[test]
    fn only_docx_takes_track_changes() {
        for f in IMPORT_FORMATS {
            assert_eq!(f.track_changes, f.id == "docx", "{}", f.id);
        }
    }

    #[test]
    fn extension_matching_is_case_insensitive() {
        assert_eq!(format_for_file_name("Report.DOCX").unwrap().id, "docx");
        assert_eq!(format_for_file_name("a.b.Odt").unwrap().id, "odt");
        assert_eq!(format_for_file_name("x.RtF").unwrap().id, "rtf");
        assert_eq!(format_for_file_name("book.epub").unwrap().id, "epub");
        assert_eq!(format_for_file_name("deck.pptx").unwrap().id, "pptx");
    }

    #[test]
    fn unsupported_names_match_nothing() {
        for name in ["notes.md", "page.html", "x.doc", "docx", "x.docx.txt", ""] {
            assert!(format_for_file_name(name).is_none(), "{name}");
        }
    }

    #[test]
    fn extension_of_handles_edge_cases() {
        assert_eq!(extension_of("a.Docx"), "Docx");
        assert_eq!(extension_of("noext"), "");
        assert_eq!(extension_of(".hidden"), "");
        assert_eq!(extension_of("dir.d/file"), "");
    }

    #[test]
    fn mime_types_match_the_epic() {
        let by_id = |id: &str| IMPORT_FORMATS.iter().find(|f| f.id == id).unwrap();
        assert_eq!(by_id("rtf").mime_types, ["application/rtf", "text/rtf"]);
        assert_eq!(by_id("epub").mime_types, ["application/epub+zip"]);
        assert!(by_id("docx").mime_types[0].ends_with("wordprocessingml.document"));
    }

    #[test]
    fn table_json_has_the_pinned_shape() {
        let v = format_table_json();
        assert_eq!(v["max_source_bytes"], 26_214_400);
        let first = &v["formats"][0];
        assert_eq!(first["id"], "docx");
        assert_eq!(first["extensions"][0], ".docx");
        assert!(first.get("track_changes").is_none());
    }
}
