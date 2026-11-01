//! KNR-1 Phase 1D: run corpus fixtures through knitr's `execute` action, with the
//! capture on or off. Real R and knitr; a missing R is a skip.

#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use quarto_core::engine::{
    CallROptions, KNITR_RESOURCES, KnitrCapturedChunk, KnitrExecuteParams, KnitrExecuteResult,
    KnitrFormatConfig, call_r,
};

use super::knitr_corpus_support::Fixture;

/// Whether Rscript with knitr, rmarkdown and jsonlite is available (the engine's needs).
pub fn r_available() -> bool {
    Command::new("Rscript")
        .args([
            "-e",
            "for (p in c('knitr', 'rmarkdown', 'jsonlite')) if (!requireNamespace(p, quietly = TRUE)) quit(status = 1)",
        ])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// One execute run. The directory stays alive so figure files can be read.
pub struct RunOutput {
    pub markdown: String,
    pub chunks: Vec<KnitrCapturedChunk>,
    pub dir: tempfile::TempDir,
}

impl RunOutput {
    /// The directory the document ran in (figure paths are relative to it).
    pub fn doc_dir(&self) -> &Path {
        self.dir.path()
    }
}

/// Lay a fixture out in a fresh temp dir, with its siblings (child documents).
pub fn materialize(f: &Fixture) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    if let Some(sib) = &f.sibling_dir {
        for entry in std::fs::read_dir(sib).unwrap().flatten() {
            let p = entry.path();
            let skip = p.extension().is_some_and(|e| e == "R");
            if p.is_file() && !skip {
                std::fs::copy(&p, dir.path().join(p.file_name().unwrap())).unwrap();
            }
        }
    }
    let qmd = dir.path().join(format!("doc.{}", f.ext));
    std::fs::write(&qmd, &f.text).unwrap();
    (dir, qmd)
}

/// Run `f` under `format` through `execute`. `Err` carries R's failure (an aborted render).
pub fn run_fixture(f: &Fixture, format: &str, capture: bool) -> Result<RunOutput, String> {
    run_fixture_with_env(f, format, capture, &[])
}

/// As [`run_fixture`], with extra environment pairs for the Rscript child (`R_LIBS` to pin a
/// knitr version; the real environment wins over these, see `CallROptions::project_env`).
pub fn run_fixture_with_env(
    f: &Fixture,
    format: &str,
    capture: bool,
    env: &[(String, String)],
) -> Result<RunOutput, String> {
    let (dir, qmd) = materialize(f);
    let path = dir.path().to_path_buf();
    let params = KnitrExecuteParams {
        input: qmd,
        markdown: f.text.clone(),
        format: KnitrFormatConfig::with_defaults(format),
        temp_dir: path.clone(),
        lib_dir: None,
        dependencies: true,
        cwd: path.clone(),
        params: None,
        resource_dir: KNITR_RESOURCES.path().unwrap().to_path_buf(),
        handled_languages: vec![],
        capture,
    };
    let options = CallROptions {
        // `KNR_VERBOSE=1` lets R's stderr through, to see why a run aborts.
        quiet: std::env::var_os("KNR_VERBOSE").is_none(),
        project_env: env.to_vec(),
        ..Default::default()
    };
    let result: KnitrExecuteResult =
        call_r("execute", &params, &path, &path, &options).map_err(|e| format!("{e}"))?;
    Ok(RunOutput {
        markdown: result.markdown,
        chunks: result.pieces,
        dir,
    })
}

/// Run `job(i)` for `0..n` on a few threads; results come back in index order.
pub fn parallel<T: Send>(n: usize, job: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<T>>> = Mutex::new((0..n).map(|_| None).collect());
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get().min(6));
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= n {
                        break;
                    }
                    let v = job(i);
                    slots.lock().unwrap()[i] = Some(v);
                }
            });
        }
    });
    slots
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|v| v.expect("every job ran"))
        .collect()
}

/// What one run did, for comparison: a render that aborts is an outcome, not a failure of the
/// harness, and a missing R package makes the fixture unrunnable here.
pub enum Outcome {
    Ran(RunOutput),
    /// R failed; the message is compared between runs.
    Aborted(String),
    /// A package the fixture needs is not installed (`Missing package: <name>`).
    Unavailable(String),
}

impl Outcome {
    pub fn from_run(r: Result<RunOutput, String>) -> Self {
        match r {
            Ok(o) => Outcome::Ran(o),
            Err(e) => match e.strip_prefix("Missing package: ") {
                Some(pkg) => Outcome::Unavailable(pkg.trim().to_string()),
                None => Outcome::Aborted(e),
            },
        }
    }
}
