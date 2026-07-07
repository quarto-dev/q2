//! Operator consent gate for code execution (bd-9lgiulr4).
//!
//! Running `{r}`/`{python}` from a shared CRDT document is remote code
//! execution on the operator's machine. To keep an attacker who hijacks the
//! automerge document from silently driving execution, every run is gated by a
//! [`ConsentGate`] the operator must satisfy **before** the engine is invoked.
//!
//! The gate is consulted after the resolved document (the post-include,
//! pre-engine QMD — exactly what the engine receives) has been written to a
//! reviewable file, so the operator reviews the *actual bytes that will run*.
//!
//! Implementations:
//! - [`InteractivePrompt`] — the default: print the review path, read
//!   accept/reject (and, under `--watch`, "accept all future") from the
//!   terminal. Prompts are serialized so concurrent requests never interleave
//!   on one terminal.
//! - [`AlwaysAccept`] — `q2 provide-hub --dangerously-accept-requests`, and
//!   the automatic choice in tests.
//! - [`AlwaysReject`] — the fail-safe used when stdin is not a TTY.
//!
//! The trait is intentionally **synchronous**: `review` is called from the
//! blocking worker that runs the engine (see `execute.rs`), so a blocking
//! stdin read is correct there and keeps the trait object plainly
//! `Send + Sync` (no `async_trait`).

use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// What the operator chose for a single review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentDecision {
    /// Run this one.
    Accept,
    /// Do not run.
    Reject,
    /// Run this one and auto-accept every subsequent request this session
    /// (offered only under `--watch`).
    AcceptAll,
}

/// Parse a single line of prompt input into a decision.
///
/// Accepts `1`/`2`/`3` (and the words `accept`/`reject`/`all`), trimming
/// surrounding whitespace. Returns `None` for anything else so the caller can
/// re-ask rather than guess.
pub fn parse_prompt_line(line: &str) -> Option<ConsentDecision> {
    match line.trim().to_ascii_lowercase().as_str() {
        "1" | "accept" | "y" | "yes" => Some(ConsentDecision::Accept),
        "2" | "reject" | "n" | "no" => Some(ConsentDecision::Reject),
        "3" | "all" => Some(ConsentDecision::AcceptAll),
        _ => None,
    }
}

/// A consent policy consulted before each execution.
///
/// `Send + Sync` so an `Arc<dyn ConsentGate>` can live on the (shared)
/// provider and be moved into per-request blocking workers.
pub trait ConsentGate: Send + Sync {
    /// Whether to execute the document `path`, whose resolved (post-include,
    /// pre-engine) form has been written to `review_file` for inspection.
    fn review(&self, path: &str, review_file: &Path) -> bool;
}

/// Auto-accept every request (`--dangerously-accept-requests`; tests).
pub struct AlwaysAccept;

impl ConsentGate for AlwaysAccept {
    fn review(&self, _path: &str, _review_file: &Path) -> bool {
        true
    }
}

/// Refuse every request. Used as the fail-safe when interactive consent was
/// requested but stdin is not a terminal.
pub struct AlwaysReject;

impl ConsentGate for AlwaysReject {
    fn review(&self, path: &str, _review_file: &Path) -> bool {
        tracing::warn!(
            path = %path,
            "refusing execution: interactive consent required but stdin is not a terminal \
             (pass --dangerously-accept-requests for unattended execution)"
        );
        false
    }
}

/// Interactive terminal prompt (the default gate).
pub struct InteractivePrompt {
    /// Serializes prompts so two concurrent requests never read the terminal
    /// at once.
    prompt_lock: Mutex<()>,
    /// Set once the operator picks "accept all future"; subsequent reviews
    /// short-circuit to accept. Process-lifetime only.
    accepted_all: AtomicBool,
    /// Whether to offer option 3 ("accept this and all future"). True only
    /// under `--watch`, where more than one request can occur.
    allow_accept_all: bool,
}

impl InteractivePrompt {
    /// `allow_accept_all` enables the "accept all future" option — pass `true`
    /// for `--watch`, `false` for one-shot (a single execution).
    pub fn new(allow_accept_all: bool) -> Self {
        Self {
            prompt_lock: Mutex::new(()),
            accepted_all: AtomicBool::new(false),
            allow_accept_all,
        }
    }

    /// The prompt/parse loop, factored out of [`review`](Self::review) so it can
    /// be tested against arbitrary readers/writers. Re-asks on unparseable
    /// input; treats EOF (empty read) as reject (fail safe).
    fn decide<R: BufRead, W: Write>(
        &self,
        path: &str,
        review_file: &Path,
        mut r: R,
        mut w: W,
    ) -> bool {
        if self.accepted_all.load(Ordering::SeqCst) {
            return true;
        }
        let _guard = self.prompt_lock.lock().expect("prompt lock poisoned");
        // Re-check under the lock: another thread may have flipped it while we
        // waited.
        if self.accepted_all.load(Ordering::SeqCst) {
            return true;
        }

        let _ = writeln!(w, "\nAn execution request has arrived for \"{path}\".");
        let _ = writeln!(w, "The resolved document to be evaluated is at:");
        let _ = writeln!(w, "    {}", review_file.display());
        let _ = writeln!(w, "Review it, then choose:");
        let _ = writeln!(w, "  1) accept");
        let _ = writeln!(w, "  2) reject");
        if self.allow_accept_all {
            let _ = writeln!(w, "  3) accept this and all future requests");
        }

        loop {
            let _ = write!(w, "> ");
            let _ = w.flush();
            let mut line = String::new();
            match r.read_line(&mut line) {
                Ok(0) => {
                    // EOF — no more input. Fail safe.
                    let _ = writeln!(w, "\nNo input (EOF); rejecting.");
                    return false;
                }
                Ok(_) => {}
                Err(e) => {
                    let _ = writeln!(w, "\nInput error ({e}); rejecting.");
                    return false;
                }
            }
            match parse_prompt_line(&line) {
                Some(ConsentDecision::Accept) => return true,
                Some(ConsentDecision::Reject) => return false,
                Some(ConsentDecision::AcceptAll) if self.allow_accept_all => {
                    self.accepted_all.store(true, Ordering::SeqCst);
                    return true;
                }
                // "3" when not offered, or unparseable — re-ask.
                _ => {
                    let _ = writeln!(w, "Please enter 1 (accept) or 2 (reject).");
                }
            }
        }
    }
}

