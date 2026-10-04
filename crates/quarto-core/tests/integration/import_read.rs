//! `import::read_pandoc_json` (P3 T3): every recording's JSON reads, and the qmd written from
//! it, with no transform applied, reads back.

use pampa::readers;
use quarto_core::import::read_pandoc_json;

use super::import_support as support;

pub fn write_qmd(pandoc: &pampa::pandoc::Pandoc) -> String {
    let mut buf = Vec::new();
    pampa::writers::qmd::write(pandoc, &mut buf).expect("qmd write");
    String::from_utf8(buf).expect("qmd is UTF-8")
}

pub fn reads(qmd: &str) -> Result<(), String> {
    readers::qmd::read(
        qmd.as_bytes(),
        false,
        "<generated>",
        &mut std::io::sink(),
        true,
        None,
    )
    .map(|_| ())
    .map_err(|e| format!("{e:?}"))
}

/// `writer-bugs-docx` holds a definition with two paragraphs (pandoc's "Definition" style).
/// The qmd writer indents the second paragraph four spaces, which the reader rejects as an
/// indented code block (Q-2-35): the definition-list limit the epic's I18 correction records
/// and P2 left out of scope. Remove this exemption when the writer is fixed.
const KNOWN_UNREREADABLE: &[(&str, &str)] = &[("writer-bugs-docx", "Q-2-35")];

#[test]
fn every_recording_reads_and_its_untransformed_qmd_rereads() {
    for name in support::FIXTURES.iter().filter(|n| **n != "corrupt-docx") {
        let pandoc = read_pandoc_json(&support::pandoc_json(name))
            .unwrap_or_else(|d| panic!("{name}: {d:?}"));
        let qmd = write_qmd(&pandoc);
        let known = KNOWN_UNREREADABLE.iter().find(|(n, _)| n == name);
        match (reads(&qmd), known) {
            (Ok(()), None) => {}
            (Err(e), None) => {
                panic!("{name}: untransformed qmd does not re-read: {e}\n--- qmd ---\n{qmd}")
            }
            (Err(e), Some((_, code))) => {
                assert!(e.contains(code), "{name}: expected {code}, got {e}")
            }
            (Ok(()), Some(_)) => panic!("{name} now re-reads: remove it from KNOWN_UNREREADABLE"),
        }
    }
}

#[test]
fn malformed_json_is_q_24_12() {
    for bad in ["not json", "[]", "{\"blocks\": 3}"] {
        let d = read_pandoc_json(bad).unwrap_err();
        assert_eq!(d.code.as_deref(), Some("Q-24-12"), "{bad}");
    }
}
