//! The git probe (G3, G4): plumbing only, no shell, `GIT_OPTIONAL_LOCKS=0`, a timeout per call, forward-slash
//! output, and no repository-configured program. The trait split mirrors the module map: the controller needs
//! [`ControllerProbe`]; the hub also uses the repository identity queries of [`GitProbe`].
//!
//! Never run here, because each can execute a program the repository configures: `hash-object` (clean
//! filters), `status`, `diff`, and `add` (filters, `core.fsmonitor`). Every call also passes
//! `-c core.fsmonitor=false`.

use quarto_util::to_forward_slashes;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, UNIX_EPOCH};
use wait_timeout::ChildExt;

/// Git is missing, refused to run here, or this is not a repository: the controller goes `off`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProbeError {
    #[error("git unavailable: {0}")]
    Unavailable(String),
    #[error("{0}")]
    Failed(String),
}

/// A `lock` marker goes stale after 60 s; a `state` marker pauses the gate indefinitely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerKind {
    Lock,
    State,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerState {
    pub name: String,
    pub kind: MarkerKind,
    pub mtime_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReflogStat {
    pub size: u64,
    pub mtime_ms: u64,
}

/// What `cheap_state()` reads with no subprocess: marker stats and the reflog file's size and mtime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheapState {
    pub markers: Vec<MarkerState>,
    /// `None` when there is no reflog (no `logs/HEAD`, e.g. `core.logAllRefUpdates=false`). Not an error.
    pub reflog: Option<ReflogStat>,
}

/// A place in the HEAD reflog. `sha` is the new-sha of the entry; `lines` the entry count at that point. With
/// `lines` the match is exact; without it the last entry with that sha is used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflogMark {
    pub sha: String,
    pub lines: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflogSlice {
    /// Subjects of the entries after the mark, oldest first.
    pub subjects: Vec<String>,
    /// The mark for the newest entry (`None` when the reflog is empty).
    pub tail: Option<ReflogMark>,
    /// False when the mark's sha was absent from the reflog: `subjects` then holds every entry.
    pub mark_found: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedFile {
    /// Project-relative, always `/` separated.
    pub path: String,
    /// Git object id of the blob at HEAD.
    pub blob: String,
    /// `100644` or `100755`; symlinks and submodules are filtered out.
    pub mode: String,
}

/// The part of the probe the controller uses.
pub trait ControllerProbe {
    /// HEAD's commit id; `None` for an unborn repository. `Unavailable` when git cannot be used here; any other
    /// error means HEAD cannot be resolved right now.
    fn head_sha(&self) -> Result<Option<String>, ProbeError>;
    /// Reflog entries after the mark; `None` when there is no reflog.
    fn reflog_since(&self, mark: Option<&ReflogMark>) -> Result<Option<ReflogSlice>, ProbeError>;
    /// No subprocess.
    fn cheap_state(&self) -> CheapState;
    /// Tracked regular files under the project prefix, at HEAD.
    fn ls_tree(&self) -> Result<Vec<TrackedFile>, ProbeError>;
    /// Colocated jj: every changed file takes the step 5 fallback.
    fn is_colocated_jj(&self) -> bool {
        false
    }
}

pub trait GitProbe: ControllerProbe {
    /// The project path inside the repo, no trailing slash; empty at the repo root.
    fn show_prefix(&self) -> Result<String, ProbeError>;
    fn root_commits(&self) -> Result<Vec<String>, ProbeError>;
    fn is_shallow(&self) -> Result<bool, ProbeError>;
    /// Blob content by id through `cat-file --batch`; missing ids are absent from the map.
    fn cat_blobs(&self, ids: &[String]) -> Result<HashMap<String, Vec<u8>>, ProbeError>;
}

#[derive(Debug, Clone)]
pub struct ProbeOptions {
    /// The project directory (inside the repository).
    pub cwd: PathBuf,
    /// The git executable; default `git`.
    pub git_path: Option<PathBuf>,
    /// Per-call timeout; default 15 000 ms.
    pub timeout_ms: u64,
}

impl ProbeOptions {
    pub fn new(cwd: impl AsRef<Path>) -> Self {
        ProbeOptions {
            cwd: cwd.as_ref().to_path_buf(),
            git_path: None,
            timeout_ms: 15_000,
        }
    }
}

const STATE_MARKERS: [&str; 7] = [
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "rebase-merge",
    "rebase-apply",
    "sequencer",
    "BISECT_LOG",
];
const LOCK_MARKERS: [&str; 1] = ["index.lock"];
const JJ_LOCK: &str = ".jj/working_copy/working_copy.lock";

struct Resolved {
    toplevel: PathBuf,
    prefix: String,
    markers: Vec<(String, MarkerKind, PathBuf)>,
    reflog_path: PathBuf,
    jj: bool,
}

struct Run {
    code: i32,
    stdout: Vec<u8>,
    stderr: String,
}

pub struct SubprocessProbe {
    opts: ProbeOptions,
    resolved: OnceLock<Result<Resolved, ProbeError>>,
}

fn mtime_ms(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64)
}

