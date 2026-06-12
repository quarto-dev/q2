//! E2E tests for install.sh, the curl|bash installer (bd-c6l13j79).
//!
//! Unix-only: the harness runs `bash install.sh` and shims `uname` with
//! executable shell scripts, so the whole suite is gated to `cfg(unix)`.
//! The Windows installer (install.ps1) is checksum-only and smoke-tested
//! by the release workflow instead.
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
        format!("{}:{}", shim.display(), SYSTEM_PATH)
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
