//! OPR-9d: reading a zip output (docx, pptx, epub) as a comparable [`Side`].
//!
//! The harness compares canonicalized TEXT PARTS, never zip bytes (a zip carries timestamps and
//! part order). Media are compared by content hash and listed in `Side::figures`. What is
//! normalized is the minimum the 9a spike (S4) measured as noise; everything else, a one-sided
//! bookmark and a picture description included, stays visible.

#![cfg(not(target_arch = "wasm32"))]

use super::opr_parity_harness::Side;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZipKind {
    Docx,
    Pptx,
    Epub,
}

pub fn zip_kind(extension: &str) -> Option<ZipKind> {
    match extension {
        "docx" => Some(ZipKind::Docx),
        "pptx" => Some(ZipKind::Pptx),
        "epub" => Some(ZipKind::Epub),
        _ => None,
    }
}

type Parts = std::collections::BTreeMap<String, Vec<u8>>;

fn read_parts(bytes: &[u8]) -> Result<Parts, String> {
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("not a readable zip: {e}"))?;
    let mut parts = Parts::new();
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("zip entry {i}: {e}"))?;
        if file.is_dir() {
            continue;
        }
        let name = file
            .name_raw()
            .iter()
            .map(|&b| b as char)
            .collect::<String>();
        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .map_err(|e| format!("zip entry {name}: {e}"))?;
        parts.insert(name, data);
    }
    Ok(parts)
}

fn text_of<'a>(parts: &'a Parts, name: &str) -> Result<&'a str, String> {
    let bytes = parts
        .get(name)
        .ok_or_else(|| format!("the zip has no `{name}`"))?;
    std::str::from_utf8(bytes).map_err(|e| format!("`{name}` is not UTF-8: {e}"))
}

/// One tag per line, so a difference is a line difference (what `xmllint --format` gave the hand
/// run). Text between a tag pair is untouched, whitespace included.
fn pretty(xml: &str) -> String {
    xml.replace("><", ">\n<")
}

/// Replace each match of `re`'s second group by `<prefix><k>`, k counting first occurrences.
fn renumber(
    text: &str,
    re: &regex::Regex,
    prefix: &str,
    seen: &mut std::collections::HashMap<String, usize>,
) -> String {
    re.replace_all(text, |c: &regex::Captures| {
        let next = seen.len() + 1;
        let k = *seen.entry(c[2].to_string()).or_insert(next);
        format!("{}{prefix}{k}{}", &c[1], &c[3])
    })
    .into_owned()
}

fn sha_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The `media/<hash>.<ext>` names of `prefix`'s parts. Plots' SVGs carry random `clip<N>` ids,
/// removed before hashing (9a S2: the pptx and odt SVGs differ only by them).
fn media(parts: &Parts, prefix: &str) -> std::collections::BTreeSet<String> {
    let clip = regex::Regex::new(r"clip\d+").unwrap();
    parts
        .iter()
        .filter(|(name, _)| name.starts_with(prefix))
        .map(|(name, bytes)| {
            let ext = name.rsplit('.').next().unwrap_or("");
            let hash = if ext.eq_ignore_ascii_case("svg") {
                let text = String::from_utf8_lossy(bytes);
                sha_hex(clip.replace_all(&text, "clip").as_bytes())
            } else {
                sha_hex(bytes)
            };
            format!("media/{hash}.{ext}")
        })
        .collect()
}

fn section(out: &mut String, name: &str, body: &str) {
    out.push_str(&format!("=== {name} ===\n{}\n", body.trim_end()));
}

