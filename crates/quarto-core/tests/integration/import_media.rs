//! The media plan and the image-link rewrite (P3 T6, I12, I16), through `finish_import`.

use quarto_core::import::finish_import;
use serde_json::{Value, json};

use super::import_support as support;

const EXTRACT: &str = "/__q2_share__/import/media";

fn image(url: &str) -> Value {
    json!({"t": "Image", "c": [["", [], []], [{"t": "Str", "c": "alt"}], [url, ""]]})
}

fn doc_with_images(urls: &[&str]) -> String {
    let inlines: Vec<Value> = urls.iter().map(|u| image(u)).collect();
    json!({
        "pandoc-api-version": [1, 23, 1],
        "meta": {},
        "blocks": [{"t": "Para", "c": inlines}]
    })
    .to_string()
}

fn stored(path: &str, sha: &str, ext: &str) -> Value {
    json!({"pandoc_path": format!("{EXTRACT}/{path}"), "status": "stored", "sha256": sha, "ext": ext})
}

fn sha(c: char) -> String {
    c.to_string().repeat(64)
}

fn finish(json: &str, target: &str, manifest: &[Value]) -> quarto_core::import::FinishOutcome {
    finish_import(
        json,
        "",
        target,
        &support::manifest_json(manifest),
        Some("docx"),
    )
}

fn codes(out: &quarto_core::import::FinishOutcome) -> Vec<String> {
    out.diagnostics
        .iter()
        .filter_map(|d| d.code.clone())
        .collect()
}

fn plan(out: &quarto_core::import::FinishOutcome) -> Vec<(String, String)> {
    out.media_plan
        .as_ref()
        .unwrap()
        .iter()
        .map(|e| (e.pandoc_path.clone(), e.project_path.clone()))
        .collect()
}

#[test]
fn images_docx_dedupes_the_twice_used_image() {
    let name = "images-docx";
    let out = finish_import(
        &support::pandoc_json(name),
        "",
        &support::target_qmd_path(name),
        &support::manifest_json(&support::media_manifest(name)),
        Some("docx"),
    );
    assert!(out.success, "{:?}", out.diagnostics);
    let manifest = support::media_manifest(name);
    let png = manifest.iter().find(|e| e["ext"] == "png").unwrap();
    let jpg = manifest.iter().find(|e| e["ext"] == "jpg").unwrap();
    let qmd = out.qmd.clone().unwrap();
    let png_link = format!("images-docx_media/{}.png", support::sha12(png));
    assert_eq!(qmd.matches(&png_link).count(), 2, "{qmd}");
    assert!(
        qmd.contains(&format!("images-docx_media/{}.jpg", support::sha12(jpg))),
        "{qmd}"
    );
    assert_eq!(plan(&out).len(), 2, "one file per distinct image");
    assert!(codes(&out).is_empty(), "{:?}", out.diagnostics);
}

#[test]
fn odt_files_with_one_hash_merge_into_one_plan_entry_and_two_links() {
    // The same image used twice extracts as Pictures/0.png and Pictures/1.png.
    let json = doc_with_images(&[
        &format!("{EXTRACT}/Pictures/0.png"),
        &format!("{EXTRACT}/Pictures/1.png"),
    ]);
    let manifest = [
        stored("Pictures/0.png", &sha('a'), "png"),
        stored("Pictures/1.png", &sha('a'), "png"),
    ];
    let out = finish(&json, "doc.qmd", &manifest);
    assert!(out.success, "{:?}", out.diagnostics);
    assert_eq!(
        plan(&out),
        [(
            format!("{EXTRACT}/Pictures/0.png"),
            "doc_media/aaaaaaaaaaaa.png".to_string()
        )]
    );
    assert_eq!(
        out.qmd
            .as_deref()
            .unwrap()
            .matches("doc_media/aaaaaaaaaaaa.png")
            .count(),
        2
    );
}

