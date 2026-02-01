/*
 * lua/types.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Lua userdata wrappers for Pandoc AST types.
 *
 * These wrappers expose Pandoc elements as Lua userdata with named field access,
 * matching Pandoc 2.17+ behavior where `type(elem)` returns "userdata".
 */

use mlua::{
    Error, IntoLua, Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods,
    UserDataRef, Value, Variadic,
};
use quarto_source_map::SourceInfo;

use crate::pandoc::{Block, Inline};

/// Wrapper for Pandoc Inline elements as Lua userdata
#[derive(Debug, Clone)]
pub struct LuaInline(pub Inline);

impl LuaInline {
    /// Get the tag name for this inline element
    pub fn tag_name(&self) -> &'static str {
        match &self.0 {
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
            Inline::Attr(_, _) => "Attr",
            Inline::Insert(_) => "Insert",
            Inline::Delete(_) => "Delete",
            Inline::Highlight(_) => "Highlight",
            Inline::EditComment(_) => "EditComment",
            Inline::Custom(_) => "Custom",
        }
    }

    /// Get the list of field names for this inline element (for pairs iteration)
    pub fn field_names(&self) -> &'static [&'static str] {
        match &self.0 {
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
            Inline::Code(_) => &["tag", "text", "attr", "clone", "walk"],
            Inline::Space(_) | Inline::SoftBreak(_) | Inline::LineBreak(_) => {
                &["tag", "clone", "walk"]
            }
            Inline::Math(_) => &["tag", "mathtype", "text", "clone", "walk"],
            Inline::RawInline(_) => &["tag", "format", "text", "clone", "walk"],
            Inline::Link(_) => &["tag", "content", "target", "title", "attr", "clone", "walk"],
            Inline::Image(_) => &["tag", "content", "src", "title", "attr", "clone", "walk"],
            Inline::Note(_) => &["tag", "content", "clone", "walk"],
            Inline::Span(_) => &["tag", "content", "attr", "clone", "walk"],
            Inline::Insert(_)
            | Inline::Delete(_)
            | Inline::Highlight(_)
            | Inline::EditComment(_) => &["tag", "content", "attr", "clone", "walk"],
            Inline::NoteReference(_) => &["tag", "id", "clone", "walk"],
            Inline::Shortcode(_) | Inline::Attr(_, _) => &["tag", "clone", "walk"],
            // Custom nodes are not exposed to Lua filters yet
            Inline::Custom(_) => &["tag", "clone"],
        }
    }

    /// Get a field value by name
    pub fn get_field(&self, lua: &Lua, key: &str) -> Result<Value> {
        match (&self.0, key) {
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
            (Inline::Code(c), "attr") => attr_to_lua_table(lua, &c.attr),
            (Inline::Code(c), "identifier") => c.attr.0.clone().into_lua(lua),
            (Inline::Code(c), "classes") => {
                let table = lua.create_table()?;
                for (i, class) in c.attr.1.iter().enumerate() {
                    table.set(i + 1, class.clone())?;
                }
                Ok(Value::Table(table))
            }

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
            (Inline::Link(l), "attr") => attr_to_lua_table(lua, &l.attr),
            (Inline::Link(l), "identifier") => l.attr.0.clone().into_lua(lua),
            (Inline::Link(l), "classes") => {
                let table = lua.create_table()?;
                for (i, class) in l.attr.1.iter().enumerate() {
                    table.set(i + 1, class.clone())?;
                }
                Ok(Value::Table(table))
            }

            // Image
            (Inline::Image(i), "content") => inlines_to_lua_table(lua, &i.content),
            (Inline::Image(i), "src") => i.target.0.clone().into_lua(lua),
            (Inline::Image(i), "title") => i.target.1.clone().into_lua(lua),
            (Inline::Image(i), "attr") => attr_to_lua_table(lua, &i.attr),
            (Inline::Image(img), "identifier") => img.attr.0.clone().into_lua(lua),
            (Inline::Image(img), "classes") => {
                let table = lua.create_table()?;
                for (j, class) in img.attr.1.iter().enumerate() {
                    table.set(j + 1, class.clone())?;
                }
                Ok(Value::Table(table))
            }

            // Note
            (Inline::Note(n), "content") => blocks_to_lua_table(lua, &n.content),

            // Span (attr already covered above for other elements with attr)
            (Inline::Span(s), "attr") => attr_to_lua_table(lua, &s.attr),
            (Inline::Span(s), "identifier") => s.attr.0.clone().into_lua(lua),
            (Inline::Span(s), "classes") => {
                let table = lua.create_table()?;
                for (i, class) in s.attr.1.iter().enumerate() {
                    table.set(i + 1, class.clone())?;
                }
                Ok(Value::Table(table))
            }

            // Cite
            (Inline::Cite(c), "content") => inlines_to_lua_table(lua, &c.content),
            (Inline::Cite(c), "citations") => citations_to_lua_table(lua, &c.citations),

            // Insert (CriticMarkup-like)
            (Inline::Insert(ins), "content") => inlines_to_lua_table(lua, &ins.content),
            (Inline::Insert(ins), "attr") => attr_to_lua_table(lua, &ins.attr),
            (Inline::Insert(ins), "identifier") => ins.attr.0.clone().into_lua(lua),
            (Inline::Insert(ins), "classes") => {
                let table = lua.create_table()?;
                for (j, class) in ins.attr.1.iter().enumerate() {
                    table.set(j + 1, class.clone())?;
                }
                Ok(Value::Table(table))
            }

            // Delete (CriticMarkup-like)
            (Inline::Delete(d), "content") => inlines_to_lua_table(lua, &d.content),
            (Inline::Delete(d), "attr") => attr_to_lua_table(lua, &d.attr),
            (Inline::Delete(d), "identifier") => d.attr.0.clone().into_lua(lua),
            (Inline::Delete(d), "classes") => {
                let table = lua.create_table()?;
                for (j, class) in d.attr.1.iter().enumerate() {
                    table.set(j + 1, class.clone())?;
                }
                Ok(Value::Table(table))
            }

            // Highlight (CriticMarkup-like)
            (Inline::Highlight(h), "content") => inlines_to_lua_table(lua, &h.content),
            (Inline::Highlight(h), "attr") => attr_to_lua_table(lua, &h.attr),
            (Inline::Highlight(h), "identifier") => h.attr.0.clone().into_lua(lua),
            (Inline::Highlight(h), "classes") => {
                let table = lua.create_table()?;
                for (j, class) in h.attr.1.iter().enumerate() {
                    table.set(j + 1, class.clone())?;
                }
                Ok(Value::Table(table))
            }

            // EditComment (CriticMarkup-like)
            (Inline::EditComment(ec), "content") => inlines_to_lua_table(lua, &ec.content),
            (Inline::EditComment(ec), "attr") => attr_to_lua_table(lua, &ec.attr),
            (Inline::EditComment(ec), "identifier") => ec.attr.0.clone().into_lua(lua),
            (Inline::EditComment(ec), "classes") => {
                let table = lua.create_table()?;
                for (j, class) in ec.attr.1.iter().enumerate() {
                    table.set(j + 1, class.clone())?;
                }
                Ok(Value::Table(table))
            }

            // NoteReference
            (Inline::NoteReference(nr), "id") => nr.id.clone().into_lua(lua),

            // Tag and t are always available
            (_, "tag") => self.tag_name().into_lua(lua),
            (_, "t") => self.tag_name().into_lua(lua),

            // Methods - clone and walk are exposed as functions
            // Clone captures self so it can be called without arguments:
            //   elem:clone() and local f = elem.clone; f() both work
            (_, "clone") => {
                let inline = self.0.clone();
                lua.create_function(move |lua, ()| lua.create_userdata(LuaInline(inline.clone())))?
                    .into_lua(lua)
            }
            // Walk is called with method syntax: elem:walk { ... }
            // Lua passes (self, filter_table) to the function
            (_, "walk") => lua
                .create_function(|lua, (ud, filter_table): (UserDataRef<LuaInline>, Table)| {
                    let filtered = walk_inline_with_filter(lua, &ud.0, &filter_table)?;
                    lua.create_userdata(LuaInline(filtered))
                })?
                .into_lua(lua),

            // Unknown field
            _ => Ok(Value::Nil),
        }
    }

    /// Set a field value by name
    pub fn set_field(&mut self, key: &str, val: Value, lua: &Lua) -> Result<()> {
        match (&mut self.0, key) {
            // Str
            (Inline::Str(s), "text") => {
                s.text = String::from_lua(val, lua)?;
                Ok(())
            }

            // Content-bearing inlines
            (Inline::Emph(e), "content") => {
                e.content = lua_table_to_inlines(lua, val)?;
                Ok(())
            }
            (Inline::Strong(s), "content") => {
                s.content = lua_table_to_inlines(lua, val)?;
                Ok(())
            }
            (Inline::Underline(u), "content") => {
                u.content = lua_table_to_inlines(lua, val)?;
                Ok(())
            }
            (Inline::Strikeout(s), "content") => {
                s.content = lua_table_to_inlines(lua, val)?;
                Ok(())
            }
            (Inline::Superscript(s), "content") => {
                s.content = lua_table_to_inlines(lua, val)?;
                Ok(())
            }
            (Inline::Subscript(s), "content") => {
                s.content = lua_table_to_inlines(lua, val)?;
                Ok(())
            }
            (Inline::SmallCaps(s), "content") => {
                s.content = lua_table_to_inlines(lua, val)?;
                Ok(())
            }
            (Inline::Span(s), "content") => {
                s.content = lua_table_to_inlines(lua, val)?;
                Ok(())
            }

            // Link
            (Inline::Link(l), "content") => {
                l.content = lua_table_to_inlines(lua, val)?;
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
                i.content = lua_table_to_inlines(lua, val)?;
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

            // Quoted
            (Inline::Quoted(q), "content") => {
                q.content = lua_table_to_inlines(lua, val)?;
                Ok(())
            }

            // Note
            (Inline::Note(n), "content") => {
                n.content = lua_table_to_blocks(lua, val)?;
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
                c.content = lua_table_to_inlines(lua, val)?;
                Ok(())
            }
            (Inline::Cite(c), "citations") => {
                c.citations = lua_table_to_citations(lua, val)?;
                Ok(())
            }

            // Insert
            (Inline::Insert(ins), "content") => {
                ins.content = lua_table_to_inlines(lua, val)?;
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
                d.content = lua_table_to_inlines(lua, val)?;
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
                h.content = lua_table_to_inlines(lua, val)?;
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
                ec.content = lua_table_to_inlines(lua, val)?;
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
        // Dynamic field access via __index
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            this.get_field(lua, &key)
        });

        // Dynamic field assignment via __newindex
        methods.add_meta_method_mut(
            MetaMethod::NewIndex,
            |lua, this, (key, val): (String, Value)| this.set_field(&key, val, lua),
        );

        // Note: clone and walk are handled by get_field() rather than add_method()
        // to allow them to capture self in closures for direct function call syntax

        // __tostring for debugging
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("{}(...)", this.tag_name()))
        });

        // __pairs for iteration (for k, v in pairs(elem))
        methods.add_meta_method(MetaMethod::Pairs, |lua, this, ()| {
            let inline = this.0.clone();

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
                lua.create_userdata(LuaInline(inline))?,
                Value::Nil,
            ))
        });
    }
}

