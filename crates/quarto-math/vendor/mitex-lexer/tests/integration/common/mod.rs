/// q2 local patch: upstream loads this from `mitex-spec-gen`'s prebuilt rkyv
/// artifact; the vendored copy loads the same spec from the JSON dump kept
/// at `crates/quarto-math/spec/upstream/mitex-default-spec.json`.
pub static DEFAULT_SPEC: once_cell::sync::Lazy<mitex_spec::CommandSpec> =
    once_cell::sync::Lazy::new(|| {
        let repr: mitex_spec::CommandSpecRepr = serde_json::from_str(include_str!(
            "../../../../../spec/upstream/mitex-default-spec.json"
        ))
        .expect("bundled mitex spec JSON parses");
        mitex_spec::CommandSpec::new(repr.commands)
    });
