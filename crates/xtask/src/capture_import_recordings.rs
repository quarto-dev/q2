//! `cargo xtask capture-import-recordings` — dev-only capture of native pandoc
//! *reader* runs (document import, plan P1 T2).
//!
//! The import fixtures live in `crates/quarto-core/tests/fixtures/import-recordings/`,
//! one directory per fixture (`<name>-<format>/`). Two subcommands:
//!
//! - `generate` builds each fixture's binary `source.<ext>` from `sources/` with the
//!   native pandoc (run once, when a source changes). Generation is deterministic
//!   under [`SOURCE_DATE_EPOCH`].
//! - the default capture reads only the checked-in `source.<ext>`, runs the native
//!   pandoc with interface 2's argv, relocates the temp paths to the canonical
//!   `/__q2_share__/import/…` and writes `argv.json`, `pandoc.json`, `stderr.txt`,
//!   `status.json`, `media/…` and `manifest.json`. A re-run is byte-identical.
//!
//! Unix only (like the writer capture) and never invoked from `cargo xtask verify`
//! or CI. `expected.qmd`, which plan P3 adds to each fixture directory, is never
//! deleted, listed or hashed here.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::capture_pandoc_recordings::{SOURCE_DATE_EPOCH, check_pandoc, pinned_version};
use crate::switch_task::current_worktree_root;

const IMPORT_DIR: &str = "crates/quarto-core/tests/fixtures/import-recordings";
const CONSTANTS: &str = "resources/pandoc-wasm.json";

/// Files the capture writes (and so may delete before re-writing). `source.<ext>`
/// and `expected.qmd` are inputs, not outputs.
const GENERATED: &[&str] = &[
    "argv.json",
    "pandoc.json",
    "stderr.txt",
    "status.json",
    "manifest.json",
    "media",
];
/// Files in a fixture directory that the manifest never lists.
const MANIFEST_SKIP: &[&str] = &["manifest.json", "expected.qmd"];

/// One fixture: `<name>-<format>/` holding `source.<ext>`.
pub struct Fixture {
    pub name: &'static str,
    pub format: &'static str,
    pub ext: &'static str,
}

impl Fixture {
    pub fn dir_name(&self) -> String {
        format!("{}-{}", self.name, self.format)
    }
}

pub const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "basic",
        format: "docx",
        ext: "docx",
    },
    Fixture {
        name: "basic",
        format: "odt",
        ext: "odt",
    },
    Fixture {
        name: "basic",
        format: "rtf",
        ext: "rtf",
    },
    Fixture {
        name: "basic",
        format: "epub",
        ext: "epub",
    },
    Fixture {
        name: "basic",
        format: "pptx",
        ext: "pptx",
    },
    Fixture {
        name: "track-changes",
        format: "docx",
        ext: "docx",
    },
    Fixture {
        name: "writer-bugs",
        format: "docx",
        ext: "docx",
    },
    Fixture {
        name: "highlights",
        format: "docx",
        ext: "docx",
    },
    Fixture {
        name: "images",
        format: "docx",
        ext: "docx",
    },
    Fixture {
        name: "corrupt",
        format: "docx",
        ext: "docx",
    },
    Fixture {
        name: "emf",
        format: "docx",
        ext: "docx",
    },
    Fixture {
        name: "comments-edge",
        format: "docx",
        ext: "docx",
    },
    Fixture {
        name: "quarto-made",
        format: "docx",
        ext: "docx",
    },
];

/// Interface 2's argv with `base` as the directory holding `source.<ext>`, `out.json`
/// and `media/`. The canonical form uses `<share_root>/import`; the native run uses a
/// temp dir and is relocated afterwards.
pub fn import_argv(format: &str, ext: &str, base: &str) -> Vec<String> {
    let mut argv = vec!["pandoc".to_string(), "-f".to_string(), format.to_string()];
    if format == "docx" {
        argv.push("--track-changes=all".to_string());
    }
    argv.extend([
        "-t".to_string(),
        "json".to_string(),
        format!("--extract-media={base}/media"),
        "-o".to_string(),
        format!("{base}/out.json"),
        format!("{base}/source.{ext}"),
    ]);
    argv
}

