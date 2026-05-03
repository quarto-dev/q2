//! CLI-shaped option struct + argument validation.
//!
//! The `quarto` binary parses raw flags into a `PublishCli`, then
//! calls `validate_and_resolve` to turn it into a `PublishUx` (and
//! to surface flag-conflict errors before any side effects happen).

use crate::types::{PublishError, PublishUx};

/// Raw CLI flags as parsed by the `quarto publish` subcommand.
///
/// `Option<bool>` encodes "explicitly set vs. defaulted"; the
/// defaults are filled in by `validate_and_resolve`.
#[derive(Debug, Clone, Default)]
pub struct PublishCli {
    pub render: Option<bool>,
    pub prompt: Option<bool>,
    pub browser: Option<bool>,
    pub wait: Option<bool>,
    pub dry_run: bool,
    pub json: bool,
}

/// Result of validation: a resolved `PublishUx` plus any
/// adjustments worth reporting to the user.
#[derive(Debug, Clone)]
pub struct ValidatedCli {
    pub ux: PublishUx,
    /// Notes the user should see (one per silent downgrade).
    pub notes: Vec<String>,
}

/// Validate flag combinations and resolve defaults.
///
/// Validation rules (per the plan):
///
/// 1. `--no-wait` together with `--browser` (the default) is an
///    explicit conflict — we reject rather than open the browser
///    to a deployment that may not yet be live.
/// 2. `--json` together with `--prompt` is an explicit conflict —
///    `--json` requires non-interactive operation.
/// 3. `--dry-run` together with `--browser` is silently downgraded
///    to `--no-browser` with a one-line note (we have no URL to
///    open).
/// 4. `--json` implies `--no-prompt` (silent — `--json` and
///    interactive prompts are inherently incompatible).
pub fn validate_and_resolve(cli: PublishCli) -> Result<ValidatedCli, PublishError> {
    let mut notes = Vec::new();

    // Resolve defaults first, then apply forced changes.
    let render = cli.render.unwrap_or(true);

    // Rule 4: --json forces --no-prompt.
    let mut prompt = cli.prompt.unwrap_or(true);
    if cli.json {
        // Reject only if the user explicitly opted *into* prompts.
        if cli.prompt == Some(true) {
            return Err(unable_to_publish(
                "publish",
                "--json is incompatible with --prompt; --json requires non-interactive operation",
            ));
        }
        prompt = false;
    }

    let mut browser = cli.browser.unwrap_or(true);
    let wait = cli.wait.unwrap_or(true);

    // Rule 3 (apply *before* the no-wait/browser check): --dry-run
    // + --browser → silent downgrade. We have no URL to open from a
    // dry run.
    if cli.dry_run && browser {
        browser = false;
        notes.push("--dry-run: not opening browser (no URL to open from a dry run).".to_string());
    }

    // Rule 1: --no-wait + --browser is rejected. (Runs after the
    // dry-run downgrade so `--dry-run --no-wait` is accepted —
    // dry-run already forced --no-browser.)
    if !wait && browser {
        return Err(unable_to_publish(
            "publish",
            "--no-wait is incompatible with opening the browser. Pass --no-browser as well \
             if you really want --no-wait (you would otherwise be opening the browser to a \
             deployment that may not yet be live).",
        ));
    }

    let ux = PublishUx {
        render,
        prompt,
        browser,
        wait,
        dry_run: cli.dry_run,
        json: cli.json,
    };

    Ok(ValidatedCli { ux, notes })
}

fn unable_to_publish(provider: &'static str, message: &str) -> PublishError {
    PublishError::UnableToPublish {
        provider,
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_publish_ux_default() {
        let v = validate_and_resolve(PublishCli::default()).unwrap();
        assert!(v.ux.render);
        assert!(v.ux.prompt);
        assert!(v.ux.browser);
        assert!(v.ux.wait);
        assert!(!v.ux.dry_run);
        assert!(!v.ux.json);
        assert!(v.notes.is_empty());
    }

    #[test]
    fn no_wait_with_default_browser_is_rejected() {
        let cli = PublishCli {
            wait: Some(false),
            ..Default::default()
        };
        let err = validate_and_resolve(cli).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("--no-wait") && msg.contains("--no-browser"),
            "expected error to mention both flags, got: {msg}"
        );
    }

    #[test]
    fn no_wait_with_explicit_no_browser_is_accepted() {
        let cli = PublishCli {
            wait: Some(false),
            browser: Some(false),
            ..Default::default()
        };
        let v = validate_and_resolve(cli).unwrap();
        assert!(!v.ux.wait);
        assert!(!v.ux.browser);
    }

    #[test]
    fn json_with_explicit_prompt_is_rejected() {
        let cli = PublishCli {
            json: true,
            prompt: Some(true),
            ..Default::default()
        };
        let err = validate_and_resolve(cli).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("--json") && msg.contains("--prompt"),
            "expected error to mention both flags, got: {msg}"
        );
    }

    #[test]
    fn json_silently_implies_no_prompt() {
        let cli = PublishCli {
            json: true,
            ..Default::default()
        };
        let v = validate_and_resolve(cli).unwrap();
        assert!(v.ux.json);
        assert!(!v.ux.prompt, "json should force prompt off");
        assert!(v.notes.is_empty(), "no note for the silent downgrade");
    }

    #[test]
    fn json_explicitly_no_prompt_is_accepted() {
        let cli = PublishCli {
            json: true,
            prompt: Some(false),
            ..Default::default()
        };
        let v = validate_and_resolve(cli).unwrap();
        assert!(v.ux.json);
        assert!(!v.ux.prompt);
    }

    #[test]
    fn dry_run_with_default_browser_silently_downgrades() {
        let cli = PublishCli {
            dry_run: true,
            ..Default::default()
        };
        let v = validate_and_resolve(cli).unwrap();
        assert!(v.ux.dry_run);
        assert!(!v.ux.browser, "dry-run should turn browser off");
        assert_eq!(v.notes.len(), 1);
        assert!(v.notes[0].contains("dry-run"));
    }

    #[test]
    fn dry_run_with_explicit_no_browser_does_not_emit_note() {
        let cli = PublishCli {
            dry_run: true,
            browser: Some(false),
            ..Default::default()
        };
        let v = validate_and_resolve(cli).unwrap();
        assert!(v.ux.dry_run);
        assert!(!v.ux.browser);
        assert!(
            v.notes.is_empty(),
            "no note when the user already passed --no-browser"
        );
    }

    #[test]
    fn dry_run_with_no_wait_is_accepted() {
        // Both knobs are perfectly fine together — dry-run forces
        // --no-browser anyway, so the no-wait/browser conflict can't
        // trigger.
        let cli = PublishCli {
            dry_run: true,
            wait: Some(false),
            ..Default::default()
        };
        let v = validate_and_resolve(cli).unwrap();
        assert!(v.ux.dry_run);
        assert!(!v.ux.wait);
        assert!(!v.ux.browser);
    }

    #[test]
    fn render_default_is_true() {
        let v = validate_and_resolve(PublishCli::default()).unwrap();
        assert!(v.ux.render);
    }

    #[test]
    fn render_can_be_disabled() {
        let cli = PublishCli {
            render: Some(false),
            ..Default::default()
        };
        let v = validate_and_resolve(cli).unwrap();
        assert!(!v.ux.render);
    }
}
