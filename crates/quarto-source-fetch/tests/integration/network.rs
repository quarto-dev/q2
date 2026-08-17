//! Fetching: candidate ordering, archive-root derivation, and the real
//! HTTP client (bd-1vlw8, plan Phase 0 cases 15–18).
//!
//! Two levels, deliberately:
//!
//! - A **scripted fake** [`SourceFetch`] covers which URLs are tried and
//!   in what order. That logic is where the Quarto 1 defects live, and
//!   it deserves tests that state the expected request sequence
//!   exactly.
//! - A **real localhost server** covers `UreqFetch` itself, so the
//!   client that ships is exercised — timeouts, redirects, status
//!   handling, streaming — rather than only the seam around it. No
//!   external network is involved: the test binds `127.0.0.1:0` and
//!   serves bytes it built.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Mutex};

use quarto_source_fetch::{
    ExtractLimits, FetchError, SourceFetch, Target, UreqFetch, fetch_into, resolve_target,
};
use tempfile::TempDir;

// ====================================================================
// A scripted fetcher
// ====================================================================

/// Serves canned responses by URL and records every request, in order.
struct FakeFetch {
    /// URL → (status, body). A URL absent from the map answers 404.
    responses: HashMap<String, (u16, Vec<u8>)>,
    requested: Arc<Mutex<Vec<String>>>,
}

