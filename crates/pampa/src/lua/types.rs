/*
 * lua/types.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Lua userdata wrappers for Pandoc AST types.
 *
 * These wrappers expose Pandoc elements as Lua userdata with named field access,
 * matching Pandoc 2.17+ behavior where `type(elem)` returns "userdata".
 */

use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

use mlua::{
    Error, IntoLua, Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods,
    UserDataRef, Value, Variadic,
};
use quarto_source_map::{By, SourceInfo};
use smallvec::SmallVec;

use crate::pandoc::{Block, Inline};

/// Wrapper for Pandoc Inline elements as Lua userdata.
///
/// The inner `Inline` lives behind `Rc<RefCell<…>>` so proxy userdata
/// (for `elem.attr`, `elem.attr.attributes`, `elem.attr.classes`) can
/// share ownership of the same cell and write back through the chain.
/// See `claude-notes/plans/2026-04-21-lua-attr-mutation-proxy.md`.
///
/// `FromLua` and the `clone()` Lua method each produce a fresh,
/// independent cell (deep-clone of the inner value) to preserve today's
/// per-invocation isolation semantics across filter boundaries.
/// hslua-style property cache (bd-hitjclzp).
///
/// Pandoc's Lua bridge caches every pushed property value in the
/// userdata's uservalue: repeated reads of `div.content` alias the
/// *same* Lua table, and marshaling the element back to the host
/// re-reads ("flushes") the cached values through the property
/// setters. That is what makes the idiomatic in-place mutation
/// pattern — `div.content:insert(x); return div` — actually persist.
///
/// q2 replicates this with a per-element cache mapping property name →
/// the exact Lua value handed out. Only properties on the
/// [`is_cacheable_property`] allowlist participate (they need reliable
/// setters for the flush); everything else keeps snapshot semantics.
/// Clones of the userdata share the cache (they also share the value
/// cell), so aliasing stays consistent.
///
/// The cached [`Value`] handles keep their Lua values alive from Rust;
/// they are released when the userdata is collected (drop chain), so
/// no uncollectable cycle survives the element itself.
#[derive(Debug, Clone, Default)]
pub struct PropertyCache(Rc<RefCell<PropertyCacheInner>>);

#[derive(Debug, Default)]
struct PropertyCacheInner {
    /// Reentrancy guard: a flush that (pathologically) reaches itself
    /// again — e.g. an element inserted into its own content — treats
    /// the inner occurrence as a snapshot instead of recursing forever.
    flushing: bool,
    entries: Vec<(String, Value)>,
}

impl PropertyCache {
    pub(crate) fn get(&self, key: &str) -> Option<Value> {
        self.0
            .borrow()
            .entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }

    pub(crate) fn store(&self, key: &str, value: &Value) {
        let mut inner = self.0.borrow_mut();
        if let Some(slot) = inner.entries.iter_mut().find(|(k, _)| k == key) {
            slot.1 = value.clone();
        } else {
            inner.entries.push((key.to_string(), value.clone()));
        }
    }

    pub(crate) fn remove(&self, key: &str) {
        self.0.borrow_mut().entries.retain(|(k, _)| k != key);
    }

    /// Start a flush: returns the entries to write back, or `None`
    /// when there is nothing to do (empty, or already flushing).
    pub(crate) fn begin_flush(&self) -> Option<Vec<(String, Value)>> {
        let mut inner = self.0.borrow_mut();
        if inner.flushing || inner.entries.is_empty() {
            return None;
        }
        inner.flushing = true;
        Some(inner.entries.clone())
    }

    pub(crate) fn end_flush(&self) {
        self.0.borrow_mut().flushing = false;
    }
}

/// Properties that participate in hslua-style caching. Each needs a
/// reliable `set_field` implementation on every element that exposes
/// it, because the flush writes the cached value back through it.
/// (`attr` is also cached, but as userdata with a dedicated flush
/// path — see `flush_cached_attr_entry`.)
fn is_cacheable_property(key: &str) -> bool {
    matches!(
        key,
        "content" | "citations" | "caption" | "classes" | "bodies" | "colspecs"
    )
}

/// Should this (key, value) pair be cached on an element? Container
/// tables for the allowlisted properties, plus the `attr` LuaAttr
/// userdata (so `el.attr` reads alias and nested attr mutations
/// survive to the flush) and the OrderedList `listAttributes`
/// userdata (same aliasing rationale; a raw-triple table assignment
/// is cached too, matching hslua — the aliases then read through it).
fn should_cache_element_property(key: &str, value: &Value) -> bool {
    (is_cacheable_property(key) && matches!(value, Value::Table(_)))
        || (matches!(key, "attr" | "listAttributes" | "head" | "foot" | "caption")
            && matches!(value, Value::UserData(_) | Value::Table(_)))
}

/// Resolve an OrderedList list-attribute alias (`start`/`style`/
/// `delimiter`) through the element's cached `listAttributes` value,
/// mirroring hslua's alias mechanism: the alias path reads the
/// property, so it observes the cached value — the live userdata, or
/// a user-assigned raw triple table (string-indexed, usually nil,
/// until a flush re-peeks the positional triple). Returns `Ok(None)`
/// when there is no cache entry (caller reads the element directly).
fn ordered_list_alias_get(cache: &PropertyCache, lua: &Lua, key: &str) -> Result<Option<Value>> {
    match cache.get("listAttributes") {
        None => Ok(None),
        Some(Value::UserData(ud)) => match ud.borrow::<super::constructors::LuaListAttributes>() {
            Ok(la) => la.get_field(lua, key).map(Some),
            Err(_) => Ok(None),
        },
        Some(Value::Table(t)) => t.get::<Value>(key).map(Some),
        Some(_) => Ok(None),
    }
}

/// Write an OrderedList list-attribute alias through the cached
/// `listAttributes` value when present (see [`ordered_list_alias_get`]).
/// Returns `Ok(false)` when there is no cache entry (caller writes to
/// the element directly).
fn ordered_list_alias_set(cache: &PropertyCache, lua: &Lua, key: &str, val: Value) -> Result<bool> {
    match cache.get("listAttributes") {
        None => Ok(false),
        Some(Value::UserData(ud)) => match ud.borrow::<super::constructors::LuaListAttributes>() {
            Ok(la) => {
                la.set_field(key, val, lua)?;
                Ok(true)
            }
            Err(_) => Ok(false),
        },
        Some(Value::Table(t)) => {
            t.set(key, val)?;
            Ok(true)
        }
        Some(_) => Ok(false),
    }
}

/// Flush one cached `attr` entry during an element flush. Returns
/// `Ok(true)` when the entry was fully handled here (the caller must
/// not run `set_field` for it).
///
/// A cached `LuaAttr` gets its own property cache flushed first (the
/// classes List table). If it is a live ref into `element_cell`'s own
/// Attr, writes have already landed — running the element's `attr`
/// setter would self-borrow. An *Owned* attr (or a ref to a different
/// element) is copied in via `apply`, matching Pandoc's
/// re-peek-at-flush semantics.
fn flush_cached_attr_entry(
    lua: &Lua,
    value: &Value,
    is_self_ref: impl FnOnce(&LuaAttr) -> bool,
    apply: impl FnOnce(crate::pandoc::Attr),
) -> Result<bool> {
    if let Value::UserData(ud) = value
        && let Ok(lua_attr) = ud.borrow::<LuaAttr>()
    {
        lua_attr.flush_property_cache(lua)?;
        if !is_self_ref(&lua_attr) {
            apply(lua_attr.clone_attr());
        }
        return Ok(true);
    }
    Ok(false)
}

#[derive(Debug, Clone)]
pub struct LuaInline(pub Rc<RefCell<Inline>>, pub PropertyCache);

impl LuaInline {
    /// Construct a `LuaInline` around a freshly-owned `Inline` in a new cell.
    pub fn new(inline: Inline) -> Self {
        LuaInline(Rc::new(RefCell::new(inline)), PropertyCache::default())
    }

    /// Write cached property values back into the inner cell (hslua
    /// "readback" semantics — see [`PropertyCache`]). Idempotent; call
    /// before any read of the inner value that must observe in-place
    /// mutations made through previously handed-out property tables.
    pub fn flush_property_cache(&self, lua: &Lua) -> Result<()> {
        let entries = match self.1.begin_flush() {
            Some(entries) => entries,
            None => return Ok(()),
        };
        let mut result = Ok(());
        for (key, value) in entries {
            let step = if key == "attr" {
                flush_cached_attr_entry(
                    lua,
                    &value,
                    |a| a.is_ref_to_inline(&self.0),
                    |tuple| {
                        let mut inner = self.0.borrow_mut();
                        if let Some(slot) = inline_attr_mut(&mut inner) {
                            *slot = tuple;
                        }
                    },
                )
                .and_then(|handled| {
                    if handled {
                        Ok(())
                    } else {
                        self.set_field(&key, value, lua)
                    }
                })
            } else {
                self.set_field(&key, value, lua)
            };
            if let Err(e) = step {
                result = Err(e);
                break;
            }
        }
        self.1.end_flush();
        result
    }

    /// Flush, then deep-clone the inner `Inline` — the blessed way to
    /// marshal a `LuaInline` back into a Rust AST value.
    pub fn extract_flushed(&self, lua: &Lua) -> Result<Inline> {
        self.flush_property_cache(lua)?;
        Ok(self.0.borrow().clone())
    }

    /// Borrow the inner `Inline` immutably.
    pub fn borrow_inline(&self) -> Ref<'_, Inline> {
        self.0.borrow()
    }

    /// Borrow the inner `Inline` mutably.
    pub fn borrow_inline_mut(&self) -> RefMut<'_, Inline> {
        self.0.borrow_mut()
    }

    /// Deep-clone the inner `Inline` into an owned value.
    pub fn clone_inline(&self) -> Inline {
        self.0.borrow().clone()
    }
}

