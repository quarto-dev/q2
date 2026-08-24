//! `q2 docs llms` — serve the embedded docs-site llms.txt artifacts
//! (bd-hwop1zii).
//!
//! The `q2 docs` namespace exposes documentation embedded in the binary.
//! This module implements its `llms` subcommand (aliased as the
//! top-level `q2 agents-info`): the llms.txt index, per-page markdown
//! companions, and llms-full.txt that `q2 render docs/` produces are
//! staged by `cargo xtask build-agents-docs` into `agents-docs-dist/`
//! and embedded via `include_dir!` (see this crate's `build.rs`).
//!
//! All lookup logic operates on an [`include_dir::Dir`] parameter so
//! tests can drive it with a synthetic tree; only the thin CLI entry
//! point binds the real embedded directory.
//!
//! Embed states:
//! - **real**: `llms.txt` is present (staged by the xtask).
//!   `embed-info.json` records provenance (git commit + dirty flag of
//!   the staging checkout); it may be absent, in which case provenance
//!   reads as unknown.
//! - **placeholder**: no `llms.txt` (fresh clone; build.rs embedded a
//!   stub). Every content mode fails with instructions; `--embed-info`
//!   reports the state instead of failing.

use include_dir::Dir;

/// The docs tree staged by `cargo xtask build-agents-docs` (or the
/// placeholder when it hasn't run); see `build.rs` for the resolution.
static EMBEDDED_DOCS: Dir<'static> = include_dir::include_dir!("$QUARTO_DOCS_LLMS_EMBED_DIR");

/// How to turn a placeholder embed into a real one. Referenced by the
/// placeholder error and by `--embed-info`.
const REBUILD_HINT: &str =
    "run `cargo xtask build-agents-docs`, then rebuild with `cargo build --bin q2`";

/// What `q2 docs llms` (or its `q2 agents-info` alias) was asked for.
/// Built by `main.rs` from the mutually-exclusive CLI flags.
pub enum Mode {
    /// Bare invocation: the llms.txt index.
    Index,
    /// `--full`: llms-full.txt.
    Full,
    /// `--list`: href + title per page.
    List,
    /// `--embed-info`: provenance of the embedded snapshot.
    EmbedInfo,
    /// `<href>`: one page.
    Page(String),
}

/// CLI entry point: print the requested view of the embedded docs to
/// stdout. Lookup failures (and placeholder embeds) come back as
/// errors, so the process exits nonzero with the message on stderr.
pub fn execute(mode: Mode) -> anyhow::Result<()> {
    let embed = DocsEmbed::new(&EMBEDDED_DOCS);
    let out = match mode {
        Mode::Index => embed.index()?,
        Mode::Full => embed.full()?.to_string(),
        Mode::List => embed
            .list()?
            .iter()
            .map(|p| format!("{}\t{}\n", p.href, p.title))
            .collect(),
        Mode::EmbedInfo => embed.embed_info_text(),
        Mode::Page(href) => embed.page(&href)?.to_string(),
    };
    write_stdout(&out)
}

/// Write to stdout, treating a closed reader as success. This output
/// exists to be piped — `| head`, `| grep`, an agent's reader that
/// stops early — and Rust ignores SIGPIPE, so an unguarded `print!`
/// would abort with "failed printing to stdout: Broken pipe" instead
/// of ending quietly.
fn write_stdout(text: &str) -> anyhow::Result<()> {
    use std::io::Write;

    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    match lock.write_all(text.as_bytes()).and_then(|()| lock.flush()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Provenance sidecar written by `cargo xtask build-agents-docs`
/// (real embeds) or by this crate's `build.rs` (placeholder embeds).
#[derive(Debug, Default, serde::Deserialize)]
pub struct EmbedInfo {
    #[serde(default)]
    pub placeholder: bool,
    #[serde(default)]
    pub commit: Option<String>,
    #[serde(default)]
    pub dirty: bool,
}

/// One page in `--list` output: companion href + page title.
#[derive(Debug, PartialEq, Eq)]
pub struct PageEntry {
    pub href: String,
    pub title: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DocsLlmsError {
    /// The binary carries the placeholder embed, not real docs.
    Placeholder,
    /// A real embed is missing an artifact it must contain
    /// (`llms-full.txt`); names the missing piece.
    Corrupt(String),
    /// `<href>` lookup miss, with up to five suggested hrefs.
    PageNotFound {
        query: String,
        suggestions: Vec<String>,
    },
}

impl std::fmt::Display for DocsLlmsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocsLlmsError::Placeholder => write!(
                f,
                "this q2 binary was built without the embedded documentation \
                 (placeholder embed); {REBUILD_HINT}"
            ),
            DocsLlmsError::Corrupt(what) => write!(
                f,
                "the embedded documentation is missing `{what}`; the embed is \
                 corrupt — {REBUILD_HINT}"
            ),
            DocsLlmsError::PageNotFound { query, suggestions } => {
                write!(f, "no embedded documentation page matches `{query}`")?;
                if !suggestions.is_empty() {
                    write!(f, "; did you mean one of:")?;
                    for s in suggestions {
                        write!(f, "\n  {s}")?;
                    }
                    writeln!(f)?;
                } else {
                    write!(f, ". ")?;
                }
                write!(f, "Use `q2 docs llms --list` to see every page.")
            }
        }
    }
}

