//! Bundled quarto-hub.com OAuth defaults (bd-c6l13j79).
//!
//! Release builds of q2 compile in default values for the three
//! environment variables the TS MCP server reads
//! (`QUARTO_HUB_MCP_CLIENT_ID`, `QUARTO_HUB_MCP_CLIENT_SECRET`,
//! `QUARTO_HUB_SERVER`), so users of the canonical quarto-hub.com hub
//! need zero configuration. The values are injected into the **node
//! child's environment** at delegation time; the TS server's env-var
//! sourcing (`oauth-config.ts`, `index.ts`) is unchanged and remains
//! the single source of truth for errors and semantics.
//!
//! Sourcing rules, per variable:
//! - a user env var that is set and non-blank always wins (private hub
//!   operators are unaffected);
//! - a set-but-blank env var counts as **unset**, mirroring
//!   `readNonEmpty` in `oauth-config.ts` (there is deliberately no
//!   "force absent" escape — to target a different hub, set real
//!   values);
//! - otherwise the compiled-in default applies, when present.
//!
//! The defaults enter at *compile* time via `option_env!` on
//! `QUARTO_HUB_BUNDLED_{CLIENT_ID,CLIENT_SECRET,SERVER}` — set only by
//! the release workflow (from GitHub secrets/vars). Fresh-clone and PR
//! builds compile with no bundled values and behave exactly like
//! today: the TS server errors, naming the required env vars. (rustc
//! records `option_env!` reads in dep-info, so changing the build env
//! correctly triggers a rebuild; no build.rs plumbing is needed.)
//!
//! Note the Desktop-app OAuth `client_secret` is a *public client*
//! credential (RFC 8252): embedding it is the industry-standard
//! distribution for native apps (gcloud, gh, rclone) and is not a
//! secrecy boundary. It is still elided from all output to keep
//! GOCSPX- scanners quiet.

/// The env var the TS server reads → the compiled-in default, if any.
pub struct BundledDefault {
    pub var: &'static str,
    pub bundled: Option<&'static str>,
    /// Elide the value from every diagnostic surface.
    pub secret: bool,
}

/// The three hub-connection variables, with whatever defaults this
/// binary was built with.
pub fn bundled_defaults() -> [BundledDefault; 3] {
    [
        BundledDefault {
            var: "QUARTO_HUB_MCP_CLIENT_ID",
            bundled: option_env!("QUARTO_HUB_BUNDLED_CLIENT_ID"),
            secret: false,
        },
        BundledDefault {
            var: "QUARTO_HUB_MCP_CLIENT_SECRET",
            bundled: option_env!("QUARTO_HUB_BUNDLED_CLIENT_SECRET"),
            secret: true,
        },
        BundledDefault {
            var: "QUARTO_HUB_SERVER",
            bundled: option_env!("QUARTO_HUB_BUNDLED_SERVER"),
            secret: false,
        },
    ]
}

/// Where a variable's effective value comes from, for `--launcher-info`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// User env var, set and non-blank: nothing injected.
    Env,
    /// No usable user value; the compiled-in default will be injected.
    Bundled,
    /// No usable user value and no compiled-in default: the TS server
    /// will error with its own message.
    Absent,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Source::Env => "env",
            Source::Bundled => "bundled",
            Source::Absent => "absent",
        })
    }
}

/// `readNonEmpty` (oauth-config.ts) semantics: set and non-blank.
fn usable(value: Option<&str>) -> bool {
    matches!(value, Some(v) if !v.trim().is_empty())
}

/// Classify one variable given the user's env value.
pub fn classify(user_value: Option<&str>, bundled: Option<&str>) -> Source {
    if usable(user_value) {
        Source::Env
    } else if bundled.is_some() {
        Source::Bundled
    } else {
        Source::Absent
    }
}

/// The (var, value) pairs to inject into the node child's environment:
/// exactly the variables classified [`Source::Bundled`].
pub fn injections(
    defaults: &[BundledDefault],
    env_lookup: impl Fn(&str) -> Option<String>,
) -> Vec<(&'static str, &'static str)> {
    defaults
        .iter()
        .filter_map(|d| {
            let user = env_lookup(d.var);
            match classify(user.as_deref(), d.bundled) {
                Source::Bundled => Some((d.var, d.bundled.expect("Bundled implies Some"))),
                Source::Env | Source::Absent => None,
            }
        })
        .collect()
}