impl LuaInline {
    /// Get the tag name for this inline element
    pub fn tag_name(&self) -> &'static str {
        match &*self.0.borrow() {
            Inline::Str(_) => "Str",
            Inline::Emph(_) => "Emph",
            Inline::Underline(_) => "Underline",
            Inline::Strong(_) => "Strong",
            Inline::Strikeout(_) => "Strikeout",
            Inline::Superscript(_) => "Superscript",
            Inline::Subscript(_) => "Subscript",
            Inline::SmallCaps(_) => "SmallCaps",
            Inline::Quoted(_) => "Quoted",
            Inline::Cite(_) => "Cite",
            Inline::Code(_) => "Code",
            Inline::Space(_) => "Space",
            Inline::SoftBreak(_) => "SoftBreak",
            Inline::LineBreak(_) => "LineBreak",
            Inline::Math(_) => "Math",
            Inline::RawInline(_) => "RawInline",
            Inline::Link(_) => "Link",
            Inline::Image(_) => "Image",
            Inline::Note(_) => "Note",
            Inline::Span(_) => "Span",
            Inline::Shortcode(_) => "Shortcode",
            Inline::NoteReference(_) => "NoteReference",
            Inline::Attr(_) => "Attr",
            Inline::Insert(_) => "Insert",
            Inline::Delete(_) => "Delete",
            Inline::Highlight(_) => "Highlight",
            Inline::EditComment(_) => "EditComment",
            Inline::Custom(_) => "Custom",
        }
    }

    /// Get the list of field names for this inline element (for pairs iteration)
    pub fn field_names(&self) -> &'static [&'static str] {
        match &*self.0.borrow() {
            Inline::Str(_) => &["tag", "text", "clone", "walk"],
            Inline::Emph(_)
            | Inline::Strong(_)
            | Inline::Underline(_)
            | Inline::Strikeout(_)
            | Inline::Superscript(_)
            | Inline::Subscript(_)
            | Inline::SmallCaps(_) => &["tag", "content", "clone", "walk"],
            Inline::Quoted(_) => &["tag", "quotetype", "content", "clone", "walk"],
            Inline::Cite(_) => &["tag", "content", "citations", "clone", "walk"],
            Inline::Code(_) => &[
                "tag",
                "text",
                "attr",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk",
            ],
            Inline::Space(_) | Inline::SoftBreak(_) | Inline::LineBreak(_) => {
                &["tag", "clone", "walk"]
            }
            Inline::Math(_) => &["tag", "mathtype", "text", "clone", "walk"],
            Inline::RawInline(_) => &["tag", "format", "text", "clone", "walk"],
            Inline::Link(_) => &[
                "tag",
                "content",
                "target",
                "title",
                "attr",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk",
            ],
            Inline::Image(_) => &[
                "tag",
                "content",
                "caption",
                "src",
                "title",
                "attr",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk",
            ],
            Inline::Note(_) => &["tag", "content", "clone", "walk"],
            Inline::Span(_) => &[
                "tag",
                "content",
                "attr",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk",
            ],
            Inline::Insert(_)
            | Inline::Delete(_)
            | Inline::Highlight(_)
            | Inline::EditComment(_) => &[
                "tag",
                "content",
                "attr",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk",
            ],
            Inline::NoteReference(_) => &["tag", "id", "clone", "walk"],
            Inline::Shortcode(_) | Inline::Attr(_) => &["tag", "clone", "walk"],
            // Custom nodes are not exposed to Lua filters yet
            Inline::Custom(_) => &["tag", "clone"],
        }
    }

    /// Get a field value by name
    pub fn get_field(&self, lua: &Lua, key: &str) -> Result<Value> {
        // Special cases that need the `Rc` (to stay shared with proxies) or
        // the userdata identity. Handle these before taking a `borrow()` so
        // the closure-captured value isn't tied to the borrow's lifetime.
        match key {
            "tag" | "t" => return self.tag_name().into_lua(lua),
            "source_info" => {
                // The Lua host binding for attribution
                // (`quarto.attribution.lookup(el)`) reads this. Snapshot
                // the SourceInfo so the userdata is independent of any
                // later mutation of the inline.
                let si = self.0.borrow().source_info().clone();
                return lua.create_userdata(LuaSourceInfo::new(si))?.into_lua(lua);
            }
            "clone" => {
                // Snapshot the inner inline at .clone-access time (matching
                // pre-refactor behavior). Each invocation of the returned
                // function produces an independent LuaInline (new cell).
                self.flush_property_cache(lua)?;
                let snapshot = self.0.borrow().clone();
                return lua
                    .create_function(move |lua, ()| {
                        lua.create_userdata(LuaInline::new(snapshot.clone()))
                    })?
                    .into_lua(lua);
            }
            "walk" => {
                return lua
                    .create_async_function(
                        |lua, (ud, filter_table): (UserDataRef<LuaInline>, Table)| async move {
                            // Snapshot to an owned Inline before awaiting so
                            // we don't hold a RefCell borrow across the await.
                            ud.flush_property_cache(&lua)?;
                            let snapshot = ud.0.borrow().clone();
                            let filtered =
                                walk_inline_with_filter(&lua, &snapshot, &filter_table).await?;
                            lua.create_userdata(LuaInline::new(filtered))
                        },
                    )?
                    .into_lua(lua);
            }
            _ => {}
        }

        let inner = self.0.borrow();
        match (&*inner, key) {
            // Str
            (Inline::Str(s), "text") => s.text.clone().into_lua(lua),

            // Content-bearing inlines (Emph, Strong, etc.)
            (Inline::Emph(e), "content") => inlines_to_lua_table(lua, &e.content),
            (Inline::Strong(s), "content") => inlines_to_lua_table(lua, &s.content),
            (Inline::Underline(u), "content") => inlines_to_lua_table(lua, &u.content),
            (Inline::Strikeout(s), "content") => inlines_to_lua_table(lua, &s.content),
            (Inline::Superscript(s), "content") => inlines_to_lua_table(lua, &s.content),
            (Inline::Subscript(s), "content") => inlines_to_lua_table(lua, &s.content),
            (Inline::SmallCaps(s), "content") => inlines_to_lua_table(lua, &s.content),
            (Inline::Span(s), "content") => inlines_to_lua_table(lua, &s.content),

            // Quoted
            (Inline::Quoted(q), "content") => inlines_to_lua_table(lua, &q.content),
            (Inline::Quoted(q), "quotetype") => {
                let qt = match q.quote_type {
                    crate::pandoc::QuoteType::SingleQuote => "SingleQuote",
                    crate::pandoc::QuoteType::DoubleQuote => "DoubleQuote",
                };
                qt.into_lua(lua)
            }

            // Code
            (Inline::Code(c), "text") => c.text.clone().into_lua(lua),
            (Inline::Code(_), "attr") => attr_to_lua_userdata_for_inline(lua, Rc::clone(&self.0)),
            (Inline::Code(c), "identifier") => c.attr.0.clone().into_lua(lua),
            (Inline::Code(_), "classes") => classes_proxy_for_inline(lua, Rc::clone(&self.0)),
            (Inline::Code(_), "attributes") => attributes_proxy_for_inline(lua, Rc::clone(&self.0)),

            // Math
            (Inline::Math(m), "text") => m.text.clone().into_lua(lua),
            (Inline::Math(m), "mathtype") => {
                let mt = match m.math_type {
                    crate::pandoc::MathType::InlineMath => "InlineMath",
                    crate::pandoc::MathType::DisplayMath => "DisplayMath",
                };
                mt.into_lua(lua)
            }

            // RawInline
            (Inline::RawInline(r), "text") => r.text.clone().into_lua(lua),
            (Inline::RawInline(r), "format") => r.format.clone().into_lua(lua),

            // Link
            (Inline::Link(l), "content") => inlines_to_lua_table(lua, &l.content),
            (Inline::Link(l), "target") => l.target.0.clone().into_lua(lua),
            (Inline::Link(l), "title") => l.target.1.clone().into_lua(lua),
            (Inline::Link(_), "attr") => attr_to_lua_userdata_for_inline(lua, Rc::clone(&self.0)),
            (Inline::Link(l), "identifier") => l.attr.0.clone().into_lua(lua),
            (Inline::Link(_), "classes") => classes_proxy_for_inline(lua, Rc::clone(&self.0)),
            (Inline::Link(_), "attributes") => attributes_proxy_for_inline(lua, Rc::clone(&self.0)),

            // Image
            (Inline::Image(i), "content") => inlines_to_lua_table(lua, &i.content),
            // Pandoc name for the image description: `caption` is an
            // alias of content (Inline.hs possibleProperty "caption").
            (Inline::Image(i), "caption") => inlines_to_lua_table(lua, &i.content),
            (Inline::Image(i), "src") => i.target.0.clone().into_lua(lua),
            (Inline::Image(i), "title") => i.target.1.clone().into_lua(lua),
            (Inline::Image(_), "attr") => attr_to_lua_userdata_for_inline(lua, Rc::clone(&self.0)),
            (Inline::Image(img), "identifier") => img.attr.0.clone().into_lua(lua),
            (Inline::Image(_), "classes") => classes_proxy_for_inline(lua, Rc::clone(&self.0)),
            (Inline::Image(_), "attributes") => {
                attributes_proxy_for_inline(lua, Rc::clone(&self.0))
            }

            // Note
            (Inline::Note(n), "content") => blocks_to_lua_table(lua, &n.content),

            // Span (attr already covered above for other elements with attr)
            (Inline::Span(_), "attr") => attr_to_lua_userdata_for_inline(lua, Rc::clone(&self.0)),
            (Inline::Span(s), "identifier") => s.attr.0.clone().into_lua(lua),
            (Inline::Span(_), "classes") => classes_proxy_for_inline(lua, Rc::clone(&self.0)),
            (Inline::Span(_), "attributes") => attributes_proxy_for_inline(lua, Rc::clone(&self.0)),

            // Cite
            (Inline::Cite(c), "content") => inlines_to_lua_table(lua, &c.content),
            (Inline::Cite(c), "citations") => citations_to_lua_table(lua, &c.citations),

            // Insert (CriticMarkup-like)
            (Inline::Insert(ins), "content") => inlines_to_lua_table(lua, &ins.content),
            (Inline::Insert(_), "attr") => attr_to_lua_userdata_for_inline(lua, Rc::clone(&self.0)),
            (Inline::Insert(ins), "identifier") => ins.attr.0.clone().into_lua(lua),
            (Inline::Insert(_), "classes") => classes_proxy_for_inline(lua, Rc::clone(&self.0)),
            (Inline::Insert(_), "attributes") => {
                attributes_proxy_for_inline(lua, Rc::clone(&self.0))
            }

            // Delete (CriticMarkup-like)
            (Inline::Delete(d), "content") => inlines_to_lua_table(lua, &d.content),
            (Inline::Delete(_), "attr") => attr_to_lua_userdata_for_inline(lua, Rc::clone(&self.0)),
            (Inline::Delete(d), "identifier") => d.attr.0.clone().into_lua(lua),
            (Inline::Delete(_), "classes") => classes_proxy_for_inline(lua, Rc::clone(&self.0)),
            (Inline::Delete(_), "attributes") => {
                attributes_proxy_for_inline(lua, Rc::clone(&self.0))
            }

            // Highlight (CriticMarkup-like)
            (Inline::Highlight(h), "content") => inlines_to_lua_table(lua, &h.content),
            (Inline::Highlight(_), "attr") => {
                attr_to_lua_userdata_for_inline(lua, Rc::clone(&self.0))
            }
            (Inline::Highlight(h), "identifier") => h.attr.0.clone().into_lua(lua),
            (Inline::Highlight(_), "classes") => classes_proxy_for_inline(lua, Rc::clone(&self.0)),
            (Inline::Highlight(_), "attributes") => {
                attributes_proxy_for_inline(lua, Rc::clone(&self.0))
            }

            // EditComment (CriticMarkup-like)
            (Inline::EditComment(ec), "content") => inlines_to_lua_table(lua, &ec.content),
            (Inline::EditComment(_), "attr") => {
                attr_to_lua_userdata_for_inline(lua, Rc::clone(&self.0))
            }
            (Inline::EditComment(ec), "identifier") => ec.attr.0.clone().into_lua(lua),
            (Inline::EditComment(_), "classes") => {
                classes_proxy_for_inline(lua, Rc::clone(&self.0))
            }
            (Inline::EditComment(_), "attributes") => {
                attributes_proxy_for_inline(lua, Rc::clone(&self.0))
            }

            // NoteReference
            (Inline::NoteReference(nr), "id") => nr.id.clone().into_lua(lua),

            // tag, t, clone, walk were handled above the borrow.

            // Unknown field
            _ => Ok(Value::Nil),
        }
    }

    /// Set a field value by name
    ///
    /// Takes `&self` (not `&mut self`): interior mutability is provided by
    /// the `RefCell`. This is what lets proxies with their own `Rc` to the
    /// same cell route writes back.
    pub fn set_field(&self, key: &str, val: Value, lua: &Lua) -> Result<()> {
        let mut inner = self.0.borrow_mut();
        match (&mut *inner, key) {
            // Str
            (Inline::Str(s), "text") => {
                s.text = String::from_lua(val, lua)?;
                Ok(())
            }

            // Content-bearing inlines
            (Inline::Emph(e), "content") => {
                e.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }
            (Inline::Strong(s), "content") => {
                s.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }
            (Inline::Underline(u), "content") => {
                u.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }
            (Inline::Strikeout(s), "content") => {
                s.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }
            (Inline::Superscript(s), "content") => {
                s.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }
            (Inline::Subscript(s), "content") => {
                s.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }
            (Inline::SmallCaps(s), "content") => {
                s.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }
            (Inline::Span(s), "content") => {
                s.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }

            // Link
            (Inline::Link(l), "content") => {
                l.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }
            (Inline::Link(l), "target") => {
                l.target.0 = String::from_lua(val, lua)?;
                Ok(())
            }
            (Inline::Link(l), "title") => {
                l.target.1 = String::from_lua(val, lua)?;
                Ok(())
            }

            // Image
            (Inline::Image(i), "content") => {
                i.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }
            (Inline::Image(i), "caption") => {
                i.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }
            (Inline::Image(i), "src") => {
                i.target.0 = String::from_lua(val, lua)?;
                Ok(())
            }
            (Inline::Image(i), "title") => {
                i.target.1 = String::from_lua(val, lua)?;
                Ok(())
            }

            // Code
            (Inline::Code(c), "text") => {
                c.text = String::from_lua(val, lua)?;
                Ok(())
            }

            // RawInline
            (Inline::RawInline(r), "text") => {
                r.text = String::from_lua(val, lua)?;
                Ok(())
            }
            (Inline::RawInline(r), "format") => {
                r.format = String::from_lua(val, lua)?;
                Ok(())
            }

            // Math
            (Inline::Math(m), "text") => {
                m.text = String::from_lua(val, lua)?;
                Ok(())
            }
            (Inline::Math(m), "mathtype") => {
                let s = String::from_lua(val, lua)?;
                m.math_type = match s.as_str() {
                    "InlineMath" => crate::pandoc::MathType::InlineMath,
                    "DisplayMath" => crate::pandoc::MathType::DisplayMath,
                    other => {
                        return Err(Error::runtime(format!(
                            "invalid math type '{other}' (expected InlineMath or DisplayMath)"
                        )));
                    }
                };
                Ok(())
            }

            // Quoted
            (Inline::Quoted(q), "content") => {
                q.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }
            (Inline::Quoted(q), "quotetype") => {
                let s = String::from_lua(val, lua)?;
                q.quote_type = match s.as_str() {
                    "SingleQuote" => crate::pandoc::QuoteType::SingleQuote,
                    "DoubleQuote" => crate::pandoc::QuoteType::DoubleQuote,
                    other => {
                        return Err(Error::runtime(format!(
                            "invalid quote type '{other}' (expected SingleQuote or DoubleQuote)"
                        )));
                    }
                };
                Ok(())
            }

            // Note
            (Inline::Note(n), "content") => {
                n.content = peek_blocks_fuzzy(lua, val)?;
                Ok(())
            }

            // Span attr and convenience accessors
            (Inline::Span(s), "attr") => {
                s.attr = lua_value_to_attr(val, lua)?;
                Ok(())
            }
            (Inline::Span(s), "identifier") => {
                s.attr.0 = String::from_lua(val, lua)?;
                Ok(())
            }
            (Inline::Span(s), "classes") => {
                s.attr.1 = lua_table_to_strings(lua, val)?;
                Ok(())
            }

            // Code attr and convenience accessors
            (Inline::Code(c), "attr") => {
                c.attr = lua_value_to_attr(val, lua)?;
                Ok(())
            }
            (Inline::Code(c), "identifier") => {
                c.attr.0 = String::from_lua(val, lua)?;
                Ok(())
            }
            (Inline::Code(c), "classes") => {
                c.attr.1 = lua_table_to_strings(lua, val)?;
                Ok(())
            }

            // Link attr and convenience accessors
            (Inline::Link(l), "attr") => {
                l.attr = lua_value_to_attr(val, lua)?;
                Ok(())
            }
            (Inline::Link(l), "identifier") => {
                l.attr.0 = String::from_lua(val, lua)?;
                Ok(())
            }
            (Inline::Link(l), "classes") => {
                l.attr.1 = lua_table_to_strings(lua, val)?;
                Ok(())
            }

            // Image attr and convenience accessors
            (Inline::Image(i), "attr") => {
                i.attr = lua_value_to_attr(val, lua)?;
                Ok(())
            }
            (Inline::Image(i), "identifier") => {
                i.attr.0 = String::from_lua(val, lua)?;
                Ok(())
            }
            (Inline::Image(i), "classes") => {
                i.attr.1 = lua_table_to_strings(lua, val)?;
                Ok(())
            }

            // Cite
            (Inline::Cite(c), "content") => {
                c.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }
            (Inline::Cite(c), "citations") => {
                c.citations = lua_table_to_citations(lua, val)?;
                Ok(())
            }

            // Insert
            (Inline::Insert(ins), "content") => {
                ins.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }
            (Inline::Insert(ins), "attr") => {
                ins.attr = lua_value_to_attr(val, lua)?;
                Ok(())
            }
            (Inline::Insert(ins), "identifier") => {
                ins.attr.0 = String::from_lua(val, lua)?;
                Ok(())
            }
            (Inline::Insert(ins), "classes") => {
                ins.attr.1 = lua_table_to_strings(lua, val)?;
                Ok(())
            }

            // Delete
            (Inline::Delete(d), "content") => {
                d.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }
            (Inline::Delete(d), "attr") => {
                d.attr = lua_value_to_attr(val, lua)?;
                Ok(())
            }
            (Inline::Delete(d), "identifier") => {
                d.attr.0 = String::from_lua(val, lua)?;
                Ok(())
            }
            (Inline::Delete(d), "classes") => {
                d.attr.1 = lua_table_to_strings(lua, val)?;
                Ok(())
            }

            // Highlight
            (Inline::Highlight(h), "content") => {
                h.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }
            (Inline::Highlight(h), "attr") => {
                h.attr = lua_value_to_attr(val, lua)?;
                Ok(())
            }
            (Inline::Highlight(h), "identifier") => {
                h.attr.0 = String::from_lua(val, lua)?;
                Ok(())
            }
            (Inline::Highlight(h), "classes") => {
                h.attr.1 = lua_table_to_strings(lua, val)?;
                Ok(())
            }

            // EditComment
            (Inline::EditComment(ec), "content") => {
                ec.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }
            (Inline::EditComment(ec), "attr") => {
                ec.attr = lua_value_to_attr(val, lua)?;
                Ok(())
            }
            (Inline::EditComment(ec), "identifier") => {
                ec.attr.0 = String::from_lua(val, lua)?;
                Ok(())
            }
            (Inline::EditComment(ec), "classes") => {
                ec.attr.1 = lua_table_to_strings(lua, val)?;
                Ok(())
            }

            // NoteReference
            (Inline::NoteReference(nr), "id") => {
                nr.id = String::from_lua(val, lua)?;
                Ok(())
            }

            // Inline-level `.attributes` / `.classes` whole-assignment
            // shortcuts for any attr-bearing inline. See the equivalent
            // block-level arms for the rationale.
            (inline, "attributes") => match inline_attr_mut(inline) {
                Some(attr) => {
                    attr.2 = lua_table_to_string_map(lua, val)?;
                    Ok(())
                }
                None => Err(Error::runtime(
                    "cannot set 'attributes' on this inline variant",
                )),
            },
            (inline, "classes") => match inline_attr_mut(inline) {
                Some(attr) => {
                    attr.1 = lua_table_to_strings(lua, val)?;
                    Ok(())
                }
                None => Err(Error::runtime(
                    "cannot set 'classes' on this inline variant",
                )),
            },

            // Read-only fields
            (_, "tag" | "t") => Err(Error::runtime("cannot set read-only field 'tag'")),

            // Unknown field
            _ => Err(Error::runtime(format!("cannot set field '{}'", key))),
        }
    }
}

impl UserData for LuaInline {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        // Static fields accessible on all inlines
        fields.add_field_method_get("t", |_, this| Ok(this.tag_name()));
        fields.add_field_method_get("tag", |_, this| Ok(this.tag_name()));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // Dynamic field access via __index. Cacheable container
        // properties return the SAME Lua table on repeated reads
        // (hslua aliasing semantics — see PropertyCache).
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            if let Some(cached) = this.1.get(&key) {
                return Ok(cached);
            }
            let value = this.get_field(lua, &key)?;
            if should_cache_element_property(&key, &value) {
                this.1.store(&key, &value);
            }
            Ok(value)
        });

        // Dynamic field assignment via __newindex. Now uses add_meta_method
        // (not _mut) because interior mutability comes from the RefCell in
        // `LuaInline.0`; set_field takes `&self`.
        methods.add_meta_method(
            MetaMethod::NewIndex,
            |lua, this, (key, val): (String, Value)| {
                this.set_field(&key, val.clone(), lua)?;
                // Keep the assigned value aliased (hslua caches the
                // set value too); other assignments just drop any
                // stale cache entry.
                if should_cache_element_property(&key, &val) {
                    this.1.store(&key, &val);
                } else if is_cacheable_property(&key) || key == "attr" {
                    this.1.remove(&key);
                }
                Ok(())
            },
        );

        // Note: clone and walk are handled by get_field() rather than add_method()
        // to allow them to capture self in closures for direct function call syntax

        // __tostring: Haskell-show format, matching Pandoc's Lua API
        methods.add_meta_method(MetaMethod::ToString, |lua, this, ()| {
            this.flush_property_cache(lua)?;
            Ok(super::show::show_inline(&this.0.borrow()))
        });

        // __eq: structural equality ignoring source info, matching
        // Pandoc (where elements carry no source information at all).
        methods.add_meta_method(MetaMethod::Eq, |lua, this, other: Value| {
            Ok(match other {
                Value::UserData(ud) => match ud.borrow::<LuaInline>() {
                    Ok(other_inline) => {
                        this.flush_property_cache(lua)?;
                        other_inline.flush_property_cache(lua)?;
                        inline_structurally_eq(&this.0.borrow(), &other_inline.0.borrow())
                    }
                    Err(_) => false,
                },
                _ => false,
            })
        });

        // __pairs for iteration (for k, v in pairs(elem))
        methods.add_meta_method(MetaMethod::Pairs, |lua, this, ()| {
            // Snapshot the inline at pairs-call time (matching pre-refactor
            // behavior: iterating over a copy, not the live cell). Mutations
            // to the original during iteration are not observed — this also
            // avoids RefCell borrow conflicts if the filter mutates while
            // iterating.
            this.flush_property_cache(lua)?;
            let snapshot = this.0.borrow().clone();

            // Create the iterator function following Lua's next() semantics:
            // - If control variable is nil, return first key-value pair
            // - If control variable is a string key, return next key-value pair after it
            let stateless_iter =
                lua.create_function(move |lua, (ud, key): (UserDataRef<LuaInline>, Value)| {
                    let field_names = ud.field_names();

                    // Find the starting index
                    let start_idx = match key {
                        Value::Nil => 0,
                        Value::String(s) => {
                            let key_str = s.to_str()?;
                            // Find the index of the current key and add 1
                            if let Some(idx) = field_names.iter().position(|&k| key_str == k) {
                                idx + 1
                            } else {
                                // Key not found, end iteration
                                return Ok(Variadic::new());
                            }
                        }
                        Value::Integer(i) => {
                            // Support integer keys for iteration protocol compatibility
                            (i as usize) + 1
                        }
                        _ => return Ok(Variadic::new()),
                    };

                    if start_idx < field_names.len() {
                        let key = field_names[start_idx];
                        let value = ud.get_field(lua, key)?;
                        // Return (key, value) - key becomes the next control variable
                        Ok(Variadic::from_iter([key.into_lua(lua)?, value]))
                    } else {
                        Ok(Variadic::new())
                    }
                })?;

            // Return (iterator, state, initial value)
            // state is the userdata, initial value is nil (start from beginning)
            Ok((
                stateless_iter,
                lua.create_userdata(LuaInline::new(snapshot))?,
                Value::Nil,
            ))
        });
    }
}

/// Wrapper for [`SourceInfo`] exposed as Lua userdata.
///
/// Returned by `el.source_info` on Block and Inline userdata. Carries
/// `:byte_range()` and `:file_id()` accessors that chain-resolve the
/// underlying `SourceInfo` to a `(file_id, start, end)` tuple in the
/// root source file. Both return `nil` when the chain resolves to
/// `SourceInfo::Concat` or a `Generated` node without an `Invocation`
/// anchor — the same rule applied by `AttributionRenderTransform`.
///
/// This is the building block of the `quarto.attribution.lookup(el)`
/// convenience: it reads `el.source_info:byte_range()` then calls
/// `quarto.attribution.lookup_range` with the resolved offsets.
#[derive(Debug, Clone)]
pub struct LuaSourceInfo(pub SourceInfo);

impl LuaSourceInfo {
    pub fn new(si: SourceInfo) -> Self {
        Self(si)
    }
}

impl UserData for LuaSourceInfo {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // `:byte_range()` returns a Lua table `{start, end}` (1-indexed
        // positional, matching Pandoc convention for Range tables) or
        // `nil` when the SourceInfo chain doesn't resolve to a single
        // contiguous byte range.
        methods.add_method("byte_range", |lua, this, ()| {
            let Some((_fid, start, end)) = this.0.resolve_byte_range() else {
                return Ok(Value::Nil);
            };
            let t = lua.create_table()?;
            t.set("start", start)?;
            t.set("end_", end)?;
            // Positional access for callers that prefer it.
            t.set(1, start)?;
            t.set(2, end)?;
            Ok(Value::Table(t))
        });

        // `:file_id()` returns the integer file_id, or `nil` when the
        // chain doesn't resolve. Useful for callers that want to skip
        // non-primary-file nodes without re-deriving the rule.
        methods.add_method("file_id", |_, this, ()| {
            Ok(this.0.resolve_byte_range().map(|(fid, _, _)| fid))
        });

        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(match this.0.resolve_byte_range() {
                Some((fid, start, end)) => format!("SourceInfo({}, {}..{})", fid, start, end),
                None => "SourceInfo(unresolved)".to_string(),
            })
        });
    }
}

/// Wrapper for Pandoc Block elements as Lua userdata.
///
/// The inner `Block` lives behind `Rc<RefCell<…>>` so proxy userdata
/// for `.attr`, `.attr.attributes`, and `.attr.classes` can share
/// ownership of the same cell and propagate writes back. See
/// `claude-notes/plans/2026-04-21-lua-attr-mutation-proxy.md`.
#[derive(Debug, Clone)]
pub struct LuaBlock(pub Rc<RefCell<Block>>, pub PropertyCache);

impl LuaBlock {
    /// Construct a `LuaBlock` around a freshly-owned `Block` in a new cell.
    pub fn new(block: Block) -> Self {
        LuaBlock(Rc::new(RefCell::new(block)), PropertyCache::default())
    }

    /// Write cached property values back into the inner cell (hslua
    /// "readback" semantics — see [`PropertyCache`]). Idempotent; call
    /// before any read of the inner value that must observe in-place
    /// mutations made through previously handed-out property tables.
    pub fn flush_property_cache(&self, lua: &Lua) -> Result<()> {
        let entries = match self.1.begin_flush() {
            Some(entries) => entries,
            None => return Ok(()),
        };
        let mut result = Ok(());
        for (key, value) in entries {
            let step = if key == "attr" {
                flush_cached_attr_entry(
                    lua,
                    &value,
                    |a| a.is_ref_to_block(&self.0),
                    |tuple| {
                        let mut inner = self.0.borrow_mut();
                        if let Some(slot) = block_attr_mut(&mut inner) {
                            *slot = tuple;
                        }
                    },
                )
                .and_then(|handled| {
                    if handled {
                        Ok(())
                    } else {
                        self.set_field(&key, value, lua)
                    }
                })
            } else {
                self.set_field(&key, value, lua)
            };
            if let Err(e) = step {
                result = Err(e);
                break;
            }
        }
        self.1.end_flush();
        result
    }

    /// Flush, then deep-clone the inner `Block` — the blessed way to
    /// marshal a `LuaBlock` back into a Rust AST value.
    pub fn extract_flushed(&self, lua: &Lua) -> Result<Block> {
        self.flush_property_cache(lua)?;
        Ok(self.0.borrow().clone())
    }

