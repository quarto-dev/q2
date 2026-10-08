//! Which part of a book a render covers, as an explicit option shared by every
//! runtime (the CLI flag, the wasm request option, the native routes).

use std::path::Path;

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_system_runtime::SystemRuntime;

use crate::error::QuartoError;
use crate::format::{Format, FormatIdentifier};
use crate::project::{ProjectContext, ProjectKind};

/// What the user chose for a book render. The option is always an
/// `Option<BookScope>`: `None` means the user did not choose, and behaves as
/// `Book` wherever the format consolidates a book into one file (typst, pdf,
/// epub).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookScope {
    /// The whole book as one document.
    Book,
    /// The active page alone, as a standalone document.
    Chapter,
}

/// A book's render list as scope resolution needs it: how many items bear a
/// file and whether the active file is one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BookMembership {
    pub chapters: usize,
    pub active_is_chapter: bool,
}

/// Cheap and pass-free (a pure function of the `book:` config and the file
/// system). `Err` carries the book's own error (`Q-5-34`/`Q-5-35`/`Q-5-36`).
pub fn book_membership(
    project: &ProjectContext,
    active: &Path,
    runtime: &dyn SystemRuntime,
) -> Result<BookMembership, QuartoError> {
    let files = super::book_chapter_files(project, runtime)?;
    let active_key = active
        .strip_prefix(&project.dir)
        .ok()
        .map(|rel| super::links::normalize_book_path(&rel.to_string_lossy().replace('\\', "/")));
    let mut active_is_chapter = false;
    for file in &files {
        let key = super::links::normalize_book_path(&file.to_string_lossy().replace('\\', "/"));
        if active_key.as_deref() == Some(key.as_str()) {
            active_is_chapter = true;
        }
    }
    Ok(BookMembership {
        chapters: files.len(),
        active_is_chapter,
    })
}

/// What a render actually covers once the request is resolved against the
/// project; also the response echo (`stats.book.scope`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedBookScope {
    /// The whole book as one document.
    Book,
    /// A chapter of the book, as a standalone document.
    Chapter,
    /// A page that is not a chapter of the book (or not in a book at all),
    /// as a standalone document.
    Page,
}

/// The outcome of [`resolve_book_scope`].
#[derive(Debug)]
pub struct BookScopeResolution {
    pub scope: ResolvedBookScope,
    /// `Some` for a book project whose chapter list could be read.
    pub membership: Option<BookMembership>,
    /// Info diagnostics the caller must deliver with the result: the
    /// non-member notice (`Q-5-41`) and the ignored-option notice (`Q-5-42`).
    pub diagnostics: Vec<DiagnosticMessage>,
}

/// Whether `format` can consolidate a whole book into one file (typst, which
/// also covers pdf, and epub). html, docx and pptx have no whole-book output.
pub fn format_consolidates_book(format: &Format) -> bool {
    matches!(
        format.identifier,
        FormatIdentifier::Typst | FormatIdentifier::Epub
    )
}

/// Turn (project, active file, requested scope, format) into what the render
/// covers.
///
/// - A non-book project is always `Page`, silently.
/// - A format that does not consolidate a book renders the active page alone:
///   `Chapter` for a member, `Page` otherwise, and an explicit `Some(_)` earns
///   the `Q-5-42` info diagnostic (`None` is silent).
/// - A consolidating format renders the whole book (`Book`) unless the user
///   asked for `Chapter`. A page that is not a member is `Page`; unless the
///   user asked for `Chapter` it carries the `Q-5-41` notice, so it is never
///   silently downgraded.
///
/// `active` must be absolute (the path the book's file list resolves
/// against). `Err` is the book's own list error (`Q-5-34`/`35`/`36`), returned
/// only when the render would consolidate the book; otherwise it is left to
/// the pipeline to report.
pub fn resolve_book_scope(
    project: &ProjectContext,
    active: &Path,
    scope: Option<BookScope>,
    format: &Format,
    runtime: &dyn SystemRuntime,
) -> Result<BookScopeResolution, QuartoError> {
    let none = |scope| BookScopeResolution {
        scope,
        membership: None,
        diagnostics: Vec::new(),
    };
    if project.project_kind() != ProjectKind::Book {
        return Ok(none(ResolvedBookScope::Page));
    }
    let consolidates = format_consolidates_book(format);
    let membership = match book_membership(project, active, runtime) {
        Ok(m) => Some(m),
        Err(e) if consolidates && scope != Some(BookScope::Chapter) => return Err(e),
        Err(_) => None,
    };
    let is_member = membership.is_some_and(|m| m.active_is_chapter);
    let mut diagnostics = Vec::new();
    let resolved = if !consolidates {
        if let Some(requested) = scope {
            diagnostics.push(scope_ignored_notice(requested, format));
        }
        if is_member {
            ResolvedBookScope::Chapter
        } else {
            ResolvedBookScope::Page
        }
    } else if scope == Some(BookScope::Chapter) {
        // Without a readable list a chapter request cannot be told from a
        // page; it is the active page alone either way.
        if is_member || membership.is_none() {
            ResolvedBookScope::Chapter
        } else {
            ResolvedBookScope::Page
        }
    } else if is_member {
        ResolvedBookScope::Book
    } else {
        diagnostics.push(not_a_chapter_notice(active, project));
        ResolvedBookScope::Page
    };
    Ok(BookScopeResolution {
        scope: resolved,
        membership,
        diagnostics,
    })
}

fn not_a_chapter_notice(active: &Path, project: &ProjectContext) -> DiagnosticMessage {
    let name = active.strip_prefix(&project.dir).unwrap_or(active);
    DiagnosticMessageBuilder::info("This page is not part of the book")
        .with_code("Q-5-41")
        .problem(format!(
            "`{}` is not in the book's chapter list, so it was rendered as a standalone \
             document instead of as part of the book.",
            name.display()
        ))
        .add_hint("Add the file to `book.chapters` to include it in the book.")
        .build()
}

/// The `Q-5-42` info diagnostic: a scope was given for a format that has no
/// whole-book output.
pub fn scope_ignored_notice(requested: BookScope, format: &Format) -> DiagnosticMessage {
    let which = match requested {
        BookScope::Book => "book",
        BookScope::Chapter => "chapter",
    };
    DiagnosticMessageBuilder::info("Book scope does not apply to this format")
        .with_code("Q-5-42")
        .problem(format!(
            "The `{which}` scope was ignored: `{}` has no whole-book output, so each \
             page is rendered on its own.",
            format.identifier
        ))
        .build()
}
