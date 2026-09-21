//! `cargo xtask gen-math-spec`: regenerate `crates/quarto-math/spec/commands.json`
//! from mitex's spec dump plus the hand-written overrides, or check that the
//! committed file is up to date (`--check`).
//!
//! The generation rules live in `quarto_math::spec::generator` so that the crate's
//! own drift test exercises exactly the code that produced the file.

use std::path::Path;

use anyhow::{Context, Result, bail};

const UPSTREAM: &str = "crates/quarto-math/spec/upstream/mitex-default-spec.json";
const OVERRIDES: &str = "crates/quarto-math/spec/overrides.json";
const OUTPUT: &str = "crates/quarto-math/spec/commands.json";

pub fn run(check: bool) -> Result<()> {
    let root = crate::create_worktree::repo_root()?;
    let read = |rel: &str| {
        std::fs::read_to_string(root.join(rel)).with_context(|| format!("reading {rel}"))
    };
    let generated = quarto_math::spec::generator::generate(&read(UPSTREAM)?, &read(OVERRIDES)?)
        .context("generating commands.json")?;
    let out = root.join(OUTPUT);
    if check {
        let current = std::fs::read_to_string(&out).unwrap_or_default();
        if current != generated {
            bail!("{OUTPUT} is stale; run `cargo xtask gen-math-spec`");
        }
        println!("{OUTPUT} is up to date");
        return Ok(());
    }
    std::fs::write(&out, &generated).with_context(|| format!("writing {OUTPUT}"))?;
    report(&out)?;
    Ok(())
}

fn report(out: &Path) -> Result<()> {
    let text = std::fs::read_to_string(out)?;
    let file: quarto_math::spec::SpecFile = serde_json::from_str(&text)?;
    let unsupported = file
        .commands
        .values()
        .filter(|r| r.sem.is_unsupported())
        .count();
    println!(
        "wrote {} ({} rows, {} unsupported)",
        out.display(),
        file.commands.len(),
        unsupported
    );
    Ok(())
}