    /// Borrow the inner `Block` immutably.
    pub fn borrow_block(&self) -> Ref<'_, Block> {
        self.0.borrow()
    }

    /// Borrow the inner `Block` mutably.
    pub fn borrow_block_mut(&self) -> RefMut<'_, Block> {
        self.0.borrow_mut()
    }

    /// Deep-clone the inner `Block` into an owned value.
    pub fn clone_block(&self) -> Block {
        self.0.borrow().clone()
    }

    /// Get the tag name for this block element
    pub fn tag_name(&self) -> &'static str {
        match &*self.0.borrow() {
            Block::Plain(_) => "Plain",
            Block::Paragraph(_) => "Para",
            Block::LineBlock(_) => "LineBlock",
            Block::CodeBlock(_) => "CodeBlock",
            Block::RawBlock(_) => "RawBlock",
            Block::BlockQuote(_) => "BlockQuote",
            Block::OrderedList(_) => "OrderedList",
            Block::BulletList(_) => "BulletList",
            Block::DefinitionList(_) => "DefinitionList",
            Block::Header(_) => "Header",
            Block::HorizontalRule(_) => "HorizontalRule",
            Block::Table(_) => "Table",
            Block::Figure(_) => "Figure",
            Block::Div(_) => "Div",
            Block::BlockMetadata(_) => "BlockMetadata",
            Block::NoteDefinitionPara(_) => "NoteDefinitionPara",
            Block::NoteDefinitionFencedBlock(_) => "NoteDefinitionFencedBlock",
            Block::CaptionBlock(_) => "CaptionBlock",
            Block::Custom(_) => "Custom",
        }
    }

    /// Get the list of field names for this block element (for pairs iteration)
    pub fn field_names(&self) -> &'static [&'static str] {
        match &*self.0.borrow() {
            Block::Plain(_) | Block::Paragraph(_) => &["tag", "content", "clone", "walk"],
            Block::LineBlock(_) => &["tag", "content", "clone", "walk"],
            Block::CodeBlock(_) => &[
                "tag",
                "text",
                "attr",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk",
            ],
            Block::RawBlock(_) => &["tag", "format", "text", "clone", "walk"],
            Block::BlockQuote(_) => &["tag", "content", "clone", "walk"],
            Block::OrderedList(_) => &[
                "tag",
                "content",
                "listAttributes",
                "start",
                "style",
                "delimiter",
                "clone",
                "walk",
            ],
            Block::BulletList(_) => &["tag", "content", "clone", "walk"],
            Block::DefinitionList(_) => &["tag", "content", "clone", "walk"],
            Block::Header(_) => &[
                "tag",
                "level",
                "content",
                "attr",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk",
            ],
            Block::HorizontalRule(_) => &["tag", "clone", "walk"],
            Block::Table(_) => &[
                "tag",
                "attr",
                "caption",
                "colspecs",
                "head",
                "bodies",
                "foot",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk",
            ],
            Block::Figure(_) => &[
                "tag",
                "content",
                "attr",
                "caption",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk",
            ],
            Block::Div(_) => &[
                "tag",
                "content",
                "attr",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk",
            ],
            Block::BlockMetadata(_)
            | Block::NoteDefinitionPara(_)
            | Block::NoteDefinitionFencedBlock(_)
            | Block::CaptionBlock(_) => &["tag", "clone", "walk"],
            // Custom nodes are not exposed to Lua filters yet
            Block::Custom(_) => &["tag", "clone"],
        }
    }

    /// Get a field value by name
    pub fn get_field(&self, lua: &Lua, key: &str) -> Result<Value> {
        // Handle tag, clone, walk up-front so we don't hold a borrow across
        // closure creation or async boundaries.
        match key {
            "tag" | "t" => return self.tag_name().into_lua(lua),
            "source_info" => {
                // See `LuaInline::get_field`'s matching branch for
                // the contract this powers (the attribution host
                // binding).
                let si = self.0.borrow().source_info().clone();
                return lua.create_userdata(LuaSourceInfo::new(si))?.into_lua(lua);
            }
            "clone" => {
                self.flush_property_cache(lua)?;
                let snapshot = self.0.borrow().clone();
                return lua
                    .create_function(move |lua, ()| {
                        lua.create_userdata(LuaBlock::new(snapshot.clone()))
                    })?
                    .into_lua(lua);
            }
            "walk" => {
                return lua
                    .create_async_function(
                        |lua, (ud, filter_table): (UserDataRef<LuaBlock>, Table)| async move {
                            ud.flush_property_cache(&lua)?;
                            let snapshot = ud.0.borrow().clone();
                            let filtered =
                                walk_block_with_filter(&lua, &snapshot, &filter_table).await?;
                            lua.create_userdata(LuaBlock::new(filtered))
                        },
                    )?
                    .into_lua(lua);
            }
            _ => {}
        }

        let inner = self.0.borrow();
        match (&*inner, key) {
            // Plain and Para have content
            (Block::Plain(p), "content") => inlines_to_lua_table(lua, &p.content),
            (Block::Paragraph(p), "content") => inlines_to_lua_table(lua, &p.content),

            // Header
            (Block::Header(h), "level") => (h.level as i64).into_lua(lua),
            (Block::Header(h), "content") => inlines_to_lua_table(lua, &h.content),
            (Block::Header(_), "attr") => attr_to_lua_userdata_for_block(lua, Rc::clone(&self.0)),
            (Block::Header(h), "identifier") => h.attr.0.clone().into_lua(lua),
            (Block::Header(_), "classes") => classes_proxy_for_block(lua, Rc::clone(&self.0)),
            (Block::Header(_), "attributes") => attributes_proxy_for_block(lua, Rc::clone(&self.0)),

            // CodeBlock
            (Block::CodeBlock(c), "text") => c.text.clone().into_lua(lua),
            (Block::CodeBlock(_), "attr") => {
                attr_to_lua_userdata_for_block(lua, Rc::clone(&self.0))
            }
            (Block::CodeBlock(c), "identifier") => c.attr.0.clone().into_lua(lua),
            (Block::CodeBlock(_), "classes") => classes_proxy_for_block(lua, Rc::clone(&self.0)),
            (Block::CodeBlock(_), "attributes") => {
                attributes_proxy_for_block(lua, Rc::clone(&self.0))
            }

            // RawBlock
            (Block::RawBlock(r), "text") => r.text.clone().into_lua(lua),
            (Block::RawBlock(r), "format") => r.format.clone().into_lua(lua),

            // BlockQuote
            (Block::BlockQuote(b), "content") => blocks_to_lua_table(lua, &b.content),

            // Div
            (Block::Div(d), "content") => blocks_to_lua_table(lua, &d.content),
            (Block::Div(_), "attr") => attr_to_lua_userdata_for_block(lua, Rc::clone(&self.0)),
            (Block::Div(d), "identifier") => d.attr.0.clone().into_lua(lua),
            (Block::Div(_), "classes") => classes_proxy_for_block(lua, Rc::clone(&self.0)),
            (Block::Div(_), "attributes") => attributes_proxy_for_block(lua, Rc::clone(&self.0)),

            // BulletList
            (Block::BulletList(b), "content") => {
                let items: Vec<Value> = b
                    .content
                    .iter()
                    .map(|blocks| blocks_to_lua_table(lua, blocks))
                    .collect::<Result<_>>()?;
                values_to_list_table(lua, items)
            }

            // OrderedList
            (Block::OrderedList(o), "content") => {
                let items: Vec<Value> = o
                    .content
                    .iter()
                    .map(|blocks| blocks_to_lua_table(lua, blocks))
                    .collect::<Result<_>>()?;
                values_to_list_table(lua, items)
            }
            (Block::OrderedList(o), "listAttributes") => {
                // Fresh userdata around a copy of the triple; the
                // Index metamethod caches it, so reads alias and
                // nested mutation persists via the flush.
                let ud = lua
                    .create_userdata(super::constructors::LuaListAttributes::new(o.attr.clone()))?;
                Ok(Value::UserData(ud))
            }
            // start/style/delimiter are hslua-style ALIASES into the
            // listAttributes property: when a cached listAttributes
            // value exists, they read through it (including the
            // pandoc quirk that a user-assigned raw triple table is
            // string-indexed, yielding nil until the flush re-peeks).
            (Block::OrderedList(o), "start") => {
                match ordered_list_alias_get(&self.1, lua, "start")? {
                    Some(v) => Ok(v),
                    None => (o.attr.0 as i64).into_lua(lua),
                }
            }
            (Block::OrderedList(o), "style") => {
                match ordered_list_alias_get(&self.1, lua, "style")? {
                    Some(v) => Ok(v),
                    None => super::constructors::list_number_style_name(&o.attr.1).into_lua(lua),
                }
            }
            (Block::OrderedList(o), "delimiter") => {
                match ordered_list_alias_get(&self.1, lua, "delimiter")? {
                    Some(v) => Ok(v),
                    None => super::constructors::list_number_delim_name(&o.attr.2).into_lua(lua),
                }
            }

            // Figure
            (Block::Figure(f), "content") => blocks_to_lua_table(lua, &f.content),
            (Block::Figure(_), "attr") => attr_to_lua_userdata_for_block(lua, Rc::clone(&self.0)),
            (Block::Figure(f), "identifier") => f.attr.0.clone().into_lua(lua),
            (Block::Figure(_), "classes") => classes_proxy_for_block(lua, Rc::clone(&self.0)),
            (Block::Figure(_), "attributes") => attributes_proxy_for_block(lua, Rc::clone(&self.0)),

            // LineBlock
            (Block::LineBlock(l), "content") => {
                let items: Vec<Value> = l
                    .content
                    .iter()
                    .map(|inlines| inlines_to_lua_table(lua, inlines))
                    .collect::<Result<_>>()?;
                values_to_list_table(lua, items)
            }

            // DefinitionList - list of (term, definitions) pairs
            (Block::DefinitionList(d), "content") => {
                let mut items = Vec::with_capacity(d.content.len());
                for (term, defs) in d.content.iter() {
                    let pair_table = lua.create_table()?;
                    // First element is the term (inlines)
                    pair_table.set(1, inlines_to_lua_table(lua, term)?)?;
                    // Second element is the definitions (list of blocks)
                    let def_values: Vec<Value> = defs
                        .iter()
                        .map(|def_blocks| blocks_to_lua_table(lua, def_blocks))
                        .collect::<Result<_>>()?;
                    pair_table.set(2, values_to_list_table(lua, def_values)?)?;
                    items.push(Value::Table(pair_table));
                }
                values_to_list_table(lua, items)
            }

            // Figure caption: Caption userdata (cached by the Index
            // metamethod so `fig.caption.long = …` persists via flush).
            (Block::Figure(f), "caption") => {
                let ud =
                    lua.create_userdata(super::constructors::LuaCaption::new(f.caption.clone()))?;
                Ok(Value::UserData(ud))
            }

            // Table basic fields
            (Block::Table(_), "attr") => attr_to_lua_userdata_for_block(lua, Rc::clone(&self.0)),
            (Block::Table(t), "caption") => {
                let ud =
                    lua.create_userdata(super::constructors::LuaCaption::new(t.caption.clone()))?;
                Ok(Value::UserData(ud))
            }
            (Block::Table(t), "head") => {
                let ud =
                    lua.create_userdata(super::constructors::LuaTableHead::new(t.head.clone()))?;
                Ok(Value::UserData(ud))
            }
            (Block::Table(t), "foot") => {
                let ud =
                    lua.create_userdata(super::constructors::LuaTableFoot::new(t.foot.clone()))?;
                Ok(Value::UserData(ud))
            }
            (Block::Table(t), "bodies") => {
                super::constructors::table_bodies_to_lua_list(lua, &t.bodies)
            }
            (Block::Table(t), "colspecs") => {
                super::constructors::colspecs_to_lua_table(lua, &t.colspec)
            }
            (Block::Table(t), "identifier") => t.attr.0.clone().into_lua(lua),
            (Block::Table(_), "classes") => classes_proxy_for_block(lua, Rc::clone(&self.0)),
            (Block::Table(_), "attributes") => attributes_proxy_for_block(lua, Rc::clone(&self.0)),

            // tag, t, clone, walk handled above the borrow.

            // Unknown field
            _ => Ok(Value::Nil),
        }
    }

    /// Set a field value by name
    ///
    /// Takes `&self`: interior mutability is provided by the `RefCell`.
    pub fn set_field(&self, key: &str, val: Value, lua: &Lua) -> Result<()> {
        let mut inner = self.0.borrow_mut();
        match (&mut *inner, key) {
            // Plain and Para
            (Block::Plain(p), "content") => {
                p.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }
            (Block::Paragraph(p), "content") => {
                p.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }

            // Header
            (Block::Header(h), "level") => {
                h.level = i64::from_lua(val, lua)? as usize;
                Ok(())
            }
            (Block::Header(h), "content") => {
                h.content = peek_inlines_fuzzy(lua, val)?;
                Ok(())
            }
            (Block::Header(h), "identifier") => {
                h.attr.0 = String::from_lua(val, lua)?;
                Ok(())
            }

            // CodeBlock
            (Block::CodeBlock(c), "text") => {
                c.text = String::from_lua(val, lua)?;
                Ok(())
            }
            (Block::CodeBlock(c), "identifier") => {
                c.attr.0 = String::from_lua(val, lua)?;
                Ok(())
            }

            // RawBlock
            (Block::RawBlock(r), "text") => {
                r.text = String::from_lua(val, lua)?;
                Ok(())
            }
            (Block::RawBlock(r), "format") => {
                r.format = String::from_lua(val, lua)?;
                Ok(())
            }

            // BlockQuote
            (Block::BlockQuote(b), "content") => {
                b.content = peek_blocks_fuzzy(lua, val)?;
                Ok(())
            }

            // Div
            (Block::Div(d), "content") => {
                d.content = peek_blocks_fuzzy(lua, val)?;
                Ok(())
            }
            (Block::Div(d), "identifier") => {
                d.attr.0 = String::from_lua(val, lua)?;
                Ok(())
            }

            // Figure
            (Block::Figure(f), "content") => {
                f.content = peek_blocks_fuzzy(lua, val)?;
                Ok(())
            }
            (Block::Figure(f), "caption") => {
                f.caption = super::constructors::parse_caption(lua, Some(val))?;
                Ok(())
            }
            (Block::Table(t), "caption") => {
                t.caption = super::constructors::parse_caption(lua, Some(val))?;
                Ok(())
            }
            (Block::Table(t), "head") => {
                t.head = super::constructors::parse_table_head(lua, val)?;
                Ok(())
            }
            (Block::Table(t), "foot") => {
                t.foot = super::constructors::parse_table_foot(lua, val)?;
                Ok(())
            }
            (Block::Table(t), "bodies") => {
                t.bodies = super::constructors::parse_table_bodies(lua, val)?;
                Ok(())
            }
            (Block::Table(t), "colspecs") => {
                t.colspec = super::constructors::parse_colspecs(lua, val)?;
                Ok(())
            }

            // List-shaped blocks: `content` assignment re-parses the
            // items the same way the constructors do (Pandoc's
            // setBlockContent re-projection semantics; also required
            // by the PropertyCache flush).
            (Block::BulletList(b), "content") => {
                b.content = super::constructors::parse_list_items(lua, val)?;
                Ok(())
            }
            (Block::OrderedList(o), "content") => {
                o.content = super::constructors::parse_list_items(lua, val)?;
                Ok(())
            }
            (Block::OrderedList(o), "listAttributes") => {
                o.attr = super::constructors::parse_list_attributes(val)?;
                Ok(())
            }
            // Aliases write through the cached listAttributes value
            // when present (hslua alias semantics), else directly
            // into the element's triple.
            (Block::OrderedList(o), "start") => {
                if !ordered_list_alias_set(&self.1, lua, "start", val.clone())? {
                    o.attr.0 = i64::from_lua(val, lua)? as usize;
                }
                Ok(())
            }
            (Block::OrderedList(o), "style") => {
                if !ordered_list_alias_set(&self.1, lua, "style", val.clone())? {
                    let s = String::from_lua(val, lua)?;
                    o.attr.1 = super::constructors::parse_list_number_style(&s)?;
                }
                Ok(())
            }
            (Block::OrderedList(o), "delimiter") => {
                if !ordered_list_alias_set(&self.1, lua, "delimiter", val.clone())? {
                    let s = String::from_lua(val, lua)?;
                    o.attr.2 = super::constructors::parse_list_number_delim(&s)?;
                }
                Ok(())
            }
            (Block::DefinitionList(d), "content") => {
                d.content = super::constructors::parse_definition_list_items(lua, val)?;
                Ok(())
            }
            (Block::LineBlock(l), "content") => {
                l.content = super::constructors::parse_line_block_content(lua, val)?;
                Ok(())
            }
            (Block::Figure(f), "identifier") => {
                f.attr.0 = String::from_lua(val, lua)?;
                Ok(())
            }
            (Block::Figure(f), "attr") => {
                f.attr = lua_value_to_attr(val, lua)?;
                Ok(())
            }

            // Header attr
            (Block::Header(h), "attr") => {
                h.attr = lua_value_to_attr(val, lua)?;
                Ok(())
            }

            // CodeBlock attr
            (Block::CodeBlock(c), "attr") => {
                c.attr = lua_value_to_attr(val, lua)?;
                Ok(())
            }

            // Div attr
            (Block::Div(d), "attr") => {
                d.attr = lua_value_to_attr(val, lua)?;
                Ok(())
            }

            // Table attr
            (Block::Table(t), "attr") => {
                t.attr = lua_value_to_attr(val, lua)?;
                Ok(())
            }
            (Block::Table(t), "identifier") => {
                t.attr.0 = String::from_lua(val, lua)?;
                Ok(())
            }

            // Block-level `.attributes` / `.classes` whole-assignment
            // shortcuts for any attr-bearing block. Matches Pandoc's
            // native Lua API (`elem.attributes = {…}`). Piecewise
            // writes (`elem.attributes["k"] = v`) are handled by the
            // LuaAttributesProxy / LuaClassesProxy `__newindex`.
            // NB: can't call `self.tag_name()` in the Err branch
            // because `inner` still holds a borrow on `self.0`.
            (block, "attributes") => match block_attr_mut(block) {
                Some(attr) => {
                    attr.2 = lua_table_to_string_map(lua, val)?;
                    Ok(())
                }
                None => Err(Error::runtime(
                    "cannot set 'attributes' on this block variant",
                )),
            },
            (block, "classes") => match block_attr_mut(block) {
                Some(attr) => {
                    attr.1 = lua_table_to_strings(lua, val)?;
                    Ok(())
                }
                None => Err(Error::runtime("cannot set 'classes' on this block variant")),
            },

            // Read-only fields
            (_, "tag" | "t") => Err(Error::runtime("cannot set read-only field 'tag'")),

            // Unknown field
            _ => Err(Error::runtime(format!("cannot set field '{}'", key))),
        }
    }
}

