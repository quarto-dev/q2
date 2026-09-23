/*
 * spec/generator.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Generation of `spec/commands.json` (feature `spec-gen`).
//!
//! Inputs: mitex's spec dump (`spec/upstream/mitex-default-spec.json`) and
//! the hand-written `spec/overrides.json`. Rules, in order, for each
//! upstream entry:
//!
//! 1. `parse` is the upstream item verbatim.
//! 2. An override row wins: its `sem` and, when given, its `typst`.
//! 3. A zero-argument command resolves its Typst alias (`null` means the
//!    command's own name) to a symbol: a literal with no ASCII letters or
//!    digits is used as-is (`+`, `≎`); otherwise the alias is looked up in
//!    `codex` (`lt.eq` → `≤`).
//! 4. Anything left is `unsupported` with a reason, so a gap is visible in
//!    the JSON rather than silent.
//!
//! An override may also add a name mitex lacks; it must then carry `parse`.
//! The output is sorted and pretty-printed, so a regeneration that changes
//! nothing changes no bytes (the drift test relies on this).

use std::collections::BTreeMap;

use mitex_spec::{ArgPattern, ArgShape, CommandSpecItem};
use serde::Deserialize;

use super::{Row, Semantics, SpecFile};

/// One hand-written override.
#[derive(Debug, Clone, Deserialize)]
pub struct OverrideRow {
    pub sem: Semantics,
    /// Replaces the upstream Typst alias when present.
    #[serde(default)]
    pub typst: Option<String>,
    /// Required when the name is not in the upstream spec.
    #[serde(default)]
    pub parse: Option<CommandSpecItem>,
}

/// The on-disk shape of `overrides.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct Overrides {
    pub rows: BTreeMap<String, OverrideRow>,
}

/// Why generation failed.
#[derive(Debug, thiserror::Error)]
pub enum GenError {
    #[error("upstream spec JSON: {0}")]
    Upstream(serde_json::Error),
    #[error("overrides JSON: {0}")]
    Overrides(serde_json::Error),
    #[error("override for {0:?} names a command mitex does not know and carries no `parse`")]
    OverrideWithoutParse(String),
}

/// Upstream spec dump shape (`{"commands": {name: CommandSpecItem}}`).
#[derive(Debug, Deserialize)]
struct Upstream {
    commands: BTreeMap<String, CommandSpecItem>,
}

/// Resolve a Typst symbol path such as `alpha` or `arrow.r.long` through
/// codex's `sym` module.
pub fn resolve_typst_symbol(path: &str) -> Option<&'static str> {
    let mut segments = path.split('.');
    let head = segments.next()?;
    let codex::Def::Module(sym) = codex::ROOT.get("sym")?.def else {
        return None;
    };
    let mut binding = sym.get(head)?;
    // Descend through nested modules while the next segment names one.
    let mut rest: Vec<&str> = segments.collect();
    while let codex::Def::Module(module) = binding.def {
        let next = rest.first()?;
        binding = module.get(next)?;
        rest.remove(0);
    }
    let codex::Def::Symbol(symbol) = binding.def else {
        return None;
    };
    let modifiers = codex::ModifierSet::from_raw_dotted(rest.join("."));
    symbol.get(modifiers.as_deref()).map(|(text, _)| text)
}

fn is_zero_arg_cmd(item: &CommandSpecItem) -> bool {
    matches!(
        item,
        CommandSpecItem::Cmd(shape)
            if matches!(shape.args, ArgShape::Right { pattern: ArgPattern::None })
    )
}

fn upstream_alias(item: &CommandSpecItem) -> Option<&str> {
    match item {
        CommandSpecItem::Cmd(shape) => shape.alias.as_deref(),
        CommandSpecItem::Env(shape) => shape.alias.as_deref(),
    }
}

/// Semantics for an entry without an override.
fn derive_semantics(name: &str, item: &CommandSpecItem) -> Semantics {
    if !is_zero_arg_cmd(item) {
        return Semantics::Unsupported {
            why: "no semantics assigned".to_string(),
        };
    }
    let alias = upstream_alias(item).unwrap_or(name);
    if alias.is_empty() {
        return Semantics::Unsupported {
            why: "no typst alias".to_string(),
        };
    }
    if alias.starts_with('#') {
        return Semantics::Unsupported {
            why: "typst function alias".to_string(),
        };
    }
    if !alias.chars().any(|c| c.is_ascii_alphanumeric()) {
        return Semantics::Sym {
            text: alias.to_string(),
        };
    }
    match resolve_typst_symbol(alias) {
        Some(text) => Semantics::Sym {
            text: text.to_string(),
        },
        None => Semantics::Unsupported {
            why: "no semantics assigned".to_string(),
        },
    }
}

/// Generate the `commands.json` text.
pub fn generate(upstream_json: &str, overrides_json: &str) -> Result<String, GenError> {
    let upstream: Upstream = serde_json::from_str(upstream_json).map_err(GenError::Upstream)?;
    let overrides: Overrides = serde_json::from_str(overrides_json).map_err(GenError::Overrides)?;

    let mut commands: BTreeMap<String, Row> = BTreeMap::new();
    for (name, item) in &upstream.commands {
        let (sem, typst) = match overrides.rows.get(name) {
            Some(o) => (
                o.sem.clone(),
                o.typst
                    .clone()
                    .or_else(|| upstream_alias(item).map(str::to_string)),
            ),
            None => (
                derive_semantics(name, item),
                upstream_alias(item).map(str::to_string),
            ),
        };
        commands.insert(
            name.clone(),
            Row {
                parse: item.clone(),
                sem,
                typst,
            },
        );
    }
    for (name, o) in &overrides.rows {
        if commands.contains_key(name) {
            continue;
        }
        let Some(parse) = o.parse.clone() else {
            return Err(GenError::OverrideWithoutParse(name.clone()));
        };
        commands.insert(
            name.clone(),
            Row {
                parse,
                sem: o.sem.clone(),
                typst: o.typst.clone(),
            },
        );
    }

    let file = SpecFile { commands };
    let mut text = serde_json::to_string_pretty(&file).expect("SpecFile serializes");
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_plain_and_dotted_typst_symbols() {
        assert_eq!(resolve_typst_symbol("alpha"), Some("α"));
        assert_eq!(resolve_typst_symbol("lt.eq"), Some("≤"));
        assert_eq!(resolve_typst_symbol("arrow.r"), Some("→"));
        assert_eq!(resolve_typst_symbol("no.such.symbol"), None);
        assert_eq!(resolve_typst_symbol("mitexsqrt"), None);
    }
}