#[test]
fn project_path_includes_the_folder_but_the_link_does_not() {
    let json = doc_with_images(&[&format!("{EXTRACT}/media/rId9.png")]);
    let manifest = [stored("media/rId9.png", &sha('b'), "png")];
    let root = finish(&json, "report.qmd", &manifest);
    assert_eq!(plan(&root)[0].1, "report_media/bbbbbbbbbbbb.png");
    assert!(
        root.qmd
            .unwrap()
            .contains("](report_media/bbbbbbbbbbbb.png)")
    );

    let nested = finish(&json, "chapters/part one/report.qmd", &manifest);
    assert_eq!(
        plan(&nested)[0].1,
        "chapters/part one/report_media/bbbbbbbbbbbb.png"
    );
    let qmd = nested.qmd.unwrap();
    assert!(qmd.contains("](report_media/bbbbbbbbbbbb.png)"), "{qmd}");
    assert!(!qmd.contains("chapters"), "{qmd}");
}

#[test]
fn each_formats_extract_layout_maps_and_unrelated_targets_are_left_alone() {
    for path in [
        "media/rId9.png",                               // docx
        "Pictures/0.png",                               // odt
        "media/file0.png",                              // epub
        "e4f416bfe8a089e022195101ec649e175f9486ca.png", // rtf, no subfolder
    ] {
        let json = doc_with_images(&[
            &format!("{EXTRACT}/{path}"),
            "https://example.com/remote.png",
            "local/figure.png",
        ]);
        let out = finish(&json, "x.qmd", &[stored(path, &sha('c'), "png")]);
        assert!(out.success, "{path}: {:?}", out.diagnostics);
        let qmd = out.qmd.unwrap();
        assert!(qmd.contains("](x_media/cccccccccccc.png)"), "{path}: {qmd}");
        assert!(qmd.contains("](https://example.com/remote.png)"), "{qmd}");
        assert!(qmd.contains("](local/figure.png)"), "{qmd}");
    }
}

#[test]
fn pandoc_paths_with_spaces_and_non_ascii_are_matched_and_the_link_is_encoded() {
    let path = "media/my picture (1) #é.png";
    let json = doc_with_images(&[&format!("{EXTRACT}/{path}")]);
    let out = finish(&json, "x.qmd", &[stored(path, &sha('d'), "png")]);
    assert!(out.success, "{:?}", out.diagnostics);
    // The stored name is the hash, so the encoded characters come from the stem only.
    assert!(out.qmd.unwrap().contains("](x_media/dddddddddddd.png)"));

    let skipped = json!({"pandoc_path": format!("{EXTRACT}/{path}"), "status": "skipped",
        "reason": "too-large", "size": 12_000_000});
    let out = finish(&json, "x.qmd", &[skipped]);
    let qmd = out.qmd.unwrap();
    assert!(
        qmd.contains("](x_media/my%20picture%20%281%29%20%23%C3%A9.png)"),
        "{qmd}"
    );
}

#[test]
fn a_stem_with_a_space_gives_a_link_the_reader_accepts_and_decodes_to_the_plan() {
    let json = doc_with_images(&[&format!("{EXTRACT}/media/rId9.png")]);
    let out = finish(
        &json,
        "dir/report 2.qmd",
        &[stored("media/rId9.png", &sha('e'), "png")],
    );
    assert!(out.success, "{:?}", out.diagnostics);
    let qmd = out.qmd.clone().unwrap();
    assert!(
        qmd.contains("](report%202_media/eeeeeeeeeeee.png)"),
        "{qmd}"
    );
    // Re-reads with no Q-2-33 (a space in a link target).
    let pandoc = support::reread(&qmd, false);
    let native = support::native(&pandoc);
    assert!(
        native.contains("report%202_media/eeeeeeeeeeee.png"),
        "{native}"
    );
    // The target decodes to the planned path's tail.
    let decoded = percent_encoding::percent_decode_str("report%202_media/eeeeeeeeeeee.png")
        .decode_utf8()
        .unwrap()
        .to_string();
    assert_eq!(
        format!("dir/{decoded}"),
        plan(&out)[0].1,
        "the link, decoded and joined to the qmd's folder, is the planned path"
    );
}