impl std::error::Error for DocsLlmsError {}

/// The embedded docs tree plus the lookup/rendering logic over it.
pub struct DocsEmbed<'a> {
    dir: &'a Dir<'a>,
}

impl<'a> DocsEmbed<'a> {
    pub fn new(dir: &'a Dir<'a>) -> Self {
        DocsEmbed { dir }
    }

    /// Placeholder embeds have no `llms.txt` (build.rs only stages the
    /// real tree when the xtask has produced one).
    pub fn is_placeholder(&self) -> bool {
        self.dir.get_file("llms.txt").is_none() || self.embed_info().placeholder
    }

    fn require_real(&self) -> Result<(), DocsLlmsError> {
        if self.is_placeholder() {
            Err(DocsLlmsError::Placeholder)
        } else {
            Ok(())
        }
    }

    fn text_file(&self, name: &str) -> Result<&'a str, DocsLlmsError> {
        self.dir
            .get_file(name)
            .and_then(|f| f.contents_utf8())
            .ok_or_else(|| DocsLlmsError::Corrupt(name.to_string()))
    }

    /// Bare `q2 docs llms`: retrieval preamble + `llms.txt` verbatim.
    pub fn index(&self) -> Result<String, DocsLlmsError> {
        self.require_real()?;
        let llms_txt = self.text_file("llms.txt")?;
        Ok(format!(
            "<!--\n\
             q2 embedded documentation index (llms.txt).\n\
             Fetch one page:  q2 docs llms <href>   (hrefs listed below)\n\
             List all pages:  q2 docs llms --list\n\
             Whole corpus:    q2 docs llms --full\n\
             -->\n\n{llms_txt}"
        ))
    }

    /// `--full`: `llms-full.txt` verbatim.
    pub fn full(&self) -> Result<&'a str, DocsLlmsError> {
        self.require_real()?;
        self.text_file("llms-full.txt")
    }

    /// Every embedded `.md` companion href, sorted, with file contents.
    fn pages(&self) -> Vec<(String, &'a str)> {
        fn walk<'a>(dir: &Dir<'a>, out: &mut Vec<(String, &'a str)>) {
            for entry in dir.entries() {
                match entry {
                    include_dir::DirEntry::Dir(d) => walk(d, out),
                    include_dir::DirEntry::File(f) => {
                        if f.path().extension().is_some_and(|e| e == "md")
                            && let (Some(path), Some(contents)) =
                                (f.path().to_str(), f.contents_utf8())
                        {
                            // include_dir records build-machine
                            // separators; hrefs are always `/`.
                            out.push((path.replace('\\', "/"), contents));
                        }
                    }
                }
            }
        }
        let mut pages = Vec::new();
        walk(self.dir, &mut pages);
        pages.sort_by(|a, b| a.0.cmp(&b.0));
        pages
    }

    /// `--list`: every embedded `.md` companion, sorted by href, with
    /// the page title (first `#` heading, else the file stem).
    pub fn list(&self) -> Result<Vec<PageEntry>, DocsLlmsError> {
        self.require_real()?;
        Ok(self
            .pages()
            .into_iter()
            .map(|(href, contents)| {
                let title = title_of(contents).unwrap_or_else(|| stem_of(&href).to_string());
                PageEntry { href, title }
            })
            .collect())
    }

    /// `<href>`: one page's companion. Exact `.md` hrefs (as printed in
    /// `llms.txt`) win; otherwise the query is normalized — backslashes
    /// to slashes, a leading `./` or `/` and a trailing `/` stripped,
    /// `.qmd`/`.html` mapped to `.md`, and an extensionless path tried
    /// as `<q>.md` then `<q>/index.md`.
    pub fn page(&self, query: &str) -> Result<&'a str, DocsLlmsError> {
        self.require_real()?;
        let q = normalize_query(query);
        for candidate in candidates(&q) {
            if let Some(contents) = self
                .dir
                .get_file(&candidate)
                .and_then(|f| f.contents_utf8())
            {
                return Ok(contents);
            }
        }
        Err(DocsLlmsError::PageNotFound {
            query: query.to_string(),
            suggestions: self.suggestions(&q),
        })
    }

    /// Near-miss help for a failed lookup: pages whose stem shares a
    /// substring with the query's stem; failing that, pages in the
    /// query's directory. At most five, sorted.
    fn suggestions(&self, normalized_query: &str) -> Vec<String> {
        let pages = self.pages();
        let q_stem = stem_of(normalized_query).to_lowercase();
        let mut hits: Vec<String> = pages
            .iter()
            .filter(|(href, _)| {
                let stem = stem_of(href).to_lowercase();
                !q_stem.is_empty() && (stem.contains(&q_stem) || q_stem.contains(&stem))
            })
            .map(|(href, _)| href.clone())
            .collect();
        if hits.is_empty()
            && let Some((parent, _)) = normalized_query.rsplit_once('/')
        {
            let prefix = format!("{parent}/");
            hits = pages
                .iter()
                .filter(|(href, _)| href.starts_with(&prefix))
                .map(|(href, _)| href.clone())
                .collect();
        }
        hits.truncate(5);
        hits
    }

    /// Parsed `embed-info.json`; defaults when absent or unreadable.
    pub fn embed_info(&self) -> EmbedInfo {
        self.dir
            .get_file("embed-info.json")
            .and_then(|f| f.contents_utf8())
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default()
    }

    /// `--embed-info`: human-readable provenance report. Works on
    /// placeholder embeds (that is its job).
    pub fn embed_info_text(&self) -> String {
        if self.is_placeholder() {
            return format!(
                "source: placeholder\n\
                 This binary carries no embedded documentation; {REBUILD_HINT}.\n"
            );
        }
        let info = self.embed_info();
        let commit = match (info.commit.as_deref(), info.dirty) {
            (Some(c), true) => format!("{c} (dirty)"),
            (Some(c), false) => c.to_string(),
            (None, _) => "unknown".to_string(),
        };
        format!(
            "source: real\ncommit: {commit}\npages: {}\n",
            self.pages().len()
        )
    }
}

