/*
 * render_scripts.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Project pre-render / post-render user scripts (bd-w348iu63).
 */

//! Project `pre-render` / `post-render` user scripts (bd-w348iu63).
//!
//! A project can declare scripts that run before and after a project
//! render:
//!
//! ```yaml
//! project:
//!   pre-render: prepare.py        # string or list
//!   post-render:
//!     - cleanup.R
//!     - tools/notify.sh
//! ```
//!
//! Scripts run with the project root as cwd and receive the
//! `QUARTO_PROJECT_*` environment contract (see
//! [`RenderScriptsContext`]). The drivers (`q2 render`, `q2 publish`)
//! bracket project discovery and the pipeline with
//! [`run_render_scripts`]; pre-render scripts may create input files
//! (the project is discovered *after* they run) but may not change
//! `project.type` or `project.output-dir`
//! ([`check_forbidden_mutations`]).
//!
//! Config extraction ([`extract_render_scripts`]) and command-line
//! parsing ([`parse_shell_run_command`]) are target-agnostic; actual
//! execution is native-only (`#[cfg(not(target_arch = "wasm32"))]`) —
//! the WASM/hub path surfaces a diagnostic instead of running
//! anything.
//!
//! Plan: `claude-notes/plans/2026-07-29-pre-post-render-scripts.md`.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::ConfigValue;
use quarto_source_map::SourceInfo;

use super::ProjectConfig;

/// One `pre-render:` / `post-render:` entry from `_quarto.yml`: the
/// raw command line as written, plus the YAML scalar's source
/// location so diagnostics can point at the entry.
#[derive(Debug, Clone)]
pub struct RenderScript {
    /// The command line as written by the user (e.g. `"prepare.py"`,
    /// `"python3 tools/gen.py --flag"`).
    pub command: String,
    /// Source location of the YAML scalar that supplied
    /// [`command`](Self::command).
    pub source_info: SourceInfo,
}

/// Extract `project.<key>` (string or list-of-strings) into
/// [`RenderScript`]s carrying each scalar's source location. A bare
/// string normalizes to a one-element list, matching Quarto 1.
pub fn extract_render_scripts(meta: &ConfigValue, key: &str) -> Vec<RenderScript> {
    let Some(value) = meta.get("project").and_then(|p| p.get(key)) else {
        return Vec::new();
    };
    if let Some(arr) = value.as_array() {
        arr.iter()
            .filter_map(|v| {
                v.as_plain_text().map(|s| RenderScript {
                    command: s,
                    source_info: v.source_info.clone(),
                })
            })
            .collect()
    } else if let Some(s) = value.as_plain_text() {
        vec![RenderScript {
            command: s,
            source_info: value.source_info.clone(),
        }]
    } else {
        Vec::new()
    }
}

/// Parse a shell-ish command line: split on runs of whitespace, with
/// double quotes grouping words (`a "b c"` → `["a", "b c"]`). An
/// unterminated quote extends to the end of the line. Quote
/// characters themselves are removed. Port of Quarto 1's
/// `parseShellRunCommand` (`src/core/run/shell.ts`).
pub fn parse_shell_run_command(cmd_line: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut in_quotes = false;
    for ch in cmd_line.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                in_token = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if in_token {
                    tokens.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }
            c => {
                current.push(c);
                in_token = true;
            }
        }
    }
    if in_token {
        tokens.push(current);
    }
    tokens
}

/// Detect the likely-typo spellings `project.pre_render` /
/// `project.post_render` (underscores instead of hyphens) and return
/// warning diagnostics naming the correct key. Q2 has no schema
/// layer, so unknown keys are otherwise silently ignored — this
/// targeted guard catches the most probable mistake.
pub fn underscore_typo_diagnostics(config: &ProjectConfig) -> Vec<DiagnosticMessage> {
    let Some(meta) = &config.metadata else {
        return Vec::new();
    };
    let Some(project) = meta.get("project") else {
        return Vec::new();
    };
    let config_name = config
        .config_path
        .as_ref()
        .map_or_else(|| "_quarto.yml".to_string(), |p| p.display().to_string());
    ["pre_render", "post_render"]
        .iter()
        .filter(|typo| project.get(typo).is_some())
        .map(|typo| {
            let correct = typo.replace('_', "-");
            DiagnosticMessageBuilder::warning(format!(
                "Unknown project key `{typo}` — did you mean `{correct}`?"
            ))
            .with_code("Q-5-11")
            .problem(format!(
                "`project.{typo}` in {config_name} is not a recognized key and is ignored. \
                 Project render scripts are configured with `project.{correct}`."
            ))
            .build()
        })
        .collect()
}

