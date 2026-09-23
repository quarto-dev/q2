//! Shared semantic extraction of docx/pptx (OOXML) content.
//!
//! This is the single extraction function used both by the dev-only
//! `cargo xtask capture-pandoc-goldens` (which captures Q1's output) and by
//! the CI-runnable golden assertion test (which asserts Q2's output against
//! the same committed snapshots). See
//! `claude-notes/plans/2026-09-18-pandoc-hybrid-P7-implementation.md`,
//! section "What the semantic extractor must preserve vs. may normalize",
//! for the preserve/normalize contract this crate implements.
//!
//! The single most load-bearing invariant: text content is preserved
//! byte-exactly, in particular the U+00A0 non-breaking space Q2 emits
//! between a crossref prefix and its number. No whitespace normalization,
//! trimming, or run-joining-with-a-separator may be applied to text found
//! inside a `<w:t>`/`<a:t>`/`<m:t>` element.

use std::collections::BTreeSet;
use std::fmt;
use std::io::{Cursor, Read};

use quick_xml::Reader;
use quick_xml::events::{BytesText, Event};
use quick_xml::name::QName;
use zip::ZipArchive;

mod golden_fixtures;
pub use golden_fixtures::{FIXTURES, FixtureEntry, golden_snapshot_name};

/// Errors returned by extraction. Extraction never panics on malformed
/// input; every failure mode returns `Err`.
#[derive(Debug)]
pub enum ExtractError {
    /// The input bytes are not a valid zip archive (or a required entry
    /// could not be read from an otherwise valid archive).
    NotAZip(String),
    /// A required entry's XML failed to parse.
    Xml(String),
    /// A required entry (e.g. `word/document.xml`) is missing.
    MissingEntry(&'static str),
}

impl fmt::Display for ExtractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExtractError::NotAZip(msg) => write!(f, "not a valid zip/OOXML package: {msg}"),
            ExtractError::Xml(msg) => write!(f, "XML parse error: {msg}"),
            ExtractError::MissingEntry(name) => write!(f, "missing required entry: {name}"),
        }
    }
}

impl std::error::Error for ExtractError {}

pub type Result<T> = std::result::Result<T, ExtractError>;

/// One paragraph's extracted text plus its style (docx `w:pStyle`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Paragraph {
    pub style: Option<String>,
    /// Byte-exact concatenation of every text run in the paragraph, in
    /// document order, with **no** separator inserted between runs.
    pub text: String,
}

/// The allow-listed subset of `docProps/core.xml`. Deliberately excludes
/// `dcterms:created`/`dcterms:modified` (wall-clock render timestamps) —
/// see the "MAY (and MUST) normalize away" list in the plan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoreProps {
    pub title: Option<String>,
    pub creator: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
}

/// One pptx slide's extracted paragraphs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Slide {
    pub paragraphs: Vec<Paragraph>,
}

/// The semantic extraction of a docx or pptx package.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extraction {
    /// docx: body paragraphs, in document order. Empty (and `slides` is
    /// `Some`) for pptx.
    pub paragraphs: Vec<Paragraph>,
    /// pptx: one entry per slide, in slide order (`slide1.xml`,
    /// `slide2.xml`, ... sorted numerically, not lexicographically).
    /// `None` for docx.
    pub slides: Option<Vec<Slide>>,
    /// `word/media/` (docx) or `ppt/media/` (pptx) file names, sorted.
    pub media: Vec<String>,
    /// `Target` of every relationship whose `Type` ends in `/image`,
    /// sorted. A raw relationship count is version-noise (a fresh
    /// image-free document already carries 7+ boilerplate relationships);
    /// this is filtered to the one signal that differs when an image is
    /// resolved.
    pub image_relationship_targets: Vec<String>,
    /// Flattened text content of each `<m:oMath>`, in document order.
    pub math: Vec<String>,
    pub drawing_count: usize,
    pub page_break_count: usize,
    pub core_props: CoreProps,
}