/// The page title is the first non-blank line iff it is an ATX `#`
/// heading (which is how the llms companion writer renders titles).
fn title_of(contents: &str) -> Option<String> {
    let first = contents.lines().find(|l| !l.trim().is_empty())?;
    let title = first.strip_prefix("# ")?.trim();
    (!title.is_empty()).then(|| title.to_string())
}

/// Final path segment without its extension (`guides/alpha.md` →
/// `alpha`).
fn stem_of(href: &str) -> &str {
    let base = href.rsplit('/').next().unwrap_or(href);
    base.rsplit_once('.').map_or(base, |(stem, _)| stem)
}

fn normalize_query(query: &str) -> String {
    let mut q = query.trim().replace('\\', "/");
    loop {
        if let Some(rest) = q.strip_prefix("./") {
            q = rest.to_string();
        } else if let Some(rest) = q.strip_prefix('/') {
            q = rest.to_string();
        } else if let Some(rest) = q.strip_suffix('/') {
            q = rest.to_string();
        } else {
            break;
        }
    }
    q
}

/// Lookup candidates for a normalized query, most-specific first.
fn candidates(q: &str) -> Vec<String> {
    if q.is_empty() {
        return Vec::new();
    }
    if q.ends_with(".md") {
        return vec![q.to_string()];
    }
    for source_ext in [".qmd", ".html"] {
        if let Some(base) = q.strip_suffix(source_ext) {
            return vec![format!("{base}.md")];
        }
    }
    // Extensionless: a page, or a directory standing for its index.
    vec![format!("{q}.md"), format!("{q}/index.md")]
}

#[cfg(test)]
mod tests {
    use super::*;
    use include_dir::{Dir, DirEntry, File};

    // A synthetic real embed:
    //   llms.txt, llms-full.txt, embed-info.json
    //   index.md, guides/alpha.md, guides/noheading.md, guides/sub/index.md
    const LLMS_TXT: &str = "# Test Site\n\n> A test corpus\n\n## Guides\n\n\
                            - [Alpha](guides/alpha.md)\n\
                            - [Sub](guides/sub/index.md)\n";
    const LLMS_FULL: &str = "# Test Site — full corpus\n\nAlpha body. Sub body.\n";