/// Which application is hosting a WASM render (bd-pq72bplh).
///
/// The two hosts have different render-script semantics (D7 in
/// `claude-notes/plans/2026-07-29-pre-post-render-scripts.md`): the
/// hub preview runs entirely in the browser and can never execute
/// scripts, while `q2 preview`'s native server runs
/// `project.pre-render` scripts once at boot, before any page render
/// (post-render scripts don't run in the preview loop — a documented
/// deviation, as nothing consumes a materialized output dir there).
/// Host-dependent diagnostics dispatch on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderHost {
    /// The hub-client web app: browser-only, no subprocesses.
    HubClient,
    /// The `q2 preview` SPA embedded in the `q2` binary, backed by a
    /// native server that ran pre-render scripts at boot.
    NativePreview,
}

/// Q-5-12: warn that configured `project.pre-render` /
/// `project.post-render` scripts will not run — but only for the host
/// where that is actually true ([`RenderHost::HubClient`]).
/// [`RenderHost::NativePreview`] returns `None`: its native side
/// already ran the pre-render scripts at boot, so the warning would
/// be false (bd-pq72bplh).
///
/// Pure decision + message builder: no once-per-session gating here.
/// The WASM caller (`wasm-quarto-hub-client`) layers an AtomicBool
/// once-gate on top so the warning shows at most once per session.
pub fn render_scripts_unsupported_diagnostic(
    host: RenderHost,
    config: &ProjectConfig,
) -> Option<DiagnosticMessage> {
    if host != RenderHost::HubClient {
        return None;
    }
    if config.pre_render_scripts.is_empty() && config.post_render_scripts.is_empty() {
        return None;
    }
    Some(
        DiagnosticMessageBuilder::warning("Project render scripts do not run in the hub preview")
            .with_code("Q-5-12")
            .problem(
                "This project configures `project.pre-render` / `project.post-render` \
                 scripts, which cannot run in the browser. The preview renders without \
                 them; use `q2 render` on a machine with the interpreters installed to \
                 run the scripts.",
            )
            .build(),
    )
}

#[cfg(not(target_arch = "wasm32"))]
mod exec {
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::OnceLock;

    use quarto_error_reporting::DiagnosticMessageBuilder;
    use quarto_source_map::{SourceContext, SourceInfo};

    use super::RenderScript;
    use crate::error::ParseError;
    use crate::project::ProjectConfig;