impl fmt::Display for Extraction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "core_props:")?;
        if let Some(t) = &self.core_props.title {
            writeln!(f, "  title: {t}")?;
        }
        if let Some(t) = &self.core_props.creator {
            writeln!(f, "  creator: {t}")?;
        }
        if let Some(t) = &self.core_props.subject {
            writeln!(f, "  subject: {t}")?;
        }
        if let Some(t) = &self.core_props.keywords {
            writeln!(f, "  keywords: {t}")?;
        }

        let write_paragraphs = |f: &mut fmt::Formatter<'_>, paragraphs: &[Paragraph]| {
            for p in paragraphs {
                match &p.style {
                    Some(s) => writeln!(f, "  [{s}] {}", p.text)?,
                    None => writeln!(f, "  {}", p.text)?,
                }
            }
            fmt::Result::Ok(())
        };

        match &self.slides {
            Some(slides) => {
                for (i, slide) in slides.iter().enumerate() {
                    writeln!(f, "slide {}:", i + 1)?;
                    write_paragraphs(f, &slide.paragraphs)?;
                }
            }
            None => {
                writeln!(f, "paragraphs:")?;
                write_paragraphs(f, &self.paragraphs)?;
            }
        }

        if !self.math.is_empty() {
            writeln!(f, "math:")?;
            for m in &self.math {
                writeln!(f, "  {m}")?;
            }
        }

        writeln!(f, "media: {:?}", self.media)?;
        writeln!(
            f,
            "image_relationships: {:?}",
            self.image_relationship_targets
        )?;
        writeln!(f, "drawing_count: {}", self.drawing_count)?;
        writeln!(f, "page_break_count: {}", self.page_break_count)?;
        Ok(())
    }
}

/// Extract the semantic content of a docx package.
pub fn extract_docx(bytes: &[u8]) -> Result<Extraction> {
    let mut archive = open_zip(bytes)?;

    let document_xml = read_entry_string(&mut archive, "word/document.xml")?
        .ok_or(ExtractError::MissingEntry("word/document.xml"))?;
    let body = parse_body(&document_xml)?;

    let mut image_relationship_targets = BTreeSet::new();
    if let Some(rels_xml) = read_entry_string(&mut archive, "word/_rels/document.xml.rels")? {
        for target in parse_image_relationship_targets(&rels_xml)? {
            image_relationship_targets.insert(target);
        }
    }

    let core_props = match read_entry_string(&mut archive, "docProps/core.xml")? {
        Some(xml) => parse_core_props(&xml)?,
        None => CoreProps::default(),
    };

    let media = media_file_names(&archive, "word/media/");

    Ok(Extraction {
        paragraphs: body.paragraphs,
        slides: None,
        media,
        image_relationship_targets: image_relationship_targets.into_iter().collect(),
        math: body.math,
        drawing_count: body.drawing_count,
        page_break_count: body.page_break_count,
        core_props,
    })
}

