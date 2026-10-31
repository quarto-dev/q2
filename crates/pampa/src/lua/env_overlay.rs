/*
 * lua/env_overlay.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The environment one Lua state sees. Project renders run filters on parallel
 * workers, so a Lua API must never change the process environment
 * (bd-with-environment-process-env-5folaygy). This module keeps everything
 * per-state instead:
 *
 * - a base, which is the host's snapshot when it supplied one and the runtime
 *   otherwise;
 * - a stack of tables, one for each `pandoc.system.with_environment` call in
 *   progress. The top one replaces the base entirely, as in Pandoc.
 *
 * `os.getenv`, `pandoc.system.environment`, `pandoc.system.command` and
 * `pandoc.pipe` all read this state. The stock `io.popen` and `os.execute`
 * cannot, because libc gives their children the process environment, so they
 * raise while a table is active rather than run a child with the wrong one.
 * `pandoc.system.xdg` reads the process environment through the runtime and
 * ignores the overlay.
 *
 * The callback runs through a Rust-side call, which Lua cannot yield across,
 * so overlays are strictly nested and a stack is enough.
 */

use mlua::{Lua, MultiValue, Result, Table, Value};
use std::collections::HashMap;
use std::sync::Arc;

use super::runtime::SystemRuntime;

type EnvMap = HashMap<String, String>;

#[derive(Default)]
struct EnvState {
    base: Option<Arc<EnvMap>>,
    overlays: Vec<EnvMap>,
}

/// Make `env` the state's whole environment, in place of the runtime's.
pub(super) fn set_base(lua: &Lua, env: Arc<EnvMap>) {
    match lua.app_data_mut::<EnvState>() {
        Some(mut state) => state.base = Some(env),
        None => {
            lua.set_app_data(EnvState {
                base: Some(env),
                overlays: Vec::new(),
            });
        }
    }
}

/// The table of the innermost `with_environment` in progress, if any.
pub(super) fn active_overlay(lua: &Lua) -> Option<EnvMap> {
    lua.app_data_ref::<EnvState>()?.overlays.last().cloned()
}

/// One variable, as the state sees it.
fn lookup(lua: &Lua, runtime: &dyn SystemRuntime, name: &str) -> Result<Option<String>> {
    if let Some(state) = lua.app_data_ref::<EnvState>() {
        if let Some(env) = state.overlays.last() {
            return Ok(env.get(name).cloned());
        }
        if let Some(env) = &state.base {
            return Ok(env.get(name).cloned());
        }
    }
    runtime
        .env_get(name)
        .map_err(|e| mlua::Error::runtime(e.to_string()))
}

/// The whole environment, as the state sees it.
pub(super) fn all_vars(lua: &Lua, runtime: &dyn SystemRuntime) -> Result<EnvMap> {
    if let Some(state) = lua.app_data_ref::<EnvState>() {
        if let Some(env) = state.overlays.last() {
            return Ok(env.clone());
        }
        if let Some(env) = &state.base {
            return Ok(env.as_ref().clone());
        }
    }
    runtime
        .env_all()
        .map_err(|e| mlua::Error::runtime(e.to_string()))
}

/// Keeps `env` active until dropped, so every exit path ends the overlay.
pub(super) struct OverlayGuard<'a> {
    lua: &'a Lua,
}

impl<'a> OverlayGuard<'a> {
    pub(super) fn push(lua: &'a Lua, env: EnvMap) -> Self {
        if lua.app_data_ref::<EnvState>().is_none() {
            lua.set_app_data(EnvState::default());
        }
        if let Some(mut state) = lua.app_data_mut::<EnvState>() {
            state.overlays.push(env);
        }
        Self { lua }
    }
}

impl Drop for OverlayGuard<'_> {
    fn drop(&mut self) {
        if let Some(mut state) = self.lua.app_data_mut::<EnvState>() {
            state.overlays.pop();
        }
    }
}

/// Route the `os` and `io` environment paths through this state.
///
/// The functions are replaced for the life of the state, not per call, so a
/// script that saved `local getenv = os.getenv` is covered too. Libraries the
/// state does not have (`io.popen` and `os.execute` on WASM) are left absent.
pub(super) fn install(lua: &Lua, runtime: Arc<dyn SystemRuntime>) -> Result<()> {
    if let Value::Table(os) = lua.globals().get::<Value>("os")? {
        os.set(
            "getenv",
            lua.create_function(move |lua, name: String| lookup(lua, runtime.as_ref(), &name))?,
        )?;
        guard_stock_spawn(lua, &os, "os", "execute")?;
    }
    if let Value::Table(io) = lua.globals().get::<Value>("io")? {
        guard_stock_spawn(lua, &io, "io", "popen")?;
    }
    Ok(())
}

/// Make a stock libc spawn function raise while an overlay is active.
fn guard_stock_spawn(lua: &Lua, library: &Table, library_name: &str, name: &str) -> Result<()> {
    let Value::Function(stock) = library.get::<Value>(name)? else {
        return Ok(());
    };
    let qualified = format!("{library_name}.{name}");
    let guarded = lua.create_function(move |lua, args: MultiValue| {
        if lua
            .app_data_ref::<EnvState>()
            .is_some_and(|state| !state.overlays.is_empty())
        {
            return Err(mlua::Error::runtime(format!(
                "{qualified} cannot run inside pandoc.system.with_environment, because its \
                 child would get the process environment; use pandoc.pipe or \
                 pandoc.system.command instead"
            )));
        }
        stock.call::<MultiValue>(args)
    })?;
    library.set(name, guarded)
}