    static REAL_ENTRIES: &[DirEntry<'static>] = &[
        DirEntry::File(File::new(
            "embed-info.json",
            b"{\"commit\":\"abc1234\",\"dirty\":false}",
        )),
        DirEntry::File(File::new("index.md", b"\n\n# Home\n\nWelcome.\n")),
        DirEntry::File(File::new("llms-full.txt", LLMS_FULL.as_bytes())),
        DirEntry::File(File::new("llms.txt", LLMS_TXT.as_bytes())),
        DirEntry::Dir(Dir::new(
            "guides",
            &[
                DirEntry::File(File::new(
                    "guides/alpha.md",
                    b"# Alpha Page\n\nAlpha body.\n",
                )),
                DirEntry::File(File::new("guides/noheading.md", b"no heading here\n")),
                DirEntry::Dir(Dir::new(
                    "guides/sub",
                    &[DirEntry::File(File::new(
                        "guides/sub/index.md",
                        b"# Sub Index\n\nSub body.\n",
                    ))],
                )),
            ],
        )),
    ];
    static REAL: Dir<'static> = Dir::new("", REAL_ENTRIES);

    static PLACEHOLDER_ENTRIES: &[DirEntry<'static>] = &[DirEntry::File(File::new(
        "embed-info.json",
        b"{\"placeholder\":true}",
    ))];
    static PLACEHOLDER: Dir<'static> = Dir::new("", PLACEHOLDER_ENTRIES);

    // Real embed with a dirty commit and no llms-full.txt (corrupt).
    static DIRTY_ENTRIES: &[DirEntry<'static>] = &[
        DirEntry::File(File::new(
            "embed-info.json",
            b"{\"commit\":\"beef999\",\"dirty\":true}",
        )),
        DirEntry::File(File::new("llms.txt", b"# Dirty\n")),
    ];
    static DIRTY: Dir<'static> = Dir::new("", DIRTY_ENTRIES);

    // Real embed with no embed-info.json at all.
    static NOINFO_ENTRIES: &[DirEntry<'static>] =
        &[DirEntry::File(File::new("llms.txt", b"# NoInfo\n"))];
    static NOINFO: Dir<'static> = Dir::new("", NOINFO_ENTRIES);

    fn real() -> DocsEmbed<'static> {
        DocsEmbed::new(&REAL)
    }
    fn placeholder() -> DocsEmbed<'static> {
        DocsEmbed::new(&PLACEHOLDER)
    }

    // ---- placeholder detection -------------------------------------

    #[test]
    fn placeholder_detected_by_missing_llms_txt() {
        assert!(!real().is_placeholder());
        assert!(placeholder().is_placeholder());
    }

    #[test]
    fn placeholder_blocks_content_modes_and_names_the_xtask() {
        let e = placeholder();
        for err in [
            e.index().unwrap_err(),
            e.full().map(|_| ()).unwrap_err(),
            e.list().map(|_| ()).unwrap_err(),
            e.page("index.md").map(|_| ()).unwrap_err(),
        ] {
            assert_eq!(err, DocsLlmsError::Placeholder);
            let msg = err.to_string();
            assert!(
                msg.contains("cargo xtask build-agents-docs"),
                "placeholder error must name the xtask: {msg}"
            );
            assert!(
                msg.contains("cargo build --bin q2"),
                "placeholder error must name the rebuild step: {msg}"
            );
        }
    }

    // ---- index / full ----------------------------------------------

    #[test]
    fn index_is_preamble_then_llms_txt_verbatim() {
        let out = real().index().unwrap();
        assert!(
            out.starts_with("<!--"),
            "preamble must be an HTML comment so the payload stays valid \
             markdown: {out}"
        );
        assert!(out.contains("q2 docs llms <href>"), "{out}");
        assert!(out.contains("q2 docs llms --list"), "{out}");
        assert!(out.contains("q2 docs llms --full"), "{out}");
        let end_of_comment = out.find("-->").expect("preamble comment must close");
        assert_eq!(
            &out[end_of_comment..],
            format!("-->\n\n{LLMS_TXT}"),
            "llms.txt must follow the preamble verbatim"
        );
    }

    #[test]
    fn full_is_llms_full_txt_verbatim() {
        assert_eq!(real().full().unwrap(), LLMS_FULL);
    }

    #[test]
    fn full_missing_from_real_embed_is_corrupt() {
        let e = DocsEmbed::new(&DIRTY);
        assert_eq!(
            e.full().map(|_| ()).unwrap_err(),
            DocsLlmsError::Corrupt("llms-full.txt".to_string())
        );
    }

    // ---- list -------------------------------------------------------

    #[test]
    fn list_returns_all_md_pages_sorted_with_titles() {
        let pages = real().list().unwrap();
        let expect = [
            ("guides/alpha.md", "Alpha Page"),
            ("guides/noheading.md", "noheading"),
            ("guides/sub/index.md", "Sub Index"),
            ("index.md", "Home"),
        ];
        let got: Vec<(&str, &str)> = pages
            .iter()
            .map(|p| (p.href.as_str(), p.title.as_str()))
            .collect();
        assert_eq!(got, expect);
    }

    // ---- page lookup + normalization -------------------------------

    #[test]
    fn page_lookup_normalization_table() {
        let e = real();
        let alpha = "# Alpha Page\n\nAlpha body.\n";
        let sub = "# Sub Index\n\nSub body.\n";
        for (query, want) in [
            ("guides/alpha.md", alpha), // exact
            ("./guides/alpha.md", alpha),
            ("/guides/alpha.md", alpha),
            ("guides/alpha.qmd", alpha),
            ("guides/alpha.html", alpha),
            ("guides/alpha", alpha),
            ("guides\\alpha.md", alpha), // windows-pasted path
            ("guides/sub", sub),         // dir -> index.md
            ("guides/sub/", sub),        // trailing slash
            ("guides/sub/index.qmd", sub),
        ] {
            assert_eq!(e.page(query).unwrap(), want, "query: {query}");
        }
    }

    #[test]
    fn page_miss_suggests_by_stem_substring() {
        let err = real().page("guides/alph").unwrap_err();
        let DocsLlmsError::PageNotFound { query, suggestions } = &err else {
            panic!("expected PageNotFound, got {err:?}");
        };
        assert_eq!(query, "guides/alph");
        assert_eq!(suggestions, &["guides/alpha.md".to_string()]);
        let msg = err.to_string();
        assert!(msg.contains("guides/alpha.md"), "{msg}");
        assert!(msg.contains("--list"), "{msg}");
    }

    #[test]
    fn page_miss_suggests_same_directory_pages() {
        let err = real().page("guides/zzz.md").unwrap_err();
        let DocsLlmsError::PageNotFound { suggestions, .. } = &err else {
            panic!("expected PageNotFound, got {err:?}");
        };
        assert_eq!(
            suggestions,
            &[
                "guides/alpha.md".to_string(),
                "guides/noheading.md".to_string(),
                "guides/sub/index.md".to_string(),
            ]
        );
    }

    #[test]
    fn page_miss_with_no_near_matches_points_at_list() {
        let err = real().page("zzz/yyy.md").unwrap_err();
        let DocsLlmsError::PageNotFound { suggestions, .. } = &err else {
            panic!("expected PageNotFound, got {err:?}");
        };
        assert!(suggestions.is_empty());
        assert!(err.to_string().contains("--list"));
    }

    // ---- embed-info -------------------------------------------------

    #[test]
    fn embed_info_parses_sidecar() {
        let info = real().embed_info();
        assert!(!info.placeholder);
        assert_eq!(info.commit.as_deref(), Some("abc1234"));
        assert!(!info.dirty);
    }

    #[test]
    fn embed_info_text_real() {
        assert_eq!(
            real().embed_info_text(),
            "source: real\ncommit: abc1234\npages: 4\n"
        );
    }

    #[test]
    fn embed_info_text_dirty_commit_is_flagged() {
        assert_eq!(
            DocsEmbed::new(&DIRTY).embed_info_text(),
            "source: real\ncommit: beef999 (dirty)\npages: 0\n"
        );
    }

    #[test]
    fn embed_info_text_missing_sidecar_reads_unknown() {
        assert_eq!(
            DocsEmbed::new(&NOINFO).embed_info_text(),
            "source: real\ncommit: unknown\npages: 0\n"
        );
    }

    #[test]
    fn embed_info_text_placeholder_names_the_fix() {
        let text = placeholder().embed_info_text();
        assert!(text.starts_with("source: placeholder\n"), "{text}");
        assert!(text.contains("cargo xtask build-agents-docs"), "{text}");
    }
}
