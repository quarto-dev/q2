//! Per-checkout isolation for the tree-sitter CLI's compiled-grammar cache.
//!
//! `tree-sitter test` / `parse` load the grammar from a compiled library
//! cached at `$TREE_SITTER_LIBDIR/<grammar-name>.dylib` (`.so`/`.dll`),
//! defaulting to `~/.cache/tree-sitter/lib`. The cache key is only the
//! grammar *name* (`markdown`, `doctemplate`) and freshness is "library
//! newer than `src/parser.c`", so every q2 checkout on a machine shares one
//! library and whichever checkout compiled last wins — another checkout's
//! grammar then silently runs against this checkout's corpus (bd-agsgrbfn).
//!
//! Every xtask invocation of the CLI goes through [`command`], which pins
//! `TREE_SITTER_LIBDIR` to a per-checkout directory, overriding any value
//! the caller exported (a global one would reintroduce the sharing).
//! `cargo xtask ts-test` ([`run_ts_test`]) is the entry point for hand runs.

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) const LIBDIR_ENV: &str = "TREE_SITTER_LIBDIR";

/// The per-checkout compiled-grammar cache: `<project_root>/target/tree-sitter-lib`.
///
/// Deliberately the literal `<root>/target`, NOT `CARGO_TARGET_DIR`: a
/// custom target dir can be shared across checkouts/worktrees, which is
/// exactly the sharing this directory exists to prevent. `target/` is
/// already per-checkout, gitignored, and wiped by `cargo clean`.
pub(crate) fn libdir(project_root: &Path) -> PathBuf {
    project_root.join("target").join("tree-sitter-lib")
}

/// A `tree-sitter` command whose compiled-grammar cache is `libdir`.
pub(crate) fn command(libdir: &Path) -> Command {
    let mut cmd = crate::util::nested_command("tree-sitter");
    cmd.env(LIBDIR_ENV, libdir);
    cmd
}

/// The tree-sitter grammars in the workspace.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum Grammar {
    /// The unified qmd grammar (grammar name `markdown`).
    Qmd,
    /// The Pandoc-template grammar (grammar name `doctemplate`).
    Doctemplate,
}

impl Grammar {
    pub(crate) fn dir(self, project_root: &Path) -> PathBuf {
        match self {
            Grammar::Qmd => project_root
                .join("crates")
                .join("tree-sitter-qmd")
                .join("tree-sitter-markdown"),
            Grammar::Doctemplate => project_root
                .join("crates")
                .join("tree-sitter-doctemplate")
                .join("grammar"),
        }
    }
}

pub(crate) struct TsTestArgs {
    pub grammar: Grammar,
    pub rebuild: bool,
    pub test_args: Vec<String>,
}

/// `cargo xtask ts-test`: `tree-sitter generate`, `build`, and `test` in the
/// grammar directory, with the per-checkout [`libdir`].
pub(crate) fn run_ts_test(args: TsTestArgs) -> Result<()> {
    let project_root = crate::verify::find_project_root()?;
    let grammar_dir = args.grammar.dir(&project_root);
    let libdir = libdir(&project_root);
    println!("{LIBDIR_ENV}={}", libdir.display());

    let mut test_args = vec!["test".to_string()];
    if args.rebuild {
        test_args.push("--rebuild".to_string());
    }
    test_args.extend(args.test_args);

    for step in [
        vec!["generate".to_string()],
        vec!["build".to_string()],
        test_args,
    ] {
        println!("\n━━━ tree-sitter {} ━━━\n", step.join(" "));
        let status = command(&libdir)
            .args(&step)
            .current_dir(&grammar_dir)
            .status()
            .with_context(|| format!("Failed to run tree-sitter {}", step[0]))?;
        if !status.success() {
            bail!("tree-sitter {} failed", step[0]);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;

    fn env_overrides(cmd: &std::process::Command) -> HashMap<String, Option<String>> {
        cmd.get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect()
    }

    #[test]
    fn libdir_is_under_the_checkouts_literal_target_dir() {
        let root = Path::new("some").join("checkout");
        assert_eq!(libdir(&root), root.join("target").join("tree-sitter-lib"));
    }

    #[test]
    fn command_sets_tree_sitter_libdir() {
        let dir = Path::new("some").join("lib");
        let cmd = command(&dir);
        assert_eq!(cmd.get_program(), "tree-sitter");
        let env = env_overrides(&cmd);
        assert_eq!(
            env.get(LIBDIR_ENV),
            Some(&Some(dir.to_string_lossy().into_owned()))
        );
    }

    #[test]
    fn grammar_dirs_exist_in_the_repo() {
        let root = crate::verify::find_project_root().unwrap();
        for grammar in [Grammar::Qmd, Grammar::Doctemplate] {
            let dir = grammar.dir(&root);
            assert!(
                dir.join("grammar.js").is_file(),
                "{} has no grammar.js",
                dir.display()
            );
        }
    }
}