impl UserData for LuaBlock {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        // Static fields accessible on all blocks
        fields.add_field_method_get("t", |_, this| Ok(this.tag_name()));
        fields.add_field_method_get("tag", |_, this| Ok(this.tag_name()));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // Dynamic field access via __index. Cacheable container
        // properties return the SAME Lua table on repeated reads
        // (hslua aliasing semantics — see PropertyCache).
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            if let Some(cached) = this.1.get(&key) {
                return Ok(cached);
            }
            let value = this.get_field(lua, &key)?;
            if should_cache_element_property(&key, &value) {
                this.1.store(&key, &value);
            }
            Ok(value)
        });

        // Dynamic field assignment via __newindex. Uses add_meta_method
        // (not _mut) because mutation goes through the RefCell in
        // `LuaBlock.0`; set_field takes `&self`.
        methods.add_meta_method(
            MetaMethod::NewIndex,
            |lua, this, (key, val): (String, Value)| {
                this.set_field(&key, val.clone(), lua)?;
                // Keep the assigned value aliased (hslua caches the
                // set value too); other assignments just drop any
                // stale cache entry.
                if should_cache_element_property(&key, &val) {
                    this.1.store(&key, &val);
                } else if is_cacheable_property(&key)
                    || matches!(key.as_str(), "attr" | "listAttributes" | "head" | "foot")
                {
                    this.1.remove(&key);
                }
                Ok(())
            },
        );

        // Note: clone and walk are handled by get_field() rather than add_method()
        // to allow them to capture self in closures for direct function call syntax

        // __tostring: Haskell-show format, matching Pandoc's Lua API
        methods.add_meta_method(MetaMethod::ToString, |lua, this, ()| {
            this.flush_property_cache(lua)?;
            Ok(super::show::show_block(&this.0.borrow()))
        });

        // __eq: structural equality ignoring source info, matching
        // Pandoc (where elements carry no source information at all).
        methods.add_meta_method(MetaMethod::Eq, |lua, this, other: Value| {
            Ok(match other {
                Value::UserData(ud) => match ud.borrow::<LuaBlock>() {
                    Ok(other_block) => {
                        this.flush_property_cache(lua)?;
                        other_block.flush_property_cache(lua)?;
                        block_structurally_eq(&this.0.borrow(), &other_block.0.borrow())
                    }
                    Err(_) => false,
                },
                _ => false,
            })
        });

        // __pairs for iteration (for k, v in pairs(elem))
        methods.add_meta_method(MetaMethod::Pairs, |lua, this, ()| {
            this.flush_property_cache(lua)?;
            let snapshot = this.0.borrow().clone();

            // Create the iterator function following Lua's next() semantics:
            // - If control variable is nil, return first key-value pair
            // - If control variable is a string key, return next key-value pair after it
            let stateless_iter =
                lua.create_function(move |lua, (ud, key): (UserDataRef<LuaBlock>, Value)| {
                    let field_names = ud.field_names();

                    // Find the starting index
                    let start_idx = match key {
                        Value::Nil => 0,
                        Value::String(s) => {
                            let key_str = s.to_str()?;
                            // Find the index of the current key and add 1
                            if let Some(idx) = field_names.iter().position(|&k| key_str == k) {
                                idx + 1
                            } else {
                                // Key not found, end iteration
                                return Ok(Variadic::new());
                            }
                        }
                        Value::Integer(i) => {
                            // Support integer keys for iteration protocol compatibility
                            (i as usize) + 1
                        }
                        _ => return Ok(Variadic::new()),
                    };

                    if start_idx < field_names.len() {
                        let key = field_names[start_idx];
                        let value = ud.get_field(lua, key)?;
                        // Return (key, value) - key becomes the next control variable
                        Ok(Variadic::from_iter([key.into_lua(lua)?, value]))
                    } else {
                        Ok(Variadic::new())
                    }
                })?;

            // Return (iterator, state, initial value)
            // state is the userdata, initial value is nil (start from beginning)
            Ok((
                stateless_iter,
                lua.create_userdata(LuaBlock::new(snapshot))?,
                Value::Nil,
            ))
        });
    }
}

// Helper functions for conversion

/// Convert Vec<Inline> to Lua table of LuaInline userdata with Inlines metatable
pub fn inlines_to_lua_table(lua: &Lua, inlines: &[Inline]) -> Result<Value> {
    super::list::create_inlines_table(lua, inlines)
}

/// Convert Vec<Block> to Lua table of LuaBlock userdata with Blocks metatable
pub fn blocks_to_lua_table(lua: &Lua, blocks: &[Block]) -> Result<Value> {
    super::list::create_blocks_table(lua, blocks)
}

/// Convert a slice of strings to a Lua table with the List metatable
fn string_list_to_lua_table(lua: &Lua, items: &[String]) -> Result<Value> {
    super::list::create_string_list_table(lua, items)
}

/// Wrap a Vec of already-converted Lua values in a table with the List metatable
fn values_to_list_table(lua: &Lua, values: Vec<Value>) -> Result<Value> {
    super::list::create_list_table(lua, values)
}

/// Convert Vec<Citation> to a pandoc-List table of Citation userdata
/// (matching Pandoc, where `cite.citations` is a List whose entries
/// are typed Citation values).
fn citations_to_lua_table(lua: &Lua, citations: &[crate::pandoc::Citation]) -> Result<Value> {
    let values = citations
        .iter()
        .map(|citation| {
            lua.create_userdata(LuaCitation::new(citation.clone()))
                .map(Value::UserData)
        })
        .collect::<Result<Vec<_>>>()?;
    super::list::create_list_table(lua, values)
}

/// Convert a Lua value to an Attr for property assignment. Delegates
/// to `parse_attr` so assignment re-runs the same peeker the
/// constructors use (Pandoc's rule): bare string → identifier-only,
/// positional triple, HTML-like map (`class` split, `id` key), the q2
/// named form, Attr/AttributeList userdata (flushed). Previously this
/// was a weaker ad-hoc parser, so `header.attr = 'id'` and
/// `code.attr = {id=…, k=…}` were rejected or silently mis-read
/// (bd-0g2yp61w).
fn lua_value_to_attr(val: Value, lua: &Lua) -> Result<crate::pandoc::Attr> {
    super::constructors::parse_attr(lua, Some(val))
}

/// Convert a Lua value to Vec<Citation>: a sequence table whose
/// entries are Citation userdata, matching Pandoc's strict
/// `peekList peekCitation` ("table expected, got <type>" /
/// "Citation expected, got <type>").
pub(crate) fn lua_table_to_citations(
    lua: &Lua,
    val: Value,
) -> Result<Vec<crate::pandoc::Citation>> {
    match val {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<Value>() {
                result.push(lua_value_to_citation(lua, item?)?);
            }
            Ok(result)
        }
        Value::UserData(ud) if ud.borrow::<LuaCitation>().is_ok() => Err(Error::runtime(
            "table expected, got Citation (a single Citation must be wrapped in a list)",
        )),
        other => Err(Error::runtime(format!(
            "table of Citations expected, got {}",
            other.type_name()
        ))),
    }
}

/// Convert MetaValue to Lua value
pub fn meta_value_to_lua(lua: &Lua, meta: &crate::pandoc::MetaValue) -> Result<Value> {
    use crate::pandoc::MetaValue;
    match meta {
        MetaValue::MetaString(s) => {
            // MetaString becomes a Lua table with t="MetaString" and text field
            let table = lua.create_table()?;
            table.set("t", "MetaString")?;
            table.set("tag", "MetaString")?;
            table.set("text", s.clone())?;
            Ok(Value::Table(table))
        }
        MetaValue::MetaBool(b) => {
            // MetaBool becomes a Lua table with t="MetaBool" and value field
            let table = lua.create_table()?;
            table.set("t", "MetaBool")?;
            table.set("tag", "MetaBool")?;
            table.set("value", *b)?;
            Ok(Value::Table(table))
        }
        MetaValue::MetaInlines(inlines) => {
            // MetaInlines becomes a Lua table with t="MetaInlines" and content field
            let table = lua.create_table()?;
            table.set("t", "MetaInlines")?;
            table.set("tag", "MetaInlines")?;
            table.set("content", inlines_to_lua_table(lua, inlines)?)?;
            Ok(Value::Table(table))
        }
        MetaValue::MetaBlocks(blocks) => {
            // MetaBlocks becomes a Lua table with t="MetaBlocks" and content field
            let table = lua.create_table()?;
            table.set("t", "MetaBlocks")?;
            table.set("tag", "MetaBlocks")?;
            table.set("content", blocks_to_lua_table(lua, blocks)?)?;
            Ok(Value::Table(table))
        }
        MetaValue::MetaList(list) => {
            // MetaList becomes a Lua table with t="MetaList" and array of values
            let table = lua.create_table()?;
            table.set("t", "MetaList")?;
            table.set("tag", "MetaList")?;
            for (i, item) in list.iter().enumerate() {
                table.set(i + 1, meta_value_to_lua(lua, item)?)?;
            }
            Ok(Value::Table(table))
        }
        MetaValue::MetaMap(map) => {
            // MetaMap becomes a Lua table with t="MetaMap" and key-value pairs
            let table = lua.create_table()?;
            table.set("t", "MetaMap")?;
            table.set("tag", "MetaMap")?;
            for (key, val) in map.iter() {
                table.set(key.clone(), meta_value_to_lua(lua, val)?)?;
            }
            Ok(Value::Table(table))
        }
    }
}

/// Convert Lua value to MetaValue
pub fn lua_to_meta_value(lua: &Lua, val: Value) -> Result<crate::pandoc::MetaValue> {
    use crate::pandoc::MetaValue;
    match val {
        Value::Boolean(b) => Ok(MetaValue::MetaBool(b)),
        Value::String(s) => Ok(MetaValue::MetaString(s.to_str()?.to_string())),
        Value::Integer(i) => Ok(MetaValue::MetaString(i.to_string())),
        Value::Number(n) => Ok(MetaValue::MetaString(n.to_string())),
        Value::Table(table) => {
            // Check if it has a 't' or 'tag' field indicating it's a typed meta value
            let tag: Option<String> = table.get("t").ok().or_else(|| table.get("tag").ok());

            if let Some(tag) = tag {
                match tag.as_str() {
                    "MetaString" => {
                        let text: String = table.get("text")?;
                        Ok(MetaValue::MetaString(text))
                    }
                    "MetaBool" => {
                        let value: bool = table.get("value")?;
                        Ok(MetaValue::MetaBool(value))
                    }
                    "MetaInlines" => {
                        let content: Value = table.get("content")?;
                        let inlines = peek_inlines_fuzzy(lua, content)?;
                        Ok(MetaValue::MetaInlines(inlines))
                    }
                    "MetaBlocks" => {
                        let content: Value = table.get("content")?;
                        let blocks = peek_blocks_fuzzy(lua, content)?;
                        Ok(MetaValue::MetaBlocks(blocks))
                    }
                    "MetaList" => {
                        let mut list = Vec::new();
                        for i in 1.. {
                            let item: Value = table.get(i)?;
                            if item == Value::Nil {
                                break;
                            }
                            list.push(lua_to_meta_value(lua, item)?);
                        }
                        Ok(MetaValue::MetaList(list))
                    }
                    "MetaMap" => {
                        let mut map = hashlink::LinkedHashMap::new();
                        for pair in table.pairs::<String, Value>() {
                            let (k, v) = pair?;
                            if k != "t" && k != "tag" {
                                map.insert(k, lua_to_meta_value(lua, v)?);
                            }
                        }
                        Ok(MetaValue::MetaMap(map))
                    }
                    _ => {
                        // Unknown tag, treat as a map
                        let mut map = hashlink::LinkedHashMap::new();
                        for pair in table.pairs::<String, Value>() {
                            let (k, v) = pair?;
                            map.insert(k, lua_to_meta_value(lua, v)?);
                        }
                        Ok(MetaValue::MetaMap(map))
                    }
                }
            } else {
                // No tag - check if it's an array or map
                let first: Value = table.get(1)?;
                if first != Value::Nil {
                    // It's a sequence/list
                    let mut list = Vec::new();
                    for item in table.sequence_values::<Value>() {
                        list.push(lua_to_meta_value(lua, item?)?);
                    }
                    Ok(MetaValue::MetaList(list))
                } else {
                    // It's a map
                    let mut map = hashlink::LinkedHashMap::new();
                    for pair in table.pairs::<String, Value>() {
                        let (k, v) = pair?;
                        map.insert(k, lua_to_meta_value(lua, v)?);
                    }
                    Ok(MetaValue::MetaMap(map))
                }
            }
        }
        Value::Nil => Ok(MetaValue::MetaBool(false)),
        _ => Err(Error::runtime("cannot convert value to MetaValue")),
    }
}

/// Convert Meta (the document metadata map) to Lua table
pub fn meta_to_lua_table(lua: &Lua, meta: &crate::pandoc::Meta) -> Result<Value> {
    let table = lua.create_table()?;
    for (key, val) in meta.iter() {
        table.set(key.clone(), meta_value_to_lua(lua, val)?)?;
    }
    Ok(Value::Table(table))
}

/// Convert Lua table to Meta
pub fn lua_table_to_meta(lua: &Lua, val: Value) -> Result<crate::pandoc::Meta> {
    match val {
        Value::Table(table) => {
            let mut meta = hashlink::LinkedHashMap::new();
            for pair in table.pairs::<String, Value>() {
                let (k, v) = pair?;
                meta.insert(k, lua_to_meta_value(lua, v)?);
            }
            Ok(meta)
        }
        _ => Err(Error::runtime("expected table for Meta")),
    }
}

// ============================================================================
// Structural equality (Lua `__eq`, strand bd-55mb0rjz)
//
// Pandoc's Lua `==` compares the underlying Haskell values, which carry
// no source information. q2's AST types derive PartialEq *including*
// `source_info`, so two identically-constructed elements from different
// filter lines would compare unequal under derived `==`. We therefore
// compare the source-free Pandoc JSON of both sides, reusing the JSON
// writer's maintained match logic (`*_to_source_free_json`) instead of
// hand-maintaining a parallel ~60-variant equality. Derived `==` serves
// as a fast path (it implies structural equality).
// ============================================================================

pub(crate) fn inline_structurally_eq(a: &Inline, b: &Inline) -> bool {
    if a == b {
        return true;
    }
    let ctx = crate::pandoc::ast_context::ASTContext::default();
    crate::writers::json::inlines_to_source_free_json(&vec![a.clone()], &ctx)
        == crate::writers::json::inlines_to_source_free_json(&vec![b.clone()], &ctx)
}

pub(crate) fn block_structurally_eq(a: &Block, b: &Block) -> bool {
    if a == b {
        return true;
    }
    let ctx = crate::pandoc::ast_context::ASTContext::default();
    crate::writers::json::blocks_to_source_free_json(std::slice::from_ref(a), &ctx)
        == crate::writers::json::blocks_to_source_free_json(std::slice::from_ref(b), &ctx)
}

/// Attr equality, order-sensitive in the attribute list (Pandoc's Attr
/// attributes are a list of pairs, so order participates in `==`).
pub(crate) fn attr_structurally_eq(a: &crate::pandoc::Attr, b: &crate::pandoc::Attr) -> bool {
    a.0 == b.0
        && a.1 == b.1
        && a.2.len() == b.2.len()
        && a.2
            .iter()
            .zip(b.2.iter())
            .all(|((k1, v1), (k2, v2))| k1 == k2 && v1 == v2)
}

/// Split a string into Inlines, matching Pandoc's `B.text` from pandoc-types Builder.hs.
///
/// Groups consecutive characters by space vs non-space:
/// - Non-space runs → `Str(text)`
/// - Space-only runs → `SoftBreak` if the run contains `\n` or `\r`, else `Space`
/// - Empty string → empty vec
pub fn split_string_to_inlines(s: &str) -> Vec<Inline> {
    use crate::pandoc::{SoftBreak, Space, Str};

    if s.is_empty() {
        return Vec::new();
    }

    let is_space = |c: char| matches!(c, ' ' | '\r' | '\n' | '\t');
    let is_newline = |c: char| matches!(c, '\r' | '\n');

    let mut result = Vec::new();
    let mut chars = s.chars().peekable();

    while chars.peek().is_some() {
        let first = *chars.peek().unwrap();
        if is_space(first) {
            // Consume all consecutive space chars
            let mut has_newline = false;
            while let Some(&c) = chars.peek() {
                if is_space(c) {
                    if is_newline(c) {
                        has_newline = true;
                    }
                    chars.next();
                } else {
                    break;
                }
            }
            if has_newline {
                result.push(Inline::SoftBreak(SoftBreak {
                    source_info: SourceInfo::generated(By::unknown()),
                }));
            } else {
                result.push(Inline::Space(Space {
                    source_info: SourceInfo::generated(By::unknown()),
                }));
            }
        } else {
            // Consume all consecutive non-space chars
            let mut word = String::new();
            while let Some(&c) = chars.peek() {
                if is_space(c) {
                    break;
                }
                word.push(c);
                chars.next();
            }
            result.push(Inline::Str(Str {
                text: word,
                source_info: SourceInfo::generated(By::unknown()),
            }));
        }
    }

    result
}

/// Call a `__toinline`/`__toblock`-style metamethod hook on a table
/// or userdata value, returning the hook's raw result. Any other
/// outcome — no metatable, absent or non-function metafield, or a
/// call error — yields `None`, and callers fall through to their
/// normal coercion. This mirrors hslua's `peekInlineMetamethod` /
/// `peekBlockMetamethod`, whose failures are recoverable `failPeek`s
/// inside `<|>` chains (bd-olz91r4v; pinned by the "metafield is
/// ignored if it's not a function" and "non-Inline return values are
/// ignored" upstream tests).
fn call_element_metamethod(val: &Value, name: &str) -> Option<Value> {
    let metafield: Value = match val {
        Value::Table(t) => t.metatable()?.get(name).ok()?,
        Value::UserData(ud) => ud.metatable().ok()?.get(name).ok()?,
        _ => return None,
    };
    match metafield {
        Value::Function(f) => f.call::<Value>(val.clone()).ok(),
        _ => None,
    }
}

/// `__toinline` hook: Some(inline) only when the hook exists, runs,
/// and returns Inline userdata.
fn peek_inline_via_metamethod(lua: &Lua, val: &Value) -> Option<Inline> {
    match call_element_metamethod(val, "__toinline")? {
        Value::UserData(ud) => {
            let lua_inline = ud.borrow::<LuaInline>().ok()?;
            lua_inline.extract_flushed(lua).ok()
        }
        _ => None,
    }
}

/// `__toblock` hook: Some(block) only when the hook exists, runs, and
/// returns Block userdata.
fn peek_block_via_metamethod(lua: &Lua, val: &Value) -> Option<Block> {
    match call_element_metamethod(val, "__toblock")? {
        Value::UserData(ud) => {
            let lua_block = ud.borrow::<LuaBlock>().ok()?;
            lua_block.extract_flushed(lua).ok()
        }
        _ => None,
    }
}

/// Peek a single Inline from a Lua value, with fuzzy coercion.
///
/// Matches Pandoc's `peekInlineFuzzy`:
/// 1. String → `Str(text)` (NO word splitting)
/// 2. UserData containing LuaInline → extract; other userdata → try
///    the `__toinline` metamethod
/// 3. Table → try the `__toinline` metamethod
/// 4. Otherwise → error
pub fn peek_inline_fuzzy(lua: &Lua, val: Value) -> Result<Inline> {
    use crate::pandoc::Str;
    match val {
        Value::String(s) => {
            let text = s.to_str()?.to_string();
            Ok(Inline::Str(Str {
                text,
                source_info: filter_source_info(lua),
            }))
        }
        Value::UserData(ref ud) => {
            if let Ok(lua_inline) = ud.borrow::<LuaInline>() {
                lua_inline.extract_flushed(lua)
            } else if let Some(inline) = peek_inline_via_metamethod(lua, &val) {
                Ok(inline)
            } else {
                Err(Error::runtime(
                    "expected Inline userdata, string, or Inline-like value",
                ))
            }
        }
        Value::Table(_) => match peek_inline_via_metamethod(lua, &val) {
            Some(inline) => Ok(inline),
            None => Err(Error::runtime(
                "expected Inline userdata, string, or Inline-like value",
            )),
        },
        _ => Err(Error::runtime(
            "expected Inline userdata, string, or Inline-like value",
        )),
    }
}

/// Peek a list of Inlines from a Lua value, with fuzzy coercion.
///
/// Matches Pandoc's `peekInlinesFuzzy`:
/// 1. String → word-split via `split_string_to_inlines()`
/// 2. Table → try the `__toinline` metamethod (singleton) first,
///    else iterate sequence values, each via `peek_inline_fuzzy()`
/// 3. UserData → singleton via `peek_inline_fuzzy()` (which consults
///    the metamethod for foreign userdata)
/// 4. Otherwise → error
pub fn peek_inlines_fuzzy(lua: &Lua, val: Value) -> Result<Vec<Inline>> {
    match val {
        Value::String(s) => {
            let text = s.to_str()?;
            Ok(split_string_to_inlines(&text))
        }
        Value::Table(ref table) => {
            if let Some(inline) = peek_inline_via_metamethod(lua, &val) {
                return Ok(vec![inline]);
            }
            let mut inlines = Vec::new();
            for pair in table.sequence_values::<Value>() {
                let value = pair?;
                inlines.push(peek_inline_fuzzy(lua, value)?);
            }
            Ok(inlines)
        }
        Value::UserData(_) => match peek_inline_fuzzy(lua, val) {
            Ok(inline) => Ok(vec![inline]),
            Err(_) => Err(Error::runtime(
                "expected Inline, list of Inlines, or string",
            )),
        },
        _ => Err(Error::runtime(
            "expected Inline, list of Inlines, or string",
        )),
    }
}

