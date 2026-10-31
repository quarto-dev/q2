/*
 * lua/host_context.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * What a Lua script learns about where it is running, from the host's
 * `HostContext` (GH #939, #940):
 *
 * - `quarto.project` — `directory`, `offset`, `output_directory`,
 *   `profile`, with Quarto 1's shape (nil paths outside a project, the
 *   profile list always present);
 * - `os.getenv` and `pandoc.system.environment` — answered from the host's
 *   environment snapshot when it supplied one (see `process_view`), so libc
 *   `getenv` never runs on a filter worker thread and `--profile` is visible
 *   as `QUARTO_PROFILE`;
 * - `quarto.doc.input_file` / `quarto.doc.output_file` — the absolute
 *   paths Quarto 1 gave, and the minimal `PANDOC_STATE` (`input_files`,
 *   `output_file`) built from the same two values.
 *
 * Registered after `quarto` and `os` exist, in both the filter and the
 * shortcode environments.
 */

use mlua::{Lua, Result, Table};

use crate::lua_options::HostContext;

/// Register everything the host context contributes to the environment.
pub fn register_host_context(lua: &Lua, host: &HostContext) -> Result<()> {
    register_quarto_project(lua, host)?;
    register_quarto_doc_paths(lua, host)?;
    register_pandoc_state(lua, host)?;
    if let Some(env) = &host.env {
        super::process_view::set_env_base(lua, env.clone());
    }
    Ok(())
}

/// `quarto.doc.input_file` / `quarto.doc.output_file`: absolute paths, as
/// in Quarto 1 (`init.lua`'s `inputFile()` / `outputFile()`). Absent (nil)
/// without a host, never a relative or placeholder path.
fn register_quarto_doc_paths(lua: &Lua, host: &HostContext) -> Result<()> {
    let quarto: Table = lua.globals().get("quarto")?;
    let doc: Table = quarto.get("doc")?;
    if let Some(input) = &host.input_file {
        doc.set("input_file", input.to_string_lossy().to_string())?;
    }
    if let Some(output) = &host.output_file {
        doc.set("output_file", output.to_string_lossy().to_string())?;
    }
    Ok(())
}

/// The minimal `PANDOC_STATE`: `input_files` (a `pandoc.List` of the one
/// input, empty without a host) and `output_file`. Pandoc's `output_file`
/// is the writer's output *name* as given on its command line; Quarto 1
/// joined it with the output directory before any filter saw it
/// (`init.lua`'s `outputFile()`), so the absolute path is what Q1 filters
/// ended up with. The other pandoc fields (`resource_path`, `source_url`,
/// `user_data_dir`, `request_headers`, `verbosity`, `log`, `trace`) have
/// no q2 equivalent and stay nil, so a filter that needs one fails at its
/// own line rather than reading a plausible wrong default.
fn register_pandoc_state(lua: &Lua, host: &HostContext) -> Result<()> {
    let state = lua.create_table()?;
    let inputs: Vec<String> = host
        .input_file
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    state.set(
        "input_files",
        super::list::create_string_list_table(lua, &inputs)?,
    )?;
    if let Some(output) = &host.output_file {
        state.set("output_file", output.to_string_lossy().to_string())?;
    }
    lua.globals().set("PANDOC_STATE", state)?;
    Ok(())
}

