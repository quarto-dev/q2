//! Which part of a book a render covers: the requested scope
//! ([`crate::format_resolution::ScopeRequest`]) resolved against the project,
//! and the notices that go with it.

use std::path::Path;

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_system_runtime::SystemRuntime;

use crate::error::QuartoError;
use crate::format::{Format, FormatIdentifier};
use crate::format_resolution::ScopeRequest;
use crate::metadata_layers::LayerKind;
use crate::project::{ProjectContext, ProjectKind};

/// A requested book scope (the `book-scope` metadata key). Absent means the
/// user did not choose, and the render's target decides.
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

/// Whether `named` includes every chapter of the book (pass-free, like
/// [`book_membership`]). The orchestrator's collapse rule uses it: naming
/// every chapter of an html book is the whole-book render.
pub fn names_every_chapter(
    project: &ProjectContext,
    named: &[std::path::PathBuf],
    runtime: &dyn SystemRuntime,
) -> Result<bool, QuartoError> {
    let key = |p: &Path| -> Option<String> {
        p.strip_prefix(&project.dir)
            .ok()
            .map(|rel| super::links::normalize_book_path(&rel.to_string_lossy().replace('\\', "/")))
    };
    let named_keys: Vec<String> = named.iter().filter_map(|p| key(p)).collect();
    let files = super::book_chapter_files(project, runtime)?;
    Ok(files.iter().all(|file| {
        let chapter = super::links::normalize_book_path(&file.to_string_lossy().replace('\\', "/"));
        named_keys.contains(&chapter)
    }))
}

/// What a render actually covers once the request is resolved against the
/// project; also the response echo (`stats.book.scope`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedBookScope {
    /// The whole book as one document.
    Book,
    /// A chapter of the book, as a standalone document.
    Chapter,
    /// No book mode: the project is not a book, or the file is not in the
    /// chapter list. Rendered as a standalone document. Echoed on the wire
    /// as `none`; a resolution result only, never an accepted input.
    NoBook,
}