/// Peek a single Block from a Lua value, with fuzzy coercion.
///
/// Matches Pandoc's `peekBlockFuzzy`:
/// 1. UserData containing LuaBlock → extract
/// 2. `__toblock` metamethod (tables and foreign userdata)
/// 3. Any value accepted by `peek_inlines_fuzzy()` → wrap in `Plain`
/// 4. Otherwise → error
pub fn peek_block_fuzzy(lua: &Lua, val: Value) -> Result<Block> {
    use crate::pandoc::Plain;
    match &val {
        Value::UserData(ud) => {
            if let Ok(lua_block) = ud.borrow::<LuaBlock>() {
                return lua_block.extract_flushed(lua);
            }
            if let Some(block) = peek_block_via_metamethod(lua, &val) {
                return Ok(block);
            }
            // Not a block — fall through to inlines coercion
            let inlines = peek_inlines_fuzzy(lua, val)?;
            Ok(Block::Plain(Plain {
                content: inlines,
                source_info: SourceInfo::generated(By::unknown()),
            }))
        }
        _ => {
            if let Some(block) = peek_block_via_metamethod(lua, &val) {
                return Ok(block);
            }
            // Try inlines coercion for strings, tables of inlines, etc.
            match peek_inlines_fuzzy(lua, val) {
                Ok(inlines) => Ok(Block::Plain(Plain {
                    content: inlines,
                    source_info: SourceInfo::generated(By::unknown()),
                })),
                Err(_) => Err(Error::runtime("expected Block, list of Inlines, or string")),
            }
        }
    }
}

/// Peek a list of Blocks from a Lua value, with fuzzy coercion.
///
/// Matches Pandoc's `peekBlocksFuzzy`:
/// 1. `__toblock` metamethod → singleton (before list interpretation)
/// 2. Table → iterate sequence values, each via `peek_block_fuzzy()`
/// 3. UserData containing LuaBlock → wrap in singleton vec
/// 4. Any value accepted by `peek_inlines_fuzzy()` → wrap in `Plain` block singleton
/// 5. Otherwise → error
pub fn peek_blocks_fuzzy(lua: &Lua, val: Value) -> Result<Vec<Block>> {
    use crate::pandoc::Plain;
    match &val {
        Value::Table(table) => {
            if let Some(block) = peek_block_via_metamethod(lua, &val) {
                return Ok(vec![block]);
            }
            let mut blocks = Vec::new();
            for pair in table.sequence_values::<Value>() {
                let value = pair?;
                blocks.push(peek_block_fuzzy(lua, value)?);
            }
            Ok(blocks)
        }
        Value::UserData(ud) => {
            if let Ok(lua_block) = ud.borrow::<LuaBlock>() {
                return Ok(vec![lua_block.extract_flushed(lua)?]);
            }
            if let Some(block) = peek_block_via_metamethod(lua, &val) {
                return Ok(vec![block]);
            }
            // Not a block — try inlines coercion
            let inlines = peek_inlines_fuzzy(lua, val)?;
            Ok(vec![Block::Plain(Plain {
                content: inlines,
                source_info: SourceInfo::generated(By::unknown()),
            })])
        }
        _ => {
            // Try inlines coercion for strings
            match peek_inlines_fuzzy(lua, val) {
                Ok(inlines) => Ok(vec![Block::Plain(Plain {
                    content: inlines,
                    source_info: SourceInfo::generated(By::unknown()),
                })]),
                Err(_) => Err(Error::runtime(
                    "expected Block, list of Blocks, or compatible element",
                )),
            }
        }
    }
}

/// Create a SourceInfo for filter-created elements
///
/// This captures the source file and line from the Lua debug info,
/// allowing error messages to point to where the element was created.
pub fn filter_source_info(lua: &Lua) -> SourceInfo {
    // Walk up the stack looking for the first Lua function call
    // Level 0 is this function itself (inside mlua), so we start at level 1
    // We look up to level 5 to find a filter function (not a C function)
    for level in 1..=5 {
        if let Some(result) = lua.inspect_stack(level, |debug| {
            let source: mlua::DebugSource = debug.source();
            let line = debug.current_line();

            // Check if this is a Lua source (not a C function)
            if source.what != "C"
                && let Some(src) = source.source
            {
                // The source often starts with "@" for file paths
                let path: &str = src.strip_prefix("@").unwrap_or(&src);
                let line_num = line.unwrap_or(0);
                return Some(SourceInfo::Generated {
                    by: By::filter(path.to_string(), line_num),
                    from: SmallVec::new(),
                });
            }
            None
        }) && let Some(info) = result
        {
            return info;
        }
    }

    // Fallback if we couldn't get debug info
    SourceInfo::generated(By::unknown())
}

// ---------------------------------------------------------------------------
// Attr-location helpers (used by LuaAttr proxy variants)
// ---------------------------------------------------------------------------
//
// Given a parent Block/Inline, return a reference to its Attr (or None if
// the variant doesn't carry an Attr). Used to read/write through the
// shared cell when a LuaAttr is in BlockRef / InlineRef mode.

pub(crate) fn block_attr_ref(block: &Block) -> Option<&crate::pandoc::Attr> {
    match block {
        Block::CodeBlock(c) => Some(&c.attr),
        Block::Header(h) => Some(&h.attr),
        Block::Div(d) => Some(&d.attr),
        Block::Figure(f) => Some(&f.attr),
        Block::Table(t) => Some(&t.attr),
        _ => None,
    }
}

pub(crate) fn block_attr_mut(block: &mut Block) -> Option<&mut crate::pandoc::Attr> {
    match block {
        Block::CodeBlock(c) => Some(&mut c.attr),
        Block::Header(h) => Some(&mut h.attr),
        Block::Div(d) => Some(&mut d.attr),
        Block::Figure(f) => Some(&mut f.attr),
        Block::Table(t) => Some(&mut t.attr),
        _ => None,
    }
}

pub(crate) fn inline_attr_ref(inline: &Inline) -> Option<&crate::pandoc::Attr> {
    match inline {
        Inline::Code(c) => Some(&c.attr),
        Inline::Link(l) => Some(&l.attr),
        Inline::Image(i) => Some(&i.attr),
        Inline::Span(s) => Some(&s.attr),
        Inline::Insert(x) => Some(&x.attr),
        Inline::Delete(x) => Some(&x.attr),
        Inline::Highlight(x) => Some(&x.attr),
        Inline::EditComment(x) => Some(&x.attr),
        _ => None,
    }
}

pub(crate) fn inline_attr_mut(inline: &mut Inline) -> Option<&mut crate::pandoc::Attr> {
    match inline {
        Inline::Code(c) => Some(&mut c.attr),
        Inline::Link(l) => Some(&mut l.attr),
        Inline::Image(i) => Some(&mut i.attr),
        Inline::Span(s) => Some(&mut s.attr),
        Inline::Insert(x) => Some(&mut x.attr),
        Inline::Delete(x) => Some(&mut x.attr),
        Inline::Highlight(x) => Some(&mut x.attr),
        Inline::EditComment(x) => Some(&mut x.attr),
        _ => None,
    }
}

/// Wrapper for Pandoc Attr (identifier, classes, attributes) as Lua userdata.
///
/// Pandoc's Attr is a tuple:
/// `(identifier: String, classes: Vec<String>, attributes: HashMap<String, String>)`.
/// This wrapper exposes it as userdata with named field access
/// (`attr.identifier`, `attr.classes`, `attr.attributes`), positional
/// access (`attr[1]..attr[3]`), and a constant `t`/`tag` of "Attr".
///
/// Three variants match the three ownership modes:
///
/// - `Owned`: the `Attr` is standalone (built via `pandoc.Attr(...)`).
///   Its own `Rc<RefCell<Attr>>` cell. Mutations through proxies derived
///   from this `LuaAttr` (Phase 4: `.attributes`, `.classes`) land back
///   in this cell.
/// - `BlockRef`: this is a *live proxy* into a parent `LuaBlock`'s
///   `Rc<RefCell<Block>>`. Reads/writes go through the block's cell and
///   through `block_attr_ref`/`block_attr_mut` to reach the `Attr` inside
///   the active variant. Mutations propagate back to the parent block.
/// - `InlineRef`: same, but for an `Inline` parent.
///
/// FromLua always produces an independent `Owned` variant, matching
/// pre-refactor semantics (ownership does not cross FromLua boundaries).
///
/// The `cache` field carries the same hslua-style property cache the
/// elements have (see [`PropertyCache`]): `attr.classes` hands out a
/// pandoc-List table that aliases across reads, and the cache is
/// flushed back through `set_field` before the attr value is read out.
#[derive(Debug, Clone)]
pub struct LuaAttr {
    target: AttrTarget,
    pub(crate) cache: PropertyCache,
}

#[derive(Debug, Clone)]
enum AttrTarget {
    Owned(Rc<RefCell<crate::pandoc::Attr>>),
    BlockRef(Rc<RefCell<Block>>),
    InlineRef(Rc<RefCell<Inline>>),
}

impl LuaAttr {
    /// Create a new standalone (Owned) LuaAttr from an Attr tuple.
    pub fn new(attr: crate::pandoc::Attr) -> Self {
        LuaAttr {
            target: AttrTarget::Owned(Rc::new(RefCell::new(attr))),
            cache: PropertyCache::default(),
        }
    }

    /// Create a proxy LuaAttr referencing the given block's Attr.
    pub fn for_block(block: Rc<RefCell<Block>>) -> Self {
        LuaAttr {
            target: AttrTarget::BlockRef(block),
            cache: PropertyCache::default(),
        }
    }

    /// Create a proxy LuaAttr referencing the given inline's Attr.
    pub fn for_inline(inline: Rc<RefCell<Inline>>) -> Self {
        LuaAttr {
            target: AttrTarget::InlineRef(inline),
            cache: PropertyCache::default(),
        }
    }

    /// Whether this LuaAttr is a live proxy into a parent element (its
    /// direct writes already land in the parent's cell).
    pub(crate) fn is_element_ref(&self) -> bool {
        !matches!(self.target, AttrTarget::Owned(_))
    }

    /// Is this a live ref into exactly this block cell?
    pub(crate) fn is_ref_to_block(&self, cell: &Rc<RefCell<Block>>) -> bool {
        matches!(&self.target, AttrTarget::BlockRef(rc) if Rc::ptr_eq(rc, cell))
    }

    /// Is this a live ref into exactly this inline cell?
    pub(crate) fn is_ref_to_inline(&self, cell: &Rc<RefCell<Inline>>) -> bool {
        matches!(&self.target, AttrTarget::InlineRef(rc) if Rc::ptr_eq(rc, cell))
    }

    /// Write cached property values (currently: the `classes` List
    /// table) back into the underlying Attr. Idempotent.
    pub fn flush_property_cache(&self, lua: &Lua) -> Result<()> {
        let entries = match self.cache.begin_flush() {
            Some(entries) => entries,
            None => return Ok(()),
        };
        let mut result = Ok(());
        for (key, value) in entries {
            let key_value = Value::String(lua.create_string(&key)?);
            if let Err(e) = self.set_field(key_value, value, lua) {
                result = Err(e);
                break;
            }
        }
        self.cache.end_flush();
        result
    }

    /// Flush, then deep-clone the underlying Attr — the blessed way to
    /// marshal a `LuaAttr` back into a Rust Attr tuple.
    pub fn extract_flushed(&self, lua: &Lua) -> Result<crate::pandoc::Attr> {
        self.flush_property_cache(lua)?;
        Ok(self.clone_attr())
    }

    /// Run `f` against the underlying Attr for reading. Panics on a
    /// structural mismatch (proxy's parent variant has no Attr) — this
    /// indicates the parent element was mutated through a different field
    /// in a way that invalidated the proxy, which shouldn't happen under
    /// normal filter usage.
    fn with_attr<R>(&self, f: impl FnOnce(&crate::pandoc::Attr) -> R) -> R {
        match &self.target {
            AttrTarget::Owned(rc) => f(&rc.borrow()),
            AttrTarget::BlockRef(rc) => {
                let block = rc.borrow();
                f(block_attr_ref(&block)
                    .expect("LuaAttr::BlockRef proxy points at a block variant without an Attr"))
            }
            AttrTarget::InlineRef(rc) => {
                let inline = rc.borrow();
                f(inline_attr_ref(&inline)
                    .expect("LuaAttr::InlineRef proxy points at an inline variant without an Attr"))
            }
        }
    }

    /// Run `f` against the underlying Attr for mutation.
    fn with_attr_mut<R>(&self, f: impl FnOnce(&mut crate::pandoc::Attr) -> R) -> R {
        match &self.target {
            AttrTarget::Owned(rc) => f(&mut rc.borrow_mut()),
            AttrTarget::BlockRef(rc) => {
                let mut block = rc.borrow_mut();
                f(block_attr_mut(&mut block)
                    .expect("LuaAttr::BlockRef proxy points at a block variant without an Attr"))
            }
            AttrTarget::InlineRef(rc) => {
                let mut inline = rc.borrow_mut();
                f(inline_attr_mut(&mut inline)
                    .expect("LuaAttr::InlineRef proxy points at an inline variant without an Attr"))
            }
        }
    }

    /// Deep-clone the underlying Attr into an independent owned value.
    pub fn clone_attr(&self) -> crate::pandoc::Attr {
        self.with_attr(|attr| attr.clone())
    }

    /// Return the identifier as a String (cloned from the underlying cell).
    pub fn identifier(&self) -> String {
        self.with_attr(|attr| attr.0.clone())
    }

    /// Clone the classes list out.
    pub fn classes(&self) -> Vec<String> {
        self.with_attr(|attr| attr.1.clone())
    }

    /// Clone the attributes map out.
    pub fn attributes(&self) -> hashlink::LinkedHashMap<String, String> {
        self.with_attr(|attr| attr.2.clone())
    }

    /// Get a field value by name or index
    pub(crate) fn get_field(&self, lua: &Lua, key: Value) -> Result<Value> {
        match key {
            // Positional access (Lua uses 1-based indexing)
            Value::Integer(1) => self.identifier().into_lua(lua),
            Value::Integer(2) => self.classes_list_value(lua),
            Value::Integer(3) => {
                let ud = lua.create_userdata(LuaAttributesProxy::new(self.clone()))?;
                Ok(Value::UserData(ud))
            }
            // Named field access
            Value::String(s) => {
                let borrowed = s.to_str()?;
                let key_str: &str = borrowed.as_ref();
                match key_str {
                    "identifier" => self.identifier().into_lua(lua),
                    "classes" => self.classes_list_value(lua),
                    "attributes" => {
                        let ud = lua.create_userdata(LuaAttributesProxy::new(self.clone()))?;
                        Ok(Value::UserData(ud))
                    }
                    "t" | "tag" => "Attr".into_lua(lua),
                    _ => Ok(Value::Nil),
                }
            }
            _ => Ok(Value::Nil),
        }
    }

    /// Read `classes` as a pandoc-List table, aliased across reads via
    /// the property cache (matching Pandoc, where `attr.classes` is a
    /// pandoc List and in-place mutation persists).
    fn classes_list_value(&self, lua: &Lua) -> Result<Value> {
        if let Some(cached) = self.cache.get("classes") {
            return Ok(cached);
        }
        let value = self.with_attr(|attr| super::list::create_string_list_table(lua, &attr.1))?;
        self.cache.store("classes", &value);
        Ok(value)
    }

    /// Set a field value by name or index. Takes `&self`: mutation goes
    /// through the appropriate `RefCell` on the enum variant.
    pub(crate) fn set_field(&self, key: Value, val: Value, lua: &Lua) -> Result<()> {
        match key {
            Value::Integer(1) => {
                let s = String::from_lua(val, lua)?;
                self.with_attr_mut(|attr| attr.0 = s);
                Ok(())
            }
            Value::Integer(2) => {
                let classes = lua_table_to_strings(lua, val)?;
                self.with_attr_mut(|attr| attr.1 = classes);
                Ok(())
            }
            Value::Integer(3) => {
                let attrs = lua_table_to_string_map(lua, val)?;
                self.with_attr_mut(|attr| attr.2 = attrs);
                Ok(())
            }
            Value::String(s) => {
                let borrowed = s.to_str()?;
                let key_str: &str = borrowed.as_ref();
                match key_str {
                    "identifier" => {
                        let id = String::from_lua(val, lua)?;
                        self.with_attr_mut(|attr| attr.0 = id);
                        Ok(())
                    }
                    "classes" => {
                        let classes = lua_table_to_strings(lua, val)?;
                        self.with_attr_mut(|attr| attr.1 = classes);
                        Ok(())
                    }
                    "attributes" => {
                        let attrs = lua_table_to_string_map(lua, val)?;
                        self.with_attr_mut(|attr| attr.2 = attrs);
                        Ok(())
                    }
                    "t" | "tag" => Err(Error::runtime("cannot set read-only field 'tag'")),
                    _ => Err(Error::runtime(format!("cannot set field '{}'", key_str))),
                }
            }
            _ => Err(Error::runtime("invalid key type for Attr")),
        }
    }
}

impl UserData for LuaAttr {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        // Static fields accessible on all Attrs
        fields.add_field_method_get("t", |_, _| Ok("Attr"));
        fields.add_field_method_get("tag", |_, _| Ok("Attr"));
        // NB: do not register add_field_method_get("identifier", …) here —
        // mlua dispatches fields before __index, so it would shadow our
        // routing through get_field. The "identifier" branch inside
        // get_field handles the read.
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // Dynamic field access via __index for both named and positional access
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: Value| {
            this.get_field(lua, key)
        });

        // Dynamic field assignment via __newindex. Uses add_meta_method
        // (not _mut): interior mutability is on the RefCell inside the
        // variant, so set_field takes `&self`.
        methods.add_meta_method(
            MetaMethod::NewIndex,
            |lua, this, (key, val): (Value, Value)| {
                let is_classes_key = matches!(&key, Value::Integer(2))
                    || matches!(&key, Value::String(s) if s.to_str().is_ok_and(|k| &*k == "classes"));
                this.set_field(key, val, lua)?;
                // User assignment invalidates the cached classes List
                // (rebuilt with the List metatable on next read). The
                // internal flush path calls set_field directly and
                // deliberately keeps the cache (aliasing survives).
                if is_classes_key {
                    this.cache.remove("classes");
                }
                Ok(())
            },
        );

        // Clone method — always produces an Owned copy, independent of
        // the source variant (BlockRef/InlineRef clones detach the Attr
        // from its parent, matching Pandoc's "elem.attr:clone()" shape).
        methods.add_method("clone", |lua, this, ()| {
            this.flush_property_cache(lua)?;
            lua.create_userdata(LuaAttr::new(this.clone_attr()))
        });

        // __tostring: Haskell-show tuple format, matching Pandoc's
        // Lua API: `("id",["c"],[("k","v")])`
        methods.add_meta_method(MetaMethod::ToString, |lua, this, ()| {
            this.flush_property_cache(lua)?;
            Ok(super::show::show_attr(&this.clone_attr()))
        });

        // __eq: component-wise equality, order-sensitive in the
        // attribute list (Pandoc's Attr is a list of pairs).
        methods.add_meta_method(MetaMethod::Eq, |lua, this, other: Value| {
            Ok(match other {
                Value::UserData(ud) => match ud.borrow::<LuaAttr>() {
                    Ok(other_attr) => {
                        this.flush_property_cache(lua)?;
                        other_attr.flush_property_cache(lua)?;
                        attr_structurally_eq(&this.clone_attr(), &other_attr.clone_attr())
                    }
                    Err(_) => false,
                },
                _ => false,
            })
        });

        // __len returns 3 (for the three components)
        methods.add_meta_method(MetaMethod::Len, |_, _, ()| Ok(3));
    }
}

