/*
 * lua/os_setlocale.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `os.setlocale` that never changes the locale (bd-1fuxtxkd). The stock
 * function calls libc `setlocale`, which is process-global and not thread-safe.
 * Project renders run filters on parallel workers, so one document's call
 * changed how every other document formats numbers, for the rest of the render.
 *
 * Lua's number formatting reads the C library's locale, so a per-state locale
 * is not possible. Instead the process stays in the "C" locale it starts in,
 * and any other request gets `nil`, which is how Lua reports a request that
 * cannot be honoured.
 */

use mlua::{Lua, Result, Table, Value};

const CATEGORIES: [&str; 6] = ["all", "collate", "ctype", "monetary", "numeric", "time"];

/// Replace `os.setlocale` where it exists. WASM's synthetic `os` has none, and
/// stays without one.
pub fn install_os_setlocale(lua: &Lua) -> Result<()> {
    let os: Table = match lua.globals().get::<Value>("os")? {
        Value::Table(t) => t,
        _ => return Ok(()),
    };
    if os.get::<Value>("setlocale")?.is_nil() {
        return Ok(());
    }
    os.set(
        "setlocale",
        lua.create_function(|_, (locale, category): (Option<String>, Option<String>)| {
            // Reject a bad category as the stock function does.
            if let Some(category) = &category
                && !CATEGORIES.contains(&category.as_str())
            {
                return Err(mlua::Error::runtime(format!(
                    "bad argument #2 to 'setlocale' (invalid option '{category}')"
                )));
            }
            Ok(match locale.as_deref() {
                None | Some("C" | "POSIX") => Some("C"),
                Some(_) => None,
            })
        })?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lua() -> Lua {
        let lua = Lua::new();
        install_os_setlocale(&lua).unwrap();
        lua
    }

    #[test]
    fn queries_and_the_c_locale_report_c() {
        let lua = lua();
        lua.load(
            r#"
            assert(os.setlocale() == 'C')
            assert(os.setlocale(nil, 'numeric') == 'C')
            assert(os.setlocale('C') == 'C')
            assert(os.setlocale('POSIX', 'time') == 'C')
            "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn other_locales_return_nil_and_change_nothing() {
        let lua = lua();
        lua.load(
            r#"
            -- "" asks for the locale of the environment.
            for _, name in ipairs({ 'de_DE.UTF-8', 'fr_FR', 'C.UTF-8', '' }) do
                assert(os.setlocale(name) == nil, name)
                assert(os.setlocale(name, 'numeric') == nil, name)
            end
            assert(os.setlocale() == 'C')
            assert(string.format('%.1f', 1.5) == '1.5')
            assert(tostring(1.5) == '1.5')
            "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn a_bad_category_raises_as_in_stock_lua() {
        let lua = lua();
        let err = lua
            .load("return os.setlocale('C', 'nonsense')")
            .exec()
            .unwrap_err();
        assert!(err.to_string().contains("invalid option"), "{err}");
    }

    #[test]
    fn a_state_without_setlocale_stays_without_one() {
        let lua = Lua::new();
        lua.load("os.setlocale = nil").exec().unwrap();
        install_os_setlocale(&lua).unwrap();
        assert!(
            lua.load("return os.setlocale == nil")
                .eval::<bool>()
                .unwrap()
        );
    }
}
