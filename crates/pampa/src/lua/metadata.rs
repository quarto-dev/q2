/*
 * lua/metadata.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * quarto.metadata.get and the `meta` proxy.
 */

//! Document metadata from Lua: `quarto.metadata.get(key)` and the `meta`
//! global.
//!
//! `quarto.metadata.get("a.b")` is Quarto 1's API (quarto-cli PR 13246):
//! the value at a dotted key, in the same native shapes a filter's `Meta`
//! handler sees (`push_config_value`: strings, numbers, booleans, `Inlines`
//! and `Blocks` lists, `pandoc.List`s, plain tables), or `nil` when the
//! key or any part of it is missing. q2 extends the path syntax with
//! numeric segments that index arrays 1-based (`"authors.1.name"`), the
//! way the shortcode engine's `meta["author.1"]` already does.
//!
//! `meta` is the same lookup as a global object you navigate instead of
//! spelling a key: `meta.a.b` is `quarto.metadata.get("a.b")`, and
//! `meta.authors[1].name` is `quarto.metadata.get("authors.1.name")`. The
//! two differ in one deliberate way:
//!
//! - a **map** reached through `meta` is a lazy, read-only *proxy* (this
//!   module's [`LuaMetaProxy`]), so navigation keeps going and a write
//!   raises an error instead of silently going nowhere;
//! - an **array** is a real `pandoc.List` of its converted items (map items
//!   are proxies again), because arrays are *iterated*: `#`, `ipairs`,
//!   `table.concat` and `List:map` all want a real table. A proxy with
//!   `__len`/`__index` could fake most of that but never `List` methods.
//!
//! Everything else (scalars, `Inlines`, `Blocks`) is the leaf value. Note
//! that a YAML string in *document* metadata is markdown and arrives as an
//! `Inlines` list, as pandoc's `MetaInlines` would; `pandoc.utils.stringify`
//! gets the text. A missing key is `nil` at that point, so `meta.missing.inner` raises the
//! usual "attempt to index a nil value"; use `quarto.metadata.get("missing.inner")`
//! when a whole chain may be absent.

use std::rc::Rc;

use mlua::{Error, Lua, MetaMethod, Result, Table, UserData, UserDataMethods, Value};
use quarto_pandoc_types::config_value::{ConfigValue, ConfigValueKind};

use super::config_value::push_config_value;
use super::list::create_list_table;

/// One step of a metadata path: a map key or a 1-based array index.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seg {
    Key(String),
    Index(usize),
}

impl Seg {
    /// How a dotted-string segment is read against the node it indexes:
    /// a map always takes it as a key (so `"2024"` reaches a map key
    /// `2024`), an array takes it as a 1-based index when it parses.
    fn from_str_for(node: &ConfigValue, part: &str) -> Option<Seg> {
        match &node.value {
            ConfigValueKind::Array(_) => part.parse::<usize>().ok().map(Seg::Index),
            _ => Some(Seg::Key(part.to_string())),
        }
    }
}

/// Follow `path` down from `root`.
fn lookup<'a>(root: &'a ConfigValue, path: &[Seg]) -> Option<&'a ConfigValue> {
    let mut cur = root;
    for seg in path {
        cur = match (seg, &cur.value) {
            (Seg::Key(k), ConfigValueKind::Map(_)) => cur.get(k)?,
            (Seg::Index(i), ConfigValueKind::Array(items)) => items.get(i.checked_sub(1)?)?,
            _ => return None,
        };
    }
    Some(cur)
}

/// Follow a dotted key (`"a.b.1.c"`) down from `root`.
fn lookup_dotted<'a>(root: &'a ConfigValue, key: &str) -> Option<&'a ConfigValue> {
    let mut cur = root;
    for part in key.split('.') {
        let seg = Seg::from_str_for(cur, part)?;
        cur = lookup(cur, std::slice::from_ref(&seg))?;
    }
    Some(cur)
}

