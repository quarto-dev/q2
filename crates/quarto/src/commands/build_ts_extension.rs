//! q2 call build-ts-extension

use std::path::PathBuf;

use anyhow::Result;

/// Arguments for the `build-ts-extension` command.
#[derive(Debug)]
pub struct BuildTsExtensionArgs {
    /// Path to the extension directory or `_extension.yml`. Defaults to cwd.
    pub path: Option<PathBuf>,
    /// Explicit `--config` override; wins over all other config sources.
    pub config: Option<PathBuf>,
    /// Force use of workspace `deno.workspace.json` (in-repo / pre-publish build).
    pub workspace: bool,
}

pub fn execute(args: BuildTsExtensionArgs) -> Result<()> {
    quarto_core::extension::build::build_ts_extension(
        quarto_core::extension::build::BuildOptions {
            ext_dir: args.path,
            config: args.config,
            workspace: args.workspace,
        },
    )?;
    Ok(())
}