/// Replaces every occurrence of the temp dir with the canonical import dir and fails if
/// any survives (a different spelling of the temp path would otherwise leak into a fixture).
fn relocate(text: &str, tmp: &str, canonical: &str) -> Result<String> {
    let out = text.replace(tmp, canonical);
    if out.contains(tmp) {
        bail!("temp path {tmp} survived relocation");
    }
    Ok(out)
}

/// Pretty-prints JSON with every object's keys sorted, plus a trailing newline.
fn pretty_sorted(value: &serde_json::Value) -> String {
    use serde_json::Value;
    fn sort(v: &Value) -> Value {
        match v {
            Value::Object(m) => {
                let mut keys: Vec<_> = m.keys().collect();
                keys.sort();
                let mut out = serde_json::Map::new();
                for k in keys {
                    out.insert(k.clone(), sort(&m[k]));
                }
                Value::Object(out)
            }
            Value::Array(a) => Value::Array(a.iter().map(sort).collect()),
            other => other.clone(),
        }
    }
    let mut s = serde_json::to_string_pretty(&sort(value)).expect("serializable");
    s.push('\n');
    s
}

fn share_root(root: &Path) -> Result<String> {
    let text = std::fs::read_to_string(root.join(CONSTANTS))?;
    let json: serde_json::Value = serde_json::from_str(&text)?;
    Ok(json["share_root"]
        .as_str()
        .context("share_root")?
        .to_string())
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(from).sort_by_file_name() {
        let entry = entry?;
        let rel = entry.path().strip_prefix(from)?;
        let dst = to.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&dst)?;
        } else if entry.file_type().is_file() {
            if let Some(p) = dst.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::copy(entry.path(), &dst)?;
        }
    }
    Ok(())
}

/// Runs the pinned native pandoc over one fixture's `source.<ext>` and rewrites the
/// fixture's generated files.
fn capture_one(fixture: &Fixture, dir: &Path, pandoc: &Path, canonical: &str) -> Result<()> {
    let source = dir.join(format!("source.{}", fixture.ext));
    if !source.is_file() {
        bail!(
            "{} missing: run `cargo xtask capture-import-recordings generate`",
            source.display()
        );
    }
    let tmp = tempfile::tempdir()?;
    // The canonical path: pandoc is handed (and so reports) exactly this spelling.
    let base = tmp.path().canonicalize()?;
    let base_str = base.to_str().context("temp dir is not UTF-8")?.to_string();
    std::fs::copy(&source, base.join(format!("source.{}", fixture.ext)))?;

    let argv = import_argv(fixture.format, fixture.ext, &base_str);
    let out = Command::new(pandoc)
        .args(&argv[1..])
        .current_dir(&base)
        .env("SOURCE_DATE_EPOCH", SOURCE_DATE_EPOCH)
        .output()
        .with_context(|| format!("spawning pandoc for {}", fixture.dir_name()))?;
    let status = out.status.code().context("pandoc was killed by a signal")?;

    for name in GENERATED {
        let p = dir.join(name);
        if p.is_dir() {
            std::fs::remove_dir_all(&p)?;
        } else if p.exists() {
            std::fs::remove_file(&p)?;
        }
    }

    let canonical_argv = import_argv(fixture.format, fixture.ext, canonical);
    std::fs::write(
        dir.join("argv.json"),
        pretty_sorted(&serde_json::json!(canonical_argv)),
    )?;
    std::fs::write(
        dir.join("status.json"),
        pretty_sorted(&serde_json::json!({ "status": status })),
    )?;
    let stderr = relocate(&String::from_utf8_lossy(&out.stderr), &base_str, canonical)?;
    std::fs::write(dir.join("stderr.txt"), stderr)?;

    let out_json = base.join("out.json");
    if status == 0 {
        let text = relocate(&std::fs::read_to_string(&out_json)?, &base_str, canonical)?;
        let value: serde_json::Value = serde_json::from_str(&text)?;
        std::fs::write(dir.join("pandoc.json"), pretty_sorted(&value))?;
    }
    let media = base.join("media");
    if media.is_dir() {
        copy_dir(&media, &dir.join("media"))?;
    }

    let manifest = quarto_pandoc_recording::tree::list_files(dir, MANIFEST_SKIP)?;
    std::fs::write(
        dir.join("manifest.json"),
        pretty_sorted(&serde_json::to_value(manifest)?),
    )?;
    Ok(())
}