/// The `meta` object and every map reached through it.
pub struct LuaMetaProxy {
    root: Rc<ConfigValue>,
    path: Vec<Seg>,
}

impl LuaMetaProxy {
    fn node(&self) -> Option<&ConfigValue> {
        lookup(&self.root, &self.path)
    }

    fn child(&self, seg: Seg) -> LuaMetaProxy {
        let mut path = self.path.clone();
        path.push(seg);
        LuaMetaProxy {
            root: self.root.clone(),
            path,
        }
    }

    fn display_path(&self) -> String {
        let mut s = String::from("meta");
        for seg in &self.path {
            match seg {
                Seg::Key(k) => {
                    s.push('.');
                    s.push_str(k);
                }
                Seg::Index(i) => s.push_str(&format!("[{i}]")),
            }
        }
        s
    }
}

/// Convert the value at `proxy.path + seg` for `meta` navigation: maps
/// become proxies, arrays become `pandoc.List`s of converted items, the
/// rest is the leaf value.
fn push_navigable(lua: &Lua, proxy: &LuaMetaProxy, seg: Seg) -> Result<Value> {
    let child = proxy.child(seg);
    let Some(node) = child.node() else {
        return Ok(Value::Nil);
    };
    match &node.value {
        ConfigValueKind::Map(_) => Ok(Value::UserData(lua.create_userdata(child)?)),
        ConfigValueKind::Array(items) => {
            let values = (1..=items.len())
                .map(|i| push_navigable(lua, &child, Seg::Index(i)))
                .collect::<Result<Vec<_>>>()?;
            create_list_table(lua, values)
        }
        _ => push_config_value(lua, node),
    }
}

impl UserData for LuaMetaProxy {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: Value| {
            let seg = match &key {
                Value::String(s) => Seg::Key(s.to_str()?.to_string()),
                Value::Integer(i) if *i >= 1 => Seg::Index(*i as usize),
                _ => return Ok(Value::Nil),
            };
            push_navigable(lua, this, seg)
        });
        methods.add_meta_method(
            MetaMethod::NewIndex,
            |_, this, (key, _value): (Value, Value)| {
                Err::<(), _>(Error::runtime(format!(
                    "{} is read-only: document metadata cannot be assigned from Lua (key {:?})",
                    this.display_path(),
                    key
                )))
            },
        );
        methods.add_meta_method(MetaMethod::Len, |_, this, ()| {
            Ok(match this.node().map(|n| &n.value) {
                Some(ConfigValueKind::Map(entries)) => entries.len(),
                _ => 0,
            })
        });
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| Ok(this.display_path()));
        // `pairs(meta.x)`: keys in document order, values converted as
        // navigation would convert them.
        methods.add_meta_method(MetaMethod::Pairs, |lua, this, ()| {
            let keys: Vec<String> = match this.node().map(|n| &n.value) {
                Some(ConfigValueKind::Map(entries)) => {
                    entries.iter().map(|e| e.key.clone()).collect()
                }
                _ => Vec::new(),
            };
            let values = keys
                .iter()
                .map(|k| push_navigable(lua, this, Seg::Key(k.clone())))
                .collect::<Result<Vec<_>>>()?;
            // Materialize into a plain table of the converted values and
            // hand Lua's `next` over it; the ordering of `pairs` is not
            // guaranteed by Lua anyway.
            let snapshot = lua.create_table()?;
            for (k, v) in keys.iter().zip(values) {
                snapshot.set(k.as_str(), v)?;
            }
            let next: mlua::Function = lua.globals().get("next")?;
            Ok((next, snapshot, Value::Nil))
        });
    }
}