/// Extract the semantic content of a pptx package.
pub fn extract_pptx(bytes: &[u8]) -> Result<Extraction> {
    let mut archive = open_zip(bytes)?;

    let mut slide_entries: Vec<(u32, String)> = Vec::new();
    for i in 0..archive.len() {
        let Some(name) = archive.name_for_index(i) else {
            continue;
        };
        if let Some(rest) = name.strip_prefix("ppt/slides/slide")
            && let Some(num_str) = rest.strip_suffix(".xml")
            && let Ok(n) = num_str.parse::<u32>()
        {
            slide_entries.push((n, name.to_string()));
        }
    }
    if slide_entries.is_empty() {
        return Err(ExtractError::MissingEntry("ppt/slides/slideN.xml"));
    }
    // Numeric, not lexicographic, sort: "slide10" must sort after "slide2".
    slide_entries.sort_by_key(|(n, _)| *n);

    let mut slides = Vec::with_capacity(slide_entries.len());
    let mut math = Vec::new();
    let mut drawing_count = 0usize;
    let mut page_break_count = 0usize;
    for (_, name) in &slide_entries {
        let xml = read_entry_string(&mut archive, name)?
            .ok_or(ExtractError::MissingEntry("ppt/slides/slideN.xml"))?;
        let body = parse_body(&xml)?;
        math.extend(body.math);
        drawing_count += body.drawing_count;
        page_break_count += body.page_break_count;
        slides.push(Slide {
            paragraphs: body.paragraphs,
        });
    }

    let mut rels_names = Vec::new();
    for i in 0..archive.len() {
        if let Some(name) = archive.name_for_index(i)
            && name.starts_with("ppt/")
            && name.ends_with(".rels")
        {
            rels_names.push(name.to_string());
        }
    }
    let mut image_relationship_targets = BTreeSet::new();
    for name in &rels_names {
        if let Some(xml) = read_entry_string(&mut archive, name)? {
            for target in parse_image_relationship_targets(&xml)? {
                image_relationship_targets.insert(target);
            }
        }
    }

    let core_props = match read_entry_string(&mut archive, "docProps/core.xml")? {
        Some(xml) => parse_core_props(&xml)?,
        None => CoreProps::default(),
    };

    let media = media_file_names(&archive, "ppt/media/");

    Ok(Extraction {
        paragraphs: Vec::new(),
        slides: Some(slides),
        media,
        image_relationship_targets: image_relationship_targets.into_iter().collect(),
        math,
        drawing_count,
        page_break_count,
        core_props,
    })
}

fn open_zip(bytes: &[u8]) -> Result<ZipArchive<Cursor<&[u8]>>> {
    ZipArchive::new(Cursor::new(bytes)).map_err(|e| ExtractError::NotAZip(e.to_string()))
}