fn pandoc_run(pandoc: &Path, cwd: &Path, args: &[&str]) -> Result<()> {
    let out = Command::new(pandoc)
        .args(args)
        .current_dir(cwd)
        .env("SOURCE_DATE_EPOCH", SOURCE_DATE_EPOCH)
        .output()?;
    if !out.status.success() {
        bail!(
            "pandoc {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

/// Rewrites a docx (zip): `edit(name, bytes)` may change an entry's bytes, and `extra`
/// entries are appended. Entry order, compression and timestamps are kept, so the output
/// is deterministic.
fn rewrite_zip(
    bytes: &[u8],
    mut edit: impl FnMut(&str, Vec<u8>) -> Result<Vec<u8>>,
    extra: &[(&str, Vec<u8>)],
) -> Result<Vec<u8>> {
    use std::io::{Cursor, Read, Write};
    use zip::write::SimpleFileOptions;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut last = None;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        let mut opts = SimpleFileOptions::default().compression_method(entry.compression());
        if let Some(t) = entry.last_modified() {
            opts = opts.last_modified_time(t);
            last = Some(t);
        }
        out.start_file(&name, opts)?;
        out.write_all(&edit(&name, data)?)?;
    }
    for (name, data) in extra {
        let mut opts =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        if let Some(t) = last {
            opts = opts.last_modified_time(t);
        }
        out.start_file(*name, opts)?;
        out.write_all(data)?;
    }
    Ok(out.finish()?.into_inner())
}

/// Puts `<w:rPr><w:ins|w:del …/></w:rPr>` in the `w:pPr` of the paragraph containing
/// `marker` (the way Word records a paragraph-mark change; pandoc cannot write one).
fn mark_paragraph(
    xml: &str,
    marker: &str,
    kind: &str,
    id: u32,
    author: &str,
    date: &str,
) -> Result<String> {
    let at = xml
        .find(marker)
        .with_context(|| format!("marker {marker:?} not in document.xml"))?;
    let ppr = xml[..at]
        .rfind("<w:pPr>")
        .context("paragraph has no w:pPr")?;
    let end = ppr + xml[ppr..].find("</w:pPr>").context("unterminated w:pPr")?;
    let mark =
        format!(r#"<w:rPr><w:{kind} w:id="{id}" w:author="{author}" w:date="{date}"/></w:rPr>"#);
    Ok(format!("{}{mark}{}", &xml[..end], &xml[end..]))
}

fn patch_document(docx: &[u8], f: impl Fn(String) -> Result<String>) -> Result<Vec<u8>> {
    rewrite_zip(
        docx,
        |name, data| {
            if name != "word/document.xml" {
                return Ok(data);
            }
            Ok(f(String::from_utf8(data)?)?.into_bytes())
        },
        &[],
    )
}

/// Word's `commentsExtended.xml` stores a comment's parent and resolved state against the
/// `w14:paraId` of its last paragraph, which pandoc neither writes nor reads.
fn comment_para_id(id: u32) -> String {
    format!("{:08X}", 0x1000_0000 + id)
}

/// Gives each comment's last paragraph a `w14:paraId` and adds a second paragraph to
/// comment 3 (multi-paragraph text).
fn patch_comments(xml: &str) -> Result<String> {
    let mut out = String::new();
    let mut rest = xml.replacen(
        "<w:comments ",
        r#"<w:comments xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" "#,
        1,
    );
    let mut rest = rest.as_mut_str().to_string();
    while let Some(start) = rest.find("<w:comment ") {
        out.push_str(&rest[..start]);
        let tail = rest[start..].to_string();
        let end = tail
            .find("</w:comment>")
            .context("unterminated w:comment")?
            + "</w:comment>".len();
        let mut comment = tail[..end].to_string();
        let id: u32 = comment
            .split("w:id=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .and_then(|s| s.parse().ok())
            .context("comment without a numeric id")?;
        if id == 3 {
            let at = comment.rfind("</w:comment>").expect("checked above");
            comment.insert_str(
                at,
                r#"<w:p><w:pPr><w:pStyle w:val="CommentText" /></w:pPr><w:r><w:t xml:space="preserve">Second paragraph of the comment.</w:t></w:r></w:p>"#,
            );
        }
        let p = comment
            .rfind("<w:p>")
            .context("comment without a paragraph")?;
        comment.replace_range(
            p..p + "<w:p>".len(),
            &format!(r#"<w:p w14:paraId="{}">"#, comment_para_id(id)),
        );
        out.push_str(&comment);
        rest = tail[end..].to_string();
    }
    out.push_str(&rest);
    Ok(out)
}

/// Comment 1 replies to comment 0; comment 2 is resolved.
fn comments_extended_xml() -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w15:commentsEx xmlns:w15="http://schemas.microsoft.com/office/word/2012/wordml">"#,
    );
    for id in 0..=6u32 {
        let parent = if id == 1 {
            format!(r#" w15:paraIdParent="{}""#, comment_para_id(0))
        } else {
            String::new()
        };
        let done = if id == 2 { 1 } else { 0 };
        s.push_str(&format!(
            r#"<w15:commentEx w15:paraId="{}"{parent} w15:done="{done}"/>"#,
            comment_para_id(id)
        ));
    }
    s.push_str("</w15:commentsEx>");
    s
}

/// The `comments-edge` docx: pandoc writes the comment spans from markdown; the xtask
/// patches in what pandoc cannot write (paragraph-mark change, `commentsExtended.xml`,
/// a second paragraph in one comment).
fn comments_edge_docx(docx: &[u8]) -> Result<Vec<u8>> {
    rewrite_zip(
        docx,
        |name, data| {
            let text = || String::from_utf8(data.clone());
            Ok(match name {
                "word/document.xml" => {
                    mark_paragraph(&text()?, "Case c", "ins", 92, "Bob", "2026-09-06T10:00:00Z")?.into_bytes()
                }
                "word/comments.xml" => patch_comments(&text()?)?.into_bytes(),
                "[Content_Types].xml" => text()?
                    .replacen(
                        "</Types>",
                        r#"<Override PartName="/word/commentsExtended.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.commentsExtended+xml"/></Types>"#,
                        1,
                    )
                    .into_bytes(),
                "word/_rels/document.xml.rels" => text()?
                    .replacen(
                        "</Relationships>",
                        r#"<Relationship Type="http://schemas.microsoft.com/office/2011/relationships/commentsExtended" Id="rId99" Target="commentsExtended.xml" /></Relationships>"#,
                        1,
                    )
                    .into_bytes(),
                _ => data,
            })
        },
        &[(
            "word/commentsExtended.xml",
            comments_extended_xml().into_bytes(),
        )],
    )
}

/// A 132-byte EMF: header, one `EMR_RECTANGLE`, `EMR_EOF`.
fn emf_bytes() -> Vec<u8> {
    let mut b = Vec::new();
    let u32le = |b: &mut Vec<u8>, v: u32| b.extend_from_slice(&v.to_le_bytes());
    let i32le = |b: &mut Vec<u8>, v: i32| b.extend_from_slice(&v.to_le_bytes());
    // EMR_HEADER (88 bytes)
    u32le(&mut b, 1);
    u32le(&mut b, 88);
    for v in [0, 0, 99, 49] {
        i32le(&mut b, v); // bounds, device units
    }
    for v in [0, 0, 2646, 1323] {
        i32le(&mut b, v); // frame, 0.01 mm
    }
    u32le(&mut b, 0x464D_4520); // " EMF"
    u32le(&mut b, 0x0001_0000);
    u32le(&mut b, 132); // bytes
    u32le(&mut b, 3); // records
    b.extend_from_slice(&1u16.to_le_bytes()); // handles
    b.extend_from_slice(&0u16.to_le_bytes());
    u32le(&mut b, 0); // nDescription
    u32le(&mut b, 0); // offDescription
    u32le(&mut b, 0); // nPalEntries
    i32le(&mut b, 1024); // device px
    i32le(&mut b, 768);
    i32le(&mut b, 320); // device mm
    i32le(&mut b, 240);
    // EMR_RECTANGLE (24 bytes)
    u32le(&mut b, 43);
    u32le(&mut b, 24);
    for v in [10, 10, 90, 40] {
        i32le(&mut b, v);
    }
    // EMR_EOF (20 bytes)
    u32le(&mut b, 14);
    u32le(&mut b, 20);
    u32le(&mut b, 0);
    u32le(&mut b, 16);
    u32le(&mut b, 20);
    assert_eq!(b.len(), 132);
    b
}

/// A placeable WMF with one `META_RECTANGLE` and `META_EOF`.
fn wmf_bytes() -> Vec<u8> {
    let mut b = Vec::new();
    let (right, bottom) = (100i16, 50i16);
    let mut words: Vec<u16> = vec![
        0xCDD7,
        0x9AC6,
        0,
        0,
        0,
        0,
        right as u16,
        bottom as u16,
        1440,
    ];
    // placeable header: key(4) hmf(2) bbox(8) inch(2) reserved(4) checksum(2) = 22 bytes
    let checksum = words.iter().fold(0u16, |a, w| a ^ w);
    for w in words.drain(..) {
        b.extend_from_slice(&w.to_le_bytes());
    }
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&checksum.to_le_bytes());
    // WMF header: type, header size (words), version, size (words, u32), objects, max record (words, u32), members
    let header_words = 9u32;
    let rect_words = 7u32;
    let eof_words = 3u32;
    let total = header_words + rect_words + eof_words;
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&9u16.to_le_bytes());
    b.extend_from_slice(&0x0300u16.to_le_bytes());
    b.extend_from_slice(&total.to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    b.extend_from_slice(&rect_words.to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    // META_RECTANGLE: size, function, bottom, right, top, left
    b.extend_from_slice(&rect_words.to_le_bytes());
    b.extend_from_slice(&0x041Bu16.to_le_bytes());
    for v in [40i16, 90, 10, 10] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    // META_EOF
    b.extend_from_slice(&eof_words.to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    b
}

/// Builds `source.<ext>` for every fixture from `sources/`.
fn generate(import_dir: &Path, pandoc: &Path) -> Result<()> {
    let sources = import_dir.join("sources");
    let fixtures = import_dir.parent().context("fixtures dir")?;
    let pandoc_to = |md: &str, out: &Path| -> Result<Vec<u8>> {
        pandoc_run(
            pandoc,
            &sources,
            &["-s", md, "-o", out.to_str().context("path is not UTF-8")?],
        )?;
        Ok(std::fs::read(out)?)
    };
    for fixture in FIXTURES {
        let dir = import_dir.join(fixture.dir_name());
        std::fs::create_dir_all(&dir)?;
        let dest = dir.join(format!("source.{}", fixture.ext));
        println!("Generating {}", dest.display());
        let bytes = match (fixture.name, fixture.format) {
            ("basic", "epub") => pandoc_to("basic-epub.md", &dest)?,
            ("basic", "pptx") => pandoc_to("basic-pptx.md", &dest)?,
            ("basic", _) => pandoc_to("basic.md", &dest)?,
            ("writer-bugs" | "highlights" | "images", "docx") => {
                pandoc_to(&format!("{}.md", fixture.name), &dest)?
            }
            ("track-changes", "docx") => {
                let docx = pandoc_to("track-changes.md", &dest)?;
                patch_document(&docx, |xml| {
                    let xml = mark_paragraph(
                        &xml,
                        "Para five",
                        "ins",
                        90,
                        "Bob",
                        "2026-09-05T10:00:00Z",
                    )?;
                    mark_paragraph(&xml, "Para six", "del", 91, "Bob", "2026-09-05T11:00:00Z")
                })?
            }
            ("comments-edge", "docx") => {
                comments_edge_docx(&pandoc_to("comments-edge.md", &dest)?)?
            }
            ("corrupt", "docx") => {
                let basic = std::fs::read(import_dir.join("basic-docx/source.docx"))
                    .context("corrupt-docx truncates basic-docx: generate basic first")?;
                basic[..basic.len() * 6 / 10].to_vec()
            }
            ("emf", "docx") => {
                let tmp = tempfile::tempdir()?;
                std::fs::write(tmp.path().join("drawing.emf"), emf_bytes())?;
                std::fs::write(tmp.path().join("drawing.wmf"), wmf_bytes())?;
                std::fs::write(
                    tmp.path().join("emf.md"),
                    "An EMF image:\n\n![A drawn rectangle (EMF)](drawing.emf)\n\nA WMF image:\n\n![A drawn rectangle (WMF)](drawing.wmf)\n",
                )?;
                pandoc_run(
                    pandoc,
                    tmp.path(),
                    &["-s", "emf.md", "-o", dest.to_str().context("path")?],
                )?;
                std::fs::read(&dest)?
            }
            ("quarto-made", "docx") => {
                let from = fixtures
                    .join("pandoc-recordings/recordings/callouts-docx/reference/callouts.docx");
                std::fs::read(&from).with_context(|| format!("reading {}", from.display()))?
            }
            (n, f) => bail!("no generator for {n}-{f}"),
        };
        std::fs::write(&dest, bytes)?;
    }
    Ok(())
}

pub fn run(generate_only: bool) -> Result<()> {
    #[cfg(not(unix))]
    {
        let _ = generate_only;
        bail!("capture-import-recordings is Unix-only (like capture-pandoc-recordings)");
    }
    #[cfg(unix)]
    {
        let root = current_worktree_root()?;
        std::env::set_current_dir(&root)?;
        let pandoc = check_pandoc(&pinned_version(&root)?)?;
        let import_dir: PathBuf = root.join(IMPORT_DIR);
        if generate_only {
            return generate(&import_dir, &pandoc);
        }
        let canonical = format!("{}/import", share_root(&root)?);
        for fixture in FIXTURES {
            println!("Capturing {}", fixture.dir_name());
            capture_one(
                fixture,
                &import_dir.join(fixture.dir_name()),
                &pandoc,
                &canonical,
            )?;
        }
        println!("Wrote import recordings to {}", import_dir.display());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fixture_file_exists() {
        let root = current_worktree_root().expect("worktree root");
        let base = root.join(IMPORT_DIR);
        for fixture in FIXTURES {
            let dir = base.join(fixture.dir_name());
            let source = dir.join(format!("source.{}", fixture.ext));
            assert!(source.exists(), "{} missing", source.display());
        }
    }

    #[test]
    fn fixture_names_are_unique() {
        let mut names: Vec<_> = FIXTURES.iter().map(Fixture::dir_name).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n);
    }

    #[test]
    fn docx_argv_tracks_changes_and_others_do_not() {
        let docx = import_argv("docx", "docx", "/__q2_share__/import");
        assert_eq!(
            docx,
            [
                "pandoc",
                "-f",
                "docx",
                "--track-changes=all",
                "-t",
                "json",
                "--extract-media=/__q2_share__/import/media",
                "-o",
                "/__q2_share__/import/out.json",
                "/__q2_share__/import/source.docx",
            ]
        );
        assert!(!import_argv("odt", "odt", "/b").contains(&"--track-changes=all".to_string()));
    }

    #[test]
    fn relocation_rewrites_every_occurrence_and_rejects_leaks() {
        let out = relocate(
            "a /tmp/x/media/i.png b /tmp/x/out",
            "/tmp/x",
            "/__q2_share__/import",
        )
        .unwrap();
        assert_eq!(
            out,
            "a /__q2_share__/import/media/i.png b /__q2_share__/import/out"
        );
        // Replacement text that itself contains the temp path can never be fully relocated.
        assert!(relocate("/tmp/x", "/tmp/x", "/tmp/x/y").is_err());
    }

    #[test]
    fn pretty_json_sorts_keys_at_every_depth() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"b":1,"a":{"d":[{"z":1,"y":2}],"c":0}}"#).unwrap();
        assert_eq!(
            pretty_sorted(&v),
            "{\n  \"a\": {\n    \"c\": 0,\n    \"d\": [\n      {\n        \"y\": 2,\n        \"z\": 1\n      }\n    ]\n  },\n  \"b\": 1\n}\n"
        );
    }

    /// Like the other captures, this is a dev command: nothing automated runs it.
    #[test]
    fn verify_does_not_invoke_the_capture() {
        let verify = include_str!("verify.rs");
        assert!(!verify.contains("capture_import_recordings"));
    }
}