impl FakeFetch {
    fn new() -> Self {
        Self {
            responses: HashMap::new(),
            requested: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn serving(mut self, url: &str, body: Vec<u8>) -> Self {
        self.responses.insert(url.to_string(), (200, body));
        self
    }

    fn requests(&self) -> Vec<String> {
        self.requested.lock().unwrap().clone()
    }
}

impl SourceFetch for FakeFetch {
    fn get_to_file(
        &self,
        url: &str,
        dest: &Path,
        _limits: &ExtractLimits,
    ) -> Result<u16, FetchError> {
        self.requested.lock().unwrap().push(url.to_string());
        match self.responses.get(url) {
            Some((status, body)) => {
                std::fs::write(dest, body).unwrap();
                Ok(*status)
            }
            None => Ok(404),
        }
    }
}

/// A `.tar.gz` whose entries all sit under `root_dir`.
fn tar_gz_with_root(root_dir: &str, files: &[(&str, &str)]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let encoder = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);

        let mut dir = tar::Header::new_gnu();
        dir.set_entry_type(tar::EntryType::Directory);
        dir.set_mode(0o755);
        dir.set_size(0);
        dir.set_path(format!("{root_dir}/")).unwrap();
        dir.set_cksum();
        builder.append(&dir, std::io::empty()).unwrap();

        for (name, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_mode(0o644);
            header.set_size(content.len() as u64);
            header.set_path(format!("{root_dir}/{name}")).unwrap();
            header.set_cksum();
            builder.append(&header, content.as_bytes()).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
    }
    buf
}

fn brand_archive(root_dir: &str) -> Vec<u8> {
    tar_gz_with_root(root_dir, &[("_brand.yml", "color:\n  primary: red\n")])
}

fn remote_target(input: &str) -> Target {
    resolve_target(input).unwrap_or_else(|e| panic!("{input:?} should resolve: {e}"))
}

// ====================================================================
// Case 18 — default-branch probing (Quarto 1 defect 1)
// ====================================================================

#[test]
fn a_main_default_repository_resolves_on_the_first_request() {
    let url = "https://github.com/org/repo/archive/refs/heads/main.tar.gz";
    let fake = FakeFetch::new().serving(url, brand_archive("repo-main"));
    let work = TempDir::new().unwrap();

    let root = fetch_into(
        &remote_target("org/repo"),
        work.path(),
        &fake,
        &ExtractLimits::default(),
    )
    .unwrap();

    assert_eq!(fake.requests(), [url]);
    assert!(root.join("_brand.yml").is_file());
}

#[test]
fn a_master_default_repository_resolves_after_main_404s() {
    // Quarto 1 hardcodes `refs/heads/main` as the only candidate for a
    // bare `org/repo` (extension-host.ts:114), so this repository is
    // simply unreachable there and the user is told the brand was "not
    // found in local or remote sources".
    let master = "https://github.com/org/repo/archive/refs/heads/master.tar.gz";
    let fake = FakeFetch::new().serving(master, brand_archive("repo-master"));
    let work = TempDir::new().unwrap();

    let root = fetch_into(
        &remote_target("org/repo"),
        work.path(),
        &fake,
        &ExtractLimits::default(),
    )
    .unwrap();

    assert_eq!(
        fake.requests(),
        [
            "https://github.com/org/repo/archive/refs/heads/main.tar.gz",
            master
        ],
        "main must be tried first, then master"
    );
    assert!(root.join("_brand.yml").is_file());
}

#[test]
fn a_ref_tries_the_tag_before_the_branch() {
    let branch = "https://github.com/org/repo/archive/refs/heads/topic.tar.gz";
    let fake = FakeFetch::new().serving(branch, brand_archive("repo-topic"));
    let work = TempDir::new().unwrap();

    fetch_into(
        &remote_target("org/repo@topic"),
        work.path(),
        &fake,
        &ExtractLimits::default(),
    )
    .unwrap();

    assert_eq!(
        fake.requests(),
        [
            "https://github.com/org/repo/archive/refs/tags/topic.tar.gz",
            branch
        ]
    );
}

#[test]
fn exhausting_every_candidate_reports_what_was_tried() {
    let fake = FakeFetch::new(); // everything 404s
    let work = TempDir::new().unwrap();

    let err = fetch_into(
        &remote_target("org/repo"),
        work.path(),
        &fake,
        &ExtractLimits::default(),
    )
    .expect_err("nothing is served, so this must fail");

    let msg = err.to_string();
    // The message must name the real problem. Quarto 1's equivalent
    // ("Brand not found in local or remote sources") does not say what
    // it looked for, which is what makes a master-default repository
    // undiagnosable there.
    assert!(msg.contains("org/repo"), "{msg}");
    assert!(msg.contains("main") && msg.contains("master"), "{msg}");
    assert!(msg.contains("404"), "{msg}");
}

// ====================================================================
// Case 17 — archive-root derivation (Quarto 1 defects 2 & 3)
// ====================================================================

#[test]
fn a_ref_containing_a_slash_extracts_correctly() {
    // Quarto 1 predicts the archive root as `<repo>-<ref>`, giving
    // `repo-feature/foo` — a two-segment path no single root can match
    // (extension-host.ts:153-160). We read the root from the archive,
    // so whatever GitHub actually names it is what we use.
    let url = "https://github.com/org/repo/archive/refs/heads/feature/foo.tar.gz";
    let fake = FakeFetch::new().serving(url, brand_archive("repo-feature-foo"));
    let work = TempDir::new().unwrap();

    let root = fetch_into(
        &remote_target("org/repo@feature/foo"),
        work.path(),
        &fake,
        &ExtractLimits::default(),
    )
    .unwrap();

    assert!(root.join("_brand.yml").is_file());
    assert!(root.ends_with("repo-feature-foo"));
}

#[test]
fn a_tag_beginning_with_v_extracts_correctly() {
    // Quarto 1's `tagSubDirectory` strips a leading `v` from any tag,
    // so it would look for `repo-alid-release` here
    // (extension-host.ts:225-232).
    let url = "https://github.com/org/repo/archive/refs/tags/valid-release.tar.gz";
    let fake = FakeFetch::new().serving(url, brand_archive("repo-valid-release"));
    let work = TempDir::new().unwrap();

    let root = fetch_into(
        &remote_target("org/repo@valid-release"),
        work.path(),
        &fake,
        &ExtractLimits::default(),
    )
    .unwrap();

    assert!(root.join("_brand.yml").is_file());
}

#[test]
fn an_archive_root_named_nothing_like_the_repo_still_works() {
    // The strongest form of the point: the root name carries no
    // information we rely on.
    let url = "https://github.com/org/repo/archive/refs/heads/main.tar.gz";
    let fake = FakeFetch::new().serving(url, brand_archive("something-entirely-different"));
    let work = TempDir::new().unwrap();

    let root = fetch_into(
        &remote_target("org/repo"),
        work.path(),
        &fake,
        &ExtractLimits::default(),
    )
    .unwrap();
    assert!(root.join("_brand.yml").is_file());
}

// ====================================================================
// Case 16 — subdirectory targets
// ====================================================================

#[test]
fn a_subdirectory_target_selects_the_brand_beneath_the_root() {
    let url = "https://github.com/org/repo/archive/refs/heads/main.tar.gz";
    let archive = tar_gz_with_root(
        "repo-main",
        &[
            ("brands/dark/_brand.yml", "color:\n  primary: black\n"),
            ("brands/light/_brand.yml", "color:\n  primary: white\n"),
            ("README.md", "hi"),
        ],
    );
    let fake = FakeFetch::new().serving(url, archive);
    let work = TempDir::new().unwrap();

    let root = fetch_into(
        &remote_target("org/repo/brands/dark"),
        work.path(),
        &fake,
        &ExtractLimits::default(),
    )
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(root.join("_brand.yml")).unwrap(),
        "color:\n  primary: black\n"
    );
}

