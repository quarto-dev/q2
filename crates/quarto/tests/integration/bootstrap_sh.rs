//! E2E tests for install.sh, the curl|bash installer (bd-c6l13j79).
//!
//! Unix-only: the harness runs `bash install.sh` and shims `uname` with
//! executable shell scripts, so the whole suite is gated to `cfg(unix)`.
//! The Windows installer (install.ps1) is checksum-only and has no
//! offline suite; its only CI coverage is the Nightly workflow's
//! `install-smoke` job (.github/workflows/nightly.yml), which runs both
//! installers' README one-liners against each freshly published nightly
//! on linux, macOS and Windows (bd-p4ljdp2e).
//!
//! Ported from cscheid/braid's installer test harness
//! (external-sources/braid/crates/braid/tests/bootstrap_sh.rs), offline
//! by construction: every install path is exercised through
//! `--artifact-url file://...` plus `--checksum`, so no test here
//! touches the network. (One ignored test validates real version
//! resolution once releases exist; the plan's Phase 4 runs it.)
//!
//! The suite REQUIRES minisign on the host (CI installs it; locally:
//! `brew install minisign` / `apt-get install minisign`). A silent
//! skip-if-missing would leave the signature contract unguarded, so
//! absence is a loud failure instead — braid's deliberate choice, kept.
//!
//! Plan: claude-notes/plans/2026-06-12-q2-github-releases-bundled-mcp.md
//!
//! Filename note: this test must not contain `install`/`setup`/`update`/
//! `patch` in its name — Windows UAC installer-detection refuses to
//! launch exes whose filename matches that heuristic (os error 740).
//! The integration binary is named `integration`, but keep the module
//! name boring anyway. Hence `bootstrap_sh`.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

/// Lowercase hex of a SHA-256 over `bytes` (sha2 0.11's digest array
/// no longer implements LowerHex).
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/quarto regardless of this file's
    // location inside tests/integration/.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn install_sh() -> PathBuf {
    repo_root().join("install.sh")
}

/// A sandbox for one installer run: its own HOME, dest dir, and a
/// deterministic PATH (system tool dirs only, optionally prefixed with a
/// shim dir so tests can fake `uname`).
struct Sandbox {
    tmp: tempfile::TempDir,
}

const SYSTEM_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

