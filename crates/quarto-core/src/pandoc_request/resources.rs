//! `resource_refs`: the document's own files that pandoc reads (images, a
//! reference doc, a template, a highlight theme, user Lua filters), with
//! their bytes copied out of the VFS snapshot (design: D3).
//!
//! Purpose-built rather than the `ResourceCopyFlushStage` collector, which
//! returns early in vfs-root mode and only knows image targets. Rust copies
//! the bytes in the same synchronous step that builds the request, so the
//! host never reads the VFS.
//!
//! Rules:
//! - a path is normalized with [`normalize_request_path`] and must lie under
//!   the allowed root (the project root); otherwise it is not mounted and a
//!   `Q-11-1`-style warning names it;
//! - a file absent from the snapshot is left to pandoc, which reports it
//!   itself (images: `Q-11-1`; `reference-doc`/`template`: `Q-5-30`, raised
//!   earlier at merge time);
//! - a filter in a subdirectory mounts that directory recursively, so its
//!   `require`/`io.open` of siblings works; a filter at the project root
//!   mounts only itself (the root holds unrelated files);
//! - collection stops at the size limits of `resources/pandoc-wasm.json`
//!   with an error naming the file, so the main thread never allocates a
//!   request the host would reject.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_system_runtime::SystemRuntime;

use super::args::PandocArg;
use super::constants::{PandocWasmLimits, constants};
use super::normalize_request_path;
use super::types::RequestFile;

/// What a mounted file is, for the per-file size limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Image,
    ReferenceDoc,
    Other,
}

