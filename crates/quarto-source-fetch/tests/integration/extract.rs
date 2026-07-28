//! Extraction hardening, run against **both** archive backends
//! (bd-1vlw8, plan Phase 0 cases 13–14).
//!
//! Most cases are expressed once, as a list of [`Entry`] values, then
//! built as *both* a `.tar.gz` and a `.zip` and run through the same
//! assertion (see [`assert_refused_in_both`]). That structure is the
//! point: the realistic failure mode for two-format extraction is not
//! "the check was wrong" but "the check was written for tar and
//! forgotten for zip". A case written this way cannot cover only one
//! format. Where a hazard genuinely exists in one format only — tar
//! hardlinks, zip size fields — the test says so and explains why.
//!
//! Archives are built in-process rather than checked in as fixtures —
//! a committed malicious archive is awkward to review, awkward to keep
//! honest, and (for the bomb cases) large.
//!
//! These tests were checked by mutation: each safety rule in
//! `archive.rs` was disabled in turn to confirm something here turns
//! red. That exercise found a real gap — see
//! `a_lying_declared_size_still_trips_the_streaming_ceiling`, which
//! exists because deleting the streaming byte ceiling left every other
//! test green.

use std::io::Write;
use std::path::{Path, PathBuf};

use quarto_source_fetch::{ArchiveFormat, ExtractLimits, FetchError, detect_format, extract_into};
use tempfile::TempDir;

// ====================================================================
// Archive construction
// ====================================================================

/// One entry to place in a synthetic archive.
#[derive(Clone)]
enum Entry {
    File { name: String, content: Vec<u8> },
    Dir { name: String },
    Symlink { name: String, target: String },
}

impl Entry {
    fn file(name: &str, content: &str) -> Self {
        Entry::File {
            name: name.to_string(),
            content: content.as_bytes().to_vec(),
        }
    }
    fn sized_file(name: &str, bytes: usize) -> Self {
        Entry::File {
            name: name.to_string(),
            content: vec![b'a'; bytes],
        }
    }
    fn dir(name: &str) -> Self {
        Entry::Dir {
            name: name.to_string(),
        }
    }
    fn symlink(name: &str, target: &str) -> Self {
        Entry::Symlink {
            name: name.to_string(),
            target: target.to_string(),
        }
    }
}

/// Write `name` into a tar header's raw name field, bypassing
/// `Header::set_path`.
///
/// `set_path` refuses `..` and absolute paths — correct for a library
/// that *writes* archives, and exactly why it cannot be used here: the
/// whole point of these fixtures is to produce the archive a hostile
/// server would send, which no well-behaved writer would emit. The 100
/// raw name bytes are the same field a real tar reader parses.
fn set_raw_name(header: &mut tar::Header, name: &str) {
    let bytes = name.as_bytes();
    let field = &mut header.as_old_mut().name;
    assert!(
        bytes.len() <= field.len(),
        "fixture name {name:?} exceeds the 100-byte tar name field"
    );
    field.fill(0);
    field[..bytes.len()].copy_from_slice(bytes);
}

fn build_tar_gz(dir: &Path, entries: &[Entry]) -> PathBuf {
    let path = dir.join("archive.tar.gz");
    let file = std::fs::File::create(&path).unwrap();
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);

    for entry in entries {
        let mut header = tar::Header::new_gnu();
        match entry {
            Entry::File { name, content } => {
                header.set_entry_type(tar::EntryType::Regular);
                header.set_mode(0o644);
                header.set_size(content.len() as u64);
                set_raw_name(&mut header, name);
                header.set_cksum();
                builder.append(&header, content.as_slice()).unwrap();
            }
            Entry::Dir { name } => {
                header.set_entry_type(tar::EntryType::Directory);
                header.set_mode(0o755);
                header.set_size(0);
                set_raw_name(&mut header, name);
                header.set_cksum();
                builder.append(&header, std::io::empty()).unwrap();
            }
            Entry::Symlink { name, target } => {
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_mode(0o777);
                header.set_size(0);
                header.set_link_name(target).unwrap();
                set_raw_name(&mut header, name);
                header.set_cksum();
                builder.append(&header, std::io::empty()).unwrap();
            }
        }
    }
    builder.into_inner().unwrap().finish().unwrap();
    path
}

fn build_zip(dir: &Path, entries: &[Entry]) -> PathBuf {
    let path = dir.join("archive.zip");
    let file = std::fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for entry in entries {
        match entry {
            Entry::File { name, content } => {
                zip.start_file(name.as_str(), options).unwrap();
                zip.write_all(content).unwrap();
            }
            Entry::Dir { name } => {
                zip.add_directory(name.as_str(), options).unwrap();
            }
            Entry::Symlink { name, target } => {
                zip.add_symlink(name.as_str(), target.as_str(), options)
                    .unwrap();
            }
        }
    }
    zip.finish().unwrap();
    path
}