impl Sandbox {
    fn new() -> Sandbox {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("home")).unwrap();
        Sandbox { tmp }
    }

    fn home(&self) -> PathBuf {
        self.tmp.path().join("home")
    }

    /// Install destination. Deliberately not created up front: the
    /// installer must create it.
    fn dest(&self) -> PathBuf {
        self.tmp.path().join("bin")
    }

    fn installed_binary(&self) -> PathBuf {
        self.dest().join("q2")
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_env(args, &[])
    }

    fn run_env(&self, args: &[&str], envs: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new("bash");
        cmd.arg(install_sh())
            .args(args)
            .env_clear()
            .env("HOME", self.home())
            // minisign is appended (not prepended) so system tools keep
            // priority; on Linux it is usually in /usr/bin already.
            .env("PATH", default_sandbox_path());
        for (k, v) in envs {
            cmd.env(k, v);
        }
        cmd.output().unwrap()
    }

    /// Write a fake `uname` responding to -s/-m, and return a PATH that
    /// resolves it first.
    fn uname_shim(&self, os: &str, arch: &str) -> String {
        let shim = self.tmp.path().join("shim");
        fs::create_dir_all(&shim).unwrap();
        let uname = shim.join("uname");
        fs::write(
            &uname,
            format!(
                "#!/bin/sh\ncase \"${{1:-}}\" in\n  -m) echo \"{arch}\" ;;\n  *) echo \"{os}\" ;;\nesac\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&uname, fs::Permissions::from_mode(0o755)).unwrap();
        // Same tail as `run`'s default PATH (system dirs + minisign), so
        // a shimmed-platform install can still verify signatures.
        format!("{}:{}", shim.display(), default_sandbox_path())
    }

    /// Replace `sleep` (in the same shim dir `uname_shim` puts first on
    /// PATH) with `body`, so the nightly lookup's retry backoff costs no
    /// wall time and a test can change the fake API between attempts.
    fn sleep_shim(&self, body: &str) {
        let shim = self.tmp.path().join("shim");
        fs::create_dir_all(&shim).unwrap();
        let sleep = shim.join("sleep");
        fs::write(&sleep, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&sleep, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn assert_success(out: &Output) {
    assert!(
        out.status.success(),
        "expected success, got {:?}\nstdout: {}\nstderr: {}",
        out.status,
        stdout(out),
        stderr(out)
    );
}

fn assert_failure(out: &Output) {
    assert!(
        !out.status.success(),
        "expected failure, got success\nstdout: {}\nstderr: {}",
        stdout(out),
        stderr(out)
    );
}

/// Build a release-shaped artifact: a tar.gz containing a single
/// executable named `q2` (a shell script standing in for the real
/// binary). Returns the artifact's file:// URL and its SHA-256.
fn make_artifact(dir: &Path) -> (String, String) {
    let payload = dir.join("payload");
    fs::create_dir_all(&payload).unwrap();
    let bin = payload.join("q2");
    fs::write(&bin, "#!/bin/sh\necho \"q2 0.0.0-test\"\n").unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();

    let archive = dir.join("q2-0.0.0-test.tar.gz");
    let status = Command::new("tar")
        .args(["-czf"])
        .arg(&archive)
        .arg("-C")
        .arg(&payload)
        .arg("q2")
        .status()
        .unwrap();
    assert!(status.success(), "tar failed");

    let sha = sha256_hex(&fs::read(&archive).unwrap());
    (format!("file://{}", archive.display()), sha)
}

fn dest_arg(sb: &Sandbox) -> String {
    sb.dest().display().to_string()
}

// --- minisign test helpers ---------------------------------------------------
//
// Signatures are part of the artifact contract, so the suite REQUIRES
// minisign on the host (CI installs it; locally: `brew install minisign`
// or `apt-get install minisign`). A silent skip-if-missing would leave
// the contract unguarded, so absence is a loud failure instead.

fn minisign_bin() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let out = Command::new("which")
            .arg("minisign")
            .output()
            .expect("run `which`");
        assert!(
            out.status.success(),
            "installer tests require minisign \
             (brew install minisign / apt-get install minisign)"
        );
        PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
    })
}

/// Sandbox PATH: system dirs plus (appended) the host minisign's
/// directory, which on macOS lives outside SYSTEM_PATH.
fn default_sandbox_path() -> String {
    format!(
        "{}:{}",
        SYSTEM_PATH,
        minisign_bin().parent().unwrap().display()
    )
}

/// A throwaway unencrypted signing keypair (the shape CI uses).
struct TestKey {
    key_file: PathBuf,
    pub_key: String,
}

impl TestKey {
    fn generate(dir: &Path) -> TestKey {
        fs::create_dir_all(dir).unwrap();
        let pub_file = dir.join("test-minisign.pub");
        let key_file = dir.join("test-minisign.key");
        let out = Command::new(minisign_bin())
            .args(["-G", "-W", "-f", "-p"])
            .arg(&pub_file)
            .arg("-s")
            .arg(&key_file)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "minisign -G failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        // .pub layout: an untrusted-comment line, then the key itself.
        let pub_key = fs::read_to_string(&pub_file)
            .unwrap()
            .lines()
            .nth(1)
            .unwrap()
            .trim()
            .to_owned();
        TestKey { key_file, pub_key }
    }

    /// Sign `file` with the given trusted comment, producing `file.minisig`.
    fn sign_with_comment(&self, file: &Path, comment: &str) {
        let out = Command::new(minisign_bin())
            .arg("-Sm")
            .arg(file)
            .arg("-s")
            .arg(&self.key_file)
            .args(["-t", comment])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "minisign -Sm failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Sign `file` the way release.yml does: trusted comment = filename.
    fn sign(&self, file: &Path) {
        self.sign_with_comment(file, file.file_name().unwrap().to_str().unwrap());
    }
}

/// `make_artifact` plus a valid signature under a fresh keypair.
/// Returns (artifact url, sha256, public key for --minisign-pubkey).
fn make_signed_artifact(dir: &Path) -> (String, String, String) {
    let (url, sha) = make_artifact(dir);
    let key = TestKey::generate(dir);
    key.sign(Path::new(url.strip_prefix("file://").unwrap()));
    (url, sha, key.pub_key)
}

// --- help & argument handling ----------------------------------------------

#[test]
fn help_lists_every_flag_and_exits_zero() {
    let sb = Sandbox::new();
    let out = sb.run(&["--help"]);
    assert_success(&out);
    let text = stdout(&out);
    for flag in [
        "--version",
        "--dest",
        "--artifact-url",
        "--checksum",
        "--insecure-skip-checksum",
        "--minisign-pubkey",
        "--insecure-skip-signature",
        "--from-source",
        "--uninstall",
        "--print-platform",
        "--nightly",
        "--quiet",
        "--help",
        "Q2_INSTALL_DIR",
    ] {
        assert!(text.contains(flag), "--help is missing {flag}\n{text}");
    }
}

#[test]
fn unknown_flag_is_an_error_naming_the_flag() {
    let sb = Sandbox::new();
    let out = sb.run(&["--frobnicate"]);
    assert_failure(&out);
    assert!(
        stderr(&out).contains("--frobnicate"),
        "stderr: {}",
        stderr(&out)
    );
}

// --- platform detection ------------------------------------------------------

#[test]
fn detects_linux_amd64() {
    let sb = Sandbox::new();
    let path = sb.uname_shim("Linux", "x86_64");
    let out = sb.run_env(&["--print-platform"], &[("PATH", &path)]);
    assert_success(&out);
    assert_eq!(stdout(&out).trim(), "linux_amd64");
}

#[test]
fn detects_linux_arm64_from_aarch64() {
    let sb = Sandbox::new();
    let path = sb.uname_shim("Linux", "aarch64");
    let out = sb.run_env(&["--print-platform"], &[("PATH", &path)]);
    assert_success(&out);
    assert_eq!(stdout(&out).trim(), "linux_arm64");
}

#[test]
fn detects_darwin_arm64() {
    let sb = Sandbox::new();
    let path = sb.uname_shim("Darwin", "arm64");
    let out = sb.run_env(&["--print-platform"], &[("PATH", &path)]);
    assert_success(&out);
    assert_eq!(stdout(&out).trim(), "darwin_arm64");
}

#[test]
fn detects_darwin_amd64() {
    let sb = Sandbox::new();
    let path = sb.uname_shim("Darwin", "x86_64");
    let out = sb.run_env(&["--print-platform"], &[("PATH", &path)]);
    assert_success(&out);
    assert_eq!(stdout(&out).trim(), "darwin_amd64");
}

#[test]
fn unsupported_os_dies_pointing_at_from_source() {
    let sb = Sandbox::new();
    let path = sb.uname_shim("SunOS", "x86_64");
    let out = sb.run_env(&["--print-platform"], &[("PATH", &path)]);
    assert_failure(&out);
    assert!(
        stderr(&out).contains("--from-source"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn unsupported_arch_dies_pointing_at_from_source() {
    let sb = Sandbox::new();
    let path = sb.uname_shim("Linux", "mips64");
    let out = sb.run_env(&["--print-platform"], &[("PATH", &path)]);
    assert_failure(&out);
    assert!(
        stderr(&out).contains("--from-source"),
        "stderr: {}",
        stderr(&out)
    );
}

// --- install from a local artifact -------------------------------------------

#[test]
fn installs_from_local_artifact_with_checksum() {
    let sb = Sandbox::new();
    let (url, sha, pk) = make_signed_artifact(sb.tmp.path());
    let out = sb.run(&[
        "--artifact-url",
        &url,
        "--checksum",
        &sha,
        "--minisign-pubkey",
        &pk,
        "--dest",
        &dest_arg(&sb),
    ]);
    assert_success(&out);

    let bin = sb.installed_binary();
    assert!(bin.is_file(), "binary not installed at {bin:?}");
    let mode = fs::metadata(&bin).unwrap().permissions().mode();
    assert_eq!(mode & 0o111, 0o111, "binary not executable: mode {mode:o}");

    let run = Command::new(&bin).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "q2 0.0.0-test");

    // Progress goes to stderr; stdout stays clean for scripting.
    assert_eq!(stdout(&out), "", "stdout should be empty");
    assert!(
        stderr(&out).contains("checksum verified"),
        "stderr: {}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("signature verified"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn creates_dest_directory_if_missing() {
    let sb = Sandbox::new();
    let (url, sha, pk) = make_signed_artifact(sb.tmp.path());
    let deep = sb.tmp.path().join("a/b/c");
    let out = sb.run(&[
        "--artifact-url",
        &url,
        "--checksum",
        &sha,
        "--minisign-pubkey",
        &pk,
        "--dest",
        &deep.display().to_string(),
    ]);
    assert_success(&out);
    assert!(deep.join("q2").is_file());
}

#[test]
fn checksum_mismatch_fails_and_installs_nothing() {
    let sb = Sandbox::new();
    let (url, _sha) = make_artifact(sb.tmp.path());
    let wrong = "0".repeat(64);
    let out = sb.run(&[
        "--artifact-url",
        &url,
        "--checksum",
        &wrong,
        "--dest",
        &dest_arg(&sb),
    ]);
    assert_failure(&out);
    assert!(
        stderr(&out).contains("mismatch"),
        "stderr: {}",
        stderr(&out)
    );

    // Nothing installed — not the binary, not a partial file.
    if sb.dest().exists() {
        let leftovers: Vec<_> = fs::read_dir(sb.dest()).unwrap().collect();
        assert!(leftovers.is_empty(), "dest not empty: {leftovers:?}");
    }
}

#[test]
fn malformed_checksum_is_rejected() {
    let sb = Sandbox::new();
    let (url, _sha) = make_artifact(sb.tmp.path());
    let out = sb.run(&[
        "--artifact-url",
        &url,
        "--checksum",
        "not-a-sha",
        "--dest",
        &dest_arg(&sb),
    ]);
    assert_failure(&out);
    assert!(!sb.installed_binary().exists());
}

#[test]
fn missing_checksum_refuses_to_install() {
    let sb = Sandbox::new();
    let (url, _sha) = make_artifact(sb.tmp.path());
    // No --checksum and no .sha256 sidecar: fail closed.
    let out = sb.run(&["--artifact-url", &url, "--dest", &dest_arg(&sb)]);
    assert_failure(&out);
    // The escape hatch must be shown in its curl|bash position — "re-run
    // with <flag>" alone leaves piped users guessing where it goes.
    assert!(
        stderr(&out).contains("bash -s -- --insecure-skip-checksum"),
        "refusal should show the escape hatch in a full re-run line\nstderr: {}",
        stderr(&out)
    );
    assert!(!sb.installed_binary().exists());
}

#[test]
fn insecure_skip_checksum_installs_with_loud_warning() {
    let sb = Sandbox::new();
    let (url, _sha, pk) = make_signed_artifact(sb.tmp.path());
    let out = sb.run(&[
        "--artifact-url",
        &url,
        "--insecure-skip-checksum",
        "--minisign-pubkey",
        &pk,
        "--dest",
        &dest_arg(&sb),
    ]);
    assert_success(&out);
    assert!(sb.installed_binary().is_file());
    assert!(
        stderr(&out).to_lowercase().contains("unverified"),
        "warning should say the install is unverified\nstderr: {}",
        stderr(&out)
    );
}

#[test]
fn checksum_sidecar_file_is_used_automatically() {
    let sb = Sandbox::new();
    let (url, sha, pk) = make_signed_artifact(sb.tmp.path());
    let archive_path = url.strip_prefix("file://").unwrap();
    fs::write(
        format!("{archive_path}.sha256"),
        format!(
            "{sha}  {}\n",
            Path::new(archive_path)
                .file_name()
                .unwrap()
                .to_string_lossy()
        ),
    )
    .unwrap();

    let out = sb.run(&[
        "--artifact-url",
        &url,
        "--minisign-pubkey",
        &pk,
        "--dest",
        &dest_arg(&sb),
    ]);
    assert_success(&out);
    assert!(sb.installed_binary().is_file());
    assert!(
        stderr(&out).contains("checksum verified"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn install_is_idempotent() {
    let sb = Sandbox::new();
    let (url, sha, pk) = make_signed_artifact(sb.tmp.path());
    let args = [
        "--artifact-url",
        url.as_str(),
        "--checksum",
        &sha,
        "--minisign-pubkey",
        &pk,
    ];
    let dest = dest_arg(&sb);

    for _ in 0..2 {
        let mut all = args.to_vec();
        all.extend_from_slice(&["--dest", &dest]);
        assert_success(&sb.run(&all));
    }
    let run = Command::new(sb.installed_binary()).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "q2 0.0.0-test");
}

#[test]
fn archive_without_q2_binary_fails_cleanly() {
    let sb = Sandbox::new();
    // An archive containing some other file, but no `q2`. Signed, so
    // the failure exercised is extraction, not verification.
    let payload = sb.tmp.path().join("other-payload");
    fs::create_dir_all(&payload).unwrap();
    fs::write(payload.join("README"), "not a binary\n").unwrap();
    let archive = sb.tmp.path().join("q2-bogus.tar.gz");
    assert!(
        Command::new("tar")
            .args(["-czf"])
            .arg(&archive)
            .arg("-C")
            .arg(&payload)
            .arg("README")
            .status()
            .unwrap()
            .success()
    );
    let sha = sha256_hex(&fs::read(&archive).unwrap());
    let key = TestKey::generate(sb.tmp.path());
    key.sign(&archive);

    let url = format!("file://{}", archive.display());
    let out = sb.run(&[
        "--artifact-url",
        &url,
        "--checksum",
        &sha,
        "--minisign-pubkey",
        &key.pub_key,
        "--dest",
        &dest_arg(&sb),
    ]);
    assert_failure(&out);
    assert!(!sb.installed_binary().exists());
}

// --- signature verification ---------------------------------------------------

#[test]
fn missing_minisig_refuses_to_install() {
    let sb = Sandbox::new();
    let (url, sha) = make_artifact(sb.tmp.path()); // checksummed but unsigned
    let key = TestKey::generate(sb.tmp.path());
    let out = sb.run(&[
        "--artifact-url",
        &url,
        "--checksum",
        &sha,
        "--minisign-pubkey",
        &key.pub_key,
        "--dest",
        &dest_arg(&sb),
    ]);
    assert_failure(&out);
    // Full re-run line, not just the flag name (see checksum twin above).
    assert!(
        stderr(&out).contains("bash -s -- --insecure-skip-signature"),
        "refusal should show the escape hatch in a full re-run line\nstderr: {}",
        stderr(&out)
    );
    assert!(!sb.installed_binary().exists());
}

#[test]
fn insecure_skip_signature_installs_with_loud_warning() {
    let sb = Sandbox::new();
    let (url, sha) = make_artifact(sb.tmp.path()); // unsigned
    let out = sb.run(&[
        "--artifact-url",
        &url,
        "--checksum",
        &sha,
        "--insecure-skip-signature",
        "--dest",
        &dest_arg(&sb),
    ]);
    assert_success(&out);
    assert!(sb.installed_binary().is_file());
    assert!(
        stderr(&out).to_lowercase().contains("unverified"),
        "warning should say the signature went unverified\nstderr: {}",
        stderr(&out)
    );
    // Checksum verification must still have happened.
    assert!(
        stderr(&out).contains("checksum verified"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn tampered_archive_fails_signature_and_installs_nothing() {
    let sb = Sandbox::new();
    let (url, _sha, pk) = make_signed_artifact(sb.tmp.path());
    let archive = PathBuf::from(url.strip_prefix("file://").unwrap());

    // Re-create the archive with different contents, keeping the old
    // .minisig; hand the installer the *new* checksum so only the
    // signature can catch the swap (the compromised-release scenario).
    let payload = sb.tmp.path().join("evil-payload");
    fs::create_dir_all(&payload).unwrap();
    let bin = payload.join("q2");
    fs::write(&bin, "#!/bin/sh\necho \"q2 6.6.6-evil\"\n").unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        Command::new("tar")
            .args(["-czf"])
            .arg(&archive)
            .arg("-C")
            .arg(&payload)
            .arg("q2")
            .status()
            .unwrap()
            .success()
    );
    let new_sha = sha256_hex(&fs::read(&archive).unwrap());

    let out = sb.run(&[
        "--artifact-url",
        &url,
        "--checksum",
        &new_sha,
        "--minisign-pubkey",
        &pk,
        "--dest",
        &dest_arg(&sb),
    ]);
    assert_failure(&out);
    assert!(!sb.installed_binary().exists());
}

#[test]
fn wrong_public_key_fails() {
    let sb = Sandbox::new();
    let (url, sha, _pk) = make_signed_artifact(sb.tmp.path());
    let other = TestKey::generate(&sb.tmp.path().join("other-key-dir"));
    let out = sb.run(&[
        "--artifact-url",
        &url,
        "--checksum",
        &sha,
        "--minisign-pubkey",
        &other.pub_key,
        "--dest",
        &dest_arg(&sb),
    ]);
    assert_failure(&out);
    assert!(!sb.installed_binary().exists());
}

#[test]
fn trusted_comment_mismatch_fails() {
    let sb = Sandbox::new();
    // Validly signed — but as a *different* artifact name. A signature
    // replayed from another (e.g. older) release must not verify.
    let (url, sha) = make_artifact(sb.tmp.path());
    let archive = PathBuf::from(url.strip_prefix("file://").unwrap());
    let key = TestKey::generate(sb.tmp.path());
    key.sign_with_comment(&archive, "q2-0.0.0-other.tar.gz");

    let out = sb.run(&[
        "--artifact-url",
        &url,
        "--checksum",
        &sha,
        "--minisign-pubkey",
        &key.pub_key,
        "--dest",
        &dest_arg(&sb),
    ]);
    assert_failure(&out);
    assert!(
        stderr(&out).contains("trusted comment"),
        "failure should explain the comment mismatch\nstderr: {}",
        stderr(&out)
    );
    assert!(!sb.installed_binary().exists());
}

#[test]
fn missing_minisign_tool_refuses_with_install_guidance() {
    let sb = Sandbox::new();
    let (url, sha, pk) = make_signed_artifact(sb.tmp.path());
    // Q2_MINISIGN points at a nonexistent binary: equivalent to a
    // machine with no minisign, regardless of what /usr/bin holds.
    let out = sb.run_env(
        &[
            "--artifact-url",
            &url,
            "--checksum",
            &sha,
            "--minisign-pubkey",
            &pk,
            "--dest",
            &dest_arg(&sb),
        ],
        &[("Q2_MINISIGN", "/nonexistent/minisign")],
    );
    assert_failure(&out);
    let err = stderr(&out);
    assert!(
        err.contains("minisign"),
        "should name the missing tool\nstderr: {err}"
    );
    assert!(
        err.contains("install"),
        "should give install guidance\nstderr: {err}"
    );
    assert!(
        err.contains("bash -s -- --insecure-skip-signature"),
        "should show the escape hatch in a full re-run line\nstderr: {err}"
    );
    assert!(!sb.installed_binary().exists());
}

// --- dest resolution ---------------------------------------------------------

#[test]
fn q2_install_dir_env_overrides_default_dest() {
    let sb = Sandbox::new();
    let (url, sha, pk) = make_signed_artifact(sb.tmp.path());
    let env_dest = sb.tmp.path().join("env-bin");
    let out = sb.run_env(
        &[
            "--artifact-url",
            &url,
            "--checksum",
            &sha,
            "--minisign-pubkey",
            &pk,
        ],
        &[("Q2_INSTALL_DIR", &env_dest.display().to_string())],
    );
    assert_success(&out);
    assert!(env_dest.join("q2").is_file());
}

#[test]
fn dest_flag_beats_q2_install_dir_env() {
    let sb = Sandbox::new();
    let (url, sha, pk) = make_signed_artifact(sb.tmp.path());
    let env_dest = sb.tmp.path().join("env-bin");
    let out = sb.run_env(
        &[
            "--artifact-url",
            &url,
            "--checksum",
            &sha,
            "--minisign-pubkey",
            &pk,
            "--dest",
            &dest_arg(&sb),
        ],
        &[("Q2_INSTALL_DIR", &env_dest.display().to_string())],
    );
    assert_success(&out);
    assert!(sb.installed_binary().is_file());
    assert!(!env_dest.exists());
}

// --- PATH advice -------------------------------------------------------------

#[test]
fn warns_when_dest_is_not_on_path() {
    let sb = Sandbox::new();
    let (url, sha, pk) = make_signed_artifact(sb.tmp.path());
    let out = sb.run(&[
        "--artifact-url",
        &url,
        "--checksum",
        &sha,
        "--minisign-pubkey",
        &pk,
        "--dest",
        &dest_arg(&sb),
    ]);
    assert_success(&out);
    assert!(
        stderr(&out).contains("PATH"),
        "expected PATH advice\nstderr: {}",
        stderr(&out)
    );
}

#[test]
fn no_path_warning_when_dest_is_on_path() {
    let sb = Sandbox::new();
    let (url, sha, pk) = make_signed_artifact(sb.tmp.path());
    let path = format!("{}:{}", sb.dest().display(), default_sandbox_path());
    let out = sb.run_env(
        &[
            "--artifact-url",
            &url,
            "--checksum",
            &sha,
            "--minisign-pubkey",
            &pk,
            "--dest",
            &dest_arg(&sb),
        ],
        &[("PATH", &path)],
    );
    assert_success(&out);
    assert!(
        !stderr(&out).contains("PATH"),
        "unexpected PATH advice\nstderr: {}",
        stderr(&out)
    );
}

// --- quiet mode ----------------------------------------------------------------

#[test]
fn quiet_successful_install_prints_nothing() {
    let sb = Sandbox::new();
    let (url, sha, pk) = make_signed_artifact(sb.tmp.path());
    // dest on PATH so there is no legitimate warning to print.
    let path = format!("{}:{}", sb.dest().display(), default_sandbox_path());
    let out = sb.run_env(
        &[
            "--quiet",
            "--artifact-url",
            &url,
            "--checksum",
            &sha,
            "--minisign-pubkey",
            &pk,
            "--dest",
            &dest_arg(&sb),
        ],
        &[("PATH", &path)],
    );
    assert_success(&out);
    assert_eq!(stdout(&out), "");
    assert_eq!(stderr(&out), "");
    assert!(sb.installed_binary().is_file());
}

#[test]
fn quiet_still_reports_errors() {
    let sb = Sandbox::new();
    let (url, _sha) = make_artifact(sb.tmp.path());
    let wrong = "0".repeat(64);
    let out = sb.run(&[
        "--quiet",
        "--artifact-url",
        &url,
        "--checksum",
        &wrong,
        "--dest",
        &dest_arg(&sb),
    ]);
    assert_failure(&out);
    assert!(
        !stderr(&out).is_empty(),
        "errors must print even under --quiet"
    );
}

// --- uninstall -----------------------------------------------------------------

#[test]
fn uninstall_removes_the_binary() {
    let sb = Sandbox::new();
    let (url, sha, pk) = make_signed_artifact(sb.tmp.path());
    let dest = dest_arg(&sb);
    assert_success(&sb.run(&[
        "--artifact-url",
        &url,
        "--checksum",
        &sha,
        "--minisign-pubkey",
        &pk,
        "--dest",
        &dest,
    ]));
    assert!(sb.installed_binary().is_file());

    assert_success(&sb.run(&["--uninstall", "--dest", &dest]));
    assert!(!sb.installed_binary().exists());
}

#[test]
fn uninstall_when_nothing_installed_succeeds_with_notice() {
    let sb = Sandbox::new();
    let out = sb.run(&["--uninstall", "--dest", &dest_arg(&sb)]);
    assert_success(&out);
    assert!(
        !stderr(&out).is_empty(),
        "expected a nothing-to-remove notice"
    );
}

// --- script hygiene ------------------------------------------------------------

#[test]
fn shellcheck_clean_if_available() {
    let shellcheck = Command::new("shellcheck").arg("--version").output();
    if shellcheck.is_err() {
        eprintln!("shellcheck not installed; skipping");
        return;
    }
    let out = Command::new("shellcheck")
        .arg("--severity=style")
        .arg(install_sh())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "shellcheck findings:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

// --- nightly channel (bd-p4ljdp2e) -----------------------------------------------
//
// `--nightly` resolves the rolling `nightly` prerelease through the
// releases API (`GET .../releases/tags/nightly`) and picks the asset for
// the detected platform by name — the version lives in the asset name,
// not the tag. Offline here via the Q2_RELEASES_API_BASE seam: a file://
// directory laid out like the API (`releases/tags/nightly` is the JSON).
// The seam moves only WHERE the release list is read from; the checksum
// and signature checks are untouched, and `nightly_refuses_an_asset_not_
// signed_by_the_pinned_key` below is the proof.

const NIGHTLY_VERSION: &str = "0.33.0-nightly.20260919";

/// A fake releases API on disk plus the signed artifacts it points at.
struct NightlyFixture {
    /// file:// URL standing in for https://api.github.com/repos/OWNER/REPO
    api_base: String,
    /// Public half of the key that signed every artifact in the fixture.
    pub_key: String,
}

/// Build the rolling nightly release for `platforms`: one signed
/// `q2-<NIGHTLY_VERSION>-<platform>.tar.gz` each (the fake `q2` inside
/// prints its platform and, last, its version — the `--version`
/// contract), plus the `.sha256` / `.minisig` sidecars the real release
/// carries, all listed as assets in `releases/tags/nightly`.
fn make_nightly_fixture(dir: &Path, platforms: &[&str]) -> NightlyFixture {
    let key = TestKey::generate(dir);
    let assets_dir = dir.join("download").join("nightly");
    fs::create_dir_all(&assets_dir).unwrap();

    let mut assets = Vec::new();
    for platform in platforms {
        let payload = dir.join(format!("payload-{platform}"));
        fs::create_dir_all(&payload).unwrap();
        let bin = payload.join("q2");
        fs::write(
            &bin,
            format!("#!/bin/sh\necho \"q2 (quarto 2) [{platform}] {NIGHTLY_VERSION}\"\n"),
        )
        .unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();

        let name = format!("q2-{NIGHTLY_VERSION}-{platform}.tar.gz");
        let archive = assets_dir.join(&name);
        let status = Command::new("tar")
            .args(["-czf"])
            .arg(&archive)
            .arg("-C")
            .arg(&payload)
            .arg("q2")
            .status()
            .unwrap();
        assert!(status.success(), "tar failed");

        let sha = sha256_hex(&fs::read(&archive).unwrap());
        fs::write(
            assets_dir.join(format!("{name}.sha256")),
            format!("{sha}  {name}\n"),
        )
        .unwrap();
        key.sign(&archive);

        for suffix in ["", ".sha256", ".minisig"] {
            let asset = format!("{name}{suffix}");
            assets.push(format!(
                r#"{{"name":"{asset}","browser_download_url":"file://{}"}}"#,
                assets_dir.join(&asset).display()
            ));
        }
    }

    let tags_dir = dir.join("api").join("releases").join("tags");
    fs::create_dir_all(&tags_dir).unwrap();
    fs::write(
        tags_dir.join("nightly"),
        format!(
            r#"{{"tag_name":"nightly","name":"q2 nightly {NIGHTLY_VERSION}","prerelease":true,"assets":[{}]}}"#,
            assets.join(",")
        ),
    )
    .unwrap();

    NightlyFixture {
        api_base: format!("file://{}", dir.join("api").display()),
        pub_key: key.pub_key,
    }
}

/// Run the installer in nightly mode against `fx` on the shimmed platform.
fn run_nightly(sb: &Sandbox, fx: &NightlyFixture, os: &str, arch: &str, extra: &[&str]) -> Output {
    let path = sb.uname_shim(os, arch);
    sb.sleep_shim("exit 0");
    let mut args = vec!["--nightly", "--minisign-pubkey", &fx.pub_key, "--dest"];
    let dest = dest_arg(sb);
    args.push(&dest);
    args.extend_from_slice(extra);
    sb.run_env(
        &args,
        &[("PATH", &path), ("Q2_RELEASES_API_BASE", &fx.api_base)],
    )
}

#[test]
fn nightly_installs_the_platform_asset_of_the_rolling_release() {
    let sb = Sandbox::new();
    let fx = make_nightly_fixture(sb.tmp.path(), &["linux_amd64", "darwin_arm64"]);
    let out = run_nightly(&sb, &fx, "Linux", "x86_64", &[]);
    assert_success(&out);

    // Progress names the channel and the resolved version.
    let log = stderr(&out);
    assert!(
        log.contains("nightly") && log.contains(NIGHTLY_VERSION),
        "log should name the nightly version\nstderr: {log}"
    );
    assert!(log.contains("checksum verified"), "stderr: {log}");
    assert!(
        log.contains(&format!(
            "signature verified (trusted comment: q2-{NIGHTLY_VERSION}-linux_amd64.tar.gz)"
        )),
        "stderr: {log}"
    );

    // The installed binary is the platform's asset and reports the
    // nightly version as its last token (the release-workflow contract).
    let run = Command::new(sb.installed_binary()).output().unwrap();
    let text = String::from_utf8_lossy(&run.stdout);
    assert!(
        text.contains("[linux_amd64]"),
        "wrong asset installed: {text}"
    );
    assert_eq!(text.split_whitespace().last(), Some(NIGHTLY_VERSION));
}

#[test]
fn nightly_picks_the_asset_for_the_detected_platform() {
    let sb = Sandbox::new();
    let fx = make_nightly_fixture(sb.tmp.path(), &["linux_amd64", "darwin_arm64"]);
    let out = run_nightly(&sb, &fx, "Darwin", "arm64", &[]);
    assert_success(&out);
    let run = Command::new(sb.installed_binary()).output().unwrap();
    let text = String::from_utf8_lossy(&run.stdout);
    assert!(
        text.contains("[darwin_arm64]"),
        "wrong asset installed: {text}"
    );
}

#[test]
fn nightly_refuses_an_asset_not_signed_by_the_pinned_key() {
    // The threat model for the API seam (plan, Decision 6): a redirected
    // release list can pick any archive and its matching .sha256, but
    // not forge the signature. No --minisign-pubkey here, so the script
    // verifies against the REAL pinned q2 key and must refuse.
    let sb = Sandbox::new();
    let fx = make_nightly_fixture(sb.tmp.path(), &["linux_amd64"]);
    let path = sb.uname_shim("Linux", "x86_64");
    let out = sb.run_env(
        &["--nightly", "--dest", &dest_arg(&sb)],
        &[("PATH", &path), ("Q2_RELEASES_API_BASE", &fx.api_base)],
    );
    assert_failure(&out);
    assert!(
        stderr(&out).contains("signature verification FAILED"),
        "stderr: {}",
        stderr(&out)
    );
    assert!(!sb.installed_binary().exists());
}

#[test]
fn nightly_and_version_are_mutually_exclusive() {
    let sb = Sandbox::new();
    let fx = make_nightly_fixture(sb.tmp.path(), &["linux_amd64"]);
    let out = run_nightly(&sb, &fx, "Linux", "x86_64", &["--version", "v0.32.0"]);
    assert_failure(&out);
    let err = stderr(&out);
    assert!(
        err.contains("--nightly") && err.contains("--version"),
        "error should name both flags\nstderr: {err}"
    );
    assert!(!sb.installed_binary().exists());
}

#[test]
fn version_with_a_nightly_suffix_points_at_the_nightly_flag() {
    // Nightlies are replaced daily, so there is no tag to pin a nightly
    // version to; the right spelling is `--nightly`.
    let sb = Sandbox::new();
    let out = sb.run(&["--version", NIGHTLY_VERSION, "--dest", &dest_arg(&sb)]);
    assert_failure(&out);
    assert!(
        stderr(&out).contains("--nightly"),
        "stderr: {}",
        stderr(&out)
    );
    assert!(!sb.installed_binary().exists());
}

#[test]
fn nightly_without_an_asset_for_the_platform_dies_cleanly() {
    let sb = Sandbox::new();
    let fx = make_nightly_fixture(sb.tmp.path(), &["linux_amd64"]);
    let out = run_nightly(&sb, &fx, "Darwin", "arm64", &[]);
    assert_failure(&out);
    let err = stderr(&out);
    assert!(
        err.contains("darwin_arm64") && err.contains("nightly"),
        "error should name the platform and the channel\nstderr: {err}"
    );
    assert!(!sb.installed_binary().exists());
}

#[test]
fn nightly_dies_cleanly_when_no_nightly_release_exists() {
    let sb = Sandbox::new();
    let empty_api = sb.tmp.path().join("empty-api");
    fs::create_dir_all(&empty_api).unwrap();
    let path = sb.uname_shim("Linux", "x86_64");
    sb.sleep_shim("exit 0");
    let api_base = format!("file://{}", empty_api.display());
    let out = sb.run_env(
        &["--nightly", "--dest", &dest_arg(&sb)],
        &[("PATH", &path), ("Q2_RELEASES_API_BASE", &api_base)],
    );
    assert_failure(&out);
    let err = stderr(&out);
    assert!(
        err.contains("could not resolve the nightly release"),
        "stderr: {err}"
    );
    // Bounded: every attempt is used, then it gives up.
    assert!(err.contains("(5/5)"), "stderr: {err}");
    assert!(!sb.installed_binary().exists());
}

#[test]
fn nightly_retries_while_the_release_is_being_replaced() {
    // The Nightly workflow deletes last night's release and publishes a
    // new one; for a few seconds after, the API can serve a release that
    // lists no assets yet (bd-n9yh30c8). The installer must retry, not
    // die. Here the first read sees an asset-less release, and the fake
    // `sleep` between attempts "finishes publishing" it.
    let sb = Sandbox::new();
    let fx = make_nightly_fixture(sb.tmp.path(), &["linux_amd64"]);
    let tag = sb.tmp.path().join("api/releases/tags/nightly");
    let full = sb.tmp.path().join("nightly.full");
    fs::rename(&tag, &full).unwrap();
    fs::write(
        &tag,
        r#"{"tag_name":"nightly","prerelease":true,"assets":[]}"#,
    )
    .unwrap();

    let path = sb.uname_shim("Linux", "x86_64");
    sb.sleep_shim(&format!("cp '{}' '{}'", full.display(), tag.display()));
    let dest = dest_arg(&sb);
    let out = sb.run_env(
        &[
            "--nightly",
            "--minisign-pubkey",
            &fx.pub_key,
            "--dest",
            &dest,
        ],
        &[
            ("PATH", &path),
            ("Q2_RELEASES_API_BASE", &fx.api_base),
            // A token must not disturb a non-api.github.com lookup.
            ("GH_TOKEN", "not-a-real-token"),
        ],
    );
    assert_success(&out);
    let err = stderr(&out);
    assert!(
        err.contains("lists no linux_amd64 archive; retrying"),
        "stderr: {err}"
    );
    assert!(sb.installed_binary().exists());
}

#[test]
fn nightly_does_not_consult_the_stable_release_lookup() {
    // The seam must never touch the stable path: with --nightly the
    // script reads releases/tags/nightly and nothing else under the API
    // base (releases/latest is deliberately absent from the fixture).
    let sb = Sandbox::new();
    let fx = make_nightly_fixture(sb.tmp.path(), &["linux_amd64"]);
    assert!(!sb.tmp.path().join("api/releases/latest").exists());
    let out = run_nightly(&sb, &fx, "Linux", "x86_64", &[]);
    assert_success(&out);
}

// --- network (run manually / in plan Phase 4 once a release exists) -------------

#[test]
#[ignore = "needs a published GitHub release; run in plan Phase 4"]
fn resolves_latest_version_from_github() {
    let sb = Sandbox::new();
    // Plain install with no --version/--artifact-url: resolves the latest
    // release, downloads, verifies the published .sha256, installs.
    let out = sb.run(&["--dest", &dest_arg(&sb)]);
    assert_success(&out);
    let run = Command::new(sb.installed_binary())
        .arg("--version")
        .output()
        .unwrap();
    // Output shape: "quarto <workspace-version>" (quarto-util/src/version.rs).
    assert!(String::from_utf8_lossy(&run.stdout).contains("quarto"));
}

#[test]
#[ignore = "needs a published nightly release; run by hand after the first Nightly run (plan Phase 4)"]
fn resolves_nightly_from_github() {
    // The real --nightly path: resolves the rolling `nightly`
    // prerelease through api.github.com, downloads this platform's
    // archive, verifies the published .sha256 and .minisig against the
    // pinned key, installs. The installed binary must report a
    // `-nightly.` version as its last token.
    let sb = Sandbox::new();
    let out = sb.run(&["--nightly", "--dest", &dest_arg(&sb)]);
    assert_success(&out);
    let run = Command::new(sb.installed_binary())
        .arg("--version")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&run.stdout);
    let version = text.split_whitespace().last().unwrap_or("");
    assert!(
        version.contains("-nightly."),
        "expected a nightly version, got {text:?}"
    );
}