fn read_entry_string(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Option<String>> {
    match archive.by_name(name) {
        Ok(mut file) => {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .map_err(|e| ExtractError::Xml(e.to_string()))?;
            String::from_utf8(buf)
                .map(Some)
                .map_err(|e| ExtractError::Xml(e.to_string()))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(e) => Err(ExtractError::NotAZip(e.to_string())),
    }
}

/// Sorted, de-duplicated file names directly under `prefix` in the archive
/// (e.g. `word/media/`). Sorting (rather than archive iteration order)
/// keeps extraction deterministic regardless of how the zip was written.
fn media_file_names(archive: &ZipArchive<Cursor<&[u8]>>, prefix: &str) -> Vec<String> {
    let mut set = BTreeSet::new();
    for i in 0..archive.len() {
        if let Some(name) = archive.name_for_index(i)
            && let Some(rest) = name.strip_prefix(prefix)
            && !rest.is_empty()
            && !rest.ends_with('/')
        {
            set.insert(rest.to_string());
        }
    }
    set.into_iter().collect()
}

fn local_name_str(name: QName<'_>) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(name.local_name().into_inner())
}

fn text_content(t: &BytesText<'_>) -> Result<String> {
    let decoded = t.decode().map_err(|e| ExtractError::Xml(e.to_string()))?;
    let unescaped =
        quick_xml::escape::unescape(&decoded).map_err(|e| ExtractError::Xml(e.to_string()))?;
    Ok(unescaped.into_owned())
}

struct BodyParts {
    paragraphs: Vec<Paragraph>,
    math: Vec<String>,
    drawing_count: usize,
    page_break_count: usize,
}

/// Parses a docx `word/document.xml` or a pptx `ppt/slides/slideN.xml`
/// body. Both use `<w:p>`/`<a:p>` for paragraphs, `<w:t>`/`<a:t>` for run
/// text (local name `t` either way), `<m:oMath>`/`<m:t>` for math content,
/// `<w:drawing>` for images, and `<w:br w:type="page"/>` for pagebreaks —
/// sharing this one walk is safe because no other element in either
/// document type has local name exactly `p`, `t`, `oMath`, `drawing`, or
/// `br`.
fn parse_body(xml: &str) -> Result<BodyParts> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text_start = false;
    reader.config_mut().trim_text_end = false;

    let mut paragraphs = Vec::new();
    let mut math = Vec::new();
    let mut drawing_count = 0usize;
    let mut page_break_count = 0usize;

    let mut in_paragraph = false;
    let mut current_style: Option<String> = None;
    let mut current_text = String::new();

    let mut math_depth = 0usize;
    let mut current_math = String::new();

    let mut in_text_run = false;

    loop {
        let event = reader
            .read_event()
            .map_err(|e| ExtractError::Xml(e.to_string()))?;
        match event {
            Event::Eof => break,
            Event::Start(ref e) | Event::Empty(ref e) => {
                let is_empty = matches!(event, Event::Empty(_));
                let local = local_name_str(e.name()).into_owned();
                match local.as_str() {
                    "p" => {
                        in_paragraph = true;
                        current_style = None;
                        current_text.clear();
                        if is_empty {
                            paragraphs.push(Paragraph {
                                style: current_style.take(),
                                text: std::mem::take(&mut current_text),
                            });
                            in_paragraph = false;
                        }
                    }
                    "pStyle" => {
                        for attr in e.attributes().flatten() {
                            if local_name_str(attr.key) == "val" {
                                current_style = Some(
                                    attr.unescape_value()
                                        .map_err(|e| ExtractError::Xml(e.to_string()))?
                                        .into_owned(),
                                );
                            }
                        }
                    }
                    "t" => {
                        in_text_run = true;
                        if is_empty {
                            in_text_run = false;
                        }
                    }
                    "oMath" => {
                        math_depth += 1;
                        if math_depth == 1 {
                            current_math.clear();
                        }
                        if is_empty && math_depth == 1 {
                            math.push(std::mem::take(&mut current_math));
                            math_depth = 0;
                        }
                    }
                    "drawing" => drawing_count += 1,
                    "br" => {
                        let is_page_break = e.attributes().flatten().any(|attr| {
                            local_name_str(attr.key) == "type"
                                && attr.unescape_value().is_ok_and(|v| v == "page")
                        });
                        if is_page_break {
                            page_break_count += 1;
                        }
                    }
                    _ => {}
                }
            }
            Event::End(ref e) => {
                let local = local_name_str(e.name()).into_owned();
                match local.as_str() {
                    "p" if in_paragraph => {
                        paragraphs.push(Paragraph {
                            style: current_style.take(),
                            text: std::mem::take(&mut current_text),
                        });
                        in_paragraph = false;
                    }
                    "t" => in_text_run = false,
                    "oMath" if math_depth > 0 => {
                        math_depth -= 1;
                        if math_depth == 0 {
                            math.push(std::mem::take(&mut current_math));
                        }
                    }
                    _ => {}
                }
            }
            Event::Text(ref t) if in_text_run => {
                let text = text_content(t)?;
                if math_depth > 0 {
                    current_math.push_str(&text);
                } else {
                    current_text.push_str(&text);
                }
            }
            _ => {}
        }
    }

    Ok(BodyParts {
        paragraphs,
        math,
        drawing_count,
        page_break_count,
    })
}

fn parse_core_props(xml: &str) -> Result<CoreProps> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text_start = false;
    reader.config_mut().trim_text_end = false;

    let mut props = CoreProps::default();
    let mut current: Option<&'static str> = None;

    loop {
        let event = reader
            .read_event()
            .map_err(|e| ExtractError::Xml(e.to_string()))?;
        match event {
            Event::Eof => break,
            Event::Start(ref e) => {
                current = match local_name_str(e.name()).as_ref() {
                    "title" => Some("title"),
                    "creator" => Some("creator"),
                    "subject" => Some("subject"),
                    "keywords" => Some("keywords"),
                    _ => None,
                };
            }
            Event::End(_) => current = None,
            Event::Text(ref t) => {
                if let Some(field) = current {
                    let text = text_content(t)?;
                    if !text.is_empty() {
                        match field {
                            "title" => props.title = Some(text),
                            "creator" => props.creator = Some(text),
                            "subject" => props.subject = Some(text),
                            "keywords" => props.keywords = Some(text),
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(props)
}

fn parse_image_relationship_targets(xml: &str) -> Result<Vec<String>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text_start = false;
    reader.config_mut().trim_text_end = false;

    let mut targets = BTreeSet::new();

    loop {
        let event = reader
            .read_event()
            .map_err(|e| ExtractError::Xml(e.to_string()))?;
        match event {
            Event::Eof => break,
            Event::Start(ref e) | Event::Empty(ref e)
                if local_name_str(e.name()) == "Relationship" =>
            {
                let mut ty: Option<String> = None;
                let mut target: Option<String> = None;
                for attr in e.attributes().flatten() {
                    let key = local_name_str(attr.key).into_owned();
                    let value = attr
                        .unescape_value()
                        .map_err(|e| ExtractError::Xml(e.to_string()))?
                        .into_owned();
                    match key.as_str() {
                        "Type" => ty = Some(value),
                        "Target" => target = Some(value),
                        _ => {}
                    }
                }
                if let (Some(ty), Some(target)) = (ty, target)
                    && ty.ends_with("/image")
                {
                    targets.insert(target);
                }
            }
            _ => {}
        }
    }

    Ok(targets.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    /// Builds a minimal zip archive from `(entry_name, content)` pairs.
    /// Uses `Stored` (no compression) so building test fixtures needs no
    /// compression backend feature.
    fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut buf));
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            for (name, content) in entries {
                writer.start_file(*name, options).unwrap();
                writer.write_all(content).unwrap();
            }
            writer.finish().unwrap();
        }
        buf
    }

    const RELS_NO_IMAGE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/>
<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/>
<Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/fontTable" Target="fontTable.xml"/>
<Relationship Id="rId6" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/webSettings" Target="webSettings.xml"/>
<Relationship Id="rId7" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/>
</Relationships>"#;

    fn rels_with_image() -> String {
        format!(
            "{}\n<!-- placeholder to keep RELS_NO_IMAGE reusable -->",
            RELS_NO_IMAGE.replace(
                "</Relationships>",
                r#"<Relationship Id="rId8" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>
</Relationships>"#,
            )
        )
    }

    const CORE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
<dc:title>My Title</dc:title>
<dc:creator>Alice</dc:creator>
<cp:lastModifiedBy>Alice</cp:lastModifiedBy>
<dcterms:created xsi:type="dcterms:W3CDTF">2026-01-01T00:00:00Z</dcterms:created>
<dcterms:modified xsi:type="dcterms:W3CDTF">2026-01-01T00:00:00Z</dcterms:modified>
<dc:subject></dc:subject>
<cp:keywords></cp:keywords>
</cp:coreProperties>"#;

    fn docx_document(body_xml: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
<w:body>
{body_xml}
</w:body>
</w:document>"#
        )
    }

    fn minimal_docx(body_xml: &str) -> Vec<u8> {
        build_zip(&[
            ("word/document.xml", docx_document(body_xml).as_bytes()),
            ("word/_rels/document.xml.rels", RELS_NO_IMAGE.as_bytes()),
            ("docProps/core.xml", CORE_XML.as_bytes()),
        ])
    }

    // --- T9.1: nbsp preserved byte-exactly -----------------------------

    #[test]
    fn test_nbsp_preserved_bytewise() {
        // U+00A0 (non-breaking space), UTF-8 bytes 0xC2 0xA0.
        let body = "<w:p><w:r><w:t>Figure\u{a0}1 is here.</w:t></w:r></w:p>";
        let docx = minimal_docx(body);
        let extraction = extract_docx(&docx).unwrap();
        assert_eq!(extraction.paragraphs.len(), 1);
        let p = &extraction.paragraphs[0].text;
        assert_eq!(p.as_bytes(), b"Figure\xc2\xa01 is here.");
    }

    // --- T9.2: run concatenation with no inserted separator -------------

    #[test]
    fn test_run_concatenation_no_separator() {
        let body = "<w:p><w:r><w:t>one</w:t></w:r><w:r><w:t>two</w:t></w:r><w:r><w:t>three</w:t></w:r></w:p>";
        let docx = minimal_docx(body);
        let extraction = extract_docx(&docx).unwrap();
        assert_eq!(extraction.paragraphs.len(), 1);
        assert_eq!(extraction.paragraphs[0].text, "onetwothree");
    }

    // --- T9.3: core-props allow-list; no timestamps ----------------------

    #[test]
    fn test_no_timestamps_in_extraction() {
        let docx = minimal_docx("<w:p><w:r><w:t>body</w:t></w:r></w:p>");
        let extraction = extract_docx(&docx).unwrap();
        assert_eq!(extraction.core_props.title.as_deref(), Some("My Title"));
        assert_eq!(extraction.core_props.creator.as_deref(), Some("Alice"));

        let rendered = extraction.to_string();
        assert!(rendered.contains("My Title"));
        assert!(!rendered.contains("dcterms:created"));
        assert!(!rendered.contains("2026-01-01T00:00:00Z"));
        // No ISO-8601-shaped timestamp anywhere in the rendered form.
        assert!(!rendered.contains("T00:00:00Z"));
    }

    // --- T9.4: media inventory only differs via the /image filter --------

    #[test]
    fn test_media_inventory_is_image_only() {
        let body = "<w:p><w:r><w:t>caption</w:t></w:r></w:p>";
        let without_image = build_zip(&[
            ("word/document.xml", docx_document(body).as_bytes()),
            ("word/_rels/document.xml.rels", RELS_NO_IMAGE.as_bytes()),
            ("docProps/core.xml", CORE_XML.as_bytes()),
        ]);
        let with_image_rels = rels_with_image();
        let with_image = build_zip(&[
            ("word/document.xml", docx_document(body).as_bytes()),
            ("word/_rels/document.xml.rels", with_image_rels.as_bytes()),
            ("docProps/core.xml", CORE_XML.as_bytes()),
            ("word/media/image1.png", b"not-a-real-png"),
        ]);

        let extraction_without = extract_docx(&without_image).unwrap();
        let extraction_with = extract_docx(&with_image).unwrap();

        assert_ne!(extraction_without, extraction_with);
        assert!(extraction_without.image_relationship_targets.is_empty());
        assert!(extraction_without.media.is_empty());

        assert_eq!(extraction_with.image_relationship_targets.len(), 1);
        assert_eq!(extraction_with.media.len(), 1);
        assert_eq!(extraction_with.media[0], "image1.png");

        // The raw (unfiltered) relationship count is 7 boilerplate
        // relationships + 1 image = 8; the *filtered* image count is 1.
        // This is the discriminator per the plan's vacuity check: an
        // unfiltered relationship count does not differ between these two
        // fixtures at all (both start from the same 7 boilerplate rows).
        assert_eq!(extraction_with.image_relationship_targets.len(), 1);
    }

    // --- T9.5: m:oMath number is visible ---------------------------------

    #[test]
    fn test_omath_number_is_visible() {
        let with_number = "<w:p><m:oMath><m:r><m:t>Equation\u{a0}1</m:t></m:r></m:oMath></w:p>";
        let without_number = "<w:p><m:oMath><m:r><m:t>x=1</m:t></m:r></m:oMath></w:p>";

        let extraction_with = extract_docx(&minimal_docx(with_number)).unwrap();
        let extraction_without = extract_docx(&minimal_docx(without_number)).unwrap();

        assert_ne!(extraction_with, extraction_without);
        assert_eq!(extraction_with.math, vec!["Equation\u{a0}1".to_string()]);
        assert_eq!(extraction_without.math, vec!["x=1".to_string()]);
    }

    // --- T9.6: <w:pStyle> capture -----------------------------------------

    #[test]
    fn test_pstyle_capture_sequence() {
        let body = concat!(
            r#"<w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>The Title</w:t></w:r></w:p>"#,
            r#"<w:p><w:pPr><w:pStyle w:val="Author"/></w:pPr><w:r><w:t>Alice</w:t></w:r></w:p>"#,
            r#"<w:p><w:pPr><w:pStyle w:val="Date"/></w:pPr><w:r><w:t>2026-01-02</w:t></w:r></w:p>"#,
            r#"<w:p><w:pPr><w:pStyle w:val="FirstParagraph"/></w:pPr><w:r><w:t>Body text.</w:t></w:r></w:p>"#,
        );
        let extraction = extract_docx(&minimal_docx(body)).unwrap();
        let styles: Vec<Option<String>> = extraction
            .paragraphs
            .iter()
            .map(|p| p.style.clone())
            .collect();
        assert_eq!(
            styles,
            vec![
                Some("Title".to_string()),
                Some("Author".to_string()),
                Some("Date".to_string()),
                Some("FirstParagraph".to_string()),
            ]
        );
    }

    // --- T9.7: pagebreak + drawing counts ----------------------------------

    #[test]
    fn test_pagebreak_and_drawing_counts() {
        let body = concat!(
            r#"<w:p><w:r><w:t>before</w:t></w:r><w:r><w:br w:type="page"/></w:r></w:p>"#,
            r#"<w:p><w:r><w:drawing><wp:inline xmlns:wp="x"><a:graphic xmlns:a="y"/></wp:inline></w:drawing></w:r></w:p>"#,
        );
        let extraction = extract_docx(&minimal_docx(body)).unwrap();
        assert_eq!(extraction.page_break_count, 1);
        assert_eq!(extraction.drawing_count, 1);
    }

    // --- T9.8: extract_pptx reports one entry per slide, in order --------

    fn pptx_slide_xml(text: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld>
</p:sld>"#
        )
    }

    #[test]
    fn test_pptx_slides_ordered_numerically() {
        // 10+ slides so lexicographic ("slide10" < "slide2") and numeric
        // order disagree — the discriminator the plan's vacuity check
        // calls for.
        let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
        for n in 1..=11u32 {
            entries.push((
                format!("ppt/slides/slide{n}.xml"),
                pptx_slide_xml(&format!("slide-{n}\u{a0}text")).into_bytes(),
            ));
        }
        let entry_refs: Vec<(&str, &[u8])> = entries
            .iter()
            .map(|(n, c)| (n.as_str(), c.as_slice()))
            .collect();
        let pptx = build_zip(&entry_refs);

        let extraction = extract_pptx(&pptx).unwrap();
        let slides = extraction.slides.expect("pptx extraction has slides");
        assert_eq!(slides.len(), 11);
        for (i, slide) in slides.iter().enumerate() {
            let n = i + 1;
            assert_eq!(slide.paragraphs.len(), 1);
            assert_eq!(
                slide.paragraphs[0].text.as_bytes(),
                format!("slide-{n}\u{a0}text").as_bytes()
            );
        }
    }

    // --- T9.9: error paths, never panic ------------------------------------

    #[test]
    fn test_not_a_zip_is_err() {
        let garbage = b"this is not a zip file at all";
        assert!(extract_docx(garbage).is_err());
        assert!(extract_pptx(garbage).is_err());
    }

    #[test]
    fn test_zip_missing_document_xml_is_err() {
        let empty_zip = build_zip(&[("docProps/core.xml", CORE_XML.as_bytes())]);
        assert!(extract_docx(&empty_zip).is_err());
        assert!(extract_pptx(&empty_zip).is_err());
    }

    // --- T9.10: determinism -------------------------------------------------

    #[test]
    fn test_extraction_is_deterministic() {
        let body = concat!(
            r#"<w:p><w:r><w:t>alpha</w:t></w:r></w:p>"#,
            r#"<w:p><w:r><w:t>beta</w:t></w:r></w:p>"#,
        );
        let with_image_rels = rels_with_image();
        let docx = build_zip(&[
            ("word/document.xml", docx_document(body).as_bytes()),
            ("word/_rels/document.xml.rels", with_image_rels.as_bytes()),
            ("docProps/core.xml", CORE_XML.as_bytes()),
            ("word/media/b.png", b"b"),
            ("word/media/a.png", b"a"),
        ]);
        let a = extract_docx(&docx).unwrap();
        let b = extract_docx(&docx).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.to_string(), b.to_string());
    }
}