impl SubprocessProbe {
    /// Infallible: the paths are resolved on the first call, and a failure there is cached as `Unavailable`.
    pub fn new(opts: ProbeOptions) -> Self {
        SubprocessProbe {
            opts,
            resolved: OnceLock::new(),
        }
    }

    /// Runs git with `args`, feeding `input` to stdin, killing it at the timeout. A spawn failure with
    /// not-found or permission-denied is `Unavailable`.
    fn run(&self, args: &[&str], input: Option<Vec<u8>>) -> Result<Run, ProbeError> {
        let git = self
            .opts
            .git_path
            .clone()
            .unwrap_or_else(|| PathBuf::from("git"));
        let mut cmd = Command::new(&git);
        cmd.args(["-c", "core.fsmonitor=false"])
            .args(args)
            .current_dir(&self.opts.cwd)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd.spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied => {
                ProbeError::Unavailable(format!(
                    "git not found or not runnable ({})",
                    git.display()
                ))
            }
            _ => ProbeError::Failed(format!("cannot start git: {e}")),
        })?;
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut stderr = child.stderr.take().expect("piped stderr");
        let mut stdin = child.stdin.take().expect("piped stdin");
        let writer = std::thread::spawn(move || {
            if let Some(bytes) = input {
                let _ = stdin.write_all(&bytes); // git may exit before reading all input
            }
        });
        let out_t = std::thread::spawn(move || {
            let mut v = Vec::new();
            let _ = stdout.read_to_end(&mut v);
            v
        });
        let err_t = std::thread::spawn(move || {
            let mut v = Vec::new();
            let _ = stderr.read_to_end(&mut v);
            v
        });
        let status = child
            .wait_timeout(Duration::from_millis(self.opts.timeout_ms))
            .map_err(|e| ProbeError::Failed(format!("waiting for git: {e}")))?;
        let code = match status {
            Some(s) => s.code().unwrap_or(-1),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                // Do not join the reader threads: a grandchild that inherited the pipes can hold them open
                // past the kill. They exit on their own when the pipes close.
                return Err(ProbeError::Failed(format!(
                    "git {} timed out after {} ms",
                    args.first().copied().unwrap_or(""),
                    self.opts.timeout_ms
                )));
            }
        };
        let _ = writer.join();
        Ok(Run {
            code,
            stdout: out_t.join().unwrap_or_default(),
            stderr: String::from_utf8_lossy(&err_t.join().unwrap_or_default()).into_owned(),
        })
    }

    fn ok(&self, args: &[&str], input: Option<Vec<u8>>) -> Result<Vec<u8>, ProbeError> {
        let r = self.run(args, input)?;
        if r.code != 0 {
            let lower = r.stderr.to_lowercase();
            if lower.contains("not a git repository")
                || lower.contains("dubious ownership")
                || lower.contains("safe.directory")
            {
                return Err(ProbeError::Unavailable(
                    r.stderr.lines().next().unwrap_or("").to_string(),
                ));
            }
            return Err(ProbeError::Failed(format!(
                "git {} failed ({}): {}",
                args.first().copied().unwrap_or(""),
                r.code,
                r.stderr.trim()
            )));
        }
        Ok(r.stdout)
    }

    /// Resolved once with `--git-path`, so linked worktrees and `GIT_DIR` setups find the right files.
    fn resolved(&self) -> Result<&Resolved, ProbeError> {
        self.resolved
            .get_or_init(|| {
                let names: Vec<&str> = LOCK_MARKERS
                    .iter()
                    .chain(STATE_MARKERS.iter())
                    .copied()
                    .chain(["logs/HEAD"])
                    .collect();
                let mut args = vec![
                    "rev-parse",
                    "--path-format=absolute",
                    "--show-toplevel",
                    "--show-prefix",
                ];
                for n in &names {
                    args.push("--git-path");
                    args.push(n);
                }
                let out = self.ok(&args, None).map_err(|e| match e {
                    ProbeError::Failed(m) => ProbeError::Unavailable(m),
                    other => other,
                })?;
                let text = String::from_utf8_lossy(&out).into_owned();
                let mut lines = text.split('\n');
                let toplevel = lines.next().unwrap_or("").trim_end_matches('\r');
                if toplevel.is_empty() {
                    return Err(ProbeError::Unavailable("not inside a work tree".into()));
                }
                let prefix = lines
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('\r')
                    .trim_end_matches('/')
                    .to_string();
                let paths: Vec<PathBuf> = lines
                    .take(names.len())
                    .map(|l| PathBuf::from(l.trim_end_matches('\r')))
                    .collect();
                if paths.len() != names.len() {
                    return Err(ProbeError::Unavailable(
                        "unexpected rev-parse output".into(),
                    ));
                }
                let mut markers = Vec::new();
                for (i, name) in LOCK_MARKERS.iter().enumerate() {
                    markers.push((name.to_string(), MarkerKind::Lock, paths[i].clone()));
                }
                for (i, name) in STATE_MARKERS.iter().enumerate() {
                    markers.push((
                        name.to_string(),
                        MarkerKind::State,
                        paths[LOCK_MARKERS.len() + i].clone(),
                    ));
                }
                let toplevel = PathBuf::from(toplevel);
                let jj = toplevel.join(".jj").is_dir();
                Ok(Resolved {
                    toplevel,
                    prefix,
                    markers,
                    reflog_path: paths[names.len() - 1].clone(),
                    jj,
                })
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// Resolves the paths now; the first call of any kind does it too.
    pub fn ready(&self) -> Result<(), ProbeError> {
        self.resolved().map(|_| ())
    }
}

fn stat(p: &Path) -> Option<std::fs::Metadata> {
    std::fs::metadata(p).ok()
}

/// `old new author <email> ts tz<TAB>subject`, one per line. `None` when there is no `logs/HEAD`.
fn parse_reflog(path: &Path) -> Option<Vec<(String, String)>> {
    let text = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&text);
    Some(
        text.split('\n')
            .filter(|l| !l.is_empty())
            .map(|l| {
                let (head, subject) = l.split_once('\t').unwrap_or((l, ""));
                let sha = head.split(' ').nth(1).unwrap_or("").to_string();
                (sha, subject.trim_end_matches('\r').to_string())
            })
            .collect(),
    )
}

impl ControllerProbe for SubprocessProbe {
    fn head_sha(&self) -> Result<Option<String>, ProbeError> {
        self.resolved()?;
        let r = self.run(&["rev-parse", "--verify", "-q", "HEAD"], None)?;
        if r.code == 0 {
            return Ok(Some(String::from_utf8_lossy(&r.stdout).trim().to_string()));
        }
        // Unborn: HEAD names a branch that has no commit yet. Anything else is an unresolvable HEAD.
        let sym = self.run(&["symbolic-ref", "-q", "HEAD"], None)?;
        if sym.code == 0 {
            let reference = String::from_utf8_lossy(&sym.stdout).trim().to_string();
            let exists = self.run(&["show-ref", "--verify", "-q", &reference], None)?;
            if exists.code != 0 {
                return Ok(None);
            }
        }
        let why = r.stderr.trim();
        Err(ProbeError::Failed(format!(
            "cannot resolve HEAD: {}",
            if why.is_empty() { "unknown error" } else { why }
        )))
    }

    fn reflog_since(&self, mark: Option<&ReflogMark>) -> Result<Option<ReflogSlice>, ProbeError> {
        let r = self.resolved()?;
        let Some(entries) = parse_reflog(&r.reflog_path) else {
            return Ok(None);
        };
        let tail = entries.last().map(|(sha, _)| ReflogMark {
            sha: sha.clone(),
            lines: Some(entries.len()),
        });
        let subjects = |from: usize| {
            entries[from..]
                .iter()
                .map(|(_, s)| s.clone())
                .collect::<Vec<_>>()
        };
        let Some(mark) = mark else {
            return Ok(Some(ReflogSlice {
                subjects: subjects(0),
                tail,
                mark_found: true,
            }));
        };
        if let Some(lines) = mark.lines
            && lines >= 1
            && lines <= entries.len()
            && entries[lines - 1].0 == mark.sha
        {
            return Ok(Some(ReflogSlice {
                subjects: subjects(lines),
                tail,
                mark_found: true,
            }));
        }
        if let Some(i) = entries.iter().rposition(|(sha, _)| *sha == mark.sha) {
            return Ok(Some(ReflogSlice {
                subjects: subjects(i + 1),
                tail,
                mark_found: true,
            }));
        }
        Ok(Some(ReflogSlice {
            subjects: subjects(0),
            tail,
            mark_found: false,
        }))
    }

    fn cheap_state(&self) -> CheapState {
        let Ok(r) = self.resolved() else {
            return CheapState::default();
        };
        let mut markers = Vec::new();
        for (name, kind, path) in &r.markers {
            if let Some(m) = stat(path) {
                markers.push(MarkerState {
                    name: name.clone(),
                    kind: *kind,
                    mtime_ms: mtime_ms(&m),
                });
            }
        }
        if r.jj {
            // The jj lock file may persist between operations; it is a `lock`, so it goes stale after 60 s.
            if let Some(m) = stat(&r.toplevel.join(JJ_LOCK)) {
                markers.push(MarkerState {
                    name: JJ_LOCK.into(),
                    kind: MarkerKind::Lock,
                    mtime_ms: mtime_ms(&m),
                });
            }
        }
        let reflog = stat(&r.reflog_path).map(|m| ReflogStat {
            size: m.len(),
            mtime_ms: mtime_ms(&m),
        });
        CheapState { markers, reflog }
    }

    fn ls_tree(&self) -> Result<Vec<TrackedFile>, ProbeError> {
        if self.head_sha()?.is_none() {
            return Ok(vec![]);
        }
        // Run in the project directory: paths come back relative to it and limited to it.
        let out = self.ok(&["ls-tree", "-r", "-z", "HEAD"], None)?;
        let mut files = Vec::new();
        for rec in out.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let Some(tab) = rec.iter().position(|b| *b == b'\t') else {
                continue;
            };
            let header = String::from_utf8_lossy(&rec[..tab]);
            let mut h = header.split(' ');
            let (Some(mode), Some(kind), Some(blob)) = (h.next(), h.next(), h.next()) else {
                continue;
            };
            if kind != "blob" || (mode != "100644" && mode != "100755") {
                continue;
            }
            let path = String::from_utf8_lossy(&rec[tab + 1..]).into_owned();
            files.push(TrackedFile {
                path: to_forward_slashes(Path::new(&path)),
                blob: blob.to_string(),
                mode: mode.to_string(),
            });
        }
        Ok(files)
    }

    fn is_colocated_jj(&self) -> bool {
        self.resolved().is_ok_and(|r| r.jj)
    }
}