/// (var, source) for every variable, for `--launcher-info`. Values are
/// never included — sources only.
pub fn sources(
    defaults: &[BundledDefault],
    env_lookup: impl Fn(&str) -> Option<String>,
) -> Vec<(&'static str, Source)> {
    defaults
        .iter()
        .map(|d| {
            let user = env_lookup(d.var);
            (d.var, classify(user.as_deref(), d.bundled))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(var: &'static str, bundled: Option<&'static str>) -> BundledDefault {
        BundledDefault {
            var,
            bundled,
            secret: false,
        }
    }

    fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(name, _)| *name == k)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn user_env_wins_over_bundled() {
        let defaults = [fake("VAR_A", Some("bundled-a"))];
        let inj = injections(&defaults, env_of(&[("VAR_A", "user-a")]));
        assert!(inj.is_empty(), "user value set: nothing to inject");
        let src = sources(&defaults, env_of(&[("VAR_A", "user-a")]));
        assert_eq!(src, vec![("VAR_A", Source::Env)]);
    }

    #[test]
    fn unset_env_gets_bundled_value() {
        let defaults = [fake("VAR_A", Some("bundled-a"))];
        let inj = injections(&defaults, env_of(&[]));
        assert_eq!(inj, vec![("VAR_A", "bundled-a")]);
        let src = sources(&defaults, env_of(&[]));
        assert_eq!(src, vec![("VAR_A", Source::Bundled)]);
    }

    #[test]
    fn empty_env_treated_as_unset_like_read_non_empty() {
        // oauth-config.ts readNonEmpty: "" and whitespace count as unset.
        let defaults = [fake("VAR_A", Some("bundled-a"))];
        for blank in ["", "   ", "\t\n"] {
            let inj = injections(&defaults, env_of(&[("VAR_A", blank)]));
            assert_eq!(
                inj,
                vec![("VAR_A", "bundled-a")],
                "blank {blank:?} must behave like unset"
            );
        }
    }

    #[test]
    fn absent_bundled_injects_nothing() {
        // Source builds: no compiled-in defaults, no injection — the TS
        // server's own MissingOAuthConfigError surfaces unchanged.
        let defaults = [fake("VAR_A", None)];
        let inj = injections(&defaults, env_of(&[]));
        assert!(inj.is_empty());
        let src = sources(&defaults, env_of(&[]));
        assert_eq!(src, vec![("VAR_A", Source::Absent)]);
    }

    #[test]
    fn absent_bundled_with_user_env_is_env() {
        let defaults = [fake("VAR_A", None)];
        let src = sources(&defaults, env_of(&[("VAR_A", "user-a")]));
        assert_eq!(src, vec![("VAR_A", Source::Env)]);
    }

    #[test]
    fn variables_resolve_independently() {
        let defaults = [
            fake("VAR_A", Some("bundled-a")),
            fake("VAR_B", Some("bundled-b")),
            fake("VAR_C", None),
        ];
        let env = env_of(&[("VAR_A", "user-a")]);
        let inj = injections(&defaults, &env);
        assert_eq!(inj, vec![("VAR_B", "bundled-b")]);
        let src = sources(&defaults, &env);
        assert_eq!(
            src,
            vec![
                ("VAR_A", Source::Env),
                ("VAR_B", Source::Bundled),
                ("VAR_C", Source::Absent),
            ]
        );
    }

    #[test]
    fn real_defaults_cover_exactly_the_three_hub_vars() {
        let vars: Vec<&str> = bundled_defaults().iter().map(|d| d.var).collect();
        assert_eq!(
            vars,
            vec![
                "QUARTO_HUB_MCP_CLIENT_ID",
                "QUARTO_HUB_MCP_CLIENT_SECRET",
                "QUARTO_HUB_SERVER",
            ]
        );
        // Only the client secret is elision-worthy.
        let secrets: Vec<&str> = bundled_defaults()
            .iter()
            .filter(|d| d.secret)
            .map(|d| d.var)
            .collect();
        assert_eq!(secrets, vec!["QUARTO_HUB_MCP_CLIENT_SECRET"]);
    }
}