/// `quarto.project`: Quarto 1's table, with nils where Q1 had them.
///
/// `profile` is a `pandoc.List` so Q1 code can call `:includes("drafts")`
/// on it; it is present (possibly empty) even outside a project, as in Q1
/// where it came from a filter param rather than the project.
fn register_quarto_project(lua: &Lua, host: &HostContext) -> Result<()> {
    let quarto: Table = lua.globals().get("quarto")?;
    let project = lua.create_table()?;
    if let Some(p) = &host.project {
        project.set("directory", p.directory.to_string_lossy().to_string())?;
        project.set("offset", p.offset.as_str())?;
        project.set(
            "output_directory",
            p.output_directory.to_string_lossy().to_string(),
        )?;
    }
    project.set(
        "profile",
        super::list::create_string_list_table(lua, &host.profiles)?,
    )?;
    quarto.set("project", project)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua_options::HostProject;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn lua_with(host: &HostContext) -> Lua {
        let lua = Lua::new();
        let runtime: Arc<dyn crate::lua::runtime::SystemRuntime> =
            Arc::new(crate::lua::runtime::NativeRuntime::new());
        super::super::constructors::register_pandoc_namespace(
            &lua,
            runtime,
            super::super::mediabag::create_shared_mediabag(),
        )
        .unwrap();
        super::super::quarto_api::register_quarto_api(&lua).unwrap();
        // Production order: quarto.doc exists before the host context
        // adds its path fields to it.
        super::super::quarto_doc::register_quarto_doc(&lua).unwrap();
        register_host_context(&lua, host).unwrap();
        lua
    }

    fn eval<T: mlua::FromLua>(lua: &Lua, src: &str) -> T {
        lua.load(src).eval().unwrap()
    }

    #[test]
    fn quarto_project_outside_a_project_has_q1_nils_and_an_empty_profile_list() {
        let lua = lua_with(&HostContext::default());
        assert!(eval::<bool>(&lua, "return quarto.project.directory == nil"));
        assert!(eval::<bool>(&lua, "return quarto.project.offset == nil"));
        assert!(eval::<bool>(
            &lua,
            "return quarto.project.output_directory == nil"
        ));
        assert_eq!(eval::<i64>(&lua, "return #quarto.project.profile"), 0);
        assert_eq!(
            eval::<String>(&lua, "return pandoc.utils.type(quarto.project.profile)"),
            "List"
        );
    }

    #[test]
    fn quarto_project_inside_a_project() {
        let dir = PathBuf::from("/proj");
        let host = HostContext {
            project: Some(HostProject {
                directory: dir.clone(),
                output_directory: dir.join("_site"),
                offset: "..".to_string(),
            }),
            profiles: vec!["drafts".to_string(), "release".to_string()],
            ..Default::default()
        };
        let lua = lua_with(&host);
        assert_eq!(
            eval::<String>(&lua, "return quarto.project.directory"),
            dir.to_string_lossy()
        );
        assert_eq!(eval::<String>(&lua, "return quarto.project.offset"), "..");
        assert_eq!(
            eval::<String>(&lua, "return quarto.project.output_directory"),
            dir.join("_site").to_string_lossy()
        );
        assert!(eval::<bool>(
            &lua,
            "return quarto.project.profile:includes('drafts') and #quarto.project.profile == 2"
        ));
    }

    #[test]
    fn doc_paths_and_pandoc_state_without_a_host() {
        let lua = lua_with(&HostContext::default());
        assert!(eval::<bool>(&lua, "return quarto.doc.input_file == nil"));
        assert!(eval::<bool>(&lua, "return quarto.doc.output_file == nil"));
        assert_eq!(eval::<i64>(&lua, "return #PANDOC_STATE.input_files"), 0);
        assert!(eval::<bool>(&lua, "return PANDOC_STATE.output_file == nil"));
    }

    #[test]
    fn doc_paths_and_pandoc_state_with_a_host() {
        let input = PathBuf::from("/proj").join("sub").join("page.qmd");
        let output = PathBuf::from("/proj")
            .join("_site")
            .join("sub")
            .join("page.html");
        let host = HostContext {
            input_file: Some(input.clone()),
            output_file: Some(output.clone()),
            ..Default::default()
        };
        let lua = lua_with(&host);
        assert_eq!(
            eval::<String>(&lua, "return quarto.doc.input_file"),
            input.to_string_lossy()
        );
        assert_eq!(
            eval::<String>(&lua, "return quarto.doc.output_file"),
            output.to_string_lossy()
        );
        assert!(eval::<bool>(
            &lua,
            "return #PANDOC_STATE.input_files == 1 and PANDOC_STATE.input_files[1] == quarto.doc.input_file"
        ));
        assert!(eval::<bool>(
            &lua,
            "return PANDOC_STATE.output_file == quarto.doc.output_file"
        ));
        assert_eq!(
            eval::<String>(&lua, "return pandoc.utils.type(PANDOC_STATE.input_files)"),
            "List"
        );
    }

    #[test]
    fn os_getenv_reads_the_snapshot_only() {
        let mut env = HashMap::new();
        env.insert("QUARTO_PROFILE".to_string(), "drafts,release".to_string());
        env.insert("FROM_FILE".to_string(), "yes".to_string());
        let host = HostContext {
            env: Some(Arc::new(env)),
            ..Default::default()
        };
        let lua = lua_with(&host);
        assert_eq!(
            eval::<String>(&lua, "return os.getenv('QUARTO_PROFILE')"),
            "drafts,release"
        );
        assert_eq!(eval::<String>(&lua, "return os.getenv('FROM_FILE')"), "yes");
        // Not in the snapshot means not visible, even when the real
        // process has it: the snapshot is the whole environment.
        assert!(eval::<bool>(&lua, "return os.getenv('PATH') == nil"));
    }

    #[test]
    fn environment_and_with_environment_share_the_snapshot_with_os_getenv() {
        let mut env = HashMap::new();
        env.insert("FROM_FILE".to_string(), "yes".to_string());
        let host = HostContext {
            env: Some(Arc::new(env)),
            ..Default::default()
        };
        let lua = lua_with(&host);
        lua.load(
            r#"
            local before = pandoc.system.environment()
            assert(before.FROM_FILE == 'yes')
            assert(before.PATH == nil, 'the snapshot is the whole environment')
            pandoc.system.with_environment({Q2_ENV_A = 'x'}, function()
                assert(os.getenv('Q2_ENV_A') == 'x')
                assert(os.getenv('FROM_FILE') == nil)
                assert(pandoc.system.environment().FROM_FILE == nil)
            end)
            assert(os.getenv('FROM_FILE') == 'yes')
            assert(os.getenv('Q2_ENV_A') == nil)
            assert(pandoc.system.environment().FROM_FILE == 'yes')
            "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn os_getenv_without_a_snapshot_is_the_process_environment() {
        let lua = lua_with(&HostContext::default());
        // PATH is set in every environment the test suite runs under.
        assert!(eval::<bool>(
            &lua,
            "return type(os.getenv('PATH')) == 'string'"
        ));
    }
}