/// Wrapper for Pandoc Block elements as Lua userdata
#[derive(Debug, Clone)]
pub struct LuaBlock(pub Block);

impl LuaBlock {
    /// Get the tag name for this block element
    pub fn tag_name(&self) -> &'static str {
        match &self.0 {
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
        match &self.0 {
            Block::Plain(_) | Block::Paragraph(_) => &["tag", "content", "clone", "walk"],
            Block::LineBlock(_) => &["tag", "content", "clone", "walk"],
            Block::CodeBlock(_) => &[
                "tag",
                "text",
                "attr",
                "identifier",
                "classes",
                "clone",
                "walk",
            ],
            Block::RawBlock(_) => &["tag", "format", "text", "clone", "walk"],
            Block::BlockQuote(_) => &["tag", "content", "clone", "walk"],
            Block::OrderedList(_) => &["tag", "content", "start", "style", "clone", "walk"],
            Block::BulletList(_) => &["tag", "content", "clone", "walk"],
            Block::DefinitionList(_) => &["tag", "content", "clone", "walk"],
            Block::Header(_) => &[
                "tag",
                "level",
                "content",
                "attr",
                "identifier",
                "classes",
                "clone",
                "walk",
            ],
            Block::HorizontalRule(_) => &["tag", "clone", "walk"],
            Block::Table(_) => &["tag", "attr", "caption", "identifier", "clone", "walk"],
            Block::Figure(_) => &[
                "tag",
                "content",
                "attr",
                "caption",
                "identifier",
                "clone",
                "walk",
            ],
            Block::Div(_) => &[
                "tag",
                "content",
                "attr",
                "identifier",
                "classes",
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
        match (&self.0, key) {
            // Plain and Para have content
            (Block::Plain(p), "content") => inlines_to_lua_table(lua, &p.content),
            (Block::Paragraph(p), "content") => inlines_to_lua_table(lua, &p.content),

            // Header
            (Block::Header(h), "level") => (h.level as i64).into_lua(lua),
            (Block::Header(h), "content") => inlines_to_lua_table(lua, &h.content),
            (Block::Header(h), "attr") => attr_to_lua_table(lua, &h.attr),
            (Block::Header(h), "identifier") => h.attr.0.clone().into_lua(lua),
            (Block::Header(h), "classes") => {
                let table = lua.create_table()?;
                for (i, class) in h.attr.1.iter().enumerate() {
                    table.set(i + 1, class.clone())?;
                }
                Ok(Value::Table(table))
            }

            // CodeBlock
            (Block::CodeBlock(c), "text") => c.text.clone().into_lua(lua),
            (Block::CodeBlock(c), "attr") => attr_to_lua_table(lua, &c.attr),
            (Block::CodeBlock(c), "identifier") => c.attr.0.clone().into_lua(lua),
            (Block::CodeBlock(c), "classes") => {
                let table = lua.create_table()?;
                for (i, class) in c.attr.1.iter().enumerate() {
                    table.set(i + 1, class.clone())?;
                }
                Ok(Value::Table(table))
            }

            // RawBlock
            (Block::RawBlock(r), "text") => r.text.clone().into_lua(lua),
            (Block::RawBlock(r), "format") => r.format.clone().into_lua(lua),

            // BlockQuote
            (Block::BlockQuote(b), "content") => blocks_to_lua_table(lua, &b.content),

            // Div
            (Block::Div(d), "content") => blocks_to_lua_table(lua, &d.content),
            (Block::Div(d), "attr") => attr_to_lua_table(lua, &d.attr),
            (Block::Div(d), "identifier") => d.attr.0.clone().into_lua(lua),
            (Block::Div(d), "classes") => {
                let table = lua.create_table()?;
                for (i, class) in d.attr.1.iter().enumerate() {
                    table.set(i + 1, class.clone())?;
                }
                Ok(Value::Table(table))
            }

            // BulletList
            (Block::BulletList(b), "content") => {
                let table = lua.create_table()?;
                for (i, blocks) in b.content.iter().enumerate() {
                    table.set(i + 1, blocks_to_lua_table(lua, blocks)?)?;
                }
                Ok(Value::Table(table))
            }

            // OrderedList
            (Block::OrderedList(o), "content") => {
                let table = lua.create_table()?;
                for (i, blocks) in o.content.iter().enumerate() {
                    table.set(i + 1, blocks_to_lua_table(lua, blocks)?)?;
                }
                Ok(Value::Table(table))
            }
            (Block::OrderedList(o), "start") => (o.attr.0 as i64).into_lua(lua),
            (Block::OrderedList(o), "style") => {
                let style = match o.attr.1 {
                    crate::pandoc::ListNumberStyle::Default => "DefaultStyle",
                    crate::pandoc::ListNumberStyle::Decimal => "Decimal",
                    crate::pandoc::ListNumberStyle::LowerAlpha => "LowerAlpha",
                    crate::pandoc::ListNumberStyle::UpperAlpha => "UpperAlpha",
                    crate::pandoc::ListNumberStyle::LowerRoman => "LowerRoman",
                    crate::pandoc::ListNumberStyle::UpperRoman => "UpperRoman",
                    crate::pandoc::ListNumberStyle::Example => "Example",
                };
                style.into_lua(lua)
            }

            // Figure
            (Block::Figure(f), "content") => blocks_to_lua_table(lua, &f.content),
            (Block::Figure(f), "attr") => attr_to_lua_table(lua, &f.attr),
            (Block::Figure(f), "identifier") => f.attr.0.clone().into_lua(lua),

            // LineBlock
            (Block::LineBlock(l), "content") => {
                let table = lua.create_table()?;
                for (i, inlines) in l.content.iter().enumerate() {
                    table.set(i + 1, inlines_to_lua_table(lua, inlines)?)?;
                }
                Ok(Value::Table(table))
            }

            // DefinitionList - list of (term, definitions) pairs
            (Block::DefinitionList(d), "content") => {
                let table = lua.create_table()?;
                for (i, (term, defs)) in d.content.iter().enumerate() {
                    let pair_table = lua.create_table()?;
                    // First element is the term (inlines)
                    pair_table.set(1, inlines_to_lua_table(lua, term)?)?;
                    // Second element is the definitions (list of blocks)
                    let defs_table = lua.create_table()?;
                    for (j, def_blocks) in defs.iter().enumerate() {
                        defs_table.set(j + 1, blocks_to_lua_table(lua, def_blocks)?)?;
                    }
                    pair_table.set(2, defs_table)?;
                    table.set(i + 1, pair_table)?;
                }
                Ok(Value::Table(table))
            }

            // Figure caption
            (Block::Figure(f), "caption") => caption_to_lua_table(lua, &f.caption),

            // Table basic fields
            (Block::Table(t), "attr") => attr_to_lua_table(lua, &t.attr),
            (Block::Table(t), "caption") => caption_to_lua_table(lua, &t.caption),
            (Block::Table(t), "identifier") => t.attr.0.clone().into_lua(lua),

            // Tag and t are always available
            (_, "tag") => self.tag_name().into_lua(lua),
            (_, "t") => self.tag_name().into_lua(lua),

            // Methods - clone and walk are exposed as functions
            // Clone captures self so it can be called without arguments:
            //   elem:clone() and local f = elem.clone; f() both work
            (_, "clone") => {
                let block = self.0.clone();
                lua.create_function(move |lua, ()| lua.create_userdata(LuaBlock(block.clone())))?
                    .into_lua(lua)
            }
            // Walk is called with method syntax: elem:walk { ... }
            // Lua passes (self, filter_table) to the function
            (_, "walk") => lua
                .create_function(|lua, (ud, filter_table): (UserDataRef<LuaBlock>, Table)| {
                    let filtered = walk_block_with_filter(lua, &ud.0, &filter_table)?;
                    lua.create_userdata(LuaBlock(filtered))
                })?
                .into_lua(lua),

            // Unknown field
            _ => Ok(Value::Nil),
        }
    }

    /// Set a field value by name
    pub fn set_field(&mut self, key: &str, val: Value, lua: &Lua) -> Result<()> {
        match (&mut self.0, key) {
            // Plain and Para
            (Block::Plain(p), "content") => {
                p.content = lua_table_to_inlines(lua, val)?;
                Ok(())
            }
            (Block::Paragraph(p), "content") => {
                p.content = lua_table_to_inlines(lua, val)?;
                Ok(())
            }

            // Header
            (Block::Header(h), "level") => {
                h.level = i64::from_lua(val, lua)? as usize;
                Ok(())
            }
            (Block::Header(h), "content") => {
                h.content = lua_table_to_inlines(lua, val)?;
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
                b.content = lua_table_to_blocks(lua, val)?;
                Ok(())
            }

            // Div
            (Block::Div(d), "content") => {
                d.content = lua_table_to_blocks(lua, val)?;
                Ok(())
            }
            (Block::Div(d), "identifier") => {
                d.attr.0 = String::from_lua(val, lua)?;
                Ok(())
            }

            // Figure
            (Block::Figure(f), "content") => {
                f.content = lua_table_to_blocks(lua, val)?;
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
        // Dynamic field access via __index
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            this.get_field(lua, &key)
        });

        // Dynamic field assignment via __newindex
        methods.add_meta_method_mut(
            MetaMethod::NewIndex,
            |lua, this, (key, val): (String, Value)| this.set_field(&key, val, lua),
        );

        // Note: clone and walk are handled by get_field() rather than add_method()
        // to allow them to capture self in closures for direct function call syntax

        // __tostring for debugging
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("{}(...)", this.tag_name()))
        });

