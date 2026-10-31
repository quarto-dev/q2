/*
 * lua/process_view.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * What one Lua state sees of the process. Project renders run filters on
 * parallel workers, and the environment and working directory are
 * process-global, so a Lua API must never change the real ones
 * (bd-with-environment-process-env-5folaygy, bd-9hzg79zn). Each state keeps its
 * own view instead:
 *
 * - The environment: a base, which is the host's snapshot when it supplied one
 *   and the runtime otherwise, and a stack of tables, one for each
 *   `pandoc.system.with_environment` in progress. The top one replaces the base
 *   entirely, as in Pandoc. `os.getenv`, `pandoc.system.environment`,
 *   `pandoc.system.command` and `pandoc.pipe` read it. `pandoc.system.xdg` reads
 *   the process environment through the runtime and ignores it.
 *
 * - The working directory: a stack of absolute paths, one for each
 *   `pandoc.system.with_working_directory` in progress, resolved in
 *   `resolve_path` by every Lua API that takes a path. The stock functions that
 *   take a path are wrapped to do the same. `require`'s stock searchers open
 *   relative `package.path` entries themselves, so they keep using the process
 *   directory.
 *
 * The stock `io.popen` and `os.execute` take neither, because libc gives their
 * children the process's own, so they raise while either stack is non-empty
 * rather than run a child in the wrong context.
 *
 * A callback runs through a Rust-side call, which Lua cannot yield across, so
 * these scopes are strictly nested and a stack is enough.
 */

use mlua::{Lua, MultiValue, Result, Table, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::runtime::SystemRuntime;

type EnvMap = HashMap<String, String>;

#[derive(Default)]
struct ProcessView {
    env_base: Option<Arc<EnvMap>>,
    env_overlays: Vec<EnvMap>,
    cwd_overlays: Vec<PathBuf>,
}

fn update(lua: &Lua, change: impl FnOnce(&mut ProcessView)) {
    if lua.app_data_ref::<ProcessView>().is_none() {
        lua.set_app_data(ProcessView::default());
    }
    if let Some(mut view) = lua.app_data_mut::<ProcessView>() {
        change(&mut view);
    }
}

/// Make `env` the state's whole environment, in place of the runtime's.
pub(super) fn set_env_base(lua: &Lua, env: Arc<EnvMap>) {
    update(lua, |view| view.env_base = Some(env));
}

/// The table of the innermost `with_environment` in progress, if any.
pub(super) fn active_env(lua: &Lua) -> Option<EnvMap> {
    lua.app_data_ref::<ProcessView>()?
        .env_overlays
        .last()
        .cloned()
}

/// The directory of the innermost `with_working_directory` in progress, if any.
pub(super) fn active_cwd(lua: &Lua) -> Option<PathBuf> {
    lua.app_data_ref::<ProcessView>()?
        .cwd_overlays
        .last()
        .cloned()
}

/// `path` as the state means it: relative paths are against the active working
/// directory, if there is one. Otherwise the path is returned unchanged, to
/// resolve against the process directory as usual.
pub(super) fn resolve_path(lua: &Lua, path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    match active_cwd(lua) {
        Some(cwd) if path.is_relative() => cwd.join(path),
        _ => path.to_path_buf(),
    }
}

/// One variable, as the state sees it.
fn lookup(lua: &Lua, runtime: &dyn SystemRuntime, name: &str) -> Result<Option<String>> {
    if let Some(view) = lua.app_data_ref::<ProcessView>() {
        if let Some(env) = view.env_overlays.last() {
            return Ok(env.get(name).cloned());
        }
        if let Some(env) = &view.env_base {
            return Ok(env.get(name).cloned());
        }
    }
    runtime
        .env_get(name)
        .map_err(|e| mlua::Error::runtime(e.to_string()))
}

/// The whole environment, as the state sees it.
pub(super) fn all_vars(lua: &Lua, runtime: &dyn SystemRuntime) -> Result<EnvMap> {
    if let Some(view) = lua.app_data_ref::<ProcessView>() {
        if let Some(env) = view.env_overlays.last() {
            return Ok(env.clone());
        }
        if let Some(env) = &view.env_base {
            return Ok(env.as_ref().clone());
        }
    }
    runtime
        .env_all()
        .map_err(|e| mlua::Error::runtime(e.to_string()))
}

/// Keeps one overlay active until dropped, so every exit path ends it.
pub(super) struct OverlayGuard<'a> {
    lua: &'a Lua,
    pop: fn(&mut ProcessView),
}

