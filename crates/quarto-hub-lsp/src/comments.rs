//! Editorial comments in one qmd file's text.

use std::path::Path;

use pampa::readers::qmd;
use quarto_core::document_profile::DocumentProfile;
use serde::Serialize;

pub use quarto_core::document_profile::JsonComment;

/// A file whose text the qmd reader rejected, for example while an author
/// is halfway through typing a construct.
///
/// Reported instead of an empty comment list, so that a client never takes
/// "this file does not parse right now" for "every comment in it was
/// resolved".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParseFailure {
    /// The reader's diagnostics, each as `code: title` (or the title alone
    /// when the diagnostic has no code).
    pub messages: Vec<String>,
}

/// The editorial comments in `text`, read as the qmd file at `path`, in
/// source order.
///
/// Comments come from `DocumentProfile::extract`, the same summary the
/// render pipeline computes, so a comment is anything the qmd reader turns
/// into a `quarto-edit-comment` span or div. The file is read on its own:
/// a comment inside an `{{{< include >}}}` target is reported for that
/// target, not for the file that includes it. Positions are 1-based.
pub fn comments_in_qmd(path: &str, text: &str) -> Result<Vec<JsonComment>, ParseFailure> {
    let mut reader_output = std::io::sink();
    let (ast, ast_context, _warnings) =
        qmd::read(text.as_bytes(), false, path, &mut reader_output, true, None).map_err(
            |diagnostics| ParseFailure {
                messages: diagnostics
                    .iter()
                    .map(|d| match &d.code {
                        Some(code) => format!("{code}: {}", d.title),
                        None => d.title.clone(),
                    })
                    .collect(),
            },
        )?;
    // Only the comments are read, so the output href and format do not
    // matter; "html" keeps the extraction on its most exercised path.
    let profile = DocumentProfile::extract(&ast, Path::new(path), "", "html");
    Ok(profile
        .comments
        .iter()
        .map(|comment| comment.to_json(&ast_context.source_context))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(path: &str, text: &str) -> Vec<JsonComment> {
        comments_in_qmd(path, text).expect("the fixture parses")
    }

    #[test]
    fn a_comment_reports_its_text_file_and_one_based_position() {
        let comments = ok("index.qmd", "Intro.\n\nProse [>> cite this ] end.\n");
        assert_eq!(comments.len(), 1);
        let c = &comments[0];
        assert_eq!(c.text, "cite this");
        assert_eq!(c.file.as_deref(), Some("index.qmd"));
        assert_eq!(c.start_line, Some(3));
        assert_eq!(c.start_column, Some(7));
        assert_eq!(c.end_line, Some(3));
        assert_eq!(c.author, None);
    }

    #[test]
    fn comments_are_listed_in_source_order() {
        let comments = ok("a.qmd", "[>> first ]\n\n# Heading\n\n[>> second ]\n");
        let texts: Vec<&str> = comments.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, vec!["first", "second"]);
    }

    #[test]
    fn a_stamped_comment_carries_its_author_and_other_attributes() {
        let comments = ok(
            "a.qmd",
            "Prose [>> fix ]{author=\"u-1\" date=\"2026-11-09\" thread=\"t-7\"} end.\n",
        );
        assert_eq!(comments.len(), 1);
        let c = &comments[0];
        assert_eq!(c.author.as_deref(), Some("u-1"));
        assert_eq!(c.date.as_deref(), Some("2026-11-09"));
        assert_eq!(
            c.attributes,
            vec![("thread".to_string(), "t-7".to_string())]
        );
    }

    #[test]
    fn a_document_without_comments_reports_none() {
        assert!(ok("a.qmd", "---\ntitle: T\n---\n\nNo comments.\n").is_empty());
    }

    #[test]
    fn a_mark_still_being_typed_is_a_parse_failure_not_an_empty_list() {
        // The author has typed `[>> ` but not yet the closing `]`.
        let failure = comments_in_qmd("a.qmd", "Prose [>> half typed\n")
            .expect_err("an unclosed span must not parse");
        assert_eq!(failure.messages.len(), 1);
        assert!(
            failure.messages[0].starts_with("Q-2-21: "),
            "expected the unclosed-comment diagnostic, got {:?}",
            failure.messages
        );
    }

    #[test]
    fn an_empty_mark_is_reported_with_empty_text() {
        // A comment the author has started typing. Filtering it out is the
        // client's policy, not the server's.
        let comments = ok("a.qmd", "Prose [>> ] end.\n");
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text, "");
    }
}