impl ConsentGate for InteractivePrompt {
    fn review(&self, path: &str, review_file: &Path) -> bool {
        let stdin = std::io::stdin();
        self.decide(path, review_file, stdin.lock(), std::io::stderr())
    }
}

/// Whether the current process has an interactive terminal on stdin. The CLI
/// uses this to fall back to [`AlwaysReject`] when interactive consent was
/// requested but there is no TTY (CI, piped input) — see Q5.
pub fn stdin_is_terminal() -> bool {
    std::io::stdin().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::path::PathBuf;

    fn review_path() -> PathBuf {
        PathBuf::from("/tmp/review/doc.resolved.qmd")
    }

    #[test]
    fn parse_maps_the_three_numeric_choices() {
        assert_eq!(parse_prompt_line("1"), Some(ConsentDecision::Accept));
        assert_eq!(parse_prompt_line("2"), Some(ConsentDecision::Reject));
        assert_eq!(parse_prompt_line("3"), Some(ConsentDecision::AcceptAll));
    }

    #[test]
    fn parse_accepts_words_and_trims_whitespace() {
        assert_eq!(
            parse_prompt_line("  accept \n"),
            Some(ConsentDecision::Accept)
        );
        assert_eq!(parse_prompt_line("REJECT"), Some(ConsentDecision::Reject));
        assert_eq!(parse_prompt_line("all"), Some(ConsentDecision::AcceptAll));
        assert_eq!(parse_prompt_line("yes"), Some(ConsentDecision::Accept));
    }

    #[test]
    fn parse_rejects_garbage() {
        assert_eq!(parse_prompt_line(""), None);
        assert_eq!(parse_prompt_line("maybe"), None);
        assert_eq!(parse_prompt_line("12"), None);
    }

    #[test]
    fn always_accept_and_reject() {
        assert!(AlwaysAccept.review("doc.qmd", &review_path()));
        assert!(!AlwaysReject.review("doc.qmd", &review_path()));
    }

    #[test]
    fn interactive_accept_on_1() {
        let gate = InteractivePrompt::new(false);
        let out = Vec::new();
        assert!(gate.decide("doc.qmd", &review_path(), Cursor::new("1\n"), out));
    }

    #[test]
    fn interactive_reject_on_2() {
        let gate = InteractivePrompt::new(false);
        assert!(!gate.decide("doc.qmd", &review_path(), Cursor::new("2\n"), Vec::new()));
    }

    #[test]
    fn interactive_reasks_on_garbage_then_accepts() {
        let gate = InteractivePrompt::new(false);
        let mut out = Vec::new();
        let ok = gate.decide(
            "doc.qmd",
            &review_path(),
            Cursor::new("huh\n\n1\n"),
            &mut out,
        );
        assert!(ok);
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("Please enter"),
            "should have re-asked: {text}"
        );
    }

    #[test]
    fn interactive_eof_is_reject() {
        let gate = InteractivePrompt::new(false);
        assert!(!gate.decide("doc.qmd", &review_path(), Cursor::new(""), Vec::new()));
    }

    #[test]
    fn accept_all_is_hidden_and_inert_without_watch() {
        // Option 3 is not offered in one-shot; "3" is treated as garbage and
        // re-asked, and the sticky flag never flips.
        let gate = InteractivePrompt::new(false);
        let mut out = Vec::new();
        // "3" (rejected as unknown) then "2" (reject).
        assert!(!gate.decide("doc.qmd", &review_path(), Cursor::new("3\n2\n"), &mut out));
        assert!(!gate.accepted_all.load(Ordering::SeqCst));
        let text = String::from_utf8(out).unwrap();
        assert!(
            !text.contains("all future"),
            "option 3 must not be shown: {text}"
        );
    }

    #[test]
    fn accept_all_sticks_for_the_session_under_watch() {
        let gate = InteractivePrompt::new(true);
        // First review: pick 3.
        assert!(gate.decide("a.qmd", &review_path(), Cursor::new("3\n"), Vec::new()));
        assert!(gate.accepted_all.load(Ordering::SeqCst));
        // Second review: empty input would EOF→reject, but the sticky flag
        // short-circuits to accept without reading.
        assert!(gate.decide("b.qmd", &review_path(), Cursor::new(""), Vec::new()));
    }
}