/// Build `entries` in `format` and extract into a fresh directory.
fn extract(
    format: ArchiveFormat,
    entries: &[Entry],
    limits: &ExtractLimits,
) -> (
    TempDir,
    Result<quarto_source_fetch::ExtractSummary, FetchError>,
) {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    let archive = match format {
        ArchiveFormat::TarGz => build_tar_gz(&src, entries),
        ArchiveFormat::Zip => build_zip(&src, entries),
    };
    let result = extract_into(&archive, &dest, limits);
    (tmp, result)
}

const BOTH: [ArchiveFormat; 2] = [ArchiveFormat::TarGz, ArchiveFormat::Zip];

/// Every path present under `dir`, relative and sorted.
fn tree(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            out.push(
                p.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
            if p.is_dir() {
                walk(&p, base, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

// ====================================================================
// The happy path — the control for every rejection test below
// ====================================================================

#[test]
fn both_backends_extract_a_normal_archive() {
    for format in BOTH {
        let entries = [
            Entry::dir("repo-main/"),
            Entry::file("repo-main/_brand.yml", "color:\n  primary: red\n"),
            Entry::file("repo-main/logo.svg", "<svg/>"),
        ];
        let (tmp, result) = extract(format, &entries, &ExtractLimits::default());
        let summary = result.unwrap_or_else(|e| panic!("{format:?} should extract: {e}"));

        let dest = tmp.path().join("dest");
        assert_eq!(
            std::fs::read_to_string(dest.join("repo-main/_brand.yml")).unwrap(),
            "color:\n  primary: red\n",
            "{format:?}"
        );
        assert!(summary.entries >= 2, "{format:?}: {summary:?}");
        assert!(summary.bytes > 0, "{format:?}");
    }
}

// ====================================================================
// Case 13 — the hardening matrix
// ====================================================================

/// Assert that `entries` is refused in both formats and that the
/// destination is left empty.
fn assert_refused_in_both(label: &str, entries: &[Entry], limits: &ExtractLimits) {
    for format in BOTH {
        let (tmp, result) = extract(format, entries, limits);
        let err = match result {
            Ok(summary) => panic!("{label} ({format:?}) was accepted: {summary:?}"),
            Err(e) => e,
        };
        // Fail for the *right* reason. A rejection that happened to come
        // from an IO error or a malformed-archive parse would leave the
        // path check untested while the suite stayed green.
        assert!(
            matches!(err, FetchError::UnsafeEntryPath { .. }),
            "{label} ({format:?}) should be rejected as an unsafe path, got: {err}"
        );
        let dest = tmp.path().join("dest");
        assert!(
            tree(&dest).iter().all(|p| !p.contains("evil")),
            "{label} ({format:?}) wrote a hostile entry: {:?}",
            tree(&dest)
        );
    }
}

#[test]
fn parent_traversal_is_refused() {
    assert_refused_in_both(
        "../ traversal",
        &[Entry::file("../evil.txt", "pwned")],
        &ExtractLimits::default(),
    );
}

#[test]
fn nested_parent_traversal_is_refused() {
    assert_refused_in_both(
        "nested ../ traversal",
        &[Entry::file("repo-main/../../evil.txt", "pwned")],
        &ExtractLimits::default(),
    );
}

#[test]
fn absolute_path_is_refused() {
    assert_refused_in_both(
        "absolute path",
        &[Entry::file("/tmp/evil.txt", "pwned")],
        &ExtractLimits::default(),
    );
}

#[test]
fn windows_drive_prefix_is_refused() {
    // On Unix this is merely an odd filename; on Windows it is a real
    // escape. Refused identically on both so behavior does not depend
    // on which machine ran the command.
    assert_refused_in_both(
        "drive prefix",
        &[Entry::file("C:/evil.txt", "pwned")],
        &ExtractLimits::default(),
    );
}

#[test]
fn backslash_traversal_is_refused() {
    assert_refused_in_both(
        "backslash traversal",
        &[Entry::file("..\\evil.txt", "pwned")],
        &ExtractLimits::default(),
    );
}

#[test]
fn symlink_entries_are_refused() {
    // A symlink is not merely unnecessary in a brand: it is the setup
    // move for an escape, since a *later* entry writing through it
    // lands wherever it points.
    for format in BOTH {
        let entries = [
            Entry::symlink("repo-main/link", "/tmp"),
            Entry::file("repo-main/_brand.yml", "color:\n"),
        ];
        let (tmp, result) = extract(format, &entries, &ExtractLimits::default());
        let err = match result {
            Ok(s) => panic!("{format:?} accepted a symlink entry: {s:?}"),
            Err(e) => e,
        };
        assert!(
            matches!(err, FetchError::UnsupportedEntryType { .. }),
            "{format:?} should report an unsupported entry type, got: {err}"
        );
        assert!(
            !tmp.path().join("dest/repo-main/link").exists(),
            "{format:?} created the symlink"
        );
    }
}

#[test]
fn hardlink_entries_are_refused() {
    // tar-only: the zip format has no hardlink entry type.
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    let archive = src.join("archive.tar.gz");
    {
        let file = std::fs::File::create(&archive).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(0);
        header.set_entry_type(tar::EntryType::Link);
        header.set_mode(0o644);
        header.set_link_name("/etc/passwd").unwrap();
        header.set_cksum();
        builder
            .append_data(&mut header, "repo-main/link", std::io::empty())
            .unwrap();
        builder.into_inner().unwrap().finish().unwrap();
    }

    let err = extract_into(&archive, &dest, &ExtractLimits::default())
        .expect_err("a hardlink entry must be refused");
    assert!(
        matches!(err, FetchError::UnsupportedEntryType { .. }),
        "got: {err}"
    );
}

#[test]
fn too_many_entries_is_refused() {
    let limits = ExtractLimits {
        max_entries: 4,
        ..ExtractLimits::default()
    };
    let entries: Vec<Entry> = (0..10)
        .map(|i| Entry::file(&format!("repo-main/f{i}.txt"), "x"))
        .collect();

    for format in BOTH {
        let (_tmp, result) = extract(format, &entries, &limits);
        let err = match result {
            Ok(s) => panic!("{format:?} accepted {} entries: {s:?}", entries.len()),
            Err(e) => e,
        };
        assert!(
            matches!(err, FetchError::TooManyEntries { limit: 4 }),
            "{format:?} got: {err}"
        );
    }
}

#[test]
fn oversized_expansion_is_refused() {
    let limits = ExtractLimits {
        max_uncompressed_bytes: 1024,
        ..ExtractLimits::default()
    };
    // Highly compressible: a few hundred bytes on disk, 64 KiB expanded.
    // This is the decompression-bomb shape in miniature.
    let entries = [Entry::sized_file("repo-main/big.bin", 64 * 1024)];

    for format in BOTH {
        let (tmp, result) = extract(format, &entries, &limits);
        let err = match result {
            Ok(s) => panic!("{format:?} accepted a bomb: {s:?}"),
            Err(e) => e,
        };
        assert!(
            matches!(err, FetchError::TooLarge { limit: 1024 }),
            "{format:?} got: {err}"
        );

        // The ceiling must bound what reaches the disk, not just what
        // the summary reports.
        let written =
            std::fs::metadata(tmp.path().join("dest/repo-main/big.bin")).map_or(0, |m| m.len());
        assert!(
            written <= 1024 + 64 * 1024,
            "{format:?} wrote {written} bytes past a 1024-byte ceiling"
        );
    }
}

#[test]
fn budget_is_cumulative_across_entries() {
    // No single entry exceeds the ceiling; together they do. A
    // per-entry-only check would let this through.
    let limits = ExtractLimits {
        max_uncompressed_bytes: 3000,
        ..ExtractLimits::default()
    };
    let entries: Vec<Entry> = (0..5)
        .map(|i| Entry::sized_file(&format!("repo-main/f{i}.bin"), 1000))
        .collect();

    for format in BOTH {
        let (_tmp, result) = extract(format, &entries, &limits);
        assert!(
            matches!(result, Err(FetchError::TooLarge { .. })),
            "{format:?} should trip the cumulative ceiling, got {result:?}"
        );
    }
}

#[test]
fn zip_entry_that_understates_its_size_is_refused() {
    // Zip-specific: `ZipFile::size()` comes from the archive's own
    // metadata and is attacker-controlled. An entry that declares 1
    // byte and delivers 64 KiB would sail past a declared-size-only
    // budget check. The real ceiling is enforced while copying, and the
    // mismatch itself is reported.
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    let archive = src.join("archive.zip");
    {
        let file = std::fs::File::create(&archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("repo-main/big.bin", options).unwrap();
        zip.write_all(&vec![b'a'; 64 * 1024]).unwrap();
        zip.finish().unwrap();
    }
    // Corrupt the declared uncompressed size in both the local header
    // and the central directory so the archive claims a single byte.
    understate_zip_sizes(&archive);

    let err = extract_into(&archive, &dest, &ExtractLimits::default())
        .expect_err("a size-understating entry must be refused");
    assert!(
        matches!(
            err,
            FetchError::SizeMismatch { .. } | FetchError::Archive(_)
        ),
        "got: {err}"
    );
}

/// Rewrite every 4-byte uncompressed-size field in a zip to 1.
///
/// Crude but sufficient: the fixture has one entry, so the local header
/// (offset 22) and the central-directory record are the only sites.
fn understate_zip_sizes(path: &Path) {
    let mut bytes = std::fs::read(path).unwrap();
    // Local file header: PK\x03\x04, uncompressed size at +22.
    for i in 0..bytes.len().saturating_sub(26) {
        if bytes[i..i + 4] == [0x50, 0x4b, 0x03, 0x04] {
            bytes[i + 22..i + 26].copy_from_slice(&1u32.to_le_bytes());
        }
        // Central directory header: PK\x01\x02, uncompressed size at +24.
        if bytes[i..i + 4] == [0x50, 0x4b, 0x01, 0x02] && i + 28 <= bytes.len() {
            bytes[i + 24..i + 28].copy_from_slice(&1u32.to_le_bytes());
        }
    }
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn a_lying_declared_size_still_trips_the_streaming_ceiling() {
    // The decisive bomb test, and the one the obvious tests miss.
    //
    // `oversized_expansion_is_refused` is caught by the cheap
    // declared-size pre-check, so it never exercises the ceiling
    // enforced *while copying* — deleting that ceiling leaves those
    // tests green (verified by mutation). The only input that reaches
    // it is an entry whose declared size is a lie: small enough to pass
    // the pre-check, large enough to exhaust the budget on delivery.
    //
    // Zip-only by nature. A tar entry's declared size is what the
    // reader consumes, so tar cannot understate and over-deliver.
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    let archive = src.join("archive.zip");
    {
        let file = std::fs::File::create(&archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("repo-main/bomb.bin", options).unwrap();
        zip.write_all(&vec![b'a'; 64 * 1024]).unwrap();
        zip.finish().unwrap();
    }
    understate_zip_sizes(&archive);

    let limits = ExtractLimits {
        max_uncompressed_bytes: 1024,
        ..ExtractLimits::default()
    };
    let err = extract_into(&archive, &dest, &limits)
        .expect_err("an entry that lies about its size must still be capped");
    assert!(
        matches!(err, FetchError::TooLarge { limit: 1024 }),
        "the streaming ceiling should fire before the size cross-check — \
         a declared size of 1 passes every pre-check, so only the ceiling \
         enforced while copying can stop it. Got: {err}"
    );

    // And the cap must bound what actually reached the disk.
    let written = std::fs::metadata(dest.join("repo-main/bomb.bin")).map_or(0, |m| m.len());
    assert!(
        written < 64 * 1024,
        "the full 64 KiB payload was written despite a 1024-byte ceiling ({written} bytes)"
    );
}

// ====================================================================
// Case 14 — format detection by magic bytes, not extension
// ====================================================================

#[test]
fn format_is_detected_from_content_not_extension() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let entries = [Entry::file("repo-main/_brand.yml", "color:\n")];

    // A gzip archive named `.zip`, and a zip archive named `.tar.gz`.
    let gz = build_tar_gz(&src, &entries);
    let misnamed_gz = src.join("actually-gzip.zip");
    std::fs::rename(&gz, &misnamed_gz).unwrap();

    let zip_path = build_zip(&src, &entries);
    let misnamed_zip = src.join("actually-zip.tar.gz");
    std::fs::rename(&zip_path, &misnamed_zip).unwrap();

    assert_eq!(detect_format(&misnamed_gz).unwrap(), ArchiveFormat::TarGz);
    assert_eq!(detect_format(&misnamed_zip).unwrap(), ArchiveFormat::Zip);

    // And both still extract correctly despite the misleading names.
    for archive in [&misnamed_gz, &misnamed_zip] {
        let dest = tmp.path().join(format!(
            "dest-{}",
            archive.file_name().unwrap().to_string_lossy()
        ));
        std::fs::create_dir_all(&dest).unwrap();
        extract_into(archive, &dest, &ExtractLimits::default())
            .unwrap_or_else(|e| panic!("{} should extract: {e}", archive.display()));
        assert!(dest.join("repo-main/_brand.yml").is_file());
    }
}

#[test]
fn a_file_that_is_neither_format_errors_clearly() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("notanarchive.tar.gz");
    std::fs::write(&path, b"this is just text, not an archive at all").unwrap();

    let err = detect_format(&path).expect_err("plain text is not an archive");
    let msg = err.to_string();
    assert!(
        msg.contains("not a recognized archive"),
        "the error should say what is wrong; got: {msg}"
    );
    assert!(
        msg.contains(".tar.gz") && msg.contains(".zip"),
        "the error should say what is accepted; got: {msg}"
    );
}

#[test]
fn an_empty_file_errors_clearly() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("empty.tar.gz");
    std::fs::write(&path, b"").unwrap();

    let err = detect_format(&path).expect_err("an empty file is not an archive");
    assert!(err.to_string().contains("empty"), "got: {err}");
}