/// Register `quarto.metadata.get` and the `meta` global for `meta`.
///
/// Both read a snapshot of `meta` taken now; a filter that rewrites
/// metadata sees its own changes through its `Meta` handler, not here.
pub fn register_quarto_metadata(lua: &Lua, meta: &ConfigValue) -> Result<()> {
    let root = Rc::new(meta.clone());

    let quarto: Table = lua.globals().get("quarto")?;
    let metadata = lua.create_table()?;
    let get_root = root.clone();
    metadata.set(
        "get",
        lua.create_function(
            move |lua, key: String| match lookup_dotted(&get_root, &key) {
                Some(node) => push_config_value(lua, node),
                None => Ok(Value::Nil),
            },
        )?,
    )?;
    quarto.set("metadata", metadata)?;

    lua.globals().set(
        "meta",
        lua.create_userdata(LuaMetaProxy {
            root,
            path: Vec::new(),
        })?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "---\ntitle: 'A *fancy* title'\nn: 3\nflag: true\nstuff: [a, b, c]\nauthor:\n  name: Ada\n  affil:\n    org: Analytical\nauthors:\n  - name: Ada\n    tags: [x, y]\n  - name: Bob\n'2024': year\n---\n\nBody.\n";

    fn env() -> Lua {
        let lua = Lua::new();
        super::super::constructors::register_pandoc_namespace(
            &lua,
            std::sync::Arc::new(super::super::runtime::NativeRuntime::new()),
            super::super::mediabag::create_shared_mediabag(),
        )
        .unwrap();
        super::super::quarto_api::register_quarto_api(&lua).unwrap();
        let mut sink: Vec<u8> = Vec::new();
        let (doc, _ctx, _w) =
            crate::readers::qmd::read(DOC.as_bytes(), false, "doc.qmd", &mut sink, true, None)
                .expect("doc parses");
        register_quarto_metadata(&lua, &doc.meta).unwrap();
        lua
    }

    fn eval<T: mlua::FromLua>(lua: &Lua, src: &str) -> T {
        lua.load(format!("return {src}")).eval().unwrap()
    }

    /// `pandoc.utils.stringify(<src>)`: strings in document metadata are
    /// markdown, so they come back as Inlines.
    fn text(lua: &Lua, src: &str) -> String {
        eval(lua, &format!("pandoc.utils.stringify({src})"))
    }

    fn eval_err(lua: &Lua, src: &str) -> String {
        lua.load(format!("return {src}"))
            .eval::<Value>()
            .unwrap_err()
            .to_string()
    }

    // ----- quarto.metadata.get -----

    #[test]
    fn get_returns_leaf_values_in_native_shapes() {
        let lua = env();
        assert_eq!(eval::<i64>(&lua, "quarto.metadata.get('n')"), 3);
        assert!(eval::<bool>(&lua, "quarto.metadata.get('flag')"));
        assert_eq!(text(&lua, "quarto.metadata.get('title')"), "A fancy title");
        assert_eq!(
            eval::<String>(&lua, "pandoc.utils.type(quarto.metadata.get('title'))"),
            "Inlines"
        );
        // A plain YAML string is markdown too: Inlines, not a Lua string.
        assert_eq!(
            eval::<String>(
                &lua,
                "pandoc.utils.type(quarto.metadata.get('author.name'))"
            ),
            "Inlines"
        );
    }

    #[test]
    fn get_follows_dotted_keys_and_numeric_array_segments() {
        let lua = env();
        assert_eq!(
            text(&lua, "quarto.metadata.get('author.affil.org')"),
            "Analytical"
        );
        assert_eq!(text(&lua, "quarto.metadata.get('authors.2.name')"), "Bob");
        assert_eq!(text(&lua, "quarto.metadata.get('authors.1.tags.2')"), "y");
        // A numeric-looking segment is a key when the node is a map.
        assert_eq!(text(&lua, "quarto.metadata.get('2024')"), "year");
    }

    #[test]
    fn get_returns_nil_for_any_missing_part() {
        let lua = env();
        for key in [
            "nope",
            "author.nope",
            "nope.deeper",
            "authors.9.name",
            "n.x",
            "authors.x",
        ] {
            let v: Value = eval(&lua, &format!("quarto.metadata.get('{key}')"));
            assert!(matches!(v, Value::Nil), "{key}");
        }
    }

    #[test]
    fn get_returns_real_lists_and_plain_tables() {
        let lua = env();
        assert_eq!(eval::<i64>(&lua, "#quarto.metadata.get('stuff')"), 3);
        assert_eq!(
            eval::<String>(&lua, "pandoc.utils.type(quarto.metadata.get('stuff'))"),
            "List"
        );
        assert_eq!(text(&lua, "quarto.metadata.get('author').name"), "Ada");
        assert_eq!(
            eval::<String>(&lua, "type(quarto.metadata.get('author'))"),
            "table"
        );
    }

    // ----- meta -----

    #[test]
    fn meta_navigates_maps_lazily_and_agrees_with_get() {
        let lua = env();
        assert_eq!(text(&lua, "meta.author.affil.org"), "Analytical");
        assert_eq!(eval::<i64>(&lua, "meta.n"), 3);
        assert_eq!(text(&lua, "meta['2024']"), "year");
        assert_eq!(
            eval::<String>(&lua, "tostring(meta.author.affil)"),
            "meta.author.affil"
        );
        assert_eq!(
            text(&lua, "meta.title"),
            text(&lua, "quarto.metadata.get('title')")
        );
    }

    #[test]
    fn meta_arrays_are_real_lists_whose_map_items_are_proxies() {
        let lua = env();
        assert_eq!(eval::<i64>(&lua, "#meta.stuff"), 3);
        assert_eq!(
            eval::<String>(&lua, "pandoc.utils.type(meta.stuff)"),
            "List"
        );
        assert_eq!(
            eval::<String>(&lua, "pandoc.utils.type(meta.stuff[1])"),
            "Inlines"
        );
        // pandoc.List methods work directly: no helper needed.
        assert_eq!(
            eval::<String>(
                &lua,
                "table.concat(meta.stuff:map(pandoc.utils.stringify), ',')"
            ),
            "a,b,c"
        );
        assert_eq!(
            eval::<String>(
                &lua,
                "table.concat(quarto.metadata.get('stuff'):map(pandoc.utils.stringify), ',')"
            ),
            "a,b,c"
        );
        assert_eq!(text(&lua, "meta.authors[1].name"), "Ada");
        assert_eq!(text(&lua, "meta.authors[1].tags[2]"), "y");
        assert_eq!(
            eval::<String>(&lua, "tostring(meta.authors[2])"),
            "meta.authors[2]"
        );
        let n: i64 = eval(
            &lua,
            "(function() local c = 0; for _, a in ipairs(meta.authors) do c = c + #pandoc.utils.stringify(a.name) end; return c end)()",
        );
        assert_eq!(n, 6);
    }

    #[test]
    fn meta_missing_keys_are_nil_and_writes_are_errors() {
        let lua = env();
        let v: Value = eval(&lua, "meta.nope");
        assert!(matches!(v, Value::Nil));
        let v: Value = eval(&lua, "meta.authors[9]");
        assert!(matches!(v, Value::Nil));
        let err = eval_err(&lua, "(function() meta.author.name = 'x' end)()");
        assert!(err.contains("meta.author is read-only"), "{err}");
        let err = eval_err(&lua, "(function() meta.new = 1 end)()");
        assert!(err.contains("meta is read-only"), "{err}");
    }

    #[test]
    fn meta_pairs_and_len_and_type() {
        let lua = env();
        assert_eq!(eval::<i64>(&lua, "#meta.author"), 2);
        let keys: String = eval(
            &lua,
            "(function() local ks = {}; for k in pairs(meta.author) do ks[#ks+1] = k end; table.sort(ks); return table.concat(ks, ',') end)()",
        );
        assert_eq!(keys, "affil,name");
        assert_eq!(eval::<String>(&lua, "pandoc.utils.type(meta)"), "Meta");
        assert_eq!(
            eval::<String>(&lua, "pandoc.utils.type(meta.author)"),
            "Meta"
        );
    }
}
