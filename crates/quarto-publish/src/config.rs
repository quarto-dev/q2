//! Resolution of `publish.<provider>.<key>` settings from
//! `_quarto.yml`, with CLI-flag override.
//!
//! Precedence (highest to lowest):
//!
//! 1. CLI flag (passed in by the caller).
//! 2. `_quarto.yml` `publish.<provider>.<key>`.
//! 3. Built-in default (provider-specific).
//!
//! The reader is best-effort and forgiving: missing keys return
//! `None`, malformed values return an explicit error
//! (`PublishError::UnableToPublish`). A future Q2-side YAML schema
//! validator (`bd-obcw`) will make malformed shapes a parse-time
//! error, but this layer needs to behave reasonably until then.

use quarto_config::ConfigValue;

use crate::common::errors::unable_to_publish;
use crate::types::PublishError;

/// Read `publish.<provider>.<key>` as a boolean, with the
/// documented precedence.
///
/// `cli_override` is the CLI-flag value (`Some(true)` /
/// `Some(false)` if the user set the flag explicitly, `None`
/// otherwise). `default` is the built-in default if neither the
/// CLI nor `_quarto.yml` provides a value.
pub fn resolve_bool(
    cli_override: Option<bool>,
    metadata: Option<&ConfigValue>,
    provider: &'static str,
    key: &str,
    default: bool,
) -> Result<bool, PublishError> {
    if let Some(v) = cli_override {
        return Ok(v);
    }
    if let Some(meta) = metadata {
        let path = ["publish", provider, key];
        if let Some(value) = meta.get_path(&path) {
            return value.as_bool().ok_or_else(|| {
                unable_to_publish(
                    provider,
                    format!(
                        "publish.{provider}.{key} in _quarto.yml must be a boolean, \
                         got a non-boolean value"
                    ),
                )
            });
        }
    }
    Ok(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_config::ConfigValue;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_source_map::SourceInfo;

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let entries = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::for_test(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(entries, SourceInfo::for_test())
    }

    fn boolean(b: bool) -> ConfigValue {
        ConfigValue::new_bool(b, SourceInfo::for_test())
    }

    fn string(s: &str) -> ConfigValue {
        ConfigValue::new_string(s.to_string(), SourceInfo::for_test())
    }

    #[test]
    fn cli_override_wins_over_yaml_and_default() {
        let meta = map(vec![(
            "publish",
            map(vec![("gh-pages", map(vec![("wait", boolean(false))]))]),
        )]);
        // CLI says true; YAML says false; default is true. Result: true.
        let v = resolve_bool(Some(true), Some(&meta), "gh-pages", "wait", true).unwrap();
        assert!(v);
        // CLI says false; YAML says true; default is true. Result: false.
        let meta = map(vec![(
            "publish",
            map(vec![("gh-pages", map(vec![("wait", boolean(true))]))]),
        )]);
        let v = resolve_bool(Some(false), Some(&meta), "gh-pages", "wait", true).unwrap();
        assert!(!v);
    }

    #[test]
    fn yaml_wins_over_default_when_no_cli_override() {
        let meta = map(vec![(
            "publish",
            map(vec![("gh-pages", map(vec![("wait", boolean(false))]))]),
        )]);
        let v = resolve_bool(None, Some(&meta), "gh-pages", "wait", true).unwrap();
        assert!(!v);
    }

    #[test]
    fn default_used_when_no_cli_and_no_yaml() {
        let v = resolve_bool(None, None, "gh-pages", "wait", true).unwrap();
        assert!(v);
        let v = resolve_bool(None, None, "gh-pages", "wait", false).unwrap();
        assert!(!v);
    }

    #[test]
    fn missing_key_in_yaml_falls_back_to_default() {
        // publish.netlify exists, but publish.gh-pages.wait does not.
        let meta = map(vec![(
            "publish",
            map(vec![("netlify", map(vec![("token", string("xyz"))]))]),
        )]);
        let v = resolve_bool(None, Some(&meta), "gh-pages", "wait", true).unwrap();
        assert!(v);
    }

    #[test]
    fn malformed_value_in_yaml_errors_with_clear_message() {
        // publish.gh-pages.wait is set to a string, not a boolean.
        let meta = map(vec![(
            "publish",
            map(vec![(
                "gh-pages",
                map(vec![("wait", string("yes please"))]),
            )]),
        )]);
        let err = resolve_bool(None, Some(&meta), "gh-pages", "wait", true).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("publish.gh-pages.wait") && msg.contains("boolean"),
            "expected error to mention the key path and the expected type, got: {msg}"
        );
    }
}
