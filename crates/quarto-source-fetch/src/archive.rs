//! Safe extraction of an untrusted archive.
//!
//! The threat model is concrete: the archive comes from a URL the user
//! typed, it is parsed by this process, and its contents are written
//! into the user's project directory. Three things must not happen:
//!
//! 1. **Escape** — an entry writing outside the destination, via `..`,
//!    an absolute path, a drive prefix, or a symlink a later entry
//!    writes through.
//! 2. **Exhaustion** — a small archive expanding to fill the disk.
//! 3. **Divergence** — extracted content differing from what the
//!    archive appears to contain (silently skipped entries, sizes that
//!    disagree with the bytes delivered).
//!
//! Two archive formats are supported, and the hardening is written
//! **once**: both backends decode entries and hand every one to the
//! same [`ExtractSink`], which owns all the checks. A rule added to the
//! sink applies to both formats by construction, which is the property
//! that matters — the alternative is a rule that exists for tar and was
//! forgotten for zip.
//!
//! Format is chosen by **magic bytes**, never by file extension: a
//! `.zip` URL that redirects to a gzip (or the reverse) is common
//! enough with CDNs, and sniffing four bytes is free.

use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};

use crate::error::{FetchError, describe_leading_bytes};
use crate::limits::ExtractLimits;

/// Archive containers we can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    TarGz,
    Zip,
}

const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];
/// Local file header. Empty and spanned archives use `PK\x05\x06` and
/// `PK\x07\x08`; both are accepted so an empty archive reports "no
/// brand file found" rather than "not an archive".
const ZIP_MAGICS: [[u8; 4]; 3] = [
    [0x50, 0x4b, 0x03, 0x04],
    [0x50, 0x4b, 0x05, 0x06],
    [0x50, 0x4b, 0x07, 0x08],
];

/// Identify `path`'s container from its leading bytes.
pub fn detect_format(path: &Path) -> Result<ArchiveFormat, FetchError> {
    let mut file =
        File::open(path).map_err(|e| FetchError::io(format!("open {}", path.display()), e))?;
    let mut magic = [0u8; 4];
    let read = read_up_to(&mut file, &mut magic)
        .map_err(|e| FetchError::io(format!("read {}", path.display()), e))?;
    let magic = &magic[..read];

    if magic.len() >= 2 && magic[..2] == GZIP_MAGIC {
        return Ok(ArchiveFormat::TarGz);
    }
    if magic.len() >= 4 && ZIP_MAGICS.iter().any(|m| magic == m) {
        return Ok(ArchiveFormat::Zip);
    }
    Err(FetchError::UnknownArchiveFormat {
        path: path.to_path_buf(),
        leading_bytes: describe_leading_bytes(magic),
    })
}

/// Read up to `buf.len()` bytes, tolerating short reads.
fn read_up_to(reader: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

/// What an extraction produced, for logging and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractSummary {
    pub entries: usize,
    pub bytes: u64,
}

/// Extract `archive` into `dest`, enforcing `limits`.
///
/// `dest` must already exist and should be a fresh directory the caller
/// owns — extraction is not merged into a populated tree, and the
/// caller inspects the result before copying anything into the user's
/// project.
pub fn extract_into(
    archive: &Path,
    dest: &Path,
    limits: &ExtractLimits,
) -> Result<ExtractSummary, FetchError> {
    // Resolve `dest` once. On macOS `/tmp` is a symlink to
    // `/private/tmp`, so an unresolved prefix comparison in the
    // containment assertion below would produce false alarms.
    let dest = dest
        .canonicalize()
        .map_err(|e| FetchError::io(format!("resolve {}", dest.display()), e))?;

    let mut sink = ExtractSink::new(dest, limits);
    match detect_format(archive)? {
        ArchiveFormat::TarGz => extract_tar_gz(archive, &mut sink)?,
        ArchiveFormat::Zip => extract_zip(archive, &mut sink)?,
    }
    Ok(sink.summary())
}

// ====================================================================
// The sink: every safety rule lives here, once, for both formats
// ====================================================================

struct ExtractSink<'a> {
    dest: PathBuf,
    limits: &'a ExtractLimits,
    entries: usize,
    bytes: u64,
}

impl<'a> ExtractSink<'a> {
    fn new(dest: PathBuf, limits: &'a ExtractLimits) -> Self {
        Self {
            dest,
            limits,
            entries: 0,
            bytes: 0,
        }
    }

    fn summary(&self) -> ExtractSummary {
        ExtractSummary {
            entries: self.entries,
            bytes: self.bytes,
        }
    }

    /// Count an entry against the entry ceiling and resolve its name to
    /// a path inside the destination.
    fn accept(&mut self, name: &str) -> Result<PathBuf, FetchError> {
        self.entries += 1;
        if self.entries > self.limits.max_entries {
            return Err(FetchError::TooManyEntries {
                limit: self.limits.max_entries,
            });
        }
        let relative = sanitize_entry_name(name)?;
        let target = self.dest.join(&relative);

        // Belt and braces. `sanitize_entry_name` already makes escape
        // impossible, so this assertion should be unreachable — which
        // is exactly why it is cheap to keep: if a future change to the
        // sanitizer weakens it, this fails closed instead of writing
        // outside the destination.
        if !target.starts_with(&self.dest) {
            return Err(FetchError::UnsafeEntryPath {
                name: name.to_string(),
                reason: "resolves outside the destination directory",
            });
        }
        Ok(target)
    }