impl GitProbe for SubprocessProbe {
    fn show_prefix(&self) -> Result<String, ProbeError> {
        Ok(self.resolved()?.prefix.clone())
    }

    fn root_commits(&self) -> Result<Vec<String>, ProbeError> {
        let out = self.ok(&["rev-list", "--max-parents=0", "HEAD"], None)?;
        Ok(String::from_utf8_lossy(&out)
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect())
    }

    fn is_shallow(&self) -> Result<bool, ProbeError> {
        let out = self.ok(&["rev-parse", "--is-shallow-repository"], None)?;
        Ok(String::from_utf8_lossy(&out).trim() == "true")
    }

    fn cat_blobs(&self, ids: &[String]) -> Result<HashMap<String, Vec<u8>>, ProbeError> {
        let mut result = HashMap::new();
        for chunk in ids.chunks(500) {
            let input = format!("{}\n", chunk.join("\n")).into_bytes();
            let r = self.run(&["cat-file", "--batch"], Some(input))?;
            if r.code != 0 {
                return Err(ProbeError::Failed(format!(
                    "git cat-file failed ({}): {}",
                    r.code,
                    r.stderr.trim()
                )));
            }
            let buf = r.stdout;
            let mut pos = 0;
            while pos < buf.len() {
                let Some(nl) = buf[pos..].iter().position(|b| *b == b'\n') else {
                    break;
                };
                let header = String::from_utf8_lossy(&buf[pos..pos + nl]).into_owned();
                pos += nl + 1;
                let mut h = header.split(' ');
                let (Some(id), Some(kind)) = (h.next(), h.next()) else {
                    continue;
                };
                if kind == "missing" {
                    continue;
                }
                let size: usize = h.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                if pos + size > buf.len() {
                    return Err(ProbeError::Failed("truncated cat-file output".into()));
                }
                result.insert(id.to_string(), buf[pos..pos + size].to_vec());
                pos += size + 1; // the content is followed by a newline
            }
        }
        Ok(result)
    }
}
