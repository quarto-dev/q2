//! Import diagnostics (epic interface 3, subsystem Q-24 `import`).
//!
//! Every user-facing message of the import pipeline is built here, so the codes, titles and
//! hint wording live in one place. The catalog (`error_catalog.json`) holds the stable
//! per-code text; the dynamic parts (counts, file names) are built into the message.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};

use crate::pandoc_filters::diagnostics::pandoc_warning_texts;

/// `kind` strings `classify_import_failure` accepts (`RunFailureKind` in the TS host).
pub const FAILURE_PANDOC_EXIT: &str = "pandoc-exit";
pub const FAILURE_NO_OUTPUT: &str = "no-output";
pub const FAILURE_OOM: &str = "oom";
pub const FAILURE_CRASH: &str = "crash";
pub const FAILURE_TIMEOUT: &str = "timeout";
pub const FAILURE_INVALID_REQUEST: &str = "invalid-request";
pub const FAILURE_SUPERSEDED: &str = "superseded";

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Q-24-1: the file's extension is not an I1 format.
pub fn unsupported_file_type(file_name: &str, accepted: &[&str]) -> DiagnosticMessage {
    DiagnosticMessageBuilder::error("Unsupported file type")
        .with_code("Q-24-1")
        .problem(format!(
            "Can't import \"{file_name}\": its file type is not supported."
        ))
        .add_hint(format!("Import one of: {}?", accepted.join(", ")))
        .build()
}

/// Q-24-2: the source is over the size cap.
pub fn source_too_large(file_name: &str, size: u64, max: u64) -> DiagnosticMessage {
    let mib = |n: u64| format!("{:.1} MB", n as f64 / 1_048_576.0);
    DiagnosticMessageBuilder::error("File too large to import")
        .with_code("Q-24-2")
        .problem(format!(
            "\"{file_name}\" is {}, and imports are limited to {}.",
            mib(size),
            mib(max)
        ))
        .add_hint("Is there a smaller version of the file, for example with its images compressed?")
        .build()
}

/// Q-24-3: pandoc couldn't read the file.
pub fn pandoc_read_failed(message: &str, hint: Option<&str>) -> DiagnosticMessage {
    let mut b = DiagnosticMessageBuilder::error("Couldn't read the file")
        .with_code("Q-24-3")
        .problem(message.trim().to_string());
    if let Some(h) = hint {
        b = b.add_hint(h.to_string());
    }
    b.build()
}

/// Q-24-4: one pandoc reader warning.
pub fn pandoc_warning(text: &str) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Warning from the document reader")
        .with_code("Q-24-4")
        .problem(text.to_string())
        .build()
}

/// Q-24-5: comments attached to their block because the range couldn't be wrapped (I13).
pub fn comments_attached_to_block(count: usize) -> DiagnosticMessage {
    DiagnosticMessageBuilder::info("Comment ranges simplified")
        .with_code("Q-24-5")
        .problem(format!(
            "{} couldn't mark its exact range and is attached to its paragraph instead.",
            plural(count, "comment", "comments")
        ))
        .build()
}

/// Q-24-6: paragraph-mark tracked changes were dropped.
pub fn paragraph_marks_dropped(count: usize) -> DiagnosticMessage {
    DiagnosticMessageBuilder::info("Paragraph-mark changes dropped")
        .with_code("Q-24-6")
        .problem(format!(
            "{} (an inserted or deleted paragraph break) was not imported.",
            plural(
                count,
                "tracked paragraph-mark change",
                "tracked paragraph-mark changes"
            )
        ))
        .build()
}

/// Q-24-7: ordered lists whose number style is not preserved (I17).
pub fn list_styles_lost(count: usize) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("List number styles lost")
        .with_code("Q-24-7")
        .problem(format!(
            "{} used letters, roman numerals or parentheses; they are numbered 1. 2. 3. here.",
            plural(count, "numbered list", "numbered lists")
        ))
        .build()
}