impl<'a> OverlayGuard<'a> {
    pub(super) fn env(lua: &'a Lua, env: EnvMap) -> Self {
        update(lua, |view| view.env_overlays.push(env));
        Self {
            lua,
            pop: |view| {
                view.env_overlays.pop();
            },
        }
    }

    /// `cwd` must be absolute.
    pub(super) fn cwd(lua: &'a Lua, cwd: PathBuf) -> Self {
        update(lua, |view| view.cwd_overlays.push(cwd));
        Self {
            lua,
            pop: |view| {
                view.cwd_overlays.pop();
            },
        }
    }
}

impl Drop for OverlayGuard<'_> {
    fn drop(&mut self) {
        update(self.lua, self.pop);
    }
}

/// Route the stock environment, spawn and path functions through this view.
///
/// The functions are replaced for the life of the state, not per call, so a
/// script that saved `local getenv = os.getenv` is covered too. Functions the
/// state does not have (`io.popen` and `os.execute` on WASM) stay absent.
pub(super) fn install(lua: &Lua, runtime: Arc<dyn SystemRuntime>) -> Result<()> {
    let globals = lua.globals();
    if let Value::Table(os) = globals.get::<Value>("os")? {
        os.set(
            "getenv",
            lua.create_function(move |lua, name: String| lookup(lua, runtime.as_ref(), &name))?,
        )?;
        guard_stock_spawn(lua, &os, "os", "execute")?;
        resolve_stock_paths(lua, &os, "remove", &[0])?;
        resolve_stock_paths(lua, &os, "rename", &[0, 1])?;
    }
    if let Value::Table(io) = globals.get::<Value>("io")? {
        guard_stock_spawn(lua, &io, "io", "popen")?;
        for name in ["open", "lines", "input", "output"] {
            resolve_stock_paths(lua, &io, name, &[0])?;
        }
    }
    resolve_stock_paths(lua, &globals, "loadfile", &[0])?;
    resolve_stock_paths(lua, &globals, "dofile", &[0])?;
    Ok(())
}

/// Make a stock libc spawn function raise while an overlay is active.
fn guard_stock_spawn(lua: &Lua, library: &Table, library_name: &str, name: &str) -> Result<()> {
    let Value::Function(stock) = library.get::<Value>(name)? else {
        return Ok(());
    };
    let qualified = format!("{library_name}.{name}");
    let guarded = lua.create_function(move |lua, args: MultiValue| {
        let scope = lua.app_data_ref::<ProcessView>().and_then(|view| {
            if !view.env_overlays.is_empty() {
                Some(("with_environment", "environment"))
            } else if !view.cwd_overlays.is_empty() {
                Some(("with_working_directory", "working directory"))
            } else {
                None
            }
        });
        if let Some((function, what)) = scope {
            return Err(mlua::Error::runtime(format!(
                "{qualified} cannot run inside pandoc.system.{function}, because its child \
                 would get the process {what}; use pandoc.pipe or pandoc.system.command instead"
            )));
        }
        stock.call::<MultiValue>(args)
    })?;
    library.set(name, guarded)
}

/// Make a stock function resolve the string arguments at `positions`. A
/// non-string there, such as the file handle `io.input` also accepts, passes
/// through.
fn resolve_stock_paths(
    lua: &Lua,
    library: &Table,
    name: &str,
    positions: &'static [usize],
) -> Result<()> {
    let Value::Function(stock) = library.get::<Value>(name)? else {
        return Ok(());
    };
    let wrapped = lua.create_function(move |lua, args: MultiValue| {
        if active_cwd(lua).is_none() {
            return stock.call::<MultiValue>(args);
        }
        let mut args: Vec<Value> = args.into_iter().collect();
        for &position in positions {
            let Some(Value::String(path)) = args.get(position) else {
                continue;
            };
            let Ok(path) = path.to_str().map(|p| p.to_string()) else {
                continue;
            };
            let resolved = resolve_path(lua, &path).to_string_lossy().to_string();
            args[position] = Value::String(lua.create_string(resolved)?);
        }
        stock.call::<MultiValue>(MultiValue::from_iter(args))
    })?;
    library.set(name, wrapped)
}