    fn directory(&mut self, name: &str) -> Result<(), FetchError> {
        let target = self.accept(name)?;
        std::fs::create_dir_all(&target)
            .map_err(|e| FetchError::io(format!("create {}", target.display()), e))
    }

    /// Write one file entry, streaming through the byte ceiling.
    ///
    /// `declared` is the size the archive claims. It is used twice, and
    /// trusted for neither: as a cheap early reject when it alone
    /// exceeds the remaining budget, and as a cross-check against the
    /// bytes actually delivered.
    fn file(
        &mut self,
        name: &str,
        reader: &mut dyn Read,
        declared: Option<u64>,
    ) -> Result<(), FetchError> {
        let target = self.accept(name)?;

        if let Some(declared) = declared
            && self.bytes.saturating_add(declared) > self.limits.max_uncompressed_bytes
        {
            return Err(FetchError::TooLarge {
                limit: self.limits.max_uncompressed_bytes,
            });
        }

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| FetchError::io(format!("create {}", parent.display()), e))?;
        }
        let mut out = File::create(&target)
            .map_err(|e| FetchError::io(format!("create {}", target.display()), e))?;

        let mut written = 0u64;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| FetchError::io(format!("read entry {name:?}"), e))?;
            if n == 0 {
                break;
            }
            // Check *before* writing, so the ceiling bounds what reaches
            // the disk rather than what reaches it plus one buffer.
            self.bytes += n as u64;
            written += n as u64;
            if self.bytes > self.limits.max_uncompressed_bytes {
                return Err(FetchError::TooLarge {
                    limit: self.limits.max_uncompressed_bytes,
                });
            }
            out.write_all(&buf[..n])
                .map_err(|e| FetchError::io(format!("write {}", target.display()), e))?;
        }

        if let Some(declared) = declared
            && declared != written
        {
            return Err(FetchError::SizeMismatch {
                name: name.to_string(),
                declared,
                actual: written,
            });
        }
        Ok(())
    }
}

/// Turn an archive entry name into a relative path guaranteed to stay
/// inside the destination — or refuse it.
///
/// Operates on the raw **string**, before any `Path` parsing, because
/// `Path` semantics are platform-dependent in exactly the ways that
/// matter here: on Unix, `..\..\evil` is one ordinary component and
/// `C:\evil` is a legal filename, so a Windows-shaped attack path would
/// pass a `Component`-based check on Linux and mean something else
/// entirely on Windows. Refusing on the string makes the verdict
/// identical on every platform.
fn sanitize_entry_name(name: &str) -> Result<PathBuf, FetchError> {
    let unsafe_path = |reason: &'static str| FetchError::UnsafeEntryPath {
        name: name.to_string(),
        reason,
    };

    if name.is_empty() {
        return Err(unsafe_path("the name is empty"));
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return Err(unsafe_path("it is an absolute path"));
    }
    // `C:` / `c:/...` — a drive-relative or drive-absolute Windows path.
    let bytes = name.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return Err(unsafe_path("it carries a Windows drive prefix"));
    }
    if name.contains('\0') {
        return Err(unsafe_path("it contains a NUL byte"));
    }

    let mut out = PathBuf::new();
    for segment in name.split(['/', '\\']) {
        match segment {
            "" | "." => continue,
            ".." => return Err(unsafe_path("it climbs above the destination with `..`")),
            other => out.push(other),
        }
    }

    if out.as_os_str().is_empty() {
        return Err(unsafe_path("it names no file"));
    }

    // `out` was built only from `Component::Normal`-shaped segments, so
    // this holds by construction; verify anyway rather than assume the
    // loop above stays correct forever.
    if out.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(unsafe_path("it does not resolve to a plain relative path"));
    }
    Ok(out)
}

// ====================================================================
// Backends
// ====================================================================

fn extract_tar_gz(archive: &Path, sink: &mut ExtractSink) -> Result<(), FetchError> {
    let file = File::open(archive)
        .map_err(|e| FetchError::io(format!("open {}", archive.display()), e))?;
    let decoder = flate2::read::GzDecoder::new(BufReader::new(file));
    let mut tar = tar::Archive::new(decoder);

    let entries = tar
        .entries()
        .map_err(|e| FetchError::Archive(format!("reading tar entries: {e}")))?;

    for entry in entries {
        let mut entry =
            entry.map_err(|e| FetchError::Archive(format!("reading tar entry: {e}")))?;
        let header = entry.header().clone();
        let name = entry
            .path()
            .map_err(|e| FetchError::Archive(format!("decoding tar entry name: {e}")))?
            .to_string_lossy()
            .into_owned();

        let kind = header.entry_type();
        if kind.is_dir() {
            sink.directory(&name)?;
            continue;
        }
        if let Some(rejected) = rejected_tar_kind(kind) {
            return Err(FetchError::UnsupportedEntryType {
                name,
                kind: rejected,
            });
        }

        // GNU long-name/long-link metadata entries carry no payload and
        // are consumed by the tar crate itself; anything else that is
        // not a plain file has already been rejected above.
        let declared = header.size().ok();
        sink.file(&name, &mut entry, declared)?;
    }
    Ok(())
}