fn docx_side(parts: &Parts) -> Result<Side, String> {
    let document = text_of(parts, "word/document.xml")?;
    let rels = text_of(parts, "word/_rels/document.xml.rels")?;
    // Renumber by first occurrence: relationship ids (and the media names built from them),
    // bookmark ids, and drawing ids. A bookmark's NAME is kept, so a one-sided bookmark shows.
    let mut rids = std::collections::HashMap::new();
    let rid = regex::Regex::new(r"()rId(\d+)()").unwrap();
    let document = renumber(document, &rid, "RID", &mut rids);
    let mut bookmarks = std::collections::HashMap::new();
    let start = regex::Regex::new(r#"(<w:bookmarkStart [^>]*?w:id=")(\d+)(")"#).unwrap();
    let end = regex::Regex::new(r#"(<w:bookmarkEnd [^>]*?w:id=")(\d+)(")"#).unwrap();
    let document = renumber(&document, &start, "BM", &mut bookmarks);
    let document = renumber(&document, &end, "BM", &mut bookmarks);
    let doc_pr = regex::Regex::new(r#"(<wp:docPr [^>]*?\bid=")(\d+)(")"#).unwrap();
    let c_nv_pr = regex::Regex::new(r#"(<pic:cNvPr [^>]*?\bid=")(\d+)(")"#).unwrap();
    let document = renumber(&document, &doc_pr, "DP", &mut Default::default());
    let document = renumber(&document, &c_nv_pr, "PC", &mut Default::default());
    // The same map applies to the rels (ids and `media/rIdN.png` targets); ids the document never
    // references keep their names.
    let rels = rid
        .replace_all(rels, |c: &regex::Captures| match rids.get(&c[2]) {
            Some(k) => format!("RID{k}"),
            None => c[0].to_string(),
        })
        .into_owned();
    let mut rel_lines: Vec<&str> = rels.lines().flat_map(|l| l.split("><")).collect();
    rel_lines.sort_unstable();
    let mut text = String::new();
    section(&mut text, "word/document.xml", &pretty(&document));
    section(
        &mut text,
        "word/_rels/document.xml.rels",
        &rel_lines.join("\n"),
    );
    Ok(Side {
        text,
        figures: media(parts, "word/media/"),
    })
}

fn numbered<'a>(parts: &'a Parts, prefix: &str, suffix: &str) -> Vec<(usize, &'a String)> {
    let mut found: Vec<(usize, &String)> = parts
        .keys()
        .filter_map(|n| {
            let num = n.strip_prefix(prefix)?.strip_suffix(suffix)?;
            Some((num.parse().ok()?, n))
        })
        .collect();
    found.sort();
    found
}

fn pptx_side(parts: &Parts) -> Result<Side, String> {
    let slides = numbered(parts, "ppt/slides/slide", ".xml");
    if slides.is_empty() {
        return Err("the pptx has no `ppt/slides/slide<N>.xml`".into());
    }
    let mut text = String::new();
    // Layouts and masters are the template, identical on both routes: skipped. So are timestamps.
    for (_, name) in slides {
        section(&mut text, name, &pretty(text_of(parts, name)?));
    }
    for (_, name) in numbered(parts, "ppt/slides/_rels/slide", ".xml.rels") {
        let rels = text_of(parts, name)?;
        let mut lines: Vec<&str> = rels.lines().flat_map(|l| l.split("><")).collect();
        lines.sort_unstable();
        section(&mut text, name, &lines.join("\n"));
    }
    Ok(Side {
        text,
        figures: media(parts, "ppt/media/"),
    })
}

fn epub_side(parts: &Parts) -> Result<Side, String> {
    let uuid = regex::Regex::new(r"urn:uuid:[0-9A-Fa-f-]{36}").unwrap();
    let identifier = regex::Regex::new(r"(<dc:identifier[^>]*>)[^<]*").unwrap();
    let date = regex::Regex::new(r"(<dc:date[^>]*>)[^<]*").unwrap();
    let modified = regex::Regex::new(r#"(<meta property="dcterms:modified">)[^<]*"#).unwrap();
    let uid = regex::Regex::new(r#"(<meta name="dtb:uid" content=")[^"]*"#).unwrap();
    let clean = |s: &str| -> String {
        let s = uuid.replace_all(s, "urn:uuid:<UUID>");
        let s = identifier.replace_all(&s, "${1}<ID>");
        let s = date.replace_all(&s, "${1}<DATE>");
        let s = modified.replace_all(&s, "${1}<DATE>");
        uid.replace_all(&s, "${1}<UID>").into_owned()
    };
    let mut text = String::new();
    let mut wanted: Vec<&String> = parts
        .keys()
        .filter(|n| {
            (n.starts_with("EPUB/text/") && n.ends_with(".xhtml"))
                || *n == "EPUB/nav.xhtml"
                || *n == "EPUB/content.opf"
                || *n == "EPUB/toc.ncx"
        })
        .collect();
    if !wanted.iter().any(|n| n.starts_with("EPUB/text/")) {
        return Err("the epub has no `EPUB/text/*.xhtml`".into());
    }
    wanted.sort();
    for name in wanted {
        let body = clean(text_of(parts, name)?);
        let body = if name.ends_with(".opf") || name.ends_with(".ncx") {
            pretty(&body)
        } else {
            body
        };
        section(&mut text, name, &body);
    }
    Ok(Side {
        text,
        figures: media(parts, "EPUB/media/"),
    })
}

/// Open a zip output and reduce it to a [`Side`] (text parts canonicalized, media hashed).
pub fn side_from_zip(bytes: &[u8], kind: ZipKind) -> Result<Side, String> {
    let parts = read_parts(bytes)?;
    match kind {
        ZipKind::Docx => docx_side(&parts),
        ZipKind::Pptx => pptx_side(&parts),
        ZipKind::Epub => epub_side(&parts),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    use crate::opr_parity_harness::side_from_output;

    /// Build a zip in memory with the `zip` writer (stored, no compression: the extractor reads
    /// either). Parts keep the order given, so tests can also vary the part order.
    fn make_zip(parts: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (name, bytes) in parts {
                w.start_file(*name, options).unwrap();
                w.write_all(bytes).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    fn side_of(bytes: &[u8], ext: &str) -> Result<Side, String> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("out.{ext}"));
        std::fs::write(&path, bytes).unwrap();
        side_from_output(&path, &[dir.path()])
    }

    const PNG_A: &[u8] = b"\x89PNG-a";
    const PNG_B: &[u8] = b"\x89PNG-b";

    /// A docx: `bookmarks` is a list of (id, name); `first` is the rId of the first image, the
    /// second is `first + 5`; `core` is the timestamp part.
    fn docx(first: usize, doc_pr: usize, bookmarks: &[(usize, &str)], core: &str) -> Vec<u8> {
        docx_with(first, doc_pr, bookmarks, core, "a dot", PNG_A)
    }

    fn docx_with(
        first: usize,
        doc_pr: usize,
        bookmarks: &[(usize, &str)],
        core: &str,
        descr_a: &str,
        png_a: &[u8],
    ) -> Vec<u8> {
        let (r1, r2) = (format!("rId{first}"), format!("rId{}", first + 5));
        let marks: String = bookmarks
            .iter()
            .map(|(id, name)| {
                format!("<w:bookmarkStart w:id=\"{id}\" w:name=\"{name}\"/><w:bookmarkEnd w:id=\"{id}\"/>")
            })
            .collect();
        let document = format!(
            "<w:document><w:body>{marks}<w:p><w:r><w:t>Hello</w:t></w:r></w:p><w:p><w:drawing><wp:docPr id=\"{doc_pr}\" name=\"Picture\" descr=\"{descr_a}\"/><pic:cNvPr id=\"{doc_pr}\" name=\"x\"/><a:blip r:embed=\"{r1}\"/></w:drawing></w:p><w:p><w:drawing><wp:docPr id=\"{}\" name=\"Picture\" descr=\"b dot\"/><a:blip r:embed=\"{r2}\"/></w:drawing></w:p></w:body></w:document>",
            doc_pr + 1
        );
        let rels = format!(
            "<Relationships><Relationship Id=\"{r2}\" Type=\"x/image\" Target=\"media/{r2}.png\"/><Relationship Id=\"{r1}\" Type=\"x/image\" Target=\"media/{r1}.png\"/><Relationship Id=\"rId1\" Type=\"x/styles\" Target=\"styles.xml\"/></Relationships>"
        );
        make_zip(&[
            ("[Content_Types].xml", b"<Types/>"),
            ("docProps/core.xml", core.as_bytes()),
            ("word/document.xml", document.as_bytes()),
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            (&format!("word/media/{r1}.png"), png_a),
            (&format!("word/media/{r2}.png"), PNG_B),
        ])
    }

    #[test]
    fn a_docx_output_becomes_a_side() {
        let side = side_of(&docx(9, 1, &[], "<core>t1</core>"), "docx").unwrap();
        assert!(side.text.contains("<w:t>Hello</w:t>"), "{}", side.text);
        assert_eq!(side.figures.len(), 2, "{:?}", side.figures);
    }

    #[test]
    fn docx_ids_relationship_ids_media_names_and_timestamps_are_noise() {
        // Same document, but every rId, docPr id and media name is shifted and the timestamps
        // differ (9a S4: the shifts were caused by the extra cell bookmarks, now gone).
        let a = side_of(&docx(9, 1, &[], "<core>t1</core>"), "docx").unwrap();
        let b = side_of(&docx(11, 7, &[], "<core>t2</core>"), "docx").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn a_bookmark_on_one_docx_side_only_is_reported_not_normalized_away() {
        let a = side_of(&docx(9, 1, &[], "<core/>"), "docx").unwrap();
        // The bookmark also shifts the ids of everything else, as it did on the real pair.
        let b = side_of(&docx(9, 1, &[(0, "_aebbe65e")], "<core/>"), "docx").unwrap();
        assert_ne!(a, b);
        assert!(b.text.contains("_aebbe65e"), "{}", b.text);
        assert!(!a.text.contains("_aebbe65e"));
    }

    #[test]
    fn a_picture_description_difference_is_reported() {
        let a = docx_with(9, 1, &[], "<core/>", "a dot", PNG_A);
        let b = docx_with(9, 1, &[], "<core/>", "a DOT", PNG_A);
        assert_ne!(side_of(&a, "docx").unwrap(), side_of(&b, "docx").unwrap());
    }

    #[test]
    fn media_are_compared_by_content_not_by_name() {
        let a = side_of(&docx(9, 1, &[], "<core/>"), "docx").unwrap();
        let b = side_of(
            &docx_with(9, 1, &[], "<core/>", "a dot", b"\x89PNG-z"),
            "docx",
        )
        .unwrap();
        assert_ne!(a.figures, b.figures);
    }

    #[test]
    fn svg_clip_ids_are_noise_but_other_svg_changes_are_not() {
        let svg = |clip: &str, fill: &str| {
            format!(
                "<svg><defs><clipPath id=\"clip{clip}\"/></defs><rect clip-path=\"url(#clip{clip})\" fill=\"{fill}\"/></svg>"
            )
        };
        let build = |s: String| {
            let document = "<w:document/>";
            make_zip(&[
                ("word/document.xml", document.as_bytes()),
                ("word/_rels/document.xml.rels", b"<Relationships/>"),
                ("word/media/rId1.svg", s.as_bytes()),
            ])
        };
        let a = side_of(&build(svg("123", "red")), "docx").unwrap();
        let b = side_of(&build(svg("987", "red")), "docx").unwrap();
        let c = side_of(&build(svg("123", "blue")), "docx").unwrap();
        assert_eq!(a.figures, b.figures);
        assert_ne!(a.figures, c.figures);
    }

    fn pptx(slide2: &str, layout: &str, core: &str, reverse_parts: bool) -> Vec<u8> {
        let mut parts: Vec<(String, Vec<u8>)> = vec![
            ("docProps/core.xml".into(), core.as_bytes().to_vec()),
            ("ppt/slides/slide1.xml".into(), b"<p:sld><a:t>one</a:t></p:sld>".to_vec()),
            ("ppt/slides/slide10.xml".into(), b"<p:sld><a:t>ten</a:t></p:sld>".to_vec()),
            ("ppt/slides/slide2.xml".into(), slide2.as_bytes().to_vec()),
            ("ppt/slides/_rels/slide2.xml.rels".into(), b"<Relationships><Relationship Id=\"rId2\" Target=\"../media/image1.png\"/></Relationships>".to_vec()),
            ("ppt/slideLayouts/slideLayout1.xml".into(), layout.as_bytes().to_vec()),
            ("ppt/media/image1.png".into(), PNG_A.to_vec()),
        ];
        if reverse_parts {
            parts.reverse();
        }
        let refs: Vec<(&str, &[u8])> = parts
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_slice()))
            .collect();
        make_zip(&refs)
    }

    #[test]
    fn pptx_compares_slides_in_numeric_order_and_ignores_the_template_and_timestamps() {
        let a = side_of(
            &pptx(
                "<p:sld><a:t>two</a:t></p:sld>",
                "<l>1</l>",
                "<c>1</c>",
                false,
            ),
            "pptx",
        )
        .unwrap();
        let b = side_of(
            &pptx(
                "<p:sld><a:t>two</a:t></p:sld>",
                "<l>2</l>",
                "<c>2</c>",
                true,
            ),
            "pptx",
        )
        .unwrap();
        assert_eq!(a, b);
        assert!(a.text.find("one").unwrap() < a.text.find("two").unwrap());
        assert!(
            a.text.find("two").unwrap() < a.text.find("ten").unwrap(),
            "numeric order"
        );
        let changed = side_of(
            &pptx(
                "<p:sld><a:t>TWO</a:t></p:sld>",
                "<l>1</l>",
                "<c>1</c>",
                false,
            ),
            "pptx",
        )
        .unwrap();
        assert_ne!(a, changed);
    }

    fn epub(xhtml: &str, uuid: &str, date: &str) -> Vec<u8> {
        let opf = format!(
            "<package><metadata><dc:identifier id=\"id\">urn:uuid:{uuid}</dc:identifier><dc:date id=\"epub-date\">{date}</dc:date><meta property=\"dcterms:modified\">{date}</meta><dc:title>T</dc:title></metadata></package>"
        );
        let ncx =
            format!("<ncx><head><meta name=\"dtb:uid\" content=\"urn:uuid:{uuid}\"/></head></ncx>");
        make_zip(&[
            ("mimetype", b"application/epub+zip"),
            ("EPUB/content.opf", opf.as_bytes()),
            ("EPUB/toc.ncx", ncx.as_bytes()),
            ("EPUB/nav.xhtml", b"<nav>n</nav>"),
            ("EPUB/text/ch001.xhtml", xhtml.as_bytes()),
            ("EPUB/media/file0.png", PNG_A),
        ])
    }

    #[test]
    fn epub_ignores_the_identifier_dates_and_uid_but_not_content() {
        let a = side_of(
            &epub(
                "<p>x</p>",
                "11111111-1111-1111-1111-111111111111",
                "2026-01-01",
            ),
            "epub",
        )
        .unwrap();
        let b = side_of(
            &epub(
                "<p>x</p>",
                "22222222-2222-2222-2222-222222222222",
                "2026-02-02",
            ),
            "epub",
        )
        .unwrap();
        assert_eq!(a, b);
        let c = side_of(
            &epub(
                "<p>y</p>",
                "11111111-1111-1111-1111-111111111111",
                "2026-01-01",
            ),
            "epub",
        )
        .unwrap();
        assert_ne!(a, c);
    }

    #[test]
    fn an_output_with_no_extractor_fails_loudly_and_says_so() {
        let err = side_of(b"%PDF-1.7", "pdf").unwrap_err();
        assert!(err.contains("no extractor"), "{err}");
        let err = side_of(b"PK", "odt").unwrap_err();
        assert!(err.contains("no extractor"), "{err}");
    }

    #[test]
    fn a_zip_missing_its_main_part_is_an_error() {
        let bytes = make_zip(&[("other.xml", b"<x/>")]);
        assert!(side_of(&bytes, "docx").is_err());
    }
}