impl FromLua for LuaAttr {
    fn from_lua(value: Value, lua: &Lua) -> Result<Self> {
        match value {
            Value::UserData(ud) => {
                let lua_attr = ud.borrow::<LuaAttr>()?;
                lua_attr.flush_property_cache(lua)?;
                // Always produce an Owned clone on FromLua: detach from
                // any parent, preserving today's "independent copy"
                // semantics.
                Ok(LuaAttr::new(lua_attr.clone_attr()))
            }
            _ => Err(Error::runtime("expected Attr userdata")),
        }
    }
}

// ---------------------------------------------------------------------------
// Proxy userdata for the `attributes` map and `classes` list
// ---------------------------------------------------------------------------
//
// These thin wrappers carry a `LuaAttr` enum (which resolves — via
// `with_attr` / `with_attr_mut` — to the underlying Attr regardless of
// whether the source is an Owned standalone, a BlockRef proxy, or an
// InlineRef proxy). Mutations through the proxy therefore land in the
// same cell the parent element is sharing, making
// `cb.attr.attributes["k"] = v` persist on the block.
//
// Borrow discipline: each metamethod grabs `borrow()` / `borrow_mut()`
// *inside* a short scope, never across a Lua callback. `__pairs` takes
// a fresh key-snapshot to avoid holding an outstanding borrow between
// `next` calls; values are fetched live on each iteration step.
//
// FromLua is **not** implemented for these proxies (no `cb.attr.attributes`
// being passed back into Rust helpers — they're read/written directly
// through Lua metamethods).

/// Proxy userdata for `attr.attributes` (the key→value attribute map).
#[derive(Debug, Clone)]
pub struct LuaAttributesProxy(pub LuaAttr);

impl LuaAttributesProxy {
    pub fn new(attr: LuaAttr) -> Self {
        LuaAttributesProxy(attr)
    }

    fn get(&self, key: &str) -> Option<String> {
        self.0.with_attr(|a| a.2.get(key).cloned())
    }

    fn set(&self, key: String, value: Option<String>) {
        self.0.with_attr_mut(|a| match value {
            Some(v) => {
                a.2.insert(key, v);
            }
            None => {
                a.2.remove(&key);
            }
        });
    }

    fn len(&self) -> usize {
        self.0.with_attr(|a| a.2.len())
    }

    /// The i-th (1-based) key/value pair, in insertion order.
    fn pair_at(&self, i: usize) -> Option<(String, String)> {
        self.0.with_attr(|a| {
            a.2.iter()
                .nth(i.checked_sub(1)?)
                .map(|(k, v)| (k.clone(), v.clone()))
        })
    }

    /// Replace (Some) or remove (None) the i-th (1-based) pair,
    /// preserving the order of the other entries. Out-of-range
    /// indices are ignored, matching Lua table semantics loosely.
    fn set_pair_at(&self, i: usize, pair: Option<(String, String)>) {
        self.0.with_attr_mut(|a| {
            let mut pairs: Vec<(String, String)> =
                a.2.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            let Some(idx) = i.checked_sub(1) else { return };
            if idx >= pairs.len() {
                return;
            }
            match pair {
                Some(p) => pairs[idx] = p,
                None => {
                    pairs.remove(idx);
                }
            }
            a.2 = pairs.into_iter().collect();
        });
    }

    /// Snapshot the attribute map (used when an AttributeList value is
    /// consumed by a constructor / attr parser).
    pub(crate) fn snapshot_map(&self) -> hashlink::LinkedHashMap<String, String> {
        self.0.with_attr(|a| a.2.clone())
    }

    fn snapshot_pairs(&self) -> Vec<(String, String)> {
        self.0
            .with_attr(|a| a.2.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
    }
}

impl UserData for LuaAttributesProxy {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // attrs["key"] — string read; attrs[i] — i-th {key, value} pair
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: Value| match key {
            Value::String(s) => {
                let k = s.to_str()?.to_string();
                match this.get(&k) {
                    Some(v) => v.into_lua(lua),
                    None => Ok(Value::Nil),
                }
            }
            Value::Integer(i) => match usize::try_from(i).ok().and_then(|i| this.pair_at(i)) {
                Some((k, v)) => {
                    let pair = lua.create_table()?;
                    pair.set(1, k)?;
                    pair.set(2, v)?;
                    Ok(Value::Table(pair))
                }
                None => Ok(Value::Nil),
            },
            _ => Ok(Value::Nil),
        });

        // attrs["key"] = "value" or attrs["key"] = nil — string write / delete
        methods.add_meta_method(
            MetaMethod::NewIndex,
            |lua, this, (key, val): (Value, Value)| {
                let k = match key {
                    Value::String(s) => s.to_str()?.to_string(),
                    // attrs[i] = {key, value} replaces; attrs[i] = nil removes
                    Value::Integer(i) => {
                        let idx = usize::try_from(i)
                            .map_err(|_| Error::runtime("AttributeList index must be positive"))?;
                        match val {
                            Value::Nil => this.set_pair_at(idx, None),
                            Value::Table(pair) => {
                                let k: String = pair.get(1)?;
                                let v: String = pair.get(2)?;
                                this.set_pair_at(idx, Some((k, v)));
                            }
                            _ => {
                                return Err(Error::runtime(
                                    "AttributeList entries must be {key, value} pairs or nil",
                                ));
                            }
                        }
                        return Ok(());
                    }
                    _ => {
                        return Err(Error::runtime(
                            "Attr.attributes proxy: only string or integer keys are supported",
                        ));
                    }
                };
                let v = match val {
                    Value::Nil => None,
                    Value::String(s) => Some(s.to_str()?.to_string()),
                    _ => Some(String::from_lua(val, lua)?),
                };
                this.set(k, v);
                Ok(())
            },
        );

        // #attrs
        methods.add_meta_method(MetaMethod::Len, |_, this, ()| Ok(this.len() as i64));

        // pairs(attrs)
        methods.add_meta_method(MetaMethod::Pairs, |lua, this, ()| {
            // Snapshot keys at pairs-call time; values read live on each
            // step so intervening writes can be observed without holding
            // a RefCell borrow between iterations.
            let keys: Vec<String> = this
                .0
                .with_attr(|a| a.2.keys().cloned().collect::<Vec<_>>());

            let stateless_iter = lua.create_function(
                move |lua, (ud, key): (UserDataRef<LuaAttributesProxy>, Value)| {
                    let idx = match key {
                        Value::Nil => 0,
                        Value::String(s) => {
                            let k = s.to_str()?;
                            match keys.iter().position(|x| x == k.as_ref()) {
                                Some(i) => i + 1,
                                None => return Ok(Variadic::new()),
                            }
                        }
                        _ => return Ok(Variadic::new()),
                    };
                    if idx < keys.len() {
                        let k = &keys[idx];
                        let v = ud.get(k).unwrap_or_default();
                        Ok(Variadic::from_iter([
                            k.clone().into_lua(lua)?,
                            v.into_lua(lua)?,
                        ]))
                    } else {
                        Ok(Variadic::new())
                    }
                },
            )?;

            Ok((
                stateless_iter,
                lua.create_userdata(this.clone())?,
                Value::Nil,
            ))
        });

        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!(
                "Attributes({:?})",
                this.0.with_attr(|a| a.2.clone())
            ))
        });

        // __eq: pairwise, order-sensitive (Pandoc's AttributeList is a
        // list of pairs).
        methods.add_meta_method(MetaMethod::Eq, |_, this, other: Value| {
            Ok(match other {
                Value::UserData(ud) => match ud.borrow::<LuaAttributesProxy>() {
                    Ok(other_proxy) => {
                        let a = this.snapshot_pairs();
                        let b = other_proxy.snapshot_pairs();
                        a == b
                    }
                    Err(_) => false,
                },
                _ => false,
            })
        });
    }
}

/// Proxy userdata for `attr.classes` (the ordered list of classes).
#[derive(Debug, Clone)]
pub struct LuaClassesProxy(pub LuaAttr);

impl LuaClassesProxy {
    pub fn new(attr: LuaAttr) -> Self {
        LuaClassesProxy(attr)
    }

    fn get(&self, i: usize) -> Option<String> {
        // 1-based Lua index → 0-based Rust.
        if i == 0 {
            return None;
        }
        self.0.with_attr(|a| a.1.get(i - 1).cloned())
    }

    /// 1-based write. Supports overwrite (1..=len), append (len+1), and
    /// delete via `nil` (1..=len; shifts subsequent elements left).
    /// Out-of-range writes error.
    fn set(&self, i: usize, value: Option<String>) -> Result<()> {
        if i == 0 {
            return Err(Error::runtime(
                "Attr.classes proxy: index must be >= 1 (Lua 1-based)",
            ));
        }
        self.0.with_attr_mut(|a| {
            let idx = i - 1;
            let len = a.1.len();
            match value {
                Some(v) => {
                    if idx < len {
                        a.1[idx] = v;
                        Ok(())
                    } else if idx == len {
                        a.1.push(v);
                        Ok(())
                    } else {
                        Err(Error::runtime(format!(
                            "Attr.classes proxy: index {} out of range (len = {}); \
                             use index in 1..={} to overwrite or {} to append",
                            i,
                            len,
                            len,
                            len + 1
                        )))
                    }
                }
                None => {
                    if idx < len {
                        a.1.remove(idx);
                        Ok(())
                    } else {
                        // Setting nil past the end is a no-op (matches Lua
                        // tables when assigning nil to an absent key).
                        Ok(())
                    }
                }
            }
        })
    }

    fn len(&self) -> usize {
        self.0.with_attr(|a| a.1.len())
    }
}

impl UserData for LuaClassesProxy {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // classes[i] — integer read; classes.<method> — look up a List
        // method from the shared metatable and bind it to a snapshot.
        //
        // Why the snapshot: the List methods in `lua::list` take a
        // `Table` as their first argument, not a userdata. We can't pass
        // our userdata directly. Instead, at method-lookup time we copy
        // the classes into a fresh list-backed table and return a
        // closure that forwards `(proxy, ...)` as `(snapshot, ...)`. The
        // snapshot is evaluated at lookup-time, so if the user does
        // something like `local f = classes.includes; … f("foo")`, the
        // snapshot was taken at the lookup. Read-only list methods
        // (`includes`, `at`, `map`, `filter`, `find`, `clone`, `iter`)
        // therefore behave as users expect. Mutating list methods
        // (`insert`, `remove`, `sort`) operate on the snapshot only —
        // same as pre-refactor, since classes already returned a fresh
        // table. For persistent mutation, users write through
        // `classes[i] = v` (which goes through __newindex on the proxy).
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: Value| match key {
            Value::Integer(i) if i >= 1 => match this.get(i as usize) {
                Some(v) => v.into_lua(lua),
                None => Ok(Value::Nil),
            },
            Value::String(s) => {
                let list_mt = super::list::get_or_create_list_metatable(lua)?;
                let method: Value = list_mt.get(s.clone())?;
                if matches!(method, Value::Nil) {
                    return Ok(Value::Nil);
                }
                // Build snapshot with list metatable.
                let snapshot = lua.create_table()?;
                for (i, cls) in this.0.with_attr(|a| a.1.clone()).iter().enumerate() {
                    snapshot.set(i + 1, cls.clone())?;
                }
                snapshot.set_metatable(Some(list_mt))?;
                // Wrap in a closure that forwards (self_ud, ...) →
                // method(snapshot, ...).
                let method_fn = match method {
                    Value::Function(f) => f,
                    other => return Ok(other),
                };
                let bound = lua.create_function(
                    move |_, args: Variadic<Value>| -> Result<Variadic<Value>> {
                        // Discard the first arg (the userdata proxy, from `:` syntax).
                        let mut iter = args.into_iter();
                        let _self_ud = iter.next();
                        let mut forward: Vec<Value> = vec![Value::Table(snapshot.clone())];
                        forward.extend(iter);
                        method_fn.call::<Variadic<Value>>(Variadic::from_iter(forward))
                    },
                )?;
                Ok(Value::Function(bound))
            }
            _ => Ok(Value::Nil),
        });

        // classes[i] = value — integer write / delete
        methods.add_meta_method(
            MetaMethod::NewIndex,
            |lua, this, (key, val): (Value, Value)| {
                let i = match key {
                    Value::Integer(i) if i >= 1 => i as usize,
                    _ => {
                        return Err(Error::runtime(
                            "Attr.classes proxy: only positive integer keys are supported",
                        ));
                    }
                };
                let v = match val {
                    Value::Nil => None,
                    Value::String(s) => Some(s.to_str()?.to_string()),
                    _ => Some(String::from_lua(val, lua)?),
                };
                this.set(i, v)
            },
        );

        // #classes
        methods.add_meta_method(MetaMethod::Len, |_, this, ()| Ok(this.len() as i64));

        // pairs(classes) / ipairs(classes) — both return integer-indexed pairs
        methods.add_meta_method(MetaMethod::Pairs, |lua, this, ()| {
            let len = this.len();
            let stateless_iter = lua.create_function(
                move |lua, (ud, key): (UserDataRef<LuaClassesProxy>, Value)| {
                    let next_idx = match key {
                        Value::Nil => 1,
                        Value::Integer(i) if i >= 0 => (i as usize) + 1,
                        _ => return Ok(Variadic::new()),
                    };
                    if next_idx <= len {
                        let v = ud.get(next_idx).unwrap_or_default();
                        Ok(Variadic::from_iter([
                            (next_idx as i64).into_lua(lua)?,
                            v.into_lua(lua)?,
                        ]))
                    } else {
                        Ok(Variadic::new())
                    }
                },
            )?;

            Ok((
                stateless_iter,
                lua.create_userdata(this.clone())?,
                Value::Nil,
            ))
        });

        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("Classes({:?})", this.0.with_attr(|a| a.1.clone())))
        });
    }
}

/// Convert a Lua table of strings to `Vec<String>`. Also accepts a
/// `LuaClassesProxy` userdata (so `pandoc.Attr(id, cb.attr.classes, …)`
/// works directly without the user having to convert).
pub(crate) fn lua_table_to_strings(_lua: &Lua, val: Value) -> Result<Vec<String>> {
    match val {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<String>() {
                result.push(item?);
            }
            Ok(result)
        }
        Value::UserData(ud) => {
            if let Ok(proxy) = ud.borrow::<LuaClassesProxy>() {
                Ok(proxy.0.classes())
            } else {
                Err(Error::runtime("expected table of strings"))
            }
        }
        _ => Err(Error::runtime("expected table of strings")),
    }
}

/// Convert a Lua table to `LinkedHashMap<String, String>`. Also accepts
/// a `LuaAttributesProxy` userdata for the same reason.
pub(crate) fn lua_table_to_string_map(
    _lua: &Lua,
    val: Value,
) -> Result<hashlink::LinkedHashMap<String, String>> {
    match val {
        Value::Table(table) => {
            let mut result = hashlink::LinkedHashMap::new();
            for pair in table.pairs::<String, String>() {
                let (k, v) = pair?;
                result.insert(k, v);
            }
            Ok(result)
        }
        Value::UserData(ud) => {
            if let Ok(proxy) = ud.borrow::<LuaAttributesProxy>() {
                Ok(proxy.0.attributes())
            } else {
                Err(Error::runtime("expected table of key-value pairs"))
            }
        }
        _ => Err(Error::runtime("expected table of key-value pairs")),
    }
}

/// Convert an Attr value into a fresh **Owned** LuaAttr userdata (detached
/// from any parent element). Used by `pandoc.Attr(...)` and by container
/// types that expose an attr snapshot (e.g. table rows/cells) where we
/// don't yet propagate writes back.
pub fn attr_to_lua_userdata(lua: &Lua, attr: &crate::pandoc::Attr) -> Result<Value> {
    let lua_attr = LuaAttr::new(attr.clone());
    let ud = lua.create_userdata(lua_attr)?;
    Ok(Value::UserData(ud))
}

/// Wrap the given block cell as a BlockRef LuaAttr proxy. Writes through
/// the returned userdata propagate back to the parent block's Attr.
pub fn attr_to_lua_userdata_for_block(lua: &Lua, block: Rc<RefCell<Block>>) -> Result<Value> {
    let ud = lua.create_userdata(LuaAttr::for_block(block))?;
    Ok(Value::UserData(ud))
}

/// Wrap the given inline cell as an InlineRef LuaAttr proxy. Writes
/// through the returned userdata propagate back to the parent inline's
/// Attr.
pub fn attr_to_lua_userdata_for_inline(lua: &Lua, inline: Rc<RefCell<Inline>>) -> Result<Value> {
    let ud = lua.create_userdata(LuaAttr::for_inline(inline))?;
    Ok(Value::UserData(ud))
}

/// Block-level `.attributes` shortcut: a LuaAttributesProxy bound to
/// the block's Attr. `cb.attributes[k] = v` is equivalent to
/// `cb.attr.attributes[k] = v`.
pub fn attributes_proxy_for_block(lua: &Lua, block: Rc<RefCell<Block>>) -> Result<Value> {
    let ud = lua.create_userdata(LuaAttributesProxy::new(LuaAttr::for_block(block)))?;
    Ok(Value::UserData(ud))
}

/// Block-level `.classes` shortcut: a LuaClassesProxy bound to the
/// block's Attr.
/// Block-level `.classes` read: a pandoc-List table of the classes.
/// (Named for its proxy-userdata history; since bd-tzwcof0n it returns
/// a List table — write-back persistence comes from the element's
/// PropertyCache, which caches it under the "classes" key.)
pub fn classes_proxy_for_block(lua: &Lua, block: Rc<RefCell<Block>>) -> Result<Value> {
    let attr = LuaAttr::for_block(block);
    attr.with_attr(|a| super::list::create_string_list_table(lua, &a.1))
}

/// Inline-level `.attributes` shortcut.
pub fn attributes_proxy_for_inline(lua: &Lua, inline: Rc<RefCell<Inline>>) -> Result<Value> {
    let ud = lua.create_userdata(LuaAttributesProxy::new(LuaAttr::for_inline(inline)))?;
    Ok(Value::UserData(ud))
}

/// Inline-level `.classes` shortcut.
/// Inline-level `.classes` read — see `classes_proxy_for_block`.
pub fn classes_proxy_for_inline(lua: &Lua, inline: Rc<RefCell<Inline>>) -> Result<Value> {
    let attr = LuaAttr::for_inline(inline);
    attr.with_attr(|a| super::list::create_string_list_table(lua, &a.1))
}

/// Parse a citation-mode string, erroring loudly on anything that is
/// not one of Pandoc's three constructor names (Pandoc's `peekRead`
/// does the same: `Could not read: <value>`).
pub(crate) fn parse_citation_mode(s: &str) -> Result<crate::pandoc::CitationMode> {
    use crate::pandoc::CitationMode;
    match s {
        "AuthorInText" => Ok(CitationMode::AuthorInText),
        "SuppressAuthor" => Ok(CitationMode::SuppressAuthor),
        "NormalCitation" => Ok(CitationMode::NormalCitation),
        other => Err(Error::runtime(format!(
            "invalid citation mode '{other}' (expected NormalCitation, AuthorInText, or SuppressAuthor)"
        ))),
    }
}

fn citation_mode_name(mode: &crate::pandoc::CitationMode) -> &'static str {
    use crate::pandoc::CitationMode;
    match mode {
        CitationMode::AuthorInText => "AuthorInText",
        CitationMode::SuppressAuthor => "SuppressAuthor",
        CitationMode::NormalCitation => "NormalCitation",
    }
}