/// Classify a tar entry type we refuse, or `None` for a plain file.
fn rejected_tar_kind(kind: tar::EntryType) -> Option<&'static str> {
    use tar::EntryType;
    match kind {
        EntryType::Regular | EntryType::Continuous => None,
        EntryType::Symlink => Some("symbolic link"),
        EntryType::Link => Some("hard link"),
        EntryType::Char => Some("character device"),
        EntryType::Block => Some("block device"),
        EntryType::Fifo => Some("named pipe"),
        // GNU/pax extension records are metadata the tar crate applies
        // to the following entry; they are not content and must not be
        // written out as files.
        EntryType::GNULongName
        | EntryType::GNULongLink
        | EntryType::XGlobalHeader
        | EntryType::XHeader => Some("metadata record"),
        _ => Some("entry of an unsupported type"),
    }
}

fn extract_zip(archive: &Path, sink: &mut ExtractSink) -> Result<(), FetchError> {
    let file = File::open(archive)
        .map_err(|e| FetchError::io(format!("open {}", archive.display()), e))?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|e| FetchError::Archive(format!("reading zip: {e}")))?;

    // The entry count is known up front here, so the ceiling can be
    // enforced before decompressing anything at all.
    if zip.len() > sink.limits.max_entries {
        return Err(FetchError::TooManyEntries {
            limit: sink.limits.max_entries,
        });
    }

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| FetchError::Archive(format!("reading zip entry {i}: {e}")))?;
        let raw_name = entry.name().to_string();

        // `enclosed_name()` is the zip crate's own containment check,
        // kept as a second, independent opinion — `sanitize_entry_name`
        // in the sink is the primary defense and catches everything our
        // hostile-archive tests throw, so removing this check alone
        // does not turn any test red (verified by mutation). It stays
        // because agreeing checks from two codebases are worth more
        // than one here, and because the tempting "fix" for a `None` —
        // falling back to `name()` — is the classic zip-slip mistake.
        if entry.enclosed_name().is_none() {
            return Err(FetchError::UnsafeEntryPath {
                name: raw_name,
                reason: "the zip entry name does not stay inside the archive",
            });
        }

        if entry.is_dir() {
            sink.directory(&raw_name)?;
            continue;
        }
        if entry.is_symlink() {
            return Err(FetchError::UnsupportedEntryType {
                name: raw_name,
                kind: "symbolic link",
            });
        }

        let declared = entry.size();
        sink.file(&raw_name, &mut entry, Some(declared))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizer_accepts_plain_relative_names() {
        assert_eq!(
            sanitize_entry_name("repo-main/_brand.yml").unwrap(),
            PathBuf::from("repo-main").join("_brand.yml")
        );
        assert_eq!(
            sanitize_entry_name("./a/./b.txt").unwrap(),
            PathBuf::from("a").join("b.txt")
        );
    }

    #[test]
    fn sanitizer_rejects_parent_traversal() {
        for name in [
            "../evil",
            "a/../../evil",
            "..",
            "a/..",
            // Backslash-separated traversal: one opaque component on
            // Unix, a real traversal on Windows. Must be refused
            // identically on both.
            "..\\evil",
            "a\\..\\..\\evil",
        ] {
            let err = sanitize_entry_name(name).unwrap_err_for(name);
            assert!(
                matches!(err, FetchError::UnsafeEntryPath { .. }),
                "{name:?} should be rejected, got {err:?}"
            );
        }
    }

    #[test]
    fn sanitizer_rejects_absolute_and_drive_paths() {
        for name in ["/etc/passwd", "\\windows\\system32", "C:/evil", "c:evil"] {
            assert!(
                sanitize_entry_name(name).is_err(),
                "{name:?} should be rejected"
            );
        }
    }

    #[test]
    fn sanitizer_rejects_empty_and_nul() {
        assert!(sanitize_entry_name("").is_err());
        assert!(sanitize_entry_name("./").is_err());
        assert!(sanitize_entry_name("a\0b").is_err());
    }

    /// Small helper so the loop above reports which input failed.
    trait UnwrapErrFor {
        fn unwrap_err_for(self, name: &str) -> FetchError;
    }
    impl UnwrapErrFor for Result<PathBuf, FetchError> {
        fn unwrap_err_for(self, name: &str) -> FetchError {
            match self {
                Ok(p) => panic!("{name:?} was accepted as {}", p.display()),
                Err(e) => e,
            }
        }
    }
}