/// Q-24-8: an image over the size limit was skipped (I16).
pub fn image_skipped(pandoc_path: &str, size: Option<u64>) -> DiagnosticMessage {
    let name = basename(pandoc_path);
    let how_big = size
        .map(|s| format!(" ({:.1} MB)", s as f64 / 1_048_576.0))
        .unwrap_or_default();
    DiagnosticMessageBuilder::warning("Image skipped")
        .with_code("Q-24-8")
        .problem(format!(
            "The image \"{name}\"{how_big} is too large to store; its link is left broken where the image would be."
        ))
        .build()
}

/// Q-24-9: EMF/WMF conversion failed, the original was stored (I8).
pub fn image_conversion_failed(pandoc_path: &str) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Image conversion failed")
        .with_code("Q-24-9")
        .problem(format!(
            "The image \"{}\" couldn't be converted to SVG; the original is stored, but browsers can't show it.",
            basename(pandoc_path)
        ))
        .build()
}

/// Q-24-10: images converted from EMF/WMF to SVG.
pub fn images_converted(count: usize) -> DiagnosticMessage {
    DiagnosticMessageBuilder::info("Images converted")
        .with_code("Q-24-10")
        .problem(format!(
            "{} converted to SVG.",
            plural(count, "EMF/WMF image was", "EMF/WMF images were")
        ))
        .build()
}

/// Q-24-11: an image is stored in a format browsers can't show.
pub fn image_format_not_displayable(pandoc_path: &str, ext: &str) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Image format not displayable")
        .with_code("Q-24-11")
        .problem(format!(
            "The image \"{}\" is stored as .{ext}, which browsers can't show.",
            basename(pandoc_path)
        ))
        .build()
}

/// Q-24-12: an internal import error; fatal.
pub fn internal_error(message: &str) -> DiagnosticMessage {
    DiagnosticMessageBuilder::error("Internal import error")
        .with_code("Q-24-12")
        .problem(message.to_string())
        .add_hint(
            "This is a bug in the importer; is the file one you can share with the developers?",
        )
        .build()
}

/// Q-24-13: pandoc ran out of memory or time, or crashed.
pub fn pandoc_resources_exhausted(kind: &str, detail: &str) -> DiagnosticMessage {
    let (problem, hint) = match kind {
        FAILURE_OOM => (
            "The converter ran out of memory on this file.",
            "Is there a smaller version of the file, for example with fewer or smaller images?",
        ),
        FAILURE_TIMEOUT => (
            "The converter took too long on this file and was stopped.",
            "Is there a smaller version of the file, or one split into parts?",
        ),
        _ => (
            "The converter crashed on this file.",
            "Does the file open in Word or LibreOffice? A damaged file can crash the converter.",
        ),
    };
    let mut b = DiagnosticMessageBuilder::error("The converter failed on this file")
        .with_code("Q-24-13")
        .problem(problem.to_string())
        .add_hint(hint.to_string());
    if !detail.trim().is_empty() {
        b = b.add_detail(detail.trim().to_string());
    }
    b.build()
}

/// Q-24-14: unmatched comment markers.
pub fn unmatched_comment_markers(count: usize) -> DiagnosticMessage {
    DiagnosticMessageBuilder::info("Unmatched comment markers")
        .with_code("Q-24-14")
        .problem(format!(
            "{} had no matching start or end; a start without an end became a point comment, an end without a start was removed.",
            plural(count, "comment marker", "comment markers")
        ))
        .build()
}

/// Q-24-4 for every `[WARNING]` entry of pandoc's stderr (continuation lines joined, other
/// stderr noise dropped).
pub fn warnings_from_stderr(stderr: &str) -> Vec<DiagnosticMessage> {
    pandoc_warning_texts(stderr, true)
        .into_iter()
        .map(|text| {
            let text = text.strip_prefix("[WARNING]").unwrap_or(&text).trim();
            pandoc_warning(text)
        })
        .collect()
}

/// The last path segment of a `/`-separated path.
pub fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
