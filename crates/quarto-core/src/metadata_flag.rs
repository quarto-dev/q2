//! `-M/--metadata KEY:VALUE` parsing, shared by every command that takes the
//! flag (`q2 render`, and later `q2 preview` and `q2 provide-hub`).
//!
//! Quarto 1's rule (`flags.ts` `parseMetadataFlagValue`): split at the first
//! `=` or `:`; the value is parsed as YAML. Differences and limits:
//!
//! - A value that does not parse as YAML is kept as a string. A flow map or
//!   list typed without spaces after the colons (`{html:{toc:true}}`) is
//!   retried with them added.
//! - An argument with no separator is an error (Quarto 1 ignores it).
//! - Repeated keys: the later wins; nothing is merged.
//! - Dotted keys are not expanded (`a.b:1` sets the key `a.b`).
//! - The result becomes the runtime metadata layer, which is built from JSON:
//!   YAML tags are lost and path-valued keys (`css:foo.css`) are not rebased
//!   relative to the document.

use serde_json::{Map, Value};

use crate::overlay_runtime::RenderOverlay;

/// Parse repeated `-M` arguments into one JSON object (`None` when there are
/// none). Errors name the offending argument.
pub fn parse_metadata_flags(args: &[String]) -> Result<Option<Value>, String> {
    if args.is_empty() {
        return Ok(None);
    }
    let mut map = Map::new();
    for arg in args {
        let (key, value) = parse_metadata_flag(arg)?;
        map.insert(key, value);
    }
    Ok(Some(Value::Object(map)))
}

/// Parse one `KEY<sep>VALUE` argument.
pub fn parse_metadata_flag(arg: &str) -> Result<(String, Value), String> {
    let Some(at) = arg.find([':', '=']) else {
        return Err(format!(
            "invalid metadata argument '{arg}': expected KEY:VALUE (or KEY=VALUE)"
        ));
    };
    let key = &arg[..at];
    if key.is_empty() {
        return Err(format!(
            "invalid metadata argument '{arg}': the key before the separator is empty"
        ));
    }
    // The separator is one ASCII byte.
    let raw = &arg[at + 1..];
    Ok((key.to_string(), parse_value(raw)))
}

fn parse_value(raw: &str) -> Value {
    if raw.trim().is_empty() {
        return Value::String(String::new());
    }
    if let Ok(v) = serde_yaml::from_str::<Value>(raw) {
        return v;
    }
    // Strict YAML rejects `{html:{toc:true}}` (no space after the colon);
    // a shell user types it that way. Retry flow collections with the space
    // added, so only values that would otherwise be strings change meaning.
    if raw.starts_with(['{', '['])
        && let Ok(v) = serde_yaml::from_str::<Value>(&space_flow_colons(raw))
    {
        return v;
    }
    Value::String(raw.to_string())
}

/// Insert a space after each `:` that is outside quotes and not already
/// followed by whitespace.
fn space_flow_colons(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 8);
    let mut quote: Option<char> = None;
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == ':' && chars.peek().is_some_and(|n| !n.is_whitespace()) => {
                out.push(' ');
            }
            None => {}
        }
    }
    out
}

/// The overlay for a set of `-M` arguments (`None` when there are none).
pub fn overlay_from_metadata_flags(
    args: &[String],
) -> Result<Option<std::sync::Arc<RenderOverlay>>, String> {
    Ok(parse_metadata_flags(args)?.map(|m| std::sync::Arc::new(RenderOverlay::with_metadata(m))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(args: &[&str]) -> Result<Option<Value>, String> {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse_metadata_flags(&owned)
    }

    #[test]
    fn no_arguments_is_none() {
        assert_eq!(parse(&[]).unwrap(), None);
    }

    #[test]
    fn scalars_are_typed_by_yaml() {
        let v = parse(&[
            "toc:true",
            "n:3",
            "ratio:1.5",
            "title:Hello",
            "nothing:null",
        ])
        .unwrap()
        .unwrap();
        assert_eq!(
            v,
            json!({"toc": true, "n": 3, "ratio": 1.5, "title": "Hello", "nothing": null})
        );
    }

    #[test]
    fn both_separators_split_at_the_first_one() {
        assert_eq!(
            parse(&["title=Hello"]).unwrap().unwrap(),
            json!({"title": "Hello"})
        );
        // The first separator wins; later ones belong to the value.
        assert_eq!(
            parse(&["title=a: b"]).unwrap().unwrap(),
            json!({"title": {"a": "b"}})
        );
        assert_eq!(
            parse(&["date:2026-10-15"]).unwrap().unwrap(),
            json!({"date": "2026-10-15"})
        );
        assert_eq!(
            parse(&["url:http://x.y/z"]).unwrap().unwrap(),
            json!({"url": "http://x.y/z"})
        );
    }

    #[test]
    fn flow_maps_and_lists_parse() {
        let v = parse(&["format:{html:{toc:true}}", "tags:[a, b]"])
            .unwrap()
            .unwrap();
        assert_eq!(
            v,
            json!({"format": {"html": {"toc": true}}, "tags": ["a", "b"]})
        );
    }

    #[test]
    fn unparseable_values_stay_strings() {
        let v = parse(&["x:{unclosed"]).unwrap().unwrap();
        assert_eq!(v, json!({"x": "{unclosed"}));
    }

    #[test]
    fn empty_value_is_an_empty_string() {
        assert_eq!(parse(&["title:"]).unwrap().unwrap(), json!({"title": ""}));
    }

    #[test]
    fn repeated_keys_later_wins_without_merging() {
        let v = parse(&["format:typst", "format:{typst:{toc:true}}"])
            .unwrap()
            .unwrap();
        assert_eq!(v, json!({"format": {"typst": {"toc": true}}}));
    }

    #[test]
    fn dotted_keys_are_not_expanded() {
        assert_eq!(parse(&["a.b:1"]).unwrap().unwrap(), json!({"a.b": 1}));
    }

    #[test]
    fn missing_separator_is_an_error() {
        let err = parse(&["title"]).unwrap_err();
        assert!(err.contains("KEY:VALUE"), "{err}");
        assert!(err.contains("title"), "{err}");
    }

    #[test]
    fn empty_key_is_an_error() {
        assert!(parse(&[":x"]).is_err());
    }

    #[test]
    fn overlay_builder() {
        assert!(overlay_from_metadata_flags(&[]).unwrap().is_none());
        let o = overlay_from_metadata_flags(&["a:1".to_string()])
            .unwrap()
            .unwrap();
        assert_eq!(o.metadata, Some(json!({"a": 1})));
    }
}