/// Wrapper for a Pandoc `Citation` as typed Lua userdata, matching
/// pandoc-lua-marshal's `typeCitation` (bd-sgfiiktn S1; previously
/// `pandoc.Citation` returned a plain Lua table).
///
/// The inner `Citation` lives behind `Rc<RefCell<…>>` so the userdata
/// handed out inside a `cite.citations` List stays live: in-place
/// mutation (`c.id = 'x'`) lands in the cell, and the Cite element's
/// cached `citations` table re-reads the same cells at flush time.
/// The Inlines-valued `prefix`/`suffix` properties get the same
/// hslua-style cache+readback treatment elements use (aliased reads,
/// `:insert` persists — see [`PropertyCache`]).
///
/// Divergence note (registry-bound, bd-9p2686pc): property assignment
/// validates eagerly here, while Pandoc caches the raw value and only
/// errors at marshal-out. Programs that observe the difference error
/// in both implementations; only the timing and message differ.
#[derive(Debug, Clone)]
pub struct LuaCitation {
    pub cell: Rc<RefCell<crate::pandoc::Citation>>,
    pub(crate) cache: PropertyCache,
}

impl LuaCitation {
    pub fn new(citation: crate::pandoc::Citation) -> Self {
        LuaCitation {
            cell: Rc::new(RefCell::new(citation)),
            cache: PropertyCache::default(),
        }
    }

    fn is_cacheable_key(key: &str) -> bool {
        matches!(key, "prefix" | "suffix")
    }

    /// Write cached `prefix`/`suffix` tables back into the cell
    /// (hslua readback semantics). Idempotent.
    pub fn flush_property_cache(&self, lua: &Lua) -> Result<()> {
        let entries = match self.cache.begin_flush() {
            Some(entries) => entries,
            None => return Ok(()),
        };
        let mut result = Ok(());
        for (key, value) in entries {
            if let Err(e) = self.set_field(&key, value, lua) {
                result = Err(e);
                break;
            }
        }
        self.cache.end_flush();
        result
    }

    /// Flush, then deep-clone the inner `Citation` — the blessed way
    /// to marshal a `LuaCitation` back into a Rust value.
    pub fn extract_flushed(&self, lua: &Lua) -> Result<crate::pandoc::Citation> {
        self.flush_property_cache(lua)?;
        Ok(self.cell.borrow().clone())
    }

    fn get_field(&self, lua: &Lua, key: &str) -> Result<Value> {
        if key == "clone" {
            self.flush_property_cache(lua)?;
            let snapshot = self.cell.borrow().clone();
            return lua
                .create_function(move |lua, ()| {
                    lua.create_userdata(LuaCitation::new(snapshot.clone()))
                })?
                .into_lua(lua);
        }
        let inner = self.cell.borrow();
        match key {
            "id" => inner.id.clone().into_lua(lua),
            "mode" => citation_mode_name(&inner.mode).into_lua(lua),
            "prefix" => inlines_to_lua_table(lua, &inner.prefix),
            "suffix" => inlines_to_lua_table(lua, &inner.suffix),
            "note_num" => (inner.note_num as i64).into_lua(lua),
            "hash" => (inner.hash as i64).into_lua(lua),
            _ => Ok(Value::Nil),
        }
    }

    fn set_field(&self, key: &str, val: Value, lua: &Lua) -> Result<()> {
        match key {
            "id" => {
                let id = String::from_lua(val, lua)?;
                self.cell.borrow_mut().id = id;
                Ok(())
            }
            "mode" => {
                let s = String::from_lua(val, lua)?;
                let mode = parse_citation_mode(&s)?;
                self.cell.borrow_mut().mode = mode;
                Ok(())
            }
            "prefix" => {
                let inlines = peek_inlines_fuzzy(lua, val)?;
                self.cell.borrow_mut().prefix = inlines;
                Ok(())
            }
            "suffix" => {
                let inlines = peek_inlines_fuzzy(lua, val)?;
                self.cell.borrow_mut().suffix = inlines;
                Ok(())
            }
            "note_num" => {
                let n = i64::from_lua(val, lua)?;
                self.cell.borrow_mut().note_num = n as usize;
                Ok(())
            }
            "hash" => {
                let n = i64::from_lua(val, lua)?;
                self.cell.borrow_mut().hash = n as usize;
                Ok(())
            }
            _ => Err(Error::runtime(format!(
                "cannot set field '{key}' on Citation"
            ))),
        }
    }

    /// Structural equality ignoring source info, via the JSON writer's
    /// source-free serialization (same approach as elements — wrap the
    /// citations in a synthetic Cite so the maintained match logic does
    /// the comparison).
    fn structurally_eq(&self, other: &LuaCitation) -> bool {
        let wrap = |c: &LuaCitation| {
            Inline::Cite(crate::pandoc::Cite {
                citations: vec![c.cell.borrow().clone()],
                content: vec![],
                source_info: SourceInfo::generated(By::unknown()),
            })
        };
        inline_structurally_eq(&wrap(self), &wrap(other))
    }
}

impl UserData for LuaCitation {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            if let Some(cached) = this.cache.get(&key) {
                return Ok(cached);
            }
            let value = this.get_field(lua, &key)?;
            if LuaCitation::is_cacheable_key(&key) && matches!(value, Value::Table(_)) {
                this.cache.store(&key, &value);
            }
            Ok(value)
        });

        methods.add_meta_method(
            MetaMethod::NewIndex,
            |lua, this, (key, val): (String, Value)| {
                this.set_field(&key, val.clone(), lua)?;
                if LuaCitation::is_cacheable_key(&key) {
                    if matches!(val, Value::Table(_)) {
                        this.cache.store(&key, &val);
                    } else {
                        this.cache.remove(&key);
                    }
                }
                Ok(())
            },
        );

        methods.add_meta_method(MetaMethod::ToString, |lua, this, ()| {
            this.flush_property_cache(lua)?;
            Ok(super::show::show_citation(&this.cell.borrow()))
        });

        methods.add_meta_method(MetaMethod::Eq, |lua, this, other: Value| {
            Ok(match other {
                Value::UserData(ud) => match ud.borrow::<LuaCitation>() {
                    Ok(other_citation) => {
                        this.flush_property_cache(lua)?;
                        other_citation.flush_property_cache(lua)?;
                        this.structurally_eq(&other_citation)
                    }
                    Err(_) => false,
                },
                _ => false,
            })
        });
    }
}

/// Marshal one Lua value into a `Citation`: only `LuaCitation`
/// userdata is accepted, matching Pandoc's strict `peekCitation`
/// ("Citation expected, got <type>").
pub(crate) fn lua_value_to_citation(lua: &Lua, val: Value) -> Result<crate::pandoc::Citation> {
    match val {
        Value::UserData(ud) => match ud.borrow::<LuaCitation>() {
            Ok(citation) => citation.extract_flushed(lua),
            Err(_) => Err(Error::runtime(format!(
                "Citation expected, got {}",
                userdata_type_name(&ud)
            ))),
        },
        other => Err(Error::runtime(format!(
            "Citation expected, got {}",
            other.type_name()
        ))),
    }
}

/// Best-effort human-readable name for a userdata value in error
/// messages (element tag for our AST wrappers, Rust wrapper name
/// otherwise).
fn userdata_type_name(ud: &mlua::AnyUserData) -> String {
    if let Ok(inline) = ud.borrow::<LuaInline>() {
        return inline.tag_name().to_string();
    }
    if let Ok(block) = ud.borrow::<LuaBlock>() {
        return block.tag_name().to_string();
    }
    if ud.borrow::<LuaAttr>().is_ok() {
        return "Attr".to_string();
    }
    "userdata".to_string()
}

// FromLua implementation for converting Lua values back to Rust types
use mlua::FromLua;

impl FromLua for LuaInline {
    fn from_lua(value: Value, lua: &Lua) -> Result<Self> {
        match value {
            Value::UserData(ud) => {
                let lua_inline = ud.borrow::<LuaInline>()?;
                // Deep-clone the inner Inline into a fresh cell. This preserves
                // pre-refactor semantics: FromLua produces an independent
                // LuaInline, not a shared alias of the source cell.
                Ok(LuaInline::new(lua_inline.extract_flushed(lua)?))
            }
            _ => Err(Error::runtime("expected Inline userdata")),
        }
    }
}

impl FromLua for LuaBlock {
    fn from_lua(value: Value, lua: &Lua) -> Result<Self> {
        match value {
            Value::UserData(ud) => {
                let lua_block = ud.borrow::<LuaBlock>()?;
                // Deep-clone the inner Block into a fresh cell.
                Ok(LuaBlock::new(lua_block.extract_flushed(lua)?))
            }
            _ => Err(Error::runtime("expected Block userdata")),
        }
    }
}

/// Apply a filter to a single inline element. Per pandoc's subtree
/// rule, only the element's CHILDREN are offered to the filter — the
/// element itself is never visited and no synthetic singleton list is
/// created.
pub async fn walk_inline_with_filter(lua: &Lua, inline: &Inline, filter: &Table) -> Result<Inline> {
    use super::filter::{WalkingOrder, get_walking_order};
    match get_walking_order(filter)? {
        WalkingOrder::Typewise => super::walk::typewise_inline_element(lua, filter, inline).await,
        WalkingOrder::Topdown => super::walk::topdown_inline_element(lua, filter, inline).await,
    }
}

/// Apply a filter to a single block element (children only — see
/// `walk_inline_with_filter`).
pub async fn walk_block_with_filter(lua: &Lua, block: &Block, filter: &Table) -> Result<Block> {
    use super::filter::{WalkingOrder, get_walking_order};
    match get_walking_order(filter)? {
        WalkingOrder::Typewise => super::walk::typewise_block_element(lua, filter, block).await,
        WalkingOrder::Topdown => super::walk::topdown_block_element(lua, filter, block).await,
    }
}

/// Apply a filter table to a list of inlines. The list itself IS
/// offered to the `Inlines` function, and all four typewise passes run
/// (block functions reach blocks nested inside Notes).
pub async fn walk_inlines_with_filter(
    lua: &Lua,
    inlines: &[Inline],
    filter: &Table,
) -> Result<Vec<Inline>> {
    use super::filter::{WalkingOrder, get_walking_order};
    match get_walking_order(filter)? {
        WalkingOrder::Typewise => super::walk::typewise_inlines(lua, filter, inlines).await,
        WalkingOrder::Topdown => super::walk::topdown_inlines(lua, filter, inlines).await,
    }
}

