/*
 * spec.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! The command specification quarto-math parses against.
//!
//! Today this wraps mitex's command spec (argument shapes per command and
//! environment) loaded from the JSON dump under `spec/upstream/`. Phase 1
//! replaces the embedded file with the q2-owned `spec/commands.json`, which
//! adds writer semantics (OMML kind, Typst alias) to the same rows; the
//! parser half of a row stays a mitex `CommandSpecItem`.

use std::sync::OnceLock;

use mitex_spec::{CommandSpec, CommandSpecRepr};

/// The bundled specification, in the JSON shape of
/// [`mitex_spec::CommandSpecRepr`].
const BUILTIN_JSON: &str = include_str!("../spec/upstream/mitex-default-spec.json");

/// A parsed command specification.
#[derive(Debug, Clone)]
pub struct Spec {
    command_spec: CommandSpec,
}

impl Spec {
    /// The specification bundled with the crate. Parsed once per process.
    pub fn builtin() -> &'static Spec {
        static BUILTIN: OnceLock<Spec> = OnceLock::new();
        BUILTIN
            .get_or_init(|| Spec::from_json(BUILTIN_JSON).expect("the bundled spec JSON is valid"))
    }

    /// Parse a specification from its JSON form (`{"commands": {...}}`).
    pub fn from_json(json: &str) -> Result<Spec, serde_json::Error> {
        let repr: CommandSpecRepr = serde_json::from_str(json)?;
        Ok(Spec {
            command_spec: CommandSpec::new(repr.commands),
        })
    }

    /// The argument-shape half of the spec, as the mitex parser wants it.
    pub fn command_spec(&self) -> &CommandSpec {
        &self.command_spec
    }

    /// Number of commands and environments in the spec.
    pub fn len(&self) -> usize {
        self.command_spec.items().count()
    }

    /// Whether the spec has no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