/// Same extension test as the host's limit check
/// (`ts-packages/pandoc-host/src/validate.ts`, `IMAGE_EXT`).
fn has_image_extension(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".gif", ".svg", ".webp", ".bmp", ".tif", ".tiff", ".ico", ".avif",
        ".emf", ".wmf", ".eps", ".pdf",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

/// `%XX` escapes decoded as pandoc does when it resolves an image target to
/// a file; malformed escapes stay literal.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// A target that is not a file on this filesystem: `https://...`, `data:...`,
/// `mailto:...` (a scheme of two or more letters; `C:` is a drive).
fn is_external_target(target: &str) -> bool {
    match target.split_once(':') {
        Some((scheme, _)) => {
            scheme.len() >= 2
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        }
        None => false,
    }
}

/// The `--default-image-extension` the arguments set, if any (either form).
pub fn default_image_extension(args: &[PandocArg]) -> Option<String> {
    let texts: Vec<&str> = args
        .iter()
        .filter_map(|a| match a {
            PandocArg::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    for (i, t) in texts.iter().enumerate() {
        if let Some(v) = t.strip_prefix("--default-image-extension=") {
            return Some(v.trim_start_matches('.').to_string());
        }
        if *t == "--default-image-extension"
            && let Some(v) = texts.get(i + 1)
        {
            return Some(v.trim_start_matches('.').to_string());
        }
    }
    None
}

fn warning(message: String) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(message)
        .with_code("Q-11-1")
        .build()
}

fn error(message: String) -> DiagnosticMessage {
    DiagnosticMessageBuilder::error(message)
        .with_code("Q-11-1")
        .build()
}

pub struct ResourceCollector<'a> {
    runtime: &'a dyn SystemRuntime,
    /// Normalized allowed root.
    root: String,
    /// Normalized paths already under another mount (the temp root), which
    /// must never be claimed again.
    skip_under: Vec<String>,
    limits: PandocWasmLimits,
    mounted: BTreeMap<String, Vec<u8>>,
    total: u64,
    stopped: bool,
    diagnostics: Vec<DiagnosticMessage>,
}

impl<'a> ResourceCollector<'a> {
    /// `base_total_bytes` is what the request already carries (share tree and
    /// `files`), counted against the total limit.
    pub fn new(
        runtime: &'a dyn SystemRuntime,
        allowed_root: &Path,
        skip_under: &Path,
        base_total_bytes: u64,
    ) -> Self {
        Self {
            runtime,
            root: normalize_request_path(allowed_root),
            skip_under: vec![normalize_request_path(skip_under)],
            limits: constants().limits,
            mounted: BTreeMap::new(),
            total: base_total_bytes,
            stopped: false,
            diagnostics: Vec::new(),
        }
    }

    fn under(path: &str, root: &str) -> bool {
        path == root
            || (root.ends_with('/') && path.starts_with(root))
            || path.starts_with(&format!("{root}/"))
    }

    /// Normalize `path` and check it against the allowed root, noting a
    /// warning when it lies outside. `None` means do not mount.
    fn admit(&mut self, path: &Path, what: &str) -> Option<String> {
        let normalized = normalize_request_path(path);
        if self
            .skip_under
            .iter()
            .any(|root| Self::under(&normalized, root))
        {
            return None;
        }
        if !Self::under(&normalized, &self.root) {
            self.diagnostics.push(warning(format!(
                "{what} {normalized} is outside the project ({}) and is not available in the browser",
                self.root
            )));
            return None;
        }
        Some(normalized)
    }

    fn is_file(&self, normalized: &str) -> bool {
        self.runtime.is_file(Path::new(normalized)).unwrap_or(false)
    }

    /// Mount one existing file; a missing file is left to pandoc.
    fn mount(&mut self, normalized: String, kind: ResourceKind) {
        if self.stopped || self.mounted.contains_key(&normalized) {
            return;
        }
        if !self.is_file(&normalized) {
            return;
        }
        // Size first, so an oversized file is never copied.
        let size = self
            .runtime
            .path_metadata(Path::new(&normalized))
            .map_or(0, |m| m.size);
        let per_file = match kind {
            ResourceKind::ReferenceDoc => Some(("reference-doc", self.limits.reference_doc_bytes)),
            _ if has_image_extension(&normalized) => Some(("image", self.limits.image_bytes)),
            _ => None,
        };
        if let Some((label, limit)) = per_file
            && size > limit
        {
            self.stopped = true;
            self.diagnostics.push(error(format!(
                "{label} {normalized} is {size} bytes; the limit is {limit}"
            )));
            return;
        }
        if self.total + size > self.limits.total_bytes {
            self.stopped = true;
            self.diagnostics.push(error(format!(
                "mounting {normalized} ({size} bytes) would exceed the {} byte limit on a browser render",
                self.limits.total_bytes
            )));
            return;
        }
        if let Ok(bytes) = self.runtime.file_read(Path::new(&normalized)) {
            self.total += bytes.len() as u64;
            self.mounted.insert(normalized, bytes);
        }
    }

    /// An `Image` target, resolved as pandoc does against `--resource-path`
    /// (the document directory): percent-decoded, with the default image
    /// extension appended when the file name has none. Remote and `data:`
    /// targets are not files.
    pub fn add_image(&mut self, doc_dir: &Path, target: &str, default_ext: Option<&str>) {
        if target.is_empty() || is_external_target(target) {
            return;
        }
        let decoded = percent_decode(target);
        let mut path: PathBuf = if decoded.starts_with('/') || decoded.starts_with('\\') {
            PathBuf::from(&decoded)
        } else {
            doc_dir.join(&decoded)
        };
        if let Some(ext) = default_ext
            && path.extension().is_none()
        {
            path.set_extension(ext);
        }
        if let Some(normalized) = self.admit(&path, "image") {
            self.mount(normalized, ResourceKind::Image);
        }
    }

    /// An absolute path named by an argument or metadata key.
    pub fn add_file(&mut self, path: &Path, kind: ResourceKind) {
        if let Some(normalized) = self.admit(path, "resource") {
            self.mount(normalized, kind);
        }
    }

    /// Every path-valued argument (`--reference-doc`, `--template`,
    /// `--highlight-style <theme>`, `--epub-cover-image=...`, ...). Entries
    /// that are not existing files (directories, the output path) are left
    /// alone.
    pub fn add_args(&mut self, args: &[PandocArg]) {
        let mut prev: Option<&str> = None;
        for arg in args {
            match arg {
                PandocArg::Text(t) => prev = Some(t.as_str()),
                PandocArg::Path(p) => {
                    let kind = if prev == Some("--reference-doc") {
                        ResourceKind::ReferenceDoc
                    } else {
                        ResourceKind::Other
                    };
                    self.add_file(p, kind);
                    prev = None;
                }
                PandocArg::FlagPath { flag, path } => {
                    let kind = if flag == "--reference-doc=" {
                        ResourceKind::ReferenceDoc
                    } else {
                        ResourceKind::Other
                    };
                    self.add_file(path, kind);
                    prev = None;
                }
            }
        }
    }

    /// A user entry-point Lua filter together with its containing
    /// directory, mounted recursively unless the filter sits at the project
    /// root (then only the file itself).
    pub fn add_lua_filter(&mut self, path: &Path) {
        let Some(normalized) = self.admit(path, "filter") else {
            return;
        };
        let parent = normalized
            .rsplit_once('/')
            .map_or("/", |(p, _)| if p.is_empty() { "/" } else { p })
            .to_string();
        let at_root = parent == self.root || (parent == "/" && self.root == "/");
        if at_root {
            self.mount(normalized, ResourceKind::Other);
            return;
        }
        self.mount_tree(&parent);
        // The filter itself, even if the directory listing missed it.
        self.mount(normalized, ResourceKind::Other);
    }

    fn mount_tree(&mut self, dir: &str) {
        if self.stopped {
            return;
        }
        let Ok(entries) = self.runtime.dir_list(Path::new(dir)) else {
            return;
        };
        let mut entries: Vec<String> = entries.iter().map(|e| normalize_request_path(e)).collect();
        entries.sort();
        for entry in entries {
            if self.stopped {
                return;
            }
            if self.runtime.is_dir(Path::new(&entry)).unwrap_or(false) {
                self.mount_tree(&entry);
            } else {
                self.mount(entry, ResourceKind::Other);
            }
        }
    }

    /// A user filter the browser cannot run: a JSON filter is an external
    /// executable.
    pub fn reject_json_filter(&mut self, path: &Path) {
        self.diagnostics.push(error(format!(
            "JSON filter {} is an external program and cannot run in the browser",
            normalize_request_path(path)
        )));
    }

    /// The mounted files (sorted by path) and the diagnostics noted.
    pub fn finish(self) -> (Vec<RequestFile>, Vec<DiagnosticMessage>) {
        let refs = self
            .mounted
            .into_iter()
            .map(|(path, bytes)| RequestFile { path, bytes })
            .collect();
        (refs, self.diagnostics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding_leaves_malformed_escapes() {
        assert_eq!(percent_decode("a%20b.png"), "a b.png");
        assert_eq!(percent_decode("100%.png"), "100%.png");
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("%E2%9C%93.png"), "\u{2713}.png");
    }

    #[test]
    fn external_targets_are_not_files() {
        for t in [
            "https://x.org/a.png",
            "data:image/png;base64,AA",
            "mailto:a@b",
        ] {
            assert!(is_external_target(t), "{t}");
        }
        for t in ["img/a.png", "C:/x/a.png", "../a.png", "a:b"] {
            assert!(!is_external_target(t), "{t}");
        }
    }

    #[test]
    fn image_extensions_match_the_host() {
        assert!(has_image_extension("/p/A.PNG"));
        assert!(has_image_extension("/p/a.tiff"));
        assert!(!has_image_extension("/p/a.docx"));
    }

    #[test]
    fn default_image_extension_reads_both_forms() {
        let joined = [PandocArg::text("--default-image-extension=png")];
        assert_eq!(default_image_extension(&joined).as_deref(), Some("png"));
        let split = [
            PandocArg::text("--default-image-extension"),
            PandocArg::text("svg"),
        ];
        assert_eq!(default_image_extension(&split).as_deref(), Some("svg"));
        assert_eq!(default_image_extension(&[PandocArg::text("--toc")]), None);
    }
}