        // __pairs for iteration (for k, v in pairs(elem))
        methods.add_meta_method(MetaMethod::Pairs, |lua, this, ()| {
            let block = this.0.clone();

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
                lua.create_userdata(LuaBlock(block))?,
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

/// Convert Caption to Lua table
fn caption_to_lua_table(lua: &Lua, caption: &crate::pandoc::Caption) -> Result<Value> {
    let table = lua.create_table()?;

    // Short caption (optional)
    if let Some(short) = &caption.short {
        table.set("short", inlines_to_lua_table(lua, short)?)?;
    }

    // Long caption (blocks, optional)
    if let Some(long) = &caption.long {
        table.set("long", blocks_to_lua_table(lua, long)?)?;
    }

    Ok(Value::Table(table))
}

/// Convert Vec<Citation> to Lua table of citation tables
fn citations_to_lua_table(lua: &Lua, citations: &[crate::pandoc::Citation]) -> Result<Value> {
    let table = lua.create_table()?;
    for (i, citation) in citations.iter().enumerate() {
        let cit_table = lua.create_table()?;
        cit_table.set("id", citation.id.clone())?;
        cit_table.set("prefix", inlines_to_lua_table(lua, &citation.prefix)?)?;
        cit_table.set("suffix", inlines_to_lua_table(lua, &citation.suffix)?)?;
        cit_table.set(
            "mode",
            match citation.mode {
                crate::pandoc::CitationMode::AuthorInText => "AuthorInText",
                crate::pandoc::CitationMode::SuppressAuthor => "SuppressAuthor",
                crate::pandoc::CitationMode::NormalCitation => "NormalCitation",
            },
        )?;
        cit_table.set("note_num", citation.note_num as i64)?;
        cit_table.set("hash", citation.hash as i64)?;
        table.set(i + 1, cit_table)?;
    }
    Ok(Value::Table(table))
}

/// Convert Lua value to Attr - accepts either LuaAttr userdata or table
fn lua_value_to_attr(val: Value, _lua: &Lua) -> Result<crate::pandoc::Attr> {
    match val {
        Value::UserData(ud) => {
            let lua_attr = ud.borrow::<LuaAttr>()?;
            Ok(lua_attr.0.clone())
        }
        Value::Table(table) => {
            // Try to interpret as {identifier, classes, attributes} table
            let identifier: Option<String> =
                table.get(1).ok().or_else(|| table.get("identifier").ok());
            let classes: Option<Value> = table.get(2).ok().or_else(|| table.get("classes").ok());
            let attributes: Option<Value> =
                table.get(3).ok().or_else(|| table.get("attributes").ok());

            let id = identifier.unwrap_or_default();
            let cls = match classes {
                Some(Value::Table(t)) => {
                    let mut result = Vec::new();
                    for item in t.sequence_values::<String>() {
                        result.push(item?);
                    }
                    result
                }
                _ => Vec::new(),
            };
            let attrs = match attributes {
                Some(Value::Table(t)) => {
                    let mut result = hashlink::LinkedHashMap::new();
                    for pair in t.pairs::<String, String>() {
                        let (k, v) = pair?;
                        result.insert(k, v);
                    }
                    result
                }
                _ => hashlink::LinkedHashMap::new(),
            };
            Ok((id, cls, attrs))
        }
        _ => Err(Error::runtime("expected Attr userdata or table")),
    }
}

/// Convert Lua table to Vec<Citation>
fn lua_table_to_citations(_lua: &Lua, val: Value) -> Result<Vec<crate::pandoc::Citation>> {
    match val {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<Table>() {
                let cit_table = item?;
                let id: String = cit_table.get("id")?;
                let prefix_val: Value = cit_table.get("prefix")?;
                let prefix = lua_table_to_inlines(_lua, prefix_val)?;
                let suffix_val: Value = cit_table.get("suffix")?;
                let suffix = lua_table_to_inlines(_lua, suffix_val)?;
                let mode_str: String = cit_table.get("mode")?;
                let mode = match mode_str.as_str() {
                    "AuthorInText" => crate::pandoc::CitationMode::AuthorInText,
                    "SuppressAuthor" => crate::pandoc::CitationMode::SuppressAuthor,
                    _ => crate::pandoc::CitationMode::NormalCitation,
                };
                let note_num: i64 = cit_table.get("note_num").unwrap_or(0);
                let hash: i64 = cit_table.get("hash").unwrap_or(0);

                result.push(crate::pandoc::Citation {
                    id,
                    prefix,
                    suffix,
                    mode,
                    note_num: note_num as usize,
                    hash: hash as usize,
                    id_source: None, // Filter-created citations don't have source info
                });
            }
            Ok(result)
        }
        _ => Err(Error::runtime("expected table of citations")),
    }
}

/// Convert Attr to LuaAttr userdata (Pandoc-compatible)
fn attr_to_lua_table(lua: &Lua, attr: &crate::pandoc::Attr) -> Result<Value> {
    attr_to_lua_userdata(lua, attr)
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
                        let inlines = lua_table_to_inlines(lua, content)?;
                        Ok(MetaValue::MetaInlines(inlines))
                    }
                    "MetaBlocks" => {
                        let content: Value = table.get("content")?;
                        let blocks = lua_table_to_blocks(lua, content)?;
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

/// Convert Lua table to Vec<Inline>
pub fn lua_table_to_inlines(_lua: &Lua, val: Value) -> Result<Vec<Inline>> {
    match val {
        Value::Table(table) => {
            let mut inlines = Vec::new();
            for pair in table.sequence_values::<Value>() {
                let value = pair?;
                match value {
                    Value::UserData(ud) => {
                        if let Ok(lua_inline) = ud.borrow::<LuaInline>() {
                            inlines.push(lua_inline.0.clone());
                        } else {
                            return Err(Error::runtime("expected Inline userdata"));
                        }
                    }
                    _ => return Err(Error::runtime("expected table of Inline userdata")),
                }
            }
            Ok(inlines)
        }
        _ => Err(Error::runtime("expected table")),
    }
}

/// Convert Lua table to Vec<Block>
pub fn lua_table_to_blocks(_lua: &Lua, val: Value) -> Result<Vec<Block>> {
    match val {
        Value::Table(table) => {
            let mut blocks = Vec::new();
            for pair in table.sequence_values::<Value>() {
                let value = pair?;
                match value {
                    Value::UserData(ud) => {
                        if let Ok(lua_block) = ud.borrow::<LuaBlock>() {
                            blocks.push(lua_block.0.clone());
                        } else {
                            return Err(Error::runtime("expected Block userdata"));
                        }
                    }
                    _ => return Err(Error::runtime("expected table of Block userdata")),
                }
            }
            Ok(blocks)
        }
        _ => Err(Error::runtime("expected table")),
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
        if let Some(debug) = lua.inspect_stack(level) {
            let source = debug.source();
            let line = debug.curr_line();

            // Check if this is a Lua source (not a C function)
            if source.what != "C"
                && let Some(src) = source.source
            {
                // The source often starts with "@" for file paths
                let path = src.strip_prefix("@").unwrap_or(&src);
                // Convert line number from i32, negative means unknown
                let line_num = if line >= 0 { line as usize } else { 0 };
                return SourceInfo::filter_provenance(path.to_string(), line_num);
            }
        }
    }

    // Fallback if we couldn't get debug info
    SourceInfo::default()
}

/// Wrapper for Pandoc Attr (identifier, classes, attributes) as Lua userdata
///
/// Pandoc's Attr is a tuple: (identifier: String, classes: Vec<String>, attributes: HashMap<String, String>)
/// This wrapper exposes it as userdata with:
/// - Named field access: `attr.identifier`, `attr.classes`, `attr.attributes`
/// - Positional access: `attr[1]`, `attr[2]`, `attr[3]`
/// - Tag field: `attr.t` and `attr.tag` return "Attr"
#[derive(Debug, Clone)]
pub struct LuaAttr(pub crate::pandoc::Attr);

impl LuaAttr {
    /// Create a new LuaAttr from an Attr tuple
    pub fn new(attr: crate::pandoc::Attr) -> Self {
        LuaAttr(attr)
    }

    /// Get the identifier (first element of the tuple)
    pub fn identifier(&self) -> &str {
        &self.0.0
    }

    /// Get the classes (second element of the tuple)
    pub fn classes(&self) -> &[String] {
        &self.0.1
    }

    /// Get the attributes (third element of the tuple)
    pub fn attributes(&self) -> &hashlink::LinkedHashMap<String, String> {
        &self.0.2
    }

    /// Get a field value by name or index
    fn get_field(&self, lua: &Lua, key: Value) -> Result<Value> {
        match key {
            // Positional access (Lua uses 1-based indexing)
            Value::Integer(1) => self.identifier().to_string().into_lua(lua),
            Value::Integer(2) => {
                let table = lua.create_table()?;
                for (i, class) in self.classes().iter().enumerate() {
                    table.set(i + 1, class.clone())?;
                }
                Ok(Value::Table(table))
            }
            Value::Integer(3) => {
                let table = lua.create_table()?;
                for (key, value) in self.attributes().iter() {
                    table.set(key.clone(), value.clone())?;
                }
                Ok(Value::Table(table))
            }
            // Named field access
            Value::String(s) => {
                let borrowed = s.to_str()?;
                let key_str: &str = borrowed.as_ref();
                match key_str {
                    "identifier" => self.identifier().to_string().into_lua(lua),
                    "classes" => {
                        let table = lua.create_table()?;
                        for (i, class) in self.classes().iter().enumerate() {
                            table.set(i + 1, class.clone())?;
                        }
                        Ok(Value::Table(table))
                    }
                    "attributes" => {
                        let table = lua.create_table()?;
                        for (key, value) in self.attributes().iter() {
                            table.set(key.clone(), value.clone())?;
                        }
                        Ok(Value::Table(table))
                    }
                    "t" | "tag" => "Attr".into_lua(lua),
                    _ => Ok(Value::Nil),
                }
            }
            _ => Ok(Value::Nil),
        }
    }

    /// Set a field value by name or index
    fn set_field(&mut self, key: Value, val: Value, lua: &Lua) -> Result<()> {
        match key {
            // Positional access
            Value::Integer(1) => {
                self.0.0 = String::from_lua(val, lua)?;
                Ok(())
            }
            Value::Integer(2) => {
                self.0.1 = lua_table_to_strings(lua, val)?;
                Ok(())
            }
            Value::Integer(3) => {
                self.0.2 = lua_table_to_string_map(lua, val)?;
                Ok(())
            }
            // Named field access
            Value::String(s) => {
                let borrowed = s.to_str()?;
                let key_str: &str = borrowed.as_ref();
                match key_str {
                    "identifier" => {
                        self.0.0 = String::from_lua(val, lua)?;
                        Ok(())
                    }
                    "classes" => {
                        self.0.1 = lua_table_to_strings(lua, val)?;
                        Ok(())
                    }
                    "attributes" => {
                        self.0.2 = lua_table_to_string_map(lua, val)?;
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
        fields.add_field_method_get("identifier", |_, this| Ok(this.identifier().to_string()));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // Dynamic field access via __index for both named and positional access
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: Value| {
            this.get_field(lua, key)
        });

        // Dynamic field assignment via __newindex
        methods.add_meta_method_mut(
            MetaMethod::NewIndex,
            |lua, this, (key, val): (Value, Value)| this.set_field(key, val, lua),
        );

        // Clone method
        methods.add_method("clone", |lua, this, ()| {
            lua.create_userdata(LuaAttr(this.0.clone()))
        });

        // __tostring for debugging
        methods.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!(
                "Attr({:?}, {:?}, {:?})",
                this.identifier(),
                this.classes(),
                this.attributes()
            ))
        });

        // __len returns 3 (for the three components)
        methods.add_meta_method(MetaMethod::Len, |_, _, ()| Ok(3));
    }
}

impl FromLua for LuaAttr {
    fn from_lua(value: Value, _lua: &Lua) -> Result<Self> {
        match value {
            Value::UserData(ud) => {
                let lua_attr = ud.borrow::<LuaAttr>()?;
                Ok(LuaAttr(lua_attr.0.clone()))
            }
            _ => Err(Error::runtime("expected Attr userdata")),
        }
    }
}

/// Convert a Lua table of strings to Vec<String>
fn lua_table_to_strings(_lua: &Lua, val: Value) -> Result<Vec<String>> {
    match val {
        Value::Table(table) => {
            let mut result = Vec::new();
            for item in table.sequence_values::<String>() {
                result.push(item?);
            }
            Ok(result)
        }
        _ => Err(Error::runtime("expected table of strings")),
    }
}

/// Convert a Lua table to LinkedHashMap<String, String>
fn lua_table_to_string_map(
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
        _ => Err(Error::runtime("expected table of key-value pairs")),
    }
}

/// Convert Attr to LuaAttr userdata
pub fn attr_to_lua_userdata(lua: &Lua, attr: &crate::pandoc::Attr) -> Result<Value> {
    let lua_attr = LuaAttr::new(attr.clone());
    let ud = lua.create_userdata(lua_attr)?;
    Ok(Value::UserData(ud))
}

// FromLua implementation for converting Lua values back to Rust types
use mlua::FromLua;

impl FromLua for LuaInline {
    fn from_lua(value: Value, _lua: &Lua) -> Result<Self> {
        match value {
            Value::UserData(ud) => {
                let lua_inline = ud.borrow::<LuaInline>()?;
                Ok(LuaInline(lua_inline.0.clone()))
            }
            _ => Err(Error::runtime("expected Inline userdata")),
        }
    }
}

impl FromLua for LuaBlock {
    fn from_lua(value: Value, _lua: &Lua) -> Result<Self> {
        match value {
            Value::UserData(ud) => {
                let lua_block = ud.borrow::<LuaBlock>()?;
                Ok(LuaBlock(lua_block.0.clone()))
            }
            _ => Err(Error::runtime("expected Block userdata")),
        }
    }
}

/// Apply a filter to a single inline element using correct traversal
/// This wraps the element in a list and applies the list-walking function
pub fn walk_inline_with_filter(lua: &Lua, inline: &Inline, filter: &Table) -> Result<Inline> {
    let filtered = walk_inlines_with_filter(lua, &[inline.clone()], filter)?;
    Ok(filtered
        .into_iter()
        .next()
        .unwrap_or_else(|| inline.clone()))
}

/// Apply a filter to a single block element using correct traversal
/// This wraps the element in a list and applies the list-walking function
pub fn walk_block_with_filter(lua: &Lua, block: &Block, filter: &Table) -> Result<Block> {
    let filtered = walk_blocks_with_filter(lua, &[block.clone()], filter)?;
    Ok(filtered.into_iter().next().unwrap_or_else(|| block.clone()))
}

/// Apply a filter table to a list of inlines using correct two-pass or topdown traversal
pub fn walk_inlines_with_filter(
    lua: &Lua,
    inlines: &[Inline],
    filter: &Table,
) -> Result<Vec<Inline>> {
    use super::filter::{
        WalkingOrder, apply_typewise_inlines, get_walking_order, walk_inlines_topdown,
    };

    match get_walking_order(filter)? {
        WalkingOrder::Typewise => apply_typewise_inlines(lua, filter, inlines),
        WalkingOrder::Topdown => walk_inlines_topdown(lua, filter, inlines),
    }
}

/// Apply a filter table to a list of blocks using correct four-pass or topdown traversal
pub fn walk_blocks_with_filter(lua: &Lua, blocks: &[Block], filter: &Table) -> Result<Vec<Block>> {
    use super::filter::{
        WalkingOrder, apply_typewise_filter, get_walking_order, walk_blocks_topdown,
    };

    match get_walking_order(filter)? {
        WalkingOrder::Typewise => apply_typewise_filter(lua, filter, blocks),
        WalkingOrder::Topdown => walk_blocks_topdown(lua, filter, blocks),
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
    use std::collections::HashMap;

    // Helper to create default SourceInfo
    fn si() -> quarto_source_map::SourceInfo {
        quarto_source_map::SourceInfo::default()
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
        assert_eq!(LuaInline(inline).tag_name(), "Str");
    }

    #[test]
    fn test_lua_inline_tag_name_emph() {
        let inline = Inline::Emph(Emph {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "Emph");
    }

    #[test]
    fn test_lua_inline_tag_name_underline() {
        let inline = Inline::Underline(Underline {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "Underline");
    }

    #[test]
    fn test_lua_inline_tag_name_strong() {
        let inline = Inline::Strong(Strong {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "Strong");
    }

    #[test]
    fn test_lua_inline_tag_name_strikeout() {
        let inline = Inline::Strikeout(Strikeout {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "Strikeout");
    }

    #[test]
    fn test_lua_inline_tag_name_superscript() {
        let inline = Inline::Superscript(Superscript {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "Superscript");
    }

    #[test]
    fn test_lua_inline_tag_name_subscript() {
        let inline = Inline::Subscript(Subscript {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "Subscript");
    }

    #[test]
    fn test_lua_inline_tag_name_smallcaps() {
        let inline = Inline::SmallCaps(SmallCaps {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "SmallCaps");
    }

    #[test]
    fn test_lua_inline_tag_name_quoted() {
        let inline = Inline::Quoted(Quoted {
            quote_type: QuoteType::SingleQuote,
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "Quoted");
    }

    #[test]
    fn test_lua_inline_tag_name_cite() {
        let inline = Inline::Cite(Cite {
            citations: vec![],
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "Cite");
    }

    #[test]
    fn test_lua_inline_tag_name_code() {
        let inline = Inline::Code(Code {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            text: "code".into(),
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "Code");
    }

    #[test]
    fn test_lua_inline_tag_name_space() {
        let inline = Inline::Space(Space { source_info: si() });
        assert_eq!(LuaInline(inline).tag_name(), "Space");
    }

    #[test]
    fn test_lua_inline_tag_name_soft_break() {
        let inline = Inline::SoftBreak(SoftBreak { source_info: si() });
        assert_eq!(LuaInline(inline).tag_name(), "SoftBreak");
    }

    #[test]
    fn test_lua_inline_tag_name_line_break() {
        let inline = Inline::LineBreak(LineBreak { source_info: si() });
        assert_eq!(LuaInline(inline).tag_name(), "LineBreak");
    }

    #[test]
    fn test_lua_inline_tag_name_math() {
        let inline = Inline::Math(Math {
            math_type: MathType::InlineMath,
            text: "x^2".into(),
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "Math");
    }

    #[test]
    fn test_lua_inline_tag_name_raw_inline() {
        let inline = Inline::RawInline(RawInline {
            format: "html".into(),
            text: "<b>".into(),
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "RawInline");
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
        assert_eq!(LuaInline(inline).tag_name(), "Link");
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
        assert_eq!(LuaInline(inline).tag_name(), "Image");
    }

    #[test]
    fn test_lua_inline_tag_name_note() {
        let inline = Inline::Note(Note {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "Note");
    }

    #[test]
    fn test_lua_inline_tag_name_span() {
        let inline = Inline::Span(Span {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "Span");
    }

    #[test]
    fn test_lua_inline_tag_name_shortcode() {
        let inline = Inline::Shortcode(Shortcode {
            is_escaped: false,
            name: "test".into(),
            positional_args: vec![],
            keyword_args: HashMap::new(),
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "Shortcode");
    }

    #[test]
    fn test_lua_inline_tag_name_note_reference() {
        let inline = Inline::NoteReference(NoteReference {
            id: "1".into(),
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "NoteReference");
    }

    #[test]
    fn test_lua_inline_tag_name_attr() {
        let inline = Inline::Attr(
            (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_si(),
        );
        assert_eq!(LuaInline(inline).tag_name(), "Attr");
    }

    #[test]
    fn test_lua_inline_tag_name_insert() {
        let inline = Inline::Insert(Insert {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "Insert");
    }

    #[test]
    fn test_lua_inline_tag_name_delete() {
        let inline = Inline::Delete(Delete {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "Delete");
    }

    #[test]
    fn test_lua_inline_tag_name_highlight() {
        let inline = Inline::Highlight(Highlight {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "Highlight");
    }

    #[test]
    fn test_lua_inline_tag_name_edit_comment() {
        let inline = Inline::EditComment(EditComment {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaInline(inline).tag_name(), "EditComment");
    }

    #[test]
    fn test_lua_inline_tag_name_custom() {
        let inline = Inline::Custom(crate::pandoc::custom::CustomNode::new(
            "test-type",
            (String::new(), vec![], hashlink::LinkedHashMap::new()),
            si(),
        ));
        assert_eq!(LuaInline(inline).tag_name(), "Custom");
    }

    // ========== LuaInline::field_names tests ==========

    #[test]
    fn test_lua_inline_field_names_str() {
        let inline = Inline::Str(Str {
            text: "hello".into(),
            source_info: si(),
        });
        assert_eq!(
            LuaInline(inline).field_names(),
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
            LuaInline(inline).field_names(),
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
            LuaInline(inline).field_names(),
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
            LuaInline(inline).field_names(),
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
            LuaInline(inline).field_names(),
            &["tag", "text", "attr", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_inline_field_names_space() {
        let inline = Inline::Space(Space { source_info: si() });
        assert_eq!(LuaInline(inline).field_names(), &["tag", "clone", "walk"]);
    }

    #[test]
    fn test_lua_inline_field_names_math() {
        let inline = Inline::Math(Math {
            math_type: MathType::DisplayMath,
            text: "E=mc^2".into(),
            source_info: si(),
        });
        assert_eq!(
            LuaInline(inline).field_names(),
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
            LuaInline(inline).field_names(),
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
            LuaInline(inline).field_names(),
            &["tag", "content", "target", "title", "attr", "clone", "walk"]
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
            LuaInline(inline).field_names(),
            &["tag", "content", "src", "title", "attr", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_inline_field_names_note() {
        let inline = Inline::Note(Note {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(
            LuaInline(inline).field_names(),
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
            LuaInline(inline).field_names(),
            &["tag", "content", "attr", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_inline_field_names_note_reference() {
        let inline = Inline::NoteReference(NoteReference {
            id: "1".into(),
            source_info: si(),
        });
        assert_eq!(
            LuaInline(inline).field_names(),
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
            LuaInline(inline).field_names(),
            &["tag", "content", "attr", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_inline_field_names_custom() {
        let inline = Inline::Custom(crate::pandoc::custom::CustomNode::new(
            "test-type",
            (String::new(), vec![], hashlink::LinkedHashMap::new()),
            si(),
        ));
        assert_eq!(LuaInline(inline).field_names(), &["tag", "clone"]);
    }

    // ========== LuaBlock::tag_name tests ==========

    #[test]
    fn test_lua_block_tag_name_plain() {
        let block = Block::Plain(Plain {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock(block).tag_name(), "Plain");
    }

    #[test]
    fn test_lua_block_tag_name_paragraph() {
        let block = Block::Paragraph(Paragraph {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock(block).tag_name(), "Para");
    }

    #[test]
    fn test_lua_block_tag_name_line_block() {
        let block = Block::LineBlock(LineBlock {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock(block).tag_name(), "LineBlock");
    }

    #[test]
    fn test_lua_block_tag_name_code_block() {
        let block = Block::CodeBlock(CodeBlock {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            text: "code".into(),
            source_info: si(),
        });
        assert_eq!(LuaBlock(block).tag_name(), "CodeBlock");
    }

    #[test]
    fn test_lua_block_tag_name_raw_block() {
        let block = Block::RawBlock(RawBlock {
            format: "html".into(),
            text: "<div>".into(),
            source_info: si(),
        });
        assert_eq!(LuaBlock(block).tag_name(), "RawBlock");
    }

    #[test]
    fn test_lua_block_tag_name_block_quote() {
        let block = Block::BlockQuote(BlockQuote {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock(block).tag_name(), "BlockQuote");
    }

    #[test]
    fn test_lua_block_tag_name_ordered_list() {
        let block = Block::OrderedList(OrderedList {
            attr: (1, ListNumberStyle::Decimal, ListNumberDelim::Period),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock(block).tag_name(), "OrderedList");
    }

    #[test]
    fn test_lua_block_tag_name_bullet_list() {
        let block = Block::BulletList(BulletList {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock(block).tag_name(), "BulletList");
    }

    #[test]
    fn test_lua_block_tag_name_definition_list() {
        let block = Block::DefinitionList(DefinitionList {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock(block).tag_name(), "DefinitionList");
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
        assert_eq!(LuaBlock(block).tag_name(), "Header");
    }

    #[test]
    fn test_lua_block_tag_name_horizontal_rule() {
        let block = Block::HorizontalRule(HorizontalRule { source_info: si() });
        assert_eq!(LuaBlock(block).tag_name(), "HorizontalRule");
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
        assert_eq!(LuaBlock(block).tag_name(), "Table");
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
        assert_eq!(LuaBlock(block).tag_name(), "Figure");
    }

    #[test]
    fn test_lua_block_tag_name_div() {
        let block = Block::Div(Div {
            attr: (String::new(), vec![], hashlink::LinkedHashMap::new()),
            attr_source: attr_si(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock(block).tag_name(), "Div");
    }

    #[test]
    fn test_lua_block_tag_name_block_metadata() {
        let block = Block::BlockMetadata(MetaBlock {
            meta: quarto_pandoc_types::ConfigValue::default(),
            source_info: si(),
        });
        assert_eq!(LuaBlock(block).tag_name(), "BlockMetadata");
    }

    #[test]
    fn test_lua_block_tag_name_note_definition_para() {
        let block = Block::NoteDefinitionPara(NoteDefinitionPara {
            id: "1".into(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock(block).tag_name(), "NoteDefinitionPara");
    }

    #[test]
    fn test_lua_block_tag_name_note_definition_fenced_block() {
        let block = Block::NoteDefinitionFencedBlock(NoteDefinitionFencedBlock {
            id: "1".into(),
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock(block).tag_name(), "NoteDefinitionFencedBlock");
    }

    #[test]
    fn test_lua_block_tag_name_caption_block() {
        let block = Block::CaptionBlock(CaptionBlock {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(LuaBlock(block).tag_name(), "CaptionBlock");
    }

    #[test]
    fn test_lua_block_tag_name_custom() {
        let block = Block::Custom(crate::pandoc::custom::CustomNode::new(
            "test-type",
            (String::new(), vec![], hashlink::LinkedHashMap::new()),
            si(),
        ));
        assert_eq!(LuaBlock(block).tag_name(), "Custom");
    }

    // ========== LuaBlock::field_names tests ==========

    #[test]
    fn test_lua_block_field_names_plain() {
        let block = Block::Plain(Plain {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(
            LuaBlock(block).field_names(),
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
            LuaBlock(block).field_names(),
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
            LuaBlock(block).field_names(),
            &[
                "tag",
                "text",
                "attr",
                "identifier",
                "classes",
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
            LuaBlock(block).field_names(),
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
            LuaBlock(block).field_names(),
            &[
                "tag",
                "level",
                "content",
                "attr",
                "identifier",
                "classes",
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
            LuaBlock(block).field_names(),
            &["tag", "content", "start", "style", "clone", "walk"]
        );
    }

    #[test]
    fn test_lua_block_field_names_bullet_list() {
        let block = Block::BulletList(BulletList {
            content: vec![],
            source_info: si(),
        });
        assert_eq!(
            LuaBlock(block).field_names(),
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
            LuaBlock(block).field_names(),
            &["tag", "attr", "caption", "identifier", "clone", "walk"]
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
            LuaBlock(block).field_names(),
            &[
                "tag",
                "content",
                "attr",
                "caption",
                "identifier",
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
            LuaBlock(block).field_names(),
            &[
                "tag",
                "content",
                "attr",
                "identifier",
                "classes",
                "clone",
                "walk"
            ]
        );
    }

    #[test]
    fn test_lua_block_field_names_horizontal_rule() {
        let block = Block::HorizontalRule(HorizontalRule { source_info: si() });
        assert_eq!(LuaBlock(block).field_names(), &["tag", "clone", "walk"]);
    }

    #[test]
    fn test_lua_block_field_names_custom() {
        let block = Block::Custom(crate::pandoc::custom::CustomNode::new(
            "test-type",
            (String::new(), vec![], hashlink::LinkedHashMap::new()),
            si(),
        ));
        assert_eq!(LuaBlock(block).field_names(), &["tag", "clone"]);
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
        let lua_attr = LuaAttr(attr);
        assert_eq!(lua_attr.identifier(), "my-id");
    }

    #[test]
    fn test_lua_attr_classes() {
        let attr = (
            String::new(),
            vec!["a".into(), "b".into()],
            hashlink::LinkedHashMap::new(),
        );
        let lua_attr = LuaAttr(attr);
        assert_eq!(lua_attr.classes(), &["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn test_lua_attr_attributes() {
        let mut attrs = hashlink::LinkedHashMap::new();
        attrs.insert("key".into(), "value".into());
        let attr = (String::new(), vec![], attrs);
        let lua_attr = LuaAttr(attr);
        assert_eq!(lua_attr.attributes().get("key"), Some(&"value".to_string()));
    }
}