/// Apply a filter table to a list of blocks (the list is offered to
/// the `Blocks` function).
pub async fn walk_blocks_with_filter(
    lua: &Lua,
    blocks: &[Block],
    filter: &Table,
) -> Result<Vec<Block>> {
    use super::filter::{WalkingOrder, get_walking_order};
    match get_walking_order(filter)? {
        WalkingOrder::Typewise => super::walk::typewise_blocks(lua, filter, blocks).await,
        WalkingOrder::Topdown => super::walk::topdown_blocks(lua, filter, blocks).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pandoc::Block;
    use crate::pandoc::Inline;
    use crate::pandoc::{
        AttrSourceInfo, BlockQuote, BulletList, Caption, CaptionBlock, Cite, Code, CodeBlock,
        DefinitionList, Delete, Div, EditComment, Emph, Figure, Header, Highlight, HorizontalRule,
        Image, Insert, LineBlock, LineBreak, Link, ListNumberDelim, ListNumberStyle, Math,
        MathType, MetaBlock, Note, NoteDefinitionFencedBlock, NoteDefinitionPara, NoteReference,
        OrderedList, Paragraph, Plain, QuoteType, Quoted, RawBlock, RawInline, Shortcode,
        SmallCaps, SoftBreak, Space, Span, Str, Strikeout, Strong, Subscript, Superscript,
        TableFoot, TableHead, TargetSourceInfo, Underline,
    };
    // Rename pandoc Table to avoid conflict with mlua Table
    use crate::pandoc::Table as PandocTable;

    // Helper to create default SourceInfo
    fn si() -> quarto_source_map::SourceInfo {
        quarto_source_map::SourceInfo::for_test()
    }

    // Helper to create empty attr source info
    fn attr_si() -> AttrSourceInfo {
        AttrSourceInfo::empty()
    }

    // Helper to create empty target source info
    fn target_si() -> TargetSourceInfo {
        TargetSourceInfo::empty()
    }

    // Helper to create empty Caption
    fn empty_caption() -> Caption {
        Caption {
            short: None,
            long: None,
            source_info: si(),
        }
    }

    // Helper to create empty TableHead
    fn empty_table_head() -> TableHead {
        TableHead {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            rows: vec![],
            source_info: si(),
        }
    }

    // Helper to create empty TableFoot
    fn empty_table_foot() -> TableFoot {
        TableFoot {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            rows: vec![],
            source_info: si(),
        }
    }

    // ========== LuaInline::tag_name tests ==========

    #[test]
    fn test_lua_inline_tag_name_str() {
        let inline = Inline::Str(Str {
            text: "hello".into(),
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Str");
    }

    #[test]
    fn test_lua_inline_tag_name_emph() {
        let inline = Inline::Emph(Emph {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Emph");
    }

    #[test]
    fn test_lua_inline_tag_name_underline() {
        let inline = Inline::Underline(Underline {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Underline");
    }

    #[test]
    fn test_lua_inline_tag_name_strong() {
        let inline = Inline::Strong(Strong {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Strong");
    }

    #[test]
    fn test_lua_inline_tag_name_strikeout() {
        let inline = Inline::Strikeout(Strikeout {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Strikeout");
    }

    #[test]
    fn test_lua_inline_tag_name_superscript() {
        let inline = Inline::Superscript(Superscript {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Superscript");
    }

    #[test]
    fn test_lua_inline_tag_name_subscript() {
        let inline = Inline::Subscript(Subscript {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Subscript");
    }

    #[test]
    fn test_lua_inline_tag_name_smallcaps() {
        let inline = Inline::SmallCaps(SmallCaps {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "SmallCaps");
    }

    #[test]
    fn test_lua_inline_tag_name_quoted() {
        let inline = Inline::Quoted(Quoted {
            quote_type: QuoteType::SingleQuote,
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Quoted");
    }

    #[test]
    fn test_lua_inline_tag_name_cite() {
        let inline = Inline::Cite(Cite {
            citations: vec![],
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Cite");
    }

    #[test]
    fn test_lua_inline_tag_name_code() {
        let inline = Inline::Code(Code {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            text: "code".into(),
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Code");
    }

    #[test]
    fn test_lua_inline_tag_name_space() {
        let inline = Inline::Space(Space { source_info: si() });
        assert_eq!(LuaInline::new(inline).tag_name(), "Space");
    }

    #[test]
    fn test_lua_inline_tag_name_soft_break() {
        let inline = Inline::SoftBreak(SoftBreak { source_info: si() });
        assert_eq!(LuaInline::new(inline).tag_name(), "SoftBreak");
    }

    #[test]
    fn test_lua_inline_tag_name_line_break() {
        let inline = Inline::LineBreak(LineBreak { source_info: si() });
        assert_eq!(LuaInline::new(inline).tag_name(), "LineBreak");
    }

    #[test]
    fn test_lua_inline_tag_name_math() {
        let inline = Inline::Math(Math {
            math_type: MathType::InlineMath,
            text: "x^2".into(),
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Math");
    }

    #[test]
    fn test_lua_inline_tag_name_raw_inline() {
        let inline = Inline::RawInline(RawInline {
            format: "html".into(),
            text: "<b>".into(),
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "RawInline");
    }

    #[test]
    fn test_lua_inline_tag_name_link() {
        let inline = Inline::Link(Link {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            target: ("url".into(), "title".into()),
            target_source: target_si(),
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Link");
    }

    #[test]
    fn test_lua_inline_tag_name_image() {
        let inline = Inline::Image(Image {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            target: ("src".into(), "alt".into()),
            target_source: target_si(),
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Image");
    }

    #[test]
    fn test_lua_inline_tag_name_note() {
        let inline = Inline::Note(Note {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Note");
    }

    #[test]
    fn test_lua_inline_tag_name_span() {
        let inline = Inline::Span(Span {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Span");
    }

    #[test]
    fn test_lua_inline_tag_name_shortcode() {
        let inline = Inline::Shortcode(Shortcode {
            is_escaped: false,
            name: "test".into(),
            positional_args: vec![],
            keyword_args: hashlink::LinkedHashMap::new(),
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Shortcode");
    }

    #[test]
    fn test_lua_inline_tag_name_note_reference() {
        let inline = Inline::NoteReference(NoteReference {
            id: "1".into(),
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "NoteReference");
    }

    #[test]
    fn test_lua_inline_tag_name_attr() {
        let inline = Inline::Attr(crate::pandoc::inline::InlineAttr::new(
            (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_si(),
            quarto_source_map::SourceInfo::for_test(),
        ));
        assert_eq!(LuaInline::new(inline).tag_name(), "Attr");
    }

    #[test]
    fn test_lua_inline_tag_name_insert() {
        let inline = Inline::Insert(Insert {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Insert");
    }

    #[test]
    fn test_lua_inline_tag_name_delete() {
        let inline = Inline::Delete(Delete {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Delete");
    }

    #[test]
    fn test_lua_inline_tag_name_highlight() {
        let inline = Inline::Highlight(Highlight {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "Highlight");
    }

    #[test]
    fn test_lua_inline_tag_name_edit_comment() {
        let inline = Inline::EditComment(EditComment {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline::new(inline).tag_name(), "EditComment");
    }

    #[test]
    fn test_lua_inline_tag_name_custom() {
        let inline = Inline::Custom(crate::pandoc::custom::CustomNode::new(
            "test-type",
            (String::new(), vec![], hashlink::LinkedHashMap::new()),
            si(),
        ));
        assert_eq!(LuaInline::new(inline).tag_name(), "Custom");
    }

    // ========== LuaInline::field_names tests ==========

    #[test]
    fn test_lua_inline_field_names_str() {
        let inline = Inline::Str(Str {
            text: "hello".into(),
            source_info: si(),
        });
        assert_eq!(
            LuaInline::new(inline).field_names(),
            &["tag", "text", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_inline_field_names_emph() {
        let inline = Inline::Emph(Emph {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(
            LuaInline::new(inline).field_names(),
            &["tag", "content", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_inline_field_names_quoted() {
        let inline = Inline::Quoted(Quoted {
            quote_type: QuoteType::DoubleQuote,
            content: vec![],
            source_info: si(),
        });
        assert_eq!(
            LuaInline::new(inline).field_names(),
            &["tag", "quotetype", "content", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_inline_field_names_cite() {
        let inline = Inline::Cite(Cite {
            citations: vec![],
            content: vec![],
            source_info: si(),
        });
        assert_eq!(
            LuaInline::new(inline).field_names(),
            &["tag", "content", "citations", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_inline_field_names_code() {
        let inline = Inline::Code(Code {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            text: "code".into(),
            source_info: si(),
        });
        assert_eq!(
            LuaInline::new(inline).field_names(),
            &[
                "tag",
                "text",
                "attr",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk"
            ]
        );
    }

    #[test]
    fn test_lua_inline_field_names_space() {
        let inline = Inline::Space(Space { source_info: si() });
        assert_eq!(
            LuaInline::new(inline).field_names(),
            &["tag", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_inline_field_names_math() {
        let inline = Inline::Math(Math {
            math_type: MathType::DisplayMath,
            text: "E=mc^2".into(),
            source_info: si(),
        });
        assert_eq!(
            LuaInline::new(inline).field_names(),
            &["tag", "mathtype", "text", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_inline_field_names_raw_inline() {
        let inline = Inline::RawInline(RawInline {
            format: "latex".into(),
            text: "\\alpha".into(),
            source_info: si(),
        });
        assert_eq!(
            LuaInline::new(inline).field_names(),
            &["tag", "format", "text", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_inline_field_names_link() {
        let inline = Inline::Link(Link {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            target: ("url".into(), "title".into()),
            target_source: target_si(),
            source_info: si(),
        });
        assert_eq!(
            LuaInline::new(inline).field_names(),
            &[
                "tag",
                "content",
                "target",
                "title",
                "attr",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk"
            ]
        );
    }

    #[test]
    fn test_lua_inline_field_names_image() {
        let inline = Inline::Image(Image {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            target: ("src".into(), "alt".into()),
            target_source: target_si(),
            source_info: si(),
        });
        assert_eq!(
            LuaInline::new(inline).field_names(),
            &[
                "tag",
                "content",
                "caption",
                "src",
                "title",
                "attr",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk"
            ]
        );
    }

    #[test]
    fn test_lua_inline_field_names_note() {
        let inline = Inline::Note(Note {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(
            LuaInline::new(inline).field_names(),
            &["tag", "content", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_inline_field_names_span() {
        let inline = Inline::Span(Span {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(
            LuaInline::new(inline).field_names(),
            &[
                "tag",
                "content",
                "attr",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk"
            ]
        );
    }

    #[test]
    fn test_lua_inline_field_names_note_reference() {
        let inline = Inline::NoteReference(NoteReference {
            id: "1".into(),
            source_info: si(),
        });
        assert_eq!(
            LuaInline::new(inline).field_names(),
            &["tag", "id", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_inline_field_names_insert() {
        let inline = Inline::Insert(Insert {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(
            LuaInline::new(inline).field_names(),
            &[
                "tag",
                "content",
                "attr",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk"
            ]
        );
    }

    #[test]
    fn test_lua_inline_field_names_custom() {
        let inline = Inline::Custom(crate::pandoc::custom::CustomNode::new(
            "test-type",
            (String::new(), vec![], hashlink::LinkedHashMap::new()),
            si(),
        ));
        assert_eq!(LuaInline::new(inline).field_names(), &["tag", "clone"]);
    }

    // ========== LuaBlock::tag_name tests ==========

    #[test]
    fn test_lua_block_tag_name_plain() {
        let block = Block::Plain(Plain {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock::new(block).tag_name(), "Plain");
    }

    #[test]
    fn test_lua_block_tag_name_paragraph() {
        let block = Block::Paragraph(Paragraph {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock::new(block).tag_name(), "Para");
    }

    #[test]
    fn test_lua_block_tag_name_line_block() {
        let block = Block::LineBlock(LineBlock {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock::new(block).tag_name(), "LineBlock");
    }

    #[test]
    fn test_lua_block_tag_name_code_block() {
        let block = Block::CodeBlock(CodeBlock {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            text: "code".into(),
            source_info: si(),
        });
        assert_eq!(LuaBlock::new(block).tag_name(), "CodeBlock");
    }

    #[test]
    fn test_lua_block_tag_name_raw_block() {
        let block = Block::RawBlock(RawBlock {
            format: "html".into(),
            text: "<div>".into(),
            source_info: si(),
        });
        assert_eq!(LuaBlock::new(block).tag_name(), "RawBlock");
    }

    #[test]
    fn test_lua_block_tag_name_block_quote() {
        let block = Block::BlockQuote(BlockQuote {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock::new(block).tag_name(), "BlockQuote");
    }

    #[test]
    fn test_lua_block_tag_name_ordered_list() {
        let block = Block::OrderedList(OrderedList {
            attr: (1, ListNumberStyle::Decimal, ListNumberDelim::Period),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock::new(block).tag_name(), "OrderedList");
    }

    #[test]
    fn test_lua_block_tag_name_bullet_list() {
        let block = Block::BulletList(BulletList {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock::new(block).tag_name(), "BulletList");
    }

    #[test]
    fn test_lua_block_tag_name_definition_list() {
        let block = Block::DefinitionList(DefinitionList {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock::new(block).tag_name(), "DefinitionList");
    }

    #[test]
    fn test_lua_block_tag_name_header() {
        let block = Block::Header(Header {
            level: 1,
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock::new(block).tag_name(), "Header");
    }

    #[test]
    fn test_lua_block_tag_name_horizontal_rule() {
        let block = Block::HorizontalRule(HorizontalRule { source_info: si() });
        assert_eq!(LuaBlock::new(block).tag_name(), "HorizontalRule");
    }

    #[test]
    fn test_lua_block_tag_name_table() {
        let block = Block::Table(PandocTable {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            caption: empty_caption(),
            colspec: vec![],
            head: empty_table_head(),
            bodies: vec![],
            foot: empty_table_foot(),
            source_info: si(),
        });
        assert_eq!(LuaBlock::new(block).tag_name(), "Table");
    }

    #[test]
    fn test_lua_block_tag_name_figure() {
        let block = Block::Figure(Figure {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            caption: empty_caption(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock::new(block).tag_name(), "Figure");
    }

    #[test]
    fn test_lua_block_tag_name_div() {
        let block = Block::Div(Div {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock::new(block).tag_name(), "Div");
    }

    #[test]
    fn test_lua_block_tag_name_block_metadata() {
        let block = Block::BlockMetadata(MetaBlock {
            meta: quarto_pandoc_types::ConfigValue::default(),
            source_info: si(),
        });
        assert_eq!(LuaBlock::new(block).tag_name(), "BlockMetadata");
    }

    #[test]
    fn test_lua_block_tag_name_note_definition_para() {
        let block = Block::NoteDefinitionPara(NoteDefinitionPara {
            id: "1".into(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock::new(block).tag_name(), "NoteDefinitionPara");
    }

    #[test]
    fn test_lua_block_tag_name_note_definition_fenced_block() {
        let block = Block::NoteDefinitionFencedBlock(NoteDefinitionFencedBlock {
            id: "1".into(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock::new(block).tag_name(), "NoteDefinitionFencedBlock");
    }

    #[test]
    fn test_lua_block_tag_name_caption_block() {
        let block = Block::CaptionBlock(CaptionBlock {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock::new(block).tag_name(), "CaptionBlock");
    }

    #[test]
    fn test_lua_block_tag_name_custom() {
        let block = Block::Custom(crate::pandoc::custom::CustomNode::new(
            "test-type",
            (String::new(), vec![], hashlink::LinkedHashMap::new()),
            si(),
        ));
        assert_eq!(LuaBlock::new(block).tag_name(), "Custom");
    }

    // ========== LuaBlock::field_names tests ==========

    #[test]
    fn test_lua_block_field_names_plain() {
        let block = Block::Plain(Plain {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(
            LuaBlock::new(block).field_names(),
            &["tag", "content", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_block_field_names_paragraph() {
        let block = Block::Paragraph(Paragraph {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(
            LuaBlock::new(block).field_names(),
            &["tag", "content", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_block_field_names_code_block() {
        let block = Block::CodeBlock(CodeBlock {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            text: "code".into(),
            source_info: si(),
        });
        assert_eq!(
            LuaBlock::new(block).field_names(),
            &[
                "tag",
                "text",
                "attr",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk"
            ]
        );
    }

    #[test]
    fn test_lua_block_field_names_raw_block() {
        let block = Block::RawBlock(RawBlock {
            format: "html".into(),
            text: "<div>".into(),
            source_info: si(),
        });
        assert_eq!(
            LuaBlock::new(block).field_names(),
            &["tag", "format", "text", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_block_field_names_header() {
        let block = Block::Header(Header {
            level: 1,
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(
            LuaBlock::new(block).field_names(),
            &[
                "tag",
                "level",
                "content",
                "attr",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk"
            ]
        );
    }

    #[test]
    fn test_lua_block_field_names_ordered_list() {
        let block = Block::OrderedList(OrderedList {
            attr: (1, ListNumberStyle::Decimal, ListNumberDelim::Period),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(
            LuaBlock::new(block).field_names(),
            &[
                "tag",
                "content",
                "listAttributes",
                "start",
                "style",
                "delimiter",
                "clone",
                "walk"
            ]
        );
    }

    #[test]
    fn test_lua_block_field_names_bullet_list() {
        let block = Block::BulletList(BulletList {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(
            LuaBlock::new(block).field_names(),
            &["tag", "content", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_block_field_names_table() {
        let block = Block::Table(PandocTable {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            caption: empty_caption(),
            colspec: vec![],
            head: empty_table_head(),
            bodies: vec![],
            foot: empty_table_foot(),
            source_info: si(),
        });
        assert_eq!(
            LuaBlock::new(block).field_names(),
            &[
                "tag",
                "attr",
                "caption",
                "colspecs",
                "head",
                "bodies",
                "foot",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk"
            ]
        );
    }

    #[test]
    fn test_lua_block_field_names_figure() {
        let block = Block::Figure(Figure {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            caption: empty_caption(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(
            LuaBlock::new(block).field_names(),
            &[
                "tag",
                "content",
                "attr",
                "caption",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk"
            ]
        );
    }

    #[test]
    fn test_lua_block_field_names_div() {
        let block = Block::Div(Div {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(
            LuaBlock::new(block).field_names(),
            &[
                "tag",
                "content",
                "attr",
                "identifier",
                "classes",
                "attributes",
                "clone",
                "walk"
            ]
        );
    }

    #[test]
    fn test_lua_block_field_names_horizontal_rule() {
        let block = Block::HorizontalRule(HorizontalRule { source_info: si() });
        assert_eq!(
            LuaBlock::new(block).field_names(),
            &["tag", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_block_field_names_custom() {
        let block = Block::Custom(crate::pandoc::custom::CustomNode::new(
            "test-type",
            (String::new(), vec![], hashlink::LinkedHashMap::new()),
            si(),
        ));
        assert_eq!(LuaBlock::new(block).field_names(), &["tag", "clone"]);
    }

    // ========== LuaAttr tests ==========

    #[test]
    fn test_lua_attr_new() {
        let attr = (
            "id".into(),
            vec!["class1".into()],
            hashlink::LinkedHashMap::new(),
        );
        let lua_attr = LuaAttr::new(attr);
        assert_eq!(lua_attr.identifier(), "id");
        assert_eq!(lua_attr.classes(), &["class1".to_string()]);
        assert!(lua_attr.attributes().is_empty());
    }

    #[test]
    fn test_lua_attr_identifier() {
        let attr = ("my-id".into(), vec![], hashlink::LinkedHashMap::new());
        let lua_attr = LuaAttr::new(attr);
        assert_eq!(lua_attr.identifier(), "my-id");
    }

    #[test]
    fn test_lua_attr_classes() {
        let attr = (
            String::new(),
            vec!["a".into(), "b".into()],
            hashlink::LinkedHashMap::new(),
        );
        let lua_attr = LuaAttr::new(attr);
        assert_eq!(lua_attr.classes(), &["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn test_lua_attr_attributes() {
        let mut attrs = hashlink::LinkedHashMap::new();
        attrs.insert("key".into(), "value".into());
        let attr = (String::new(), vec![], attrs);
        let lua_attr = LuaAttr::new(attr);
        assert_eq!(lua_attr.attributes().get("key"), Some(&"value".to_string()));
    }

    // ========== split_string_to_inlines tests ==========

    /// Helper to extract text representation from inlines for easier assertion
    fn inlines_to_tags(inlines: &[Inline]) -> Vec<String> {
        inlines
            .iter()
            .map(|i| match i {
                Inline::Str(s) => format!("Str({})", s.text),
                Inline::Space(_) => "Space".to_string(),
                Inline::SoftBreak(_) => "SoftBreak".to_string(),
                _ => format!("{:?}", i),
            })
            .collect()
    }

    #[test]
    fn test_split_string_empty() {
        let result = split_string_to_inlines("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_split_string_single_word() {
        let result = split_string_to_inlines("hello");
        assert_eq!(inlines_to_tags(&result), vec!["Str(hello)"]);
    }

    #[test]
    fn test_split_string_multi_word() {
        let result = split_string_to_inlines("hello world");
        assert_eq!(
            inlines_to_tags(&result),
            vec!["Str(hello)", "Space", "Str(world)"]
        );
    }

    #[test]
    fn test_split_string_multiple_spaces_collapse() {
        let result = split_string_to_inlines("hello   world");
        assert_eq!(
            inlines_to_tags(&result),
            vec!["Str(hello)", "Space", "Str(world)"]
        );
    }

    #[test]
    fn test_split_string_newline_becomes_softbreak() {
        let result = split_string_to_inlines("hello\nworld");
        assert_eq!(
            inlines_to_tags(&result),
            vec!["Str(hello)", "SoftBreak", "Str(world)"]
        );
    }

    #[test]
    fn test_split_string_mixed_space_newline_becomes_softbreak() {
        let result = split_string_to_inlines("hello \n world");
        assert_eq!(
            inlines_to_tags(&result),
            vec!["Str(hello)", "SoftBreak", "Str(world)"]
        );
    }

    #[test]
    fn test_split_string_tab_is_space() {
        let result = split_string_to_inlines("hello\tworld");
        assert_eq!(
            inlines_to_tags(&result),
            vec!["Str(hello)", "Space", "Str(world)"]
        );
    }

    #[test]
    fn test_split_string_leading_trailing_space() {
        let result = split_string_to_inlines(" hello ");
        assert_eq!(
            inlines_to_tags(&result),
            vec!["Space", "Str(hello)", "Space"]
        );
    }

    #[test]
    fn test_split_string_carriage_return() {
        let result = split_string_to_inlines("hello\r\nworld");
        assert_eq!(
            inlines_to_tags(&result),
            vec!["Str(hello)", "SoftBreak", "Str(world)"]
        );
    }

    #[test]
    fn test_split_string_only_spaces() {
        let result = split_string_to_inlines("   ");
        assert_eq!(inlines_to_tags(&result), vec!["Space"]);
    }

    #[test]
    fn test_split_string_only_newline() {
        let result = split_string_to_inlines("\n");
        assert_eq!(inlines_to_tags(&result), vec!["SoftBreak"]);
    }

    // ========== peek_inlines_fuzzy tests ==========

    #[test]
    fn test_peek_inlines_fuzzy_string_word_split() {
        let lua = Lua::new();
        let val = Value::String(lua.create_string("hello world").unwrap());
        let result = peek_inlines_fuzzy(&lua, val).unwrap();
        assert_eq!(
            inlines_to_tags(&result),
            vec!["Str(hello)", "Space", "Str(world)"]
        );
    }

    #[test]
    fn test_peek_inlines_fuzzy_table_of_inlines() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table
            .set(
                1,
                lua.create_userdata(LuaInline::new(Inline::Str(Str {
                    text: "a".into(),
                    source_info: si(),
                })))
                .unwrap(),
            )
            .unwrap();
        table
            .set(
                2,
                lua.create_userdata(LuaInline::new(Inline::Space(Space { source_info: si() })))
                    .unwrap(),
            )
            .unwrap();
        let result = peek_inlines_fuzzy(&lua, Value::Table(table)).unwrap();
        assert_eq!(inlines_to_tags(&result), vec!["Str(a)", "Space"]);
    }

    #[test]
    fn test_peek_inlines_fuzzy_table_with_mixed_strings() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set(1, lua.create_string("hello").unwrap()).unwrap();
        table
            .set(
                2,
                lua.create_userdata(LuaInline::new(Inline::Space(Space { source_info: si() })))
                    .unwrap(),
            )
            .unwrap();
        table.set(3, lua.create_string("world").unwrap()).unwrap();
        let result = peek_inlines_fuzzy(&lua, Value::Table(table)).unwrap();
        assert_eq!(
            inlines_to_tags(&result),
            vec!["Str(hello)", "Space", "Str(world)"]
        );
    }

    #[test]
    fn test_peek_inlines_fuzzy_single_userdata() {
        let lua = Lua::new();
        let ud = lua
            .create_userdata(LuaInline::new(Inline::Str(Str {
                text: "solo".into(),
                source_info: si(),
            })))
            .unwrap();
        let result = peek_inlines_fuzzy(&lua, Value::UserData(ud)).unwrap();
        assert_eq!(inlines_to_tags(&result), vec!["Str(solo)"]);
    }

    #[test]
    fn test_peek_inlines_fuzzy_error_on_number() {
        let lua = Lua::new();
        let result = peek_inlines_fuzzy(&lua, Value::Integer(42));
        assert!(result.is_err());
    }

    #[test]
    fn test_peek_inline_fuzzy_string_no_word_split() {
        let lua = Lua::new();
        let val = Value::String(lua.create_string("hello world").unwrap());
        let result = peek_inline_fuzzy(&lua, val).unwrap();
        match result {
            Inline::Str(s) => assert_eq!(s.text, "hello world"),
            _ => panic!("expected Str, got {:?}", result),
        }
    }

    // ========== peek_blocks_fuzzy tests ==========

    #[test]
    fn test_peek_blocks_fuzzy_table_of_blocks() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table
            .set(
                1,
                lua.create_userdata(LuaBlock::new(Block::HorizontalRule(HorizontalRule {
                    source_info: si(),
                })))
                .unwrap(),
            )
            .unwrap();
        let result = peek_blocks_fuzzy(&lua, Value::Table(table)).unwrap();
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], Block::HorizontalRule(_)));
    }

    #[test]
    fn test_peek_blocks_fuzzy_single_block() {
        let lua = Lua::new();
        let ud = lua
            .create_userdata(LuaBlock::new(Block::HorizontalRule(HorizontalRule {
                source_info: si(),
            })))
            .unwrap();
        let result = peek_blocks_fuzzy(&lua, Value::UserData(ud)).unwrap();
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], Block::HorizontalRule(_)));
    }

    #[test]
    fn test_peek_blocks_fuzzy_string_becomes_plain() {
        let lua = Lua::new();
        let val = Value::String(lua.create_string("hello").unwrap());
        let result = peek_blocks_fuzzy(&lua, val).unwrap();
        assert_eq!(result.len(), 1);
        match &result[0] {
            Block::Plain(p) => {
                assert_eq!(inlines_to_tags(&p.content), vec!["Str(hello)"]);
            }
            _ => panic!("expected Plain, got {:?}", result[0]),
        }
    }

    #[test]
    fn test_peek_blocks_fuzzy_table_of_inlines_becomes_multiple_plains() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table
            .set(
                1,
                lua.create_userdata(LuaInline::new(Inline::Str(Str {
                    text: "x".into(),
                    source_info: si(),
                })))
                .unwrap(),
            )
            .unwrap();
        table
            .set(
                2,
                lua.create_userdata(LuaInline::new(Inline::Str(Str {
                    text: "y".into(),
                    source_info: si(),
                })))
                .unwrap(),
            )
            .unwrap();
        let result = peek_blocks_fuzzy(&lua, Value::Table(table)).unwrap();
        assert_eq!(result.len(), 2);
        match &result[0] {
            Block::Plain(p) => assert_eq!(inlines_to_tags(&p.content), vec!["Str(x)"]),
            _ => panic!("expected Plain"),
        }
        match &result[1] {
            Block::Plain(p) => assert_eq!(inlines_to_tags(&p.content), vec!["Str(y)"]),
            _ => panic!("expected Plain"),
        }
    }

    #[test]
    fn test_peek_blocks_fuzzy_error_on_number() {
        let lua = Lua::new();
        let result = peek_blocks_fuzzy(&lua, Value::Integer(42));
        assert!(result.is_err());
    }

    #[test]
    fn test_peek_block_fuzzy_string_becomes_plain_with_word_split() {
        let lua = Lua::new();
        let val = Value::String(lua.create_string("hello world").unwrap());
        let result = peek_block_fuzzy(&lua, val).unwrap();
        match result {
            Block::Plain(p) => {
                assert_eq!(
                    inlines_to_tags(&p.content),
                    vec!["Str(hello)", "Space", "Str(world)"]
                );
            }
            _ => panic!("expected Plain, got {:?}", result),
        }
    }
}