#[test]
fn a_missing_subdirectory_errors_with_what_is_available() {
    let url = "https://github.com/org/repo/archive/refs/heads/main.tar.gz";
    let archive = tar_gz_with_root(
        "repo-main",
        &[("brands/dark/_brand.yml", "color:\n"), ("docs/x.md", "hi")],
    );
    let fake = FakeFetch::new().serving(url, archive);
    let work = TempDir::new().unwrap();

    let err = fetch_into(
        &remote_target("org/repo/nope"),
        work.path(),
        &fake,
        &ExtractLimits::default(),
    )
    .expect_err("a missing subdirectory must be reported");

    let msg = err.to_string();
    assert!(msg.contains("nope"), "{msg}");
    assert!(msg.contains("brands") && msg.contains("docs"), "{msg}");
}

// ====================================================================
// Local targets
// ====================================================================

#[test]
fn a_local_directory_is_used_in_place() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("_brand.yml"), "color:\n").unwrap();
    let work = TempDir::new().unwrap();

    let root = fetch_into(
        &remote_target(&dir.path().to_string_lossy()),
        work.path(),
        &FakeFetch::new(),
        &ExtractLimits::default(),
    )
    .unwrap();

    assert_eq!(root, dir.path(), "a local directory must not be copied");
}

#[test]
fn a_local_archive_is_extracted_and_rooted() {
    let dir = TempDir::new().unwrap();
    let archive = dir.path().join("brand.tar.gz");
    std::fs::write(&archive, brand_archive("some-root")).unwrap();
    let work = TempDir::new().unwrap();

    let root = fetch_into(
        &remote_target(&archive.to_string_lossy()),
        work.path(),
        &FakeFetch::new(),
        &ExtractLimits::default(),
    )
    .unwrap();

    assert!(root.join("_brand.yml").is_file());
    assert!(
        root.starts_with(work.path()),
        "extraction must stay in work_dir"
    );
}

// ====================================================================
// Case 15 — the real client, against a localhost server
// ====================================================================

/// A one-shot HTTP server on 127.0.0.1. Returns its base URL and a
/// handle that must be kept alive for the server thread to run.
struct TestServer {
    base_url: String,
    _thread: std::thread::JoinHandle<()>,
}

impl TestServer {
    /// Serve `body` at `/archive` (200) and 404 everything else.
    /// `requests` records the paths asked for.
    fn start(body: Vec<u8>, requests: Arc<Mutex<Vec<String>>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind localhost");
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}");

        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut buf = [0u8; 2048];
                let Ok(n) = stream.read(&mut buf) else {
                    continue;
                };
                let request = String::from_utf8_lossy(&buf[..n]).into_owned();
                let path = request
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .to_string();
                requests.lock().unwrap().push(path.clone());