#[test]
fn a_skipped_entry_keeps_a_deliberately_broken_link_and_q_24_8() {
    let json = doc_with_images(&[&format!("{EXTRACT}/media/rId9.bmp")]);
    let skipped = json!({"pandoc_path": format!("{EXTRACT}/media/rId9.bmp"), "status": "skipped",
        "reason": "too-large", "size": 30_000_000});
    let out = finish(&json, "x.qmd", &[skipped]);
    assert!(out.success);
    assert!(out.media_plan.as_ref().unwrap().is_empty());
    assert!(out.qmd.as_deref().unwrap().contains("](x_media/rId9.bmp)"));
    let d = out
        .diagnostics
        .iter()
        .find(|d| d.code.as_deref() == Some("Q-24-8"))
        .unwrap();
    assert!(format!("{d:?}").contains("rId9.bmp"));
}

#[test]
fn a_converted_entry_is_reported_once_with_a_count() {
    let json = doc_with_images(&[
        &format!("{EXTRACT}/media/rId9.emf"),
        &format!("{EXTRACT}/media/rId12.wmf"),
    ]);
    let mut a = stored("media/rId9.emf", &sha('1'), "png");
    a["converted_from"] = json!("emf");
    let mut b = stored("media/rId12.wmf", &sha('2'), "png");
    b["converted_from"] = json!("wmf");
    let out = finish(&json, "x.qmd", &[a, b]);
    assert!(out.success);
    assert_eq!(codes(&out), ["Q-24-10"], "{:?}", out.diagnostics);
    assert!(format!("{:?}", out.diagnostics[0]).contains("2 EMF/WMF images"));
}

#[test]
fn a_failed_conversion_is_q_24_9_only_and_links_the_original_extension() {
    let name = "emf-docx";
    let manifest = support::media_manifest_failed_conversion(name);
    let out = finish_import(
        &support::pandoc_json(name),
        "",
        &support::target_qmd_path(name),
        &support::manifest_json(&manifest),
        Some("docx"),
    );
    assert!(out.success, "{:?}", out.diagnostics);
    assert_eq!(
        codes(&out),
        ["Q-24-9", "Q-24-9"],
        "no Q-24-11 for these files"
    );
    let emf = manifest.iter().find(|e| e["ext"] == "emf").unwrap();
    assert!(
        out.qmd
            .as_deref()
            .unwrap()
            .contains(&format!("emf-docx_media/{}.emf", support::sha12(emf)))
    );
}

#[test]
fn a_stored_format_browsers_cannot_show_is_q_24_11_but_displayable_ones_are_not() {
    let json = doc_with_images(&[
        &format!("{EXTRACT}/media/a.tiff"),
        &format!("{EXTRACT}/media/b.PNG"),
        &format!("{EXTRACT}/media/c.svg"),
    ]);
    let out = finish(
        &json,
        "x.qmd",
        &[
            stored("media/a.tiff", &sha('3'), "tiff"),
            stored("media/b.PNG", &sha('4'), "PNG"),
            stored("media/c.svg", &sha('5'), "svg"),
        ],
    );
    assert!(out.success);
    assert_eq!(codes(&out), ["Q-24-11"], "{:?}", out.diagnostics);
    assert!(format!("{:?}", out.diagnostics[0]).contains("a.tiff"));
}

#[test]
fn an_image_under_the_extract_dir_without_an_entry_is_fatal() {
    let json = doc_with_images(&[&format!("{EXTRACT}/media/rId9.png")]);
    let out = finish(&json, "x.qmd", &[]);
    assert!(!out.success);
    assert!(out.qmd.is_none() && out.media_plan.is_none());
    assert_eq!(
        out.diagnostics.last().unwrap().code.as_deref(),
        Some("Q-24-12")
    );
}

#[test]
fn a_malformed_manifest_is_fatal() {
    let out = finish_import(&doc_with_images(&[]), "", "x.qmd", "not json", None);
    assert!(!out.success && out.qmd.is_none());
    assert_eq!(
        out.diagnostics.last().unwrap().code.as_deref(),
        Some("Q-24-12")
    );
}
