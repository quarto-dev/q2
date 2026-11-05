/*
 * dashboard/valuebox.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Value boxes: Quarto 1's `valuebox.lua` (`makeValueBox`,
//! `parseStdOutToOptions`).
//!
//! A value box is a card (classes `card valuebox`) whose one body holds
//! Q1's value-box `Div`: `data-color` / `data-bg-color` / `data-fg-color`
//! attributes, an optional `.value-box-showcase` (`data-icon`,
//! `data-showcase-position`), and a `.value-box-area` with the title, the
//! value and any extra content. [`super::resolve`] lowers it as Q1's
//! `processValueBoxes` does.
//!
//! A `content: valuebox` cell prints its options. Q1 evaluated the printed
//! Python dict as Lua source (`load()`); q2 parses the two printed shapes
//! with [`parse_printed`] and warns (`Q-27-1`) when the output cannot be
//! read (dashboard plan D6).

use hashlink::LinkedHashMap;

/// What a printed value-box cell output said.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Printed {
    pub value: Option<String>,
    /// Options in the order printed (`icon`, `color`, `bg-color`,
    /// `fg-color`, and `title` for R lists).
    pub options: LinkedHashMap<String, String>,
}

/// Option keys Q1 reads from a printed Python dict (`kValueBoxOptions`).
const DICT_OPTIONS: &[&str] = &["icon", "color", "bg-color", "fg-color"];
/// Keys Q1 reads from a printed R list (`kForwardValueFromCodeCell`).
const LIST_KEYS: &[&str] = &["title", "value", "color", "bg-color", "fg-color", "icon"];

/// Parse a value-box cell's printed output (Q1 `parseStdOutToOptions`):
///
/// - `{...}`: a Python dict repr, `{'key': value, ...}`;
/// - `$key` lines each followed by `[1] value`: an R list;
/// - `[1] value`: an R vector;
/// - anything else: the value itself, with surrounding single quotes
///   (a Python string repr) removed.
///
/// `Err` (with a description of what went wrong) only for a `{` or `$`
/// output that does not have the expected shape.
pub fn parse_printed(text: &str) -> Result<Printed, String> {
    let text = text.trim_end_matches('\n');
    match text.chars().next() {
        Some('{') => parse_dict(text),
        Some('$') => parse_r_list(text),
        Some('[') => Ok(Printed {
            value: Some(r_vector_value(text).unwrap_or_default()),
            ..Default::default()
        }),
        _ => Ok(Printed {
            // Q1 strips a leading and a trailing quote independently.
            value: Some({
                let t = text.strip_prefix('\'').unwrap_or(text);
                t.strip_suffix('\'').unwrap_or(t).to_string()
            }),
            ..Default::default()
        }),
    }
}

/// Q1's `%[1%] "?([^"]*)`: the text after `[1] `, an opening quote
/// dropped, up to the next quote.
fn r_vector_value(line: &str) -> Option<String> {
    let rest = line.split_once("[1] ")?.1;
    let rest = rest.strip_prefix('"').unwrap_or(rest);
    Some(rest.split('"').next().unwrap_or("").to_string())
}

fn parse_r_list(text: &str) -> Result<Printed, String> {
    let mut out = Printed::default();
    let mut pending: Option<String> = None;
    for line in text.lines() {
        if let Some(key) = pending.take() {
            let value = r_vector_value(line)
                .ok_or_else(|| format!("expected `[1] <value>` after `${key}`"))?;
            if key == "value" {
                out.value = Some(value);
            } else {
                out.options.insert(key, value);
            }
        } else if let Some(key) = line.strip_prefix('$') {
            let key = key.trim();
            if LIST_KEYS.contains(&key) {
                pending = Some(key.to_string());
            }
        }
    }
    Ok(out)
}

/// A minimal reader for the dict reprs Python prints: string keys, and
/// values that are quoted strings or bare tokens (numbers, `True`, `None`,
/// …). Nested containers are read as raw text.
fn parse_dict(text: &str) -> Result<Printed, String> {
    let inner = text
        .trim()
        .strip_prefix('{')
        .and_then(|t| t.strip_suffix('}'))
        .ok_or_else(|| "a printed dict must end with `}`".to_string())?;
    let mut out = Printed::default();
    let chars: Vec<char> = inner.chars().collect();
    let mut i = 0;
    let skip_ws = |i: &mut usize| {
        while *i < chars.len() && chars[*i].is_whitespace() {
            *i += 1;
        }
    };
    loop {
        skip_ws(&mut i);
        if i >= chars.len() {
            break;
        }
        let key = read_string(&chars, &mut i)
            .ok_or_else(|| format!("expected a quoted key at `{}`", rest(&chars, i)))?;
        skip_ws(&mut i);
        if chars.get(i) != Some(&':') {
            return Err(format!("expected `:` after the key `{key}`"));
        }
        i += 1;
        skip_ws(&mut i);
        let value = match chars.get(i) {
            Some('\'' | '"') => read_string(&chars, &mut i)
                .ok_or_else(|| format!("unterminated string for `{key}`"))?,
            Some(_) => read_token(&chars, &mut i),
            None => return Err(format!("missing a value for `{key}`")),
        };
        if key == "value" {
            out.value = Some(value);
        } else if DICT_OPTIONS.contains(&key.as_str()) {
            out.options.insert(key, value);
        }
        skip_ws(&mut i);
        match chars.get(i) {
            Some(',') => i += 1,
            None => break,
            Some(c) => return Err(format!("expected `,` or the end of the dict, found `{c}`")),
        }
    }
    Ok(out)
}

fn rest(chars: &[char], i: usize) -> String {
    chars[i..].iter().take(20).collect()
}

/// A Python string literal: single or double quotes, backslash escapes.
fn read_string(chars: &[char], i: &mut usize) -> Option<String> {
    let quote = *chars.get(*i)?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    *i += 1;
    let mut s = String::new();
    while let Some(&c) = chars.get(*i) {
        *i += 1;
        match c {
            '\\' => {
                let next = *chars.get(*i)?;
                *i += 1;
                s.push(match next {
                    'n' => '\n',
                    't' => '\t',
                    other => other,
                });
            }
            c if c == quote => return Some(s),
            c => s.push(c),
        }
    }
    None
}

/// A bare value up to the next top-level comma (brackets and quotes
/// nest), trimmed.
fn read_token(chars: &[char], i: &mut usize) -> String {
    let start = *i;
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    while let Some(&c) = chars.get(*i) {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '\'' | '"' => quote = Some(c),
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ',' if depth == 0 => break,
                _ => {}
            },
        }
        *i += 1;
    }
    chars[start..*i]
        .iter()
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(p: &Printed) -> Vec<(&str, &str)> {
        p.options
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }

    #[test]
    fn python_dict() {
        let p = parse_printed("{'icon': 'chat', 'color': 'warning', 'value': 126}").unwrap();
        assert_eq!(p.value.as_deref(), Some("126"));
        assert_eq!(opts(&p), [("icon", "chat"), ("color", "warning")]);
    }

    /// Q1 reads only icon and colors from a dict; `title` is not one of them.
    #[test]
    fn python_dict_ignores_title_and_unknown_keys() {
        let p =
            parse_printed("{'title': 'T', 'value': 4.5, 'other': [1, 2], 'fg-color': \"#fff\"}")
                .unwrap();
        assert_eq!(p.value.as_deref(), Some("4.5"));
        assert_eq!(opts(&p), [("fg-color", "#fff")]);
    }

    #[test]
    fn python_dict_errors_are_reported() {
        assert!(parse_printed("{'value': 1").is_err());
        assert!(parse_printed("{value: 1}").is_err());
        assert!(parse_printed("{'value' 1}").is_err());
        assert!(parse_printed("{'value': 'open}").is_err());
    }

    #[test]
    fn r_list() {
        let p = parse_printed("$icon\n[1] \"trash\"\n\n$color\n[1] \"danger\"\n\n$value\n[1] 50\n")
            .unwrap();
        assert_eq!(p.value.as_deref(), Some("50"));
        assert_eq!(opts(&p), [("icon", "trash"), ("color", "danger")]);
    }

    #[test]
    fn r_vector_and_bare_values() {
        assert_eq!(
            parse_printed("[1] 45.5").unwrap().value.as_deref(),
            Some("45.5")
        );
        assert_eq!(
            parse_printed("'hello'").unwrap().value.as_deref(),
            Some("hello")
        );
        assert_eq!(parse_printed("12").unwrap().value.as_deref(), Some("12"));
    }
}