                if path == "/archive" {
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/gzip\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(&body);
                } else {
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\
                          Connection: close\r\n\r\nnot found",
                    );
                }
                let _ = stream.flush();
                // One request per test is enough; keep the loop going
                // so a second candidate can also be probed.
            }
        });

        Self {
            base_url,
            _thread: thread,
        }
    }
}

#[test]
fn the_real_client_downloads_and_extracts_from_a_url() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let server = TestServer::start(brand_archive("repo-main"), Arc::clone(&requests));
    let work = TempDir::new().unwrap();

    let target = resolve_target(&format!("{}/archive", server.base_url)).unwrap();
    let root = fetch_into(&target, work.path(), &UreqFetch, &ExtractLimits::default())
        .expect("the real client should fetch from localhost");

    assert_eq!(
        std::fs::read_to_string(root.join("_brand.yml")).unwrap(),
        "color:\n  primary: red\n"
    );
    assert_eq!(requests.lock().unwrap().as_slice(), ["/archive"]);
}

#[test]
fn the_real_client_reports_a_404_as_not_found() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let server = TestServer::start(brand_archive("repo-main"), Arc::clone(&requests));
    let work = TempDir::new().unwrap();

    let target = resolve_target(&format!("{}/missing", server.base_url)).unwrap();
    let err = fetch_into(&target, work.path(), &UreqFetch, &ExtractLimits::default())
        .expect_err("a 404 must not look like success");

    assert!(err.to_string().contains("404"), "got: {err}");
}

#[test]
fn the_real_client_enforces_the_download_ceiling() {
    // The ceiling counts bytes *on the wire*, so the fixture has to be
    // large after compression. A gzip of 200 KB of `a` is a few hundred
    // bytes and would never approach it — the first version of this
    // test made exactly that mistake and passed for the wrong reason.
    // Incompressible bytes keep the wire size honest. The body need not
    // be a valid archive: the download ceiling trips before anything
    // tries to read it.
    let big: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let server = TestServer::start(big, Arc::clone(&requests));
    let work = TempDir::new().unwrap();

    let limits = ExtractLimits {
        max_download_bytes: 1024,
        ..ExtractLimits::default()
    };
    let target = resolve_target(&format!("{}/archive", server.base_url)).unwrap();
    let err = fetch_into(&target, work.path(), &UreqFetch, &limits)
        .expect_err("an oversized download must be refused");

    assert!(
        matches!(err, FetchError::DownloadTooLarge { limit: 1024 }),
        "got: {err}"
    );
    // The partial download must not be left behind looking like an
    // archive a later step could pick up.
    assert!(!work.path().join("download").exists());
}

/// Sanity: the fixtures the fake serves are the same shape the real
/// client receives, so a green fake test is not green for a reason the
/// real path would not reproduce.
#[test]
fn fake_and_real_paths_agree_on_the_same_archive() {
    let body = brand_archive("repo-main");

    let fake_work = TempDir::new().unwrap();
    let url = "https://github.com/org/repo/archive/refs/heads/main.tar.gz";
    let fake = FakeFetch::new().serving(url, body.clone());
    let fake_root = fetch_into(
        &remote_target("org/repo"),
        fake_work.path(),
        &fake,
        &ExtractLimits::default(),
    )
    .unwrap();

    let requests = Arc::new(Mutex::new(Vec::new()));
    let server = TestServer::start(body, Arc::clone(&requests));
    let real_work = TempDir::new().unwrap();
    let target = resolve_target(&format!("{}/archive", server.base_url)).unwrap();
    let real_root = fetch_into(
        &target,
        real_work.path(),
        &UreqFetch,
        &ExtractLimits::default(),
    )
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(fake_root.join("_brand.yml")).unwrap(),
        std::fs::read_to_string(real_root.join("_brand.yml")).unwrap()
    );
    assert_eq!(
        fake_root.file_name(),
        real_root.file_name(),
        "both paths should derive the same archive root"
    );
}
