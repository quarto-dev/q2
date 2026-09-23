//! Dump mitex's prebuilt command spec (`default.rkyv`, from the
//! `mitex-rs/artifacts` submodule) to sorted JSON in `CommandSpecRepr`
//! shape: `{"commands": {name: item, ...}}`. Output lands in
//! `crates/quarto-math/spec/upstream/mitex-default-spec.json`.
//!
//! Requires the artifact at
//! `external-sources/mitex/crates/mitex-spec-gen/assets/artifacts/spec/default.rkyv`
//! (`git submodule update --init` inside `external-sources/mitex`).
use std::collections::BTreeMap;

#[derive(serde::Serialize)]
struct Repr {
    commands: BTreeMap<String, mitex_spec::CommandSpecItem>,
}

fn main() {
    let spec = mitex_spec_gen::DEFAULT_SPEC.clone();
    let mut commands = BTreeMap::new();
    for (name, item) in spec.items() {
        commands.insert(name.to_string(), item.clone());
    }
    let (cmds, envs) = commands.values().fold((0, 0), |(c, e), item| match item {
        mitex_spec::CommandSpecItem::Cmd(_) => (c + 1, e),
        mitex_spec::CommandSpecItem::Env(_) => (c, e + 1),
    });
    eprintln!("{} entries: {cmds} commands, {envs} environments", commands.len());
    println!("{}", serde_json::to_string_pretty(&Repr { commands }).unwrap());
}