    /// Which script list is being run. Controls the phase-specific
    /// environment variable (`QUARTO_PROJECT_INPUT_FILES` vs
    /// `QUARTO_PROJECT_OUTPUT_FILES`) and its file-based escape
    /// hatch.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ScriptPhase {
        PreRender,
        PostRender,
    }

    impl ScriptPhase {
        fn label(self) -> &'static str {
            match self {
                ScriptPhase::PreRender => "pre-render",
                ScriptPhase::PostRender => "post-render",
            }
        }

        fn files_var(self) -> &'static str {
            match self {
                ScriptPhase::PreRender => "QUARTO_PROJECT_INPUT_FILES",
                ScriptPhase::PostRender => "QUARTO_PROJECT_OUTPUT_FILES",
            }
        }

        /// Env-size escape hatch (Q1 issue #10828): when the user
        /// sets this variable to a path, the file list is written
        /// there instead of into the environment.
        fn use_file_var(self) -> &'static str {
            match self {
                ScriptPhase::PreRender => "QUARTO_USE_FILE_FOR_PROJECT_INPUT_FILES",
                ScriptPhase::PostRender => "QUARTO_USE_FILE_FOR_PROJECT_OUTPUT_FILES",
            }
        }
    }

    /// Shared facts the `QUARTO_PROJECT_*` environment is assembled
    /// from. Computed fresh per phase by the driver — notably the
    /// post-render env reflects the *actual* render results, fixing
    /// Q1's staleness wart.
    #[derive(Debug)]
    pub struct RenderScriptsContext<'a> {
        /// Absolute project root; also the scripts' cwd.
        pub project_dir: &'a Path,
        /// Absolute output directory (= project dir when no
        /// `output-dir` is configured).
        pub output_dir: &'a Path,
        /// Path of the `_quarto.yml` the scripts came from, used to
        /// attach source snippets to diagnostics.
        pub config_path: Option<&'a Path>,
        /// Manifest paths of the discovered extensions
        /// ([`crate::project::ProjectConfig::extension_manifest_paths`]):
        /// a script entry contributed via
        /// `contributes.metadata.project` carries a `SourceInfo`
        /// anchored in its `_extension.yml`, and diagnostics must
        /// bind that file — not `_quarto.yml` — to the resolved
        /// FileId (bd-m6wmztln).
        pub extension_manifest_paths: &'a [PathBuf],
        /// Project-profile overlay / `_quarto.yml.local` paths
        /// ([`crate::project::ProjectConfig::profile_config_paths`],
        /// bd-fu16z22k): a script entry written in an overlay carries
        /// that file's FileId, same binding discipline as manifests.
        pub profile_config_paths: &'a [PathBuf],
        /// True iff the whole project is being rendered. Exported as
        /// `QUARTO_PROJECT_RENDER_ALL=1`; the variable is *absent*
        /// otherwise (not `"0"`), matching Q1.
        pub render_all: bool,
        /// From `--quiet`: suppresses progress lines and captures
        /// script stdout (stderr is still shown on failure).
        pub quiet: bool,
        /// Number of files in the render set, for the
        /// `QUARTO_PROJECT_SCRIPT_PROGRESS` hint (`"1"` on a
        /// multi-file render when not quiet).
        pub file_count: usize,
        /// Project `_environment` pairs to set on script children,
        /// pre-filtered to keys the real environment does not define
        /// ([`crate::project::environment::env_for_subprocess`]).
        /// Applied before the `QUARTO_PROJECT_*` variables, so those
        /// win any collision.
        pub project_env: &'a [(String, String)],
        /// Normalized active project-profile list for the child's
        /// `QUARTO_PROFILE`
        /// ([`crate::project::project_profile::quarto_profile_env_value`],
        /// bd-fu16z22k). Applied unconditionally — overrides an
        /// inherited `QUARTO_PROFILE`, unlike `project_env` pairs.
        pub quarto_profile: Option<String>,
    }

    impl RenderScriptsContext<'_> {
        /// The environment variables shared by both phases.
        fn shared_env(&self) -> Vec<(&'static str, String)> {
            let mut env: Vec<(&'static str, String)> = vec![
                ("QUARTO_PROJECT_DIR", self.project_dir.display().to_string()),
                (
                    "QUARTO_PROJECT_OUTPUT_DIR",
                    self.output_dir.display().to_string(),
                ),
                (
                    "QUARTO_PROJECT_SCRIPT_PROGRESS",
                    if self.file_count > 1 && !self.quiet {
                        "1"
                    } else {
                        "0"
                    }
                    .to_string(),
                ),
                (
                    "QUARTO_PROJECT_SCRIPT_QUIET",
                    if self.quiet { "1" } else { "0" }.to_string(),
                ),
            ];
            if self.render_all {
                env.push(("QUARTO_PROJECT_RENDER_ALL", "1".to_string()));
            }
            if let Some(quarto_profile) = &self.quarto_profile {
                env.push(("QUARTO_PROFILE", quarto_profile.clone()));
            }
            env
        }
    }

    /// Run one phase's scripts in declaration order, stopping at the
    /// first failure (Q1-compatible). `files` is the phase-specific
    /// list — input files about to render (pre) or produced output
    /// files (post) — as paths relative to the project dir.
    pub fn run_render_scripts(
        phase: ScriptPhase,
        scripts: &[RenderScript],
        ctx: &RenderScriptsContext,
        files: &[PathBuf],
    ) -> Result<(), ParseError> {
        if scripts.is_empty() {
            return Ok(());
        }

        let mut env = ctx.shared_env();
        let files_joined = files
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        if let Ok(list_path) = std::env::var(phase.use_file_var()) {
            std::fs::write(&list_path, &files_joined).map_err(|e| {
                script_error(
                    ctx,
                    None,
                    "Q-5-10",
                    format!("Failed to run {} scripts", phase.label()),
                    format!(
                        "Could not write the {} list to `{list_path}` \
                         (from `{}`): {e}",
                        phase.files_var(),
                        phase.use_file_var()
                    ),
                )
            })?;
        } else {
            env.push((phase.files_var(), files_joined));
        }

        for script in scripts {
            run_one_script(phase, script, ctx, &env)?;
        }
        Ok(())
    }

    fn run_one_script(
        phase: ScriptPhase,
        script: &RenderScript,
        ctx: &RenderScriptsContext,
        env: &[(&'static str, String)],
    ) -> Result<(), ParseError> {
        let tokens = super::parse_shell_run_command(&script.command);
        if tokens.is_empty() {
            return Err(script_error(
                ctx,
                Some(&script.source_info),
                "Q-5-10",
                format!("Empty {} script entry", phase.label()),
                format!(
                    "The `{}` entry is empty — expected a script path or command line.",
                    phase.label()
                ),
            ));
        }

        quarto_util::user_status!(
            ctx.quiet,
            "Running {} script: {}",
            phase.label(),
            script.command
        );

        let mut cmd = build_script_command(ctx.project_dir, &tokens);
        cmd.current_dir(ctx.project_dir);
        for (k, v) in ctx.project_env {
            cmd.env(k, v);
        }
        for (k, v) in env {
            cmd.env(k, v);
        }

        // Under --quiet, capture output (script stdout is
        // suppressed; captured stderr is replayed on failure so
        // errors are never swallowed). Otherwise inherit both.
        let status = if ctx.quiet {
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::piped());
            match cmd.spawn().and_then(|child| child.wait_with_output()) {
                Ok(output) => {
                    if !output.status.success() {
                        eprint!("{}", String::from_utf8_lossy(&output.stderr));
                    }
                    Ok(output.status)
                }
                Err(e) => Err(e),
            }
        } else {
            cmd.status()
        }
        .map_err(|e| {
            script_error(
                ctx,
                Some(&script.source_info),
                "Q-5-10",
                format!("Could not launch {} script", phase.label()),
                format!(
                    "Failed to launch `{}`: {e}. Check that the interpreter is on PATH \
                     (or set QUARTO_PYTHON / QUARTO_R / QUARTO_NODE), and that a \
                     directly-executed script has a shebang line and the executable bit.",
                    script.command
                ),
            )
        })?;

        if !status.success() {
            let exit_desc = match status.code() {
                Some(code) => format!("exited with status {code}"),
                None => "was terminated by a signal".to_string(),
            };
            return Err(script_error(
                ctx,
                Some(&script.source_info),
                "Q-5-8",
                format!("{} script failed", capitalize(phase.label())),
                format!(
                    "Script `{}` {exit_desc}. The script's own output appears above.",
                    script.command
                ),
            ));
        }
        Ok(())
    }

    /// Build the [`Command`] for a parsed script command line.
    ///
    /// Dispatch is by extension of the first token (Q1-compatible,
    /// minus Deno and the `.lua` pandoc-filter special case — see the
    /// plan's D3):
    /// - `.py` → `QUARTO_PYTHON`, else `python3`/`python` on PATH
    /// - `.r` → `QUARTO_R` (via the knitr discovery), else `Rscript`
    /// - `.ts`/`.js` → `QUARTO_NODE`, else `node`
    /// - anything else → direct exec, no shell (a `.sh` needs a
    ///   shebang and the executable bit; batch files work on Windows
    ///   because `Command` routes them through `cmd.exe`)
    ///
    /// The first token is resolved to an absolute path when it names
    /// an existing file under the project dir — the scripts' cwd is
    /// the project root, but `Command::new("foo.sh")` would otherwise
    /// hit PATH lookup, which does not include the cwd.
    fn build_script_command(project_dir: &Path, tokens: &[String]) -> Command {
        let first = &tokens[0];
        let candidate = project_dir.join(first);
        let program: PathBuf = if candidate.is_file() {
            candidate
        } else {
            PathBuf::from(first)
        };

        let ext = program
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        let interpreter: Option<String> = match ext.as_deref() {
            Some("py") => Some(find_python().to_string()),
            Some("r") => Some(find_rscript_program()),
            Some("ts" | "js") => {
                Some(std::env::var("QUARTO_NODE").unwrap_or_else(|_| "node".to_string()))
            }
            _ => None,
        };

        let mut cmd = match interpreter {
            Some(interp) => {
                let mut c = Command::new(interp);
                c.arg(&program);
                c
            }
            None => Command::new(&program),
        };
        cmd.args(&tokens[1..]);
        cmd
    }

    /// Python discovery: `QUARTO_PYTHON` override, else the first of
    /// `python3`/`python` (`python`/`python3` on Windows) that
    /// answers `--version`. Cached for the process lifetime.
    fn find_python() -> &'static str {
        static PYTHON: OnceLock<String> = OnceLock::new();
        PYTHON.get_or_init(|| {
            if let Ok(p) = std::env::var("QUARTO_PYTHON")
                && !p.is_empty()
            {
                return p;
            }
            let candidates: &[&str] = if cfg!(windows) {
                &["python", "python3"]
            } else {
                &["python3", "python"]
            };
            for candidate in candidates {
                if let Ok(status) = Command::new(candidate)
                    .arg("--version")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    && status.success()
                {
                    return candidate.to_string();
                }
            }
            "python3".to_string()
        })
    }

    /// Rscript discovery: reuse the knitr engine's `find_rscript`
    /// (which honors `QUARTO_R` as a binary path, an R home, or a bin
    /// directory), falling back to plain `Rscript` on PATH.
    fn find_rscript_program() -> String {
        crate::engine::find_rscript()
            .map_or_else(|| "Rscript".to_string(), |p| p.display().to_string())
    }

    fn capitalize(s: &str) -> String {
        let mut chars = s.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => String::new(),
        }
    }

    /// Assemble a [`ParseError`] for a script problem, attaching the
    /// snippet of the config file the offending entry was *written
    /// in* — the project's `_quarto.yml` or a contributing
    /// extension's `_extension.yml` — chosen by FileId match via
    /// [`crate::config_sources::bind_config_source`] (bd-m6wmztln).
    /// When the entry comes from an extension manifest, an info line
    /// says so; when no candidate matches, the diagnostic degrades to
    /// a span-less render.
    fn script_error(
        ctx: &RenderScriptsContext,
        source_info: Option<&SourceInfo>,
        code: &str,
        title: String,
        problem: String,
    ) -> ParseError {
        let mut source_context = SourceContext::new();
        let mut builder = DiagnosticMessageBuilder::error(title)
            .with_code(code)
            .problem(problem);
        if let Some(info) = source_info {
            let candidates = ctx
                .config_path
                .into_iter()
                .chain(ctx.profile_config_paths.iter().map(PathBuf::as_path))
                .chain(ctx.extension_manifest_paths.iter().map(PathBuf::as_path));
            let matched =
                crate::config_sources::bind_config_source(&mut source_context, info, candidates);
            if let Some(path) = matched
                && ctx.config_path != Some(path)
            {
                builder = builder.add_info(format!(
                    "This entry is contributed by the extension manifest `{}` \
                     (`contributes.metadata.project`), not by your project \
                     configuration file.",
                    path.display()
                ));
            }
            builder = builder.with_location(info.clone());
        }
        ParseError::new(vec![builder.build()], source_context)
    }

    /// Q1-compatible mutation guard: a pre-render script may not
    /// change `project.type` or `project.output-dir` — the scripts
    /// already received `QUARTO_PROJECT_OUTPUT_DIR`, so a change
    /// would hand them a stale value. Compares the pre-script parse
    /// with the post-script re-parse; violation aborts the render.
    pub fn check_forbidden_mutations(
        before: &ProjectConfig,
        after: &ProjectConfig,
    ) -> Result<(), ParseError> {
        let mut violations: Vec<String> = Vec::new();
        if before.project_kind != after.project_kind {
            violations.push(format!(
                "`project.type` changed from `{}` to `{}`",
                before.project_kind.as_str(),
                after.project_kind.as_str()
            ));
        }
        if before.output_dir != after.output_dir {
            let show = |d: &Option<std::path::PathBuf>| {
                d.as_ref()
                    .map_or_else(|| "(unset)".to_string(), |p| format!("`{}`", p.display()))
            };
            violations.push(format!(
                "`project.output-dir` changed from {} to {}",
                show(&before.output_dir),
                show(&after.output_dir)
            ));
        }
        if violations.is_empty() {
            return Ok(());
        }
        let config_name = after
            .config_path
            .as_ref()
            .map_or_else(|| "_quarto.yml".to_string(), |p| p.display().to_string());
        let diagnostic = DiagnosticMessageBuilder::error(
            "Pre-render script changed a forbidden project setting",
        )
        .with_code("Q-5-9")
        .problem(format!(
            "While pre-render scripts ran, {config_name} changed: {}. \
             Pre-render scripts may modify the project (add inputs, edit config), \
             but `project.type` and `project.output-dir` must stay fixed — \
             the scripts already received QUARTO_PROJECT_OUTPUT_DIR based on them.",
            violations.join("; ")
        ))
        .build();
        Err(ParseError::new(vec![diagnostic], SourceContext::new()))
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use exec::{RenderScriptsContext, ScriptPhase, check_forbidden_mutations, run_render_scripts};

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_shell_run_command ─────────────────────────────────────

    #[test]
    fn parse_single_token() {
        assert_eq!(parse_shell_run_command("prepare.py"), vec!["prepare.py"]);
    }

    #[test]
    fn parse_multiple_tokens_collapse_spaces() {
        assert_eq!(
            parse_shell_run_command("python3  tools/gen.py   --flag"),
            vec!["python3", "tools/gen.py", "--flag"]
        );
    }

    #[test]
    fn parse_double_quotes_group_words() {
        assert_eq!(
            parse_shell_run_command(r#"run.py --msg "two words" tail"#),
            vec!["run.py", "--msg", "two words", "tail"]
        );
    }

    #[test]
    fn parse_unterminated_quote_extends_to_end() {
        assert_eq!(
            parse_shell_run_command(r#"run.py "a b c"#),
            vec!["run.py", "a b c"]
        );
    }

    #[test]
    fn parse_empty_and_whitespace_only() {
        assert!(parse_shell_run_command("").is_empty());
        assert!(parse_shell_run_command("   ").is_empty());
    }

    #[test]
    fn parse_adjacent_quotes_join_token() {
        // Q1 parity: quotes glue onto the surrounding token.
        assert_eq!(parse_shell_run_command(r#"--opt="a b""#), vec!["--opt=a b"]);
    }

    // ── extract_render_scripts ──────────────────────────────────────

    fn config_value_from_yaml(yaml: &str) -> ConfigValue {
        use pampa::pandoc::yaml_to_config_value;
        use pampa::utils::diagnostic_collector::DiagnosticCollector;
        use quarto_config::InterpretationContext;
        let parsed = quarto_yaml::parse_file(yaml, "_quarto.yml").expect("valid yaml");
        let mut diagnostics = DiagnosticCollector::new();
        yaml_to_config_value(
            parsed,
            InterpretationContext::ProjectConfig,
            &mut diagnostics,
        )
    }

    #[test]
    fn extract_string_form_normalizes_to_one_element() {
        let meta = config_value_from_yaml("project:\n  pre-render: prepare.py\n");
        let scripts = extract_render_scripts(&meta, "pre-render");
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].command, "prepare.py");
    }

    #[test]
    fn extract_list_form_preserves_order() {
        let meta =
            config_value_from_yaml("project:\n  post-render:\n    - cleanup.R\n    - notify.sh\n");
        let scripts = extract_render_scripts(&meta, "post-render");
        assert_eq!(
            scripts
                .iter()
                .map(|s| s.command.as_str())
                .collect::<Vec<_>>(),
            vec!["cleanup.R", "notify.sh"]
        );
    }

    #[test]
    fn extract_absent_key_is_empty() {
        let meta = config_value_from_yaml("project:\n  type: website\n");
        assert!(extract_render_scripts(&meta, "pre-render").is_empty());
        assert!(extract_render_scripts(&meta, "post-render").is_empty());
    }

    #[test]
    fn extract_carries_source_info() {
        let meta = config_value_from_yaml("project:\n  pre-render: prepare.py\n");
        let scripts = extract_render_scripts(&meta, "pre-render");
        assert!(
            scripts[0].source_info.resolve_byte_range().is_some(),
            "the YAML scalar's source location must be preserved"
        );
    }

    // ── underscore_typo_diagnostics ─────────────────────────────────

    #[test]
    fn typo_guard_flags_underscore_spellings() {
        let meta = config_value_from_yaml("project:\n  pre_render: x.py\n  post_render: y.py\n");
        let config = ProjectConfig {
            metadata: Some(meta),
            ..Default::default()
        };
        let diags = underscore_typo_diagnostics(&config);
        assert_eq!(diags.len(), 2);
        let rendered: Vec<String> = diags.iter().map(|d| d.to_text(None)).collect();
        assert!(rendered[0].contains("pre_render") && rendered[0].contains("pre-render"));
        assert!(rendered[1].contains("post_render") && rendered[1].contains("post-render"));
    }

    #[test]
    fn typo_guard_silent_on_correct_spelling() {
        let meta = config_value_from_yaml("project:\n  pre-render: x.py\n");
        let config = ProjectConfig {
            metadata: Some(meta),
            ..Default::default()
        };
        assert!(underscore_typo_diagnostics(&config).is_empty());
    }

    // ── mutation guard ──────────────────────────────────────────────

    #[cfg(not(target_arch = "wasm32"))]
    mod mutation_guard {
        use crate::project::{ProjectConfig, ProjectKind};
        use std::path::PathBuf;

        use super::super::check_forbidden_mutations;

        fn config(kind: ProjectKind, output_dir: Option<&str>) -> ProjectConfig {
            ProjectConfig {
                project_kind: kind,
                output_dir: output_dir.map(PathBuf::from),
                ..Default::default()
            }
        }

        #[test]
        fn unchanged_config_passes() {
            let before = config(ProjectKind::Website, Some("_site"));
            let after = config(ProjectKind::Website, Some("_site"));
            assert!(check_forbidden_mutations(&before, &after).is_ok());
        }

        #[test]
        fn type_change_is_forbidden() {
            let before = config(ProjectKind::Website, Some("_site"));
            let after = config(ProjectKind::Default, Some("_site"));
            let err = check_forbidden_mutations(&before, &after).unwrap_err();
            let text = format!("{err}");
            assert!(text.contains("project.type"), "got: {text}");
        }

        #[test]
        fn output_dir_change_is_forbidden() {
            let before = config(ProjectKind::Website, Some("_site"));
            let after = config(ProjectKind::Website, Some("_other"));
            let err = check_forbidden_mutations(&before, &after).unwrap_err();
            let text = format!("{err}");
            assert!(text.contains("output-dir"), "got: {text}");
        }

        #[test]
        fn other_changes_are_allowed() {
            let before = config(ProjectKind::Website, Some("_site"));
            let mut after = config(ProjectKind::Website, Some("_site"));
            after.render_patterns = vec![crate::glob::RawGlob::new(
                "*.qmd",
                quarto_source_map::SourceInfo::generated(
                    quarto_source_map::By::programmatic_config(),
                ),
            )];
            assert!(check_forbidden_mutations(&before, &after).is_ok());
        }
    }

    // ── host-dependent Q-5-12 diagnostic (bd-pq72bplh) ──────────────

    mod unsupported_diagnostic {
        use super::super::{RenderHost, RenderScript, render_scripts_unsupported_diagnostic};
        use crate::project::ProjectConfig;

        fn script(cmd: &str) -> RenderScript {
            RenderScript {
                command: cmd.to_string(),
                source_info: quarto_source_map::SourceInfo::generated(
                    quarto_source_map::By::programmatic_config(),
                ),
            }
        }

        fn config_with_scripts(pre: &[&str], post: &[&str]) -> ProjectConfig {
            ProjectConfig {
                pre_render_scripts: pre.iter().map(|c| script(c)).collect(),
                post_render_scripts: post.iter().map(|c| script(c)).collect(),
                ..Default::default()
            }
        }

        #[test]
        fn hub_client_with_pre_render_scripts_warns() {
            let config = config_with_scripts(&["prepare.py"], &[]);
            let diag = render_scripts_unsupported_diagnostic(RenderHost::HubClient, &config)
                .expect("hub preview must warn: the browser cannot run the scripts");
            let text = diag.to_text(None);
            assert!(text.contains("[Q-5-12]"), "got: {text}");
            assert!(text.contains("hub preview"), "got: {text}");
        }

        #[test]
        fn hub_client_with_post_render_scripts_only_warns() {
            let config = config_with_scripts(&[], &["cleanup.R"]);
            assert!(
                render_scripts_unsupported_diagnostic(RenderHost::HubClient, &config).is_some(),
                "post-render-only projects must still warn in the hub preview"
            );
        }

        #[test]
        fn native_preview_with_scripts_is_silent() {
            // `q2 preview`'s native host runs pre-render scripts at
            // boot (D7, 2026-07-29 plan) — the warning would be false.
            for config in [
                config_with_scripts(&["prepare.py"], &[]),
                config_with_scripts(&[], &["cleanup.R"]),
                config_with_scripts(&["prepare.py"], &["cleanup.R"]),
            ] {
                assert!(
                    render_scripts_unsupported_diagnostic(RenderHost::NativePreview, &config)
                        .is_none(),
                    "q2 preview must not warn: its native host runs the scripts"
                );
            }
        }

        #[test]
        fn no_scripts_is_silent_for_both_hosts() {
            let config = config_with_scripts(&[], &[]);
            for host in [RenderHost::HubClient, RenderHost::NativePreview] {
                assert!(render_scripts_unsupported_diagnostic(host, &config).is_none());
            }
        }
    }

    // ── project env propagation (bd-environment-files-372u9qbs) ─────

    /// A pre-render script sees `_environment`-derived pairs passed
    /// via `RenderScriptsContext::project_env`. Unix-only: the script
    /// runs through `sh`; Windows coverage is the shared
    /// `env_for_subprocess` unit tests plus the mechanical
    /// `cmd.env` application.
    #[cfg(unix)]
    #[test]
    fn scripts_receive_project_env() {
        use quarto_source_map::By;

        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path();
        let out_path = project_dir.join("env-out.txt");

        let script = RenderScript {
            command: format!(
                "sh -c \"printf %s $Q2_TEST_SCRIPT_ENV_VAR > {}\"",
                out_path.display()
            ),
            source_info: SourceInfo::generated(By::unknown()),
        };
        let project_env = vec![(
            "Q2_TEST_SCRIPT_ENV_VAR".to_string(),
            "from-env-file".to_string(),
        )];
        let ctx = RenderScriptsContext {
            project_dir,
            output_dir: project_dir,
            config_path: None,
            extension_manifest_paths: &[],
            profile_config_paths: &[],
            quarto_profile: None,
            render_all: true,
            quiet: true,
            file_count: 1,
            project_env: &project_env,
        };
        run_render_scripts(ScriptPhase::PreRender, &[script], &ctx, &[])
            .expect("script should succeed");
        assert_eq!(
            std::fs::read_to_string(&out_path).expect("script wrote the file"),
            "from-env-file"
        );
    }

    /// A script sees the normalized `QUARTO_PROFILE` from
    /// `RenderScriptsContext::quarto_profile` (bd-fu16z22k) — applied
    /// via `cmd.env`, so it overrides anything inherited.
    #[cfg(unix)]
    #[test]
    fn scripts_receive_quarto_profile() {
        use quarto_source_map::By;

        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir.path();
        let out_path = project_dir.join("profile-out.txt");

        let script = RenderScript {
            command: format!(
                "sh -c \"printf %s $QUARTO_PROFILE > {}\"",
                out_path.display()
            ),
            source_info: SourceInfo::generated(By::unknown()),
        };
        let ctx = RenderScriptsContext {
            project_dir,
            output_dir: project_dir,
            config_path: None,
            extension_manifest_paths: &[],
            profile_config_paths: &[],
            quarto_profile: Some("advanced,production".to_string()),
            render_all: true,
            quiet: true,
            file_count: 1,
            project_env: &[],
        };
        run_render_scripts(ScriptPhase::PreRender, &[script], &ctx, &[])
            .expect("script should succeed");
        assert_eq!(
            std::fs::read_to_string(&out_path).expect("script wrote the file"),
            "advanced,production"
        );
    }

    // ── error catalog registration ──────────────────────────────────

    #[test]
    fn render_script_error_codes_are_registered_in_catalog() {
        for code in ["Q-5-8", "Q-5-9", "Q-5-10", "Q-5-11", "Q-5-12"] {
            assert!(
                quarto_error_catalog::ERROR_CATALOG.get(code).is_some(),
                "{code} must be registered in the quarto-error-catalog"
            );
        }
    }
}