/// The outcome of [`resolve_book_scope`].
#[derive(Debug)]
pub struct BookScopeResolution {
    pub scope: ResolvedBookScope,
    /// `Some` for a book project whose chapter list could be read.
    pub membership: Option<BookMembership>,
    /// Notices the caller must deliver with the result: those of
    /// [`scope_notices`] (`Q-5-41`, `Q-5-48`, `Q-5-42`).
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
/// covers. `request` is the scope the resolver computed
/// ([`crate::format_resolution::resolve_render_scope`]): the target's default
/// (`Book` for a whole-project render, `Chapter` for one document) is already
/// inside `request.value`.
///
/// - A non-book project is always `NoBook`, silently.
/// - A format that does not consolidate a book renders the active page alone:
///   `Chapter` for a member, `NoBook` otherwise.
/// - A consolidating format covers `request.value` (`Book` or `Chapter`) for a
///   member.
/// - A page that is not a member is `NoBook` for every request and format: it
///   renders as a standalone page.
///
/// The notices (`Q-5-41`, the non-member warning, `Q-5-42`) come from
/// [`scope_notices`] and are returned in `diagnostics`.
///
/// `active` must be absolute (the path the book's file list resolves
/// against). `Err` is the book's own list error (`Q-5-34`/`35`/`36`), returned
/// only when the render would consolidate the book; otherwise it is left to
/// the pipeline to report.
pub fn resolve_book_scope(
    project: &ProjectContext,
    active: &Path,
    request: &ScopeRequest,
    format: &Format,
    runtime: &dyn SystemRuntime,
) -> Result<BookScopeResolution, QuartoError> {
    if project.project_kind() != ProjectKind::Book {
        return Ok(BookScopeResolution {
            scope: ResolvedBookScope::NoBook,
            membership: None,
            diagnostics: Vec::new(),
        });
    }
    let consolidates = format_consolidates_book(format);
    let membership = match book_membership(project, active, runtime) {
        Ok(m) => Some(m),
        Err(e) if consolidates && request.value != BookScope::Chapter => return Err(e),
        Err(_) => None,
    };
    let is_member = membership.is_some_and(|m| m.active_is_chapter);
    let resolved = if !consolidates {
        if is_member {
            ResolvedBookScope::Chapter
        } else {
            ResolvedBookScope::NoBook
        }
    } else if membership.is_none() {
        // Without a readable list a chapter request cannot be told from a
        // page; it is the active page alone either way. (A book request
        // returned the list error above.)
        ResolvedBookScope::Chapter
    } else if is_member {
        match request.value {
            BookScope::Book => ResolvedBookScope::Book,
            BookScope::Chapter => ResolvedBookScope::Chapter,
        }
    } else {
        ResolvedBookScope::NoBook
    };
    let diagnostics = scope_notices(
        request,
        membership.as_ref(),
        format,
        Some(active),
        &project.dir,
    );
    Ok(BookScopeResolution {
        scope: resolved,
        membership,
        diagnostics,
    })
}

/// Every scope notice of one render, decided in one place (epic FR10, FR22).
/// Pure: no runtime, no I/O. `membership` is `None` for a project that is not
/// a book (or whose chapter list could not be read), which gets no notices.
/// `file` is the named file the notice is about, or `None` for a render that
/// names none (a whole-project render).
///
/// For each named file at most one notice is produced:
///
/// - A **non-member** (a named file in the book project that is not a
///   chapter), under any request: an explicit runtime `book` gets the warning
///   `Q-5-48` (the request cannot be honoured); anything else, the info
///   `Q-5-41` (the file renders on its own, with its own numbering).
/// - A **member**, or a render that names no file: `Q-5-42` iff an explicit
///   runtime `book` meets a format with no whole-book output.
///
/// Call it from exactly one place per render: [`resolve_book_scope`] does, so
/// a dispatch that went through it must not call this again.
pub fn scope_notices(
    request: &ScopeRequest,
    membership: Option<&BookMembership>,
    format: &Format,
    file: Option<&Path>,
    project_dir: &Path,
) -> Vec<DiagnosticMessage> {
    let Some(membership) = membership else {
        return Vec::new();
    };
    let explicit_runtime_book =
        request.explicit == Some(LayerKind::Runtime) && request.value == BookScope::Book;
    match file {
        Some(file) if !membership.active_is_chapter => {
            let name = file.strip_prefix(project_dir).unwrap_or(file);
            vec![if explicit_runtime_book {
                book_not_honoured_notice(name)
            } else {
                not_a_chapter_notice(name)
            }]
        }
        _ if explicit_runtime_book && !format_consolidates_book(format) => {
            vec![scope_ignored_notice(format)]
        }
        _ => Vec::new(),
    }
}

/// `Q-5-41`, a quiet info: a named file that is not a chapter was rendered on
/// its own.
fn not_a_chapter_notice(name: &Path) -> DiagnosticMessage {
    DiagnosticMessageBuilder::info("This page is not a chapter of the book")
        .with_code("Q-5-41")
        .problem(format!(
            "`{}` is not a chapter of this book, so it was rendered on its own: it has its \
             own numbering, and it cannot link to the book's chapters.",
            name.display()
        ))
        .add_hint("Add the file to `book.chapters` to include it in the book.")
        .build()
}

/// `Q-5-48`, a warning: `book-scope: book` was requested for this render, but
/// the named file is not in the chapter list.
fn book_not_honoured_notice(name: &Path) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("The book was requested for a page outside it")
        .with_code("Q-5-48")
        .problem(format!(
            "`book-scope: book` was requested, but `{}` is not in the book's chapter list, \
             so it was rendered on its own.",
            name.display()
        ))
        .add_hint("Add the file to `book.chapters` to include it in the book.")
        .build()
}

/// `Q-5-42`, a warning: `book-scope: book` was requested for this render, for
/// a format that has no whole-book output.
fn scope_ignored_notice(format: &Format) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning("Book scope does not apply to this format")
        .with_code("Q-5-42")
        .problem(format!(
            "`book-scope: book` was requested for this render, but `{}` has no whole-book \
             output, so each page is rendered on its own.",
            format.identifier
        ))
        .build()
}

#[cfg(test)]
mod tests;
