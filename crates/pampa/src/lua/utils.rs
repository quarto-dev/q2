/*
 * lua/utils.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Pandoc utility functions for Lua filters.
 *
 * This module provides the `pandoc.utils` namespace with utility functions
 * like `pandoc.utils.stringify()`.
 */

use mlua::{Function, Lua, Result, Table, Value};
use sha1::{Digest, Sha1};

use crate::pandoc::{Block, Inline, LineBreak};
use quarto_source_map::SourceInfo;

use super::types::{LuaBlock, LuaInline, filter_source_info, inlines_to_lua_table};

/// Register the pandoc.utils namespace
pub fn register_pandoc_utils(lua: &Lua, pandoc: &Table) -> Result<()> {
    let utils = lua.create_table()?;

    // pandoc.utils.stringify(element)
    utils.set(
        "stringify",
        lua.create_function(|_lua, value: Value| {
            let result = stringify_value(&value)?;
            Ok(result)
        })?,
    )?;

    // pandoc.utils.blocks_to_inlines(blocks, sep?)
    utils.set("blocks_to_inlines", create_blocks_to_inlines(lua)?)?;

    // pandoc.utils.equals(elem1, elem2)
    utils.set("equals", create_equals(lua)?)?;

    // pandoc.utils.type(value)
    utils.set("type", create_type(lua)?)?;

    // pandoc.utils.sha1(input)
    utils.set("sha1", create_sha1(lua)?)?;

    // pandoc.utils.normalize_date(date)
    utils.set("normalize_date", create_normalize_date(lua)?)?;

    // pandoc.utils.to_roman_numeral(n)
    utils.set("to_roman_numeral", create_to_roman_numeral(lua)?)?;

    pandoc.set("utils", utils)?;

    Ok(())
}

/// blocks_to_inlines(blocks, sep?)
/// Squash a list of blocks into a list of inlines.
fn create_blocks_to_inlines(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, (blocks, sep): (Value, Option<Value>)| {
        // Get separator inlines (default to LineBreak)
        let separator = match sep {
            Some(Value::Table(t)) => {
                // Extract inlines from table
                let mut sep_inlines = Vec::new();
                for i in 1..=t.raw_len() {
                    let val: Value = t.raw_get(i)?;
                    if let Value::UserData(ud) = val
                        && let Ok(inline) = ud.borrow::<LuaInline>()
                    {
                        sep_inlines.push(inline.0.clone());
                    }
                }
                sep_inlines
            }
            _ => vec![Inline::LineBreak(LineBreak {
                source_info: filter_source_info(lua),
            })],
        };

        // Extract blocks
        let block_list = extract_blocks(&blocks)?;

        // Convert blocks to inlines
        let mut result_inlines: Vec<Inline> = Vec::new();
        let mut first = true;

        for block in &block_list {
            if !first && !separator.is_empty() {
                result_inlines.extend(separator.clone());
            }
            first = false;
            result_inlines.extend(block_to_inlines(block));
        }

        // Create result table with Inlines metatable
        inlines_to_lua_table(lua, &result_inlines)
    })
}

/// Extract blocks from a Lua value (either table of blocks or single block)
fn extract_blocks(value: &Value) -> Result<Vec<Block>> {
    match value {
        Value::Table(table) => {
            let mut blocks = Vec::new();
            for i in 1..=table.raw_len() {
                let val: Value = table.raw_get(i)?;
                if let Value::UserData(ud) = val
                    && let Ok(block) = ud.borrow::<LuaBlock>()
                {
                    blocks.push(block.0.clone());
                }
            }
            Ok(blocks)
        }
        Value::UserData(ud) => {
            if let Ok(block) = ud.borrow::<LuaBlock>() {
                Ok(vec![block.0.clone()])
            } else {
                Ok(vec![])
            }
        }
        _ => Ok(vec![]),
    }
}

/// Convert a block to its inline content
fn block_to_inlines(block: &Block) -> Vec<Inline> {
    match block {
        Block::Paragraph(p) => p.content.clone(),
        Block::Plain(p) => p.content.clone(),
        Block::Header(h) => h.content.clone(),
        Block::BlockQuote(b) => {
            let mut result = Vec::new();
            for (i, inner_block) in b.content.iter().enumerate() {
                if i > 0 {
                    result.push(Inline::LineBreak(LineBreak {
                        source_info: SourceInfo::default(),
                    }));
                }
                result.extend(block_to_inlines(inner_block));
            }
            result
        }
        Block::BulletList(l) => {
            let mut result = Vec::new();
            for (i, items) in l.content.iter().enumerate() {
                if i > 0 {
                    result.push(Inline::LineBreak(LineBreak {
                        source_info: SourceInfo::default(),
                    }));
                }
                for (j, block) in items.iter().enumerate() {
                    if j > 0 {
                        result.push(Inline::LineBreak(LineBreak {
                            source_info: SourceInfo::default(),
                        }));
                    }
                    result.extend(block_to_inlines(block));
                }
            }
            result
        }
        Block::OrderedList(l) => {
            let mut result = Vec::new();
            for (i, items) in l.content.iter().enumerate() {
                if i > 0 {
                    result.push(Inline::LineBreak(LineBreak {
                        source_info: SourceInfo::default(),
                    }));
                }
                for (j, block) in items.iter().enumerate() {
                    if j > 0 {
                        result.push(Inline::LineBreak(LineBreak {
                            source_info: SourceInfo::default(),
                        }));
                    }
                    result.extend(block_to_inlines(block));
                }
            }
            result
        }
        Block::Div(d) => {
            let mut result = Vec::new();
            for (i, inner_block) in d.content.iter().enumerate() {
                if i > 0 {
                    result.push(Inline::LineBreak(LineBreak {
                        source_info: SourceInfo::default(),
                    }));
                }
                result.extend(block_to_inlines(inner_block));
            }
            result
        }
        Block::LineBlock(l) => {
            let mut result = Vec::new();
            for (i, line) in l.content.iter().enumerate() {
                if i > 0 {
                    result.push(Inline::LineBreak(LineBreak {
                        source_info: SourceInfo::default(),
                    }));
                }
                result.extend(line.clone());
            }
            result
        }
        Block::DefinitionList(d) => {
            let mut result = Vec::new();
            for (i, (term, defs)) in d.content.iter().enumerate() {
                if i > 0 {
                    result.push(Inline::LineBreak(LineBreak {
                        source_info: SourceInfo::default(),
                    }));
                }
                result.extend(term.clone());
                for def_blocks in defs {
                    for block in def_blocks {
                        result.push(Inline::LineBreak(LineBreak {
                            source_info: SourceInfo::default(),
                        }));
                        result.extend(block_to_inlines(block));
                    }
                }
            }
            result
        }
        Block::Figure(f) => {
            let mut result = Vec::new();
            for (i, inner_block) in f.content.iter().enumerate() {
                if i > 0 {
                    result.push(Inline::LineBreak(LineBreak {
                        source_info: SourceInfo::default(),
                    }));
                }
                result.extend(block_to_inlines(inner_block));
            }
            result
        }
        // Code blocks, raw blocks, horizontal rules, tables have no inline content
        Block::CodeBlock(_)
        | Block::RawBlock(_)
        | Block::HorizontalRule(_)
        | Block::Table(_)
        | Block::CaptionBlock(_)
        | Block::BlockMetadata(_)
        | Block::NoteDefinitionPara(_)
        | Block::NoteDefinitionFencedBlock(_)
        | Block::Custom(_) => vec![],
    }
}

/// equals(elem1, elem2)
/// Test equality of AST elements. This is deprecated in Pandoc (use == instead).
fn create_equals(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, (elem1, elem2): (Value, Value)| {
        // Use Lua's equality comparison which handles __eq metamethod
        lua.globals().set("__utils_eq_a", elem1)?;
        lua.globals().set("__utils_eq_b", elem2)?;
        let result: bool = lua.load("return __utils_eq_a == __utils_eq_b").eval()?;
        lua.globals().set("__utils_eq_a", Value::Nil)?;
        lua.globals().set("__utils_eq_b", Value::Nil)?;
        Ok(result)
    })
}

/// type(value)
/// Pandoc-friendly version of Lua's type function.
/// Returns the __name metafield if available, otherwise standard type.
fn create_type(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, value: Value| {
        match &value {
            Value::Table(t) => {
                // For tables, check for __name in metatable
                if let Some(mt) = t.metatable()
                    && let Ok(name) = mt.get::<String>("__name")
                {
                    return Ok(name);
                }
            }
            Value::UserData(ud) => {
                // For our Pandoc userdata types, return the specific element type
                // Check these first before falling back to generic __name
                if let Ok(inline) = ud.borrow::<LuaInline>() {
                    return Ok(get_inline_type_name(&inline.0));
                }
                if let Ok(block) = ud.borrow::<LuaBlock>() {
                    return Ok(get_block_type_name(&block.0));
                }
                // For other userdata, try metatable __name
                if let Ok(mt) = ud.metatable()
                    && let Ok(name) = mt.get::<String>("__name")
                {
                    return Ok(name);
                }
            }
            _ => {}
        }

        // Fall back to standard Lua type
        lua.globals().set("__utils_type_val", value)?;
        let type_name: String = lua.load("return type(__utils_type_val)").eval()?;
        lua.globals().set("__utils_type_val", Value::Nil)?;
        Ok(type_name)
    })
}

/// Get the type name for an inline element
fn get_inline_type_name(inline: &Inline) -> String {
    match inline {
        Inline::Str(_) => "Str".to_string(),
        Inline::Space(_) => "Space".to_string(),
        Inline::SoftBreak(_) => "SoftBreak".to_string(),
        Inline::LineBreak(_) => "LineBreak".to_string(),
        Inline::Emph(_) => "Emph".to_string(),
        Inline::Strong(_) => "Strong".to_string(),
        Inline::Underline(_) => "Underline".to_string(),
        Inline::Strikeout(_) => "Strikeout".to_string(),
        Inline::Superscript(_) => "Superscript".to_string(),
        Inline::Subscript(_) => "Subscript".to_string(),
        Inline::SmallCaps(_) => "SmallCaps".to_string(),
        Inline::Quoted(_) => "Quoted".to_string(),
        Inline::Code(_) => "Code".to_string(),
        Inline::Math(_) => "Math".to_string(),
        Inline::RawInline(_) => "RawInline".to_string(),
        Inline::Link(_) => "Link".to_string(),
        Inline::Image(_) => "Image".to_string(),
        Inline::Span(_) => "Span".to_string(),
        Inline::Note(_) => "Note".to_string(),
        Inline::Cite(_) => "Cite".to_string(),
        Inline::Shortcode(_) => "Shortcode".to_string(),
        Inline::NoteReference(_) => "NoteReference".to_string(),
        Inline::Attr(_, _) => "Attr".to_string(),
        Inline::Insert(_) => "Insert".to_string(),
        Inline::Delete(_) => "Delete".to_string(),
        Inline::Highlight(_) => "Highlight".to_string(),
        Inline::EditComment(_) => "EditComment".to_string(),
        Inline::Custom(_) => "Custom".to_string(),
    }
}

/// Get the type name for a block element
fn get_block_type_name(block: &Block) -> String {
    match block {
        Block::Paragraph(_) => "Para".to_string(),
        Block::Plain(_) => "Plain".to_string(),
        Block::Header(_) => "Header".to_string(),
        Block::CodeBlock(_) => "CodeBlock".to_string(),
        Block::RawBlock(_) => "RawBlock".to_string(),
        Block::BlockQuote(_) => "BlockQuote".to_string(),
        Block::BulletList(_) => "BulletList".to_string(),
        Block::OrderedList(_) => "OrderedList".to_string(),
        Block::DefinitionList(_) => "DefinitionList".to_string(),
        Block::Div(_) => "Div".to_string(),
        Block::LineBlock(_) => "LineBlock".to_string(),
        Block::Table(_) => "Table".to_string(),
        Block::Figure(_) => "Figure".to_string(),
        Block::HorizontalRule(_) => "HorizontalRule".to_string(),
        Block::CaptionBlock(_) => "CaptionBlock".to_string(),
        Block::BlockMetadata(_) => "BlockMetadata".to_string(),
        Block::NoteDefinitionPara(_) => "NoteDefinitionPara".to_string(),
        Block::NoteDefinitionFencedBlock(_) => "NoteDefinitionFencedBlock".to_string(),
        Block::Custom(_) => "Custom".to_string(),
    }
}

/// sha1(input)
/// Computes the SHA1 hash of the given string input.
fn create_sha1(lua: &Lua) -> Result<Function> {
    lua.create_function(|_lua, input: String| {
        let mut hasher = Sha1::new();
        hasher.update(input.as_bytes());
        let result = hasher.finalize();
        // Convert to hex string
        let hex = result
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();
        Ok(hex)
    })
}

/// normalize_date(date)
/// Parse a date and convert (if possible) to "YYYY-MM-DD" format.
/// Returns nil if the conversion failed.
fn create_normalize_date(lua: &Lua) -> Result<Function> {
    lua.create_function(|_lua, date: String| {
        // Try to parse and normalize the date
        if let Some(normalized) = normalize_date_string(&date) {
            Ok(Value::String(_lua.create_string(&normalized)?))
        } else {
            Ok(Value::Nil)
        }
    })
}

/// Attempt to normalize a date string to YYYY-MM-DD format.
/// This follows Pandoc's normalizeDate implementation which supports these formats:
/// - %m/%d/%Y (MM/DD/YYYY, e.g., "12/31/2017")
/// - %m/%d/%y (MM/DD/YY, e.g., "12/31/17")
/// - %F (YYYY-MM-DD, e.g., "2017-12-31")
/// - %d %b %Y (DD Mon YYYY, e.g., "09 Nov 1989")
/// - %e %B %Y (D Month YYYY, e.g., "9 November 1989")
/// - %b. %e, %Y (Mon. D, YYYY, e.g., "Nov. 9, 1989")
/// - %B %e, %Y (Month D, YYYY, e.g., "November 9, 1989")
/// - %Y%m%d (YYYYMMDD, e.g., "19891109")
/// - %Y%m (YYYYMM, e.g., "198911")
/// - %Y (YYYY, e.g., "1989")
fn normalize_date_string(date: &str) -> Option<String> {
    let date = date.trim();

    // Try each format in order (similar to Pandoc's msum over formats)

    // %F: YYYY-MM-DD (ISO format)
    if let Some(parsed) = try_parse_iso(date) {
        return Some(parsed);
    }

    // %m/%d/%Y: MM/DD/YYYY (US format with 4-digit year)
    if let Some(parsed) = try_parse_us_long(date) {
        return Some(parsed);
    }

    // %m/%d/%y or %D: MM/DD/YY (US format with 2-digit year)
    if let Some(parsed) = try_parse_us_short(date) {
        return Some(parsed);
    }

    // %d %b %Y: "09 Nov 1989" (day abbreviated-month year)
    if let Some(parsed) = try_parse_day_abbrev_month_year(date) {
        return Some(parsed);
    }

    // %e %B %Y: "9 November 1989" (day full-month year)
    if let Some(parsed) = try_parse_day_full_month_year(date) {
        return Some(parsed);
    }

    // %b. %e, %Y: "Nov. 9, 1989" (abbreviated month with period, day, year)
    if let Some(parsed) = try_parse_abbrev_month_dot_day_year(date) {
        return Some(parsed);
    }

    // %B %e, %Y: "November 9, 1989" (full month, day, year)
    if let Some(parsed) = try_parse_full_month_day_year(date) {
        return Some(parsed);
    }

    // %Y%m%d: YYYYMMDD (compact date)
    if let Some(parsed) = try_parse_compact(date) {
        return Some(parsed);
    }

    // %Y%m: YYYYMM (year-month only)
    if let Some(parsed) = try_parse_year_month(date) {
        return Some(parsed);
    }

    // %Y: YYYY (year only)
    if let Some(parsed) = try_parse_year_only(date) {
        return Some(parsed);
    }

    None
}

/// %F: YYYY-MM-DD
fn try_parse_iso(date: &str) -> Option<String> {
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() == 3 {
        let year: i32 = parts[0].parse().ok()?;
        let month: u32 = parts[1].parse().ok()?;
        let day: u32 = parts[2].parse().ok()?;
        if is_valid_date(year, month, day) {
            return Some(format!("{:04}-{:02}-{:02}", year, month, day));
        }
    }
    None
}

/// %m/%d/%Y: MM/DD/YYYY
fn try_parse_us_long(date: &str) -> Option<String> {
    let parts: Vec<&str> = date.split('/').collect();
    if parts.len() == 3 && parts[2].len() == 4 {
        let month: u32 = parts[0].parse().ok()?;
        let day: u32 = parts[1].parse().ok()?;
        let year: i32 = parts[2].parse().ok()?;
        if is_valid_date(year, month, day) {
            return Some(format!("{:04}-{:02}-{:02}", year, month, day));
        }
    }
    None
}

/// %m/%d/%y or %D: MM/DD/YY (2-digit year)
fn try_parse_us_short(date: &str) -> Option<String> {
    let parts: Vec<&str> = date.split('/').collect();
    if parts.len() == 3 && parts[2].len() == 2 {
        let month: u32 = parts[0].parse().ok()?;
        let day: u32 = parts[1].parse().ok()?;
        let short_year: i32 = parts[2].parse().ok()?;
        // Convert 2-digit year: 00-68 -> 2000-2068, 69-99 -> 1969-1999
        let year = if short_year <= 68 {
            2000 + short_year
        } else {
            1900 + short_year
        };
        if is_valid_date(year, month, day) {
            return Some(format!("{:04}-{:02}-{:02}", year, month, day));
        }
    }
    None
}

/// Month names for parsing
const MONTH_NAMES: &[(&str, u32)] = &[
    ("january", 1),
    ("february", 2),
    ("march", 3),
    ("april", 4),
    ("may", 5),
    ("june", 6),
    ("july", 7),
    ("august", 8),
    ("september", 9),
    ("october", 10),
    ("november", 11),
    ("december", 12),
];

const MONTH_ABBREVS: &[(&str, u32)] = &[
    ("jan", 1),
    ("feb", 2),
    ("mar", 3),
    ("apr", 4),
    ("may", 5),
    ("jun", 6),
    ("jul", 7),
    ("aug", 8),
    ("sep", 9),
    ("oct", 10),
    ("nov", 11),
    ("dec", 12),
];

/// %d %b %Y: "09 Nov 1989" (day abbreviated-month year)
fn try_parse_day_abbrev_month_year(date: &str) -> Option<String> {
    let lower = date.to_lowercase();
    let parts: Vec<&str> = lower.split_whitespace().collect();
    if parts.len() == 3 {
        let day: u32 = parts[0].parse().ok()?;
        let month = month_from_abbrev(parts[1])?;
        let year: i32 = parts[2].parse().ok()?;
        if is_valid_date(year, month, day) {
            return Some(format!("{:04}-{:02}-{:02}", year, month, day));
        }
    }
    None
}

/// %e %B %Y: "9 November 1989" (day full-month year)
fn try_parse_day_full_month_year(date: &str) -> Option<String> {
    let lower = date.to_lowercase();
    let parts: Vec<&str> = lower.split_whitespace().collect();
    if parts.len() == 3 {
        let day: u32 = parts[0].parse().ok()?;
        let month = month_from_full_name(parts[1])?;
        let year: i32 = parts[2].parse().ok()?;
        if is_valid_date(year, month, day) {
            return Some(format!("{:04}-{:02}-{:02}", year, month, day));
        }
    }
    None
}

/// %b. %e, %Y: "Nov. 9, 1989" (abbreviated month with period, day, year)
fn try_parse_abbrev_month_dot_day_year(date: &str) -> Option<String> {
    let lower = date.to_lowercase();
    // Remove commas and periods for parsing
    let cleaned: String = lower
        .chars()
        .map(|c| if c == ',' || c == '.' { ' ' } else { c })
        .collect();
    let parts: Vec<&str> = cleaned.split_whitespace().collect();
    if parts.len() >= 3 {
        let month = month_from_abbrev(parts[0])?;
        let day: u32 = parts[1].parse().ok()?;
        let year: i32 = parts[2].parse().ok()?;
        if is_valid_date(year, month, day) {
            return Some(format!("{:04}-{:02}-{:02}", year, month, day));
        }
    }
    None
}

/// %B %e, %Y: "November 9, 1989" (full month, day, year)
fn try_parse_full_month_day_year(date: &str) -> Option<String> {
    let lower = date.to_lowercase();
    // Remove commas for parsing
    let cleaned: String = lower
        .chars()
        .map(|c| if c == ',' { ' ' } else { c })
        .collect();
    let parts: Vec<&str> = cleaned.split_whitespace().collect();
    if parts.len() >= 3 {
        let month = month_from_full_name(parts[0])?;
        let day: u32 = parts[1].parse().ok()?;
        let year: i32 = parts[2].parse().ok()?;
        if is_valid_date(year, month, day) {
            return Some(format!("{:04}-{:02}-{:02}", year, month, day));
        }
    }
    None
}

/// %Y%m%d: YYYYMMDD (compact date)
fn try_parse_compact(date: &str) -> Option<String> {
    if date.len() == 8 && date.chars().all(|c| c.is_ascii_digit()) {
        let year: i32 = date[0..4].parse().ok()?;
        let month: u32 = date[4..6].parse().ok()?;
        let day: u32 = date[6..8].parse().ok()?;
        if is_valid_date(year, month, day) {
            return Some(format!("{:04}-{:02}-{:02}", year, month, day));
        }
    }
    None
}

/// %Y%m: YYYYMM (year-month only, returns first day of month)
fn try_parse_year_month(date: &str) -> Option<String> {
    if date.len() == 6 && date.chars().all(|c| c.is_ascii_digit()) {
        let year: i32 = date[0..4].parse().ok()?;
        let month: u32 = date[4..6].parse().ok()?;
        if is_valid_date(year, month, 1) {
            return Some(format!("{:04}-{:02}-01", year, month));
        }
    }
    None
}

/// %Y: YYYY (year only, returns January 1st)
fn try_parse_year_only(date: &str) -> Option<String> {
    if date.len() == 4 && date.chars().all(|c| c.is_ascii_digit()) {
        let year: i32 = date.parse().ok()?;
        if (1601..=9999).contains(&year) {
            return Some(format!("{:04}-01-01", year));
        }
    }
    None
}

/// Get month number from abbreviated name (case-insensitive)
fn month_from_abbrev(name: &str) -> Option<u32> {
    let name_lower = name.to_lowercase();
    // Handle both "nov" and "nov." style
    let name_clean = name_lower.trim_end_matches('.');
    for (abbrev, num) in MONTH_ABBREVS {
        if name_clean == *abbrev {
            return Some(*num);
        }
    }
    None
}

/// Get month number from full name (case-insensitive)
fn month_from_full_name(name: &str) -> Option<u32> {
    let name_lower = name.to_lowercase();
    for (full_name, num) in MONTH_NAMES {
        if name_lower == *full_name {
            return Some(*num);
        }
    }
    None
}

/// Check if a date is valid (year between 1601-9999, month 1-12, day valid for month)
fn is_valid_date(year: i32, month: u32, day: u32) -> bool {
    if !(1601..=9999).contains(&year) {
        return false;
    }
    if !(1..=12).contains(&month) {
        return false;
    }
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => return false,
    };
    (1..=days_in_month).contains(&day)
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// to_roman_numeral(n)
/// Converts an integer < 4000 to uppercase roman numeral.
fn create_to_roman_numeral(lua: &Lua) -> Result<Function> {
    lua.create_function(|_lua, n: i64| {
        if n <= 0 || n >= 4000 {
            return Err(mlua::Error::runtime(
                "to_roman_numeral: number must be between 1 and 3999",
            ));
        }

        let mut result = String::new();
        let mut num = n as u32;

        let numerals = [
            (1000, "M"),
            (900, "CM"),
            (500, "D"),
            (400, "CD"),
            (100, "C"),
            (90, "XC"),
            (50, "L"),
            (40, "XL"),
            (10, "X"),
            (9, "IX"),
            (5, "V"),
            (4, "IV"),
            (1, "I"),
        ];

        for (value, numeral) in numerals {
            while num >= value {
                result.push_str(numeral);
                num -= value;
            }
        }

        Ok(result)
    })
}

/// Convert a Lua value (block, inline, list of elements) to plain text
fn stringify_value(value: &Value) -> Result<String> {
    match value {
        Value::UserData(ud) => {
            // Try to extract as LuaInline
            if let Ok(inline) = ud.borrow::<LuaInline>() {
                return Ok(stringify_inline(&inline.0));
            }
            // Try to extract as LuaBlock
            if let Ok(block) = ud.borrow::<LuaBlock>() {
                return Ok(stringify_block(&block.0));
            }
            Ok(String::new())
        }
        Value::Table(table) => {
            // Handle table of elements
            let mut result = String::new();
            for item in table.clone().sequence_values::<Value>() {
                let item = item?;
                result.push_str(&stringify_value(&item)?);
            }
            Ok(result)
        }
        Value::String(s) => Ok(s.to_str()?.to_string()),
        _ => Ok(String::new()),
    }
}

/// Convert a single inline element to plain text
fn stringify_inline(inline: &Inline) -> String {
    match inline {
        Inline::Str(s) => s.text.clone(),
        Inline::Space(_) => " ".to_string(),
        Inline::SoftBreak(_) => "\n".to_string(),
        Inline::LineBreak(_) => "\n".to_string(),
        Inline::Emph(e) => stringify_inlines(&e.content),
        Inline::Strong(s) => stringify_inlines(&s.content),
        Inline::Underline(u) => stringify_inlines(&u.content),
        Inline::Strikeout(s) => stringify_inlines(&s.content),
        Inline::Superscript(s) => stringify_inlines(&s.content),
        Inline::Subscript(s) => stringify_inlines(&s.content),
        Inline::SmallCaps(s) => stringify_inlines(&s.content),
        Inline::Quoted(q) => {
            let content = stringify_inlines(&q.content);
            format!("\"{}\"", content)
        }
        Inline::Code(c) => c.text.clone(),
        Inline::Math(m) => m.text.clone(),
        Inline::RawInline(_) => String::new(), // Raw content is dropped
        Inline::Link(l) => stringify_inlines(&l.content),
        Inline::Image(i) => stringify_inlines(&i.content),
        Inline::Span(s) => stringify_inlines(&s.content),
        Inline::Note(n) => stringify_blocks(&n.content),
        Inline::Cite(c) => stringify_inlines(&c.content),
        // Additional inline types
        Inline::Shortcode(_) => String::new(),
        Inline::NoteReference(_) => String::new(),
        Inline::Attr(_, _) => String::new(),
        Inline::Insert(i) => stringify_inlines(&i.content),
        Inline::Delete(d) => stringify_inlines(&d.content),
        Inline::Highlight(h) => stringify_inlines(&h.content),
        Inline::EditComment(_) => String::new(),
        // Custom nodes: we don't attempt to stringify their contents
        Inline::Custom(_) => String::new(),
    }
}

/// Convert a list of inline elements to plain text
fn stringify_inlines(inlines: &[Inline]) -> String {
    inlines.iter().map(stringify_inline).collect()
}

/// Convert a single block element to plain text
fn stringify_block(block: &Block) -> String {
    match block {
        Block::Paragraph(p) => stringify_inlines(&p.content),
        Block::Plain(p) => stringify_inlines(&p.content),
        Block::Header(h) => stringify_inlines(&h.content),
        Block::CodeBlock(c) => c.text.clone(),
        Block::RawBlock(_) => String::new(), // Raw content is dropped
        Block::BlockQuote(b) => stringify_blocks(&b.content),
        Block::BulletList(l) => l
            .content
            .iter()
            .map(|items| stringify_blocks(items))
            .collect::<Vec<_>>()
            .join("\n"),
        Block::OrderedList(l) => l
            .content
            .iter()
            .map(|items| stringify_blocks(items))
            .collect::<Vec<_>>()
            .join("\n"),
        Block::DefinitionList(d) => d
            .content
            .iter()
            .map(|(term, defs)| {
                let term_str = stringify_inlines(term);
                let defs_str = defs
                    .iter()
                    .map(|def| stringify_blocks(def))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("{}: {}", term_str, defs_str)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Block::Div(d) => stringify_blocks(&d.content),
        Block::LineBlock(l) => l
            .content
            .iter()
            .map(|line| stringify_inlines(line))
            .collect::<Vec<_>>()
            .join("\n"),
        Block::Table(t) => {
            // Stringify table caption
            let mut result = String::new();
            if let Some(ref long) = t.caption.long {
                result.push_str(&stringify_blocks(long));
            }
            result
        }
        Block::Figure(f) => {
            let mut result = stringify_blocks(&f.content);
            if let Some(ref long) = f.caption.long {
                result.push_str(&stringify_blocks(long));
            }
            result
        }
        Block::HorizontalRule(_) => String::new(),
        Block::CaptionBlock(c) => stringify_inlines(&c.content),
        // Additional block types
        Block::BlockMetadata(_) => String::new(),
        Block::NoteDefinitionPara(n) => stringify_inlines(&n.content),
        Block::NoteDefinitionFencedBlock(n) => stringify_blocks(&n.content),
        // Custom nodes: we don't attempt to stringify their contents
        Block::Custom(_) => String::new(),
    }
}

/// Convert a list of block elements to plain text
fn stringify_blocks(blocks: &[Block]) -> String {
    blocks
        .iter()
        .map(stringify_block)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::constructors::register_pandoc_namespace;
    use crate::lua::mediabag::create_shared_mediabag;
    use crate::lua::runtime::NativeRuntime;
    use std::sync::Arc;

    fn create_test_lua() -> Lua {
        let lua = Lua::new();
        let runtime = Arc::new(NativeRuntime::new());
        register_pandoc_namespace(&lua, runtime, create_shared_mediabag()).unwrap();
        lua
    }

    // ========== stringify tests ==========

    #[test]
    fn test_stringify_str() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.stringify(pandoc.Str('hello'))")
            .eval()
            .unwrap();

        assert_eq!(result, "hello");
    }

    #[test]
    fn test_stringify_emph() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.stringify(pandoc.Emph{pandoc.Str('emphasized')})")
            .eval()
            .unwrap();

        assert_eq!(result, "emphasized");
    }

    #[test]
    fn test_stringify_strong() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.stringify(pandoc.Strong{pandoc.Str('bold')})")
            .eval()
            .unwrap();

        assert_eq!(result, "bold");
    }

    #[test]
    fn test_stringify_space() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.stringify(pandoc.Space())")
            .eval()
            .unwrap();

        assert_eq!(result, " ");
    }

    #[test]
    fn test_stringify_linebreak() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.stringify(pandoc.LineBreak())")
            .eval()
            .unwrap();

        assert_eq!(result, "\n");
    }

    #[test]
    fn test_stringify_para() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.stringify(pandoc.Para{pandoc.Str('paragraph')})")
            .eval()
            .unwrap();

        assert_eq!(result, "paragraph");
    }

    #[test]
    fn test_stringify_list_of_inlines() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.stringify({pandoc.Str('hello'), pandoc.Space(), pandoc.Str('world')})")
            .eval()
            .unwrap();

        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_stringify_plain_string() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.stringify('plain text')")
            .eval()
            .unwrap();

        assert_eq!(result, "plain text");
    }

    #[test]
    fn test_stringify_quoted() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.stringify(pandoc.Quoted('DoubleQuote', {pandoc.Str('quoted')}))")
            .eval()
            .unwrap();

        assert_eq!(result, "\"quoted\"");
    }

    #[test]
    fn test_stringify_code() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.stringify(pandoc.Code('let x = 1'))")
            .eval()
            .unwrap();

        assert_eq!(result, "let x = 1");
    }

    // ========== blocks_to_inlines tests ==========

    #[test]
    fn test_blocks_to_inlines_para() {
        let lua = create_test_lua();

        let result: Table = lua
            .load("return pandoc.utils.blocks_to_inlines({pandoc.Para{pandoc.Str('hello')}})")
            .eval()
            .unwrap();

        assert!(result.len().unwrap() >= 1);
    }

    #[test]
    fn test_blocks_to_inlines_with_separator() {
        let lua = create_test_lua();

        let result: Table = lua
            .load("return pandoc.utils.blocks_to_inlines({pandoc.Para{pandoc.Str('a')}, pandoc.Para{pandoc.Str('b')}}, {pandoc.Str('-')})")
            .eval()
            .unwrap();

        // Should have a, -, b
        assert!(result.len().unwrap() >= 3);
    }

    // ========== equals tests ==========

    #[test]
    fn test_equals_same_var() {
        let lua = create_test_lua();

        // Same variable should be equal
        let result: bool = lua
            .load("local s = pandoc.Str('a'); return pandoc.utils.equals(s, s)")
            .eval()
            .unwrap();

        assert!(result);
    }

    #[test]
    fn test_equals_different() {
        let lua = create_test_lua();

        let result: bool = lua
            .load("return pandoc.utils.equals(pandoc.Str('a'), pandoc.Str('b'))")
            .eval()
            .unwrap();

        assert!(!result);
    }

    #[test]
    fn test_equals_primitives() {
        let lua = create_test_lua();

        let result: bool = lua
            .load("return pandoc.utils.equals(42, 42)")
            .eval()
            .unwrap();

        assert!(result);
    }

    // ========== type tests ==========

    #[test]
    fn test_type_str() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.type(pandoc.Str('test'))")
            .eval()
            .unwrap();

        assert_eq!(result, "Str");
    }

    #[test]
    fn test_type_para() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.type(pandoc.Para{pandoc.Str('test')})")
            .eval()
            .unwrap();

        assert_eq!(result, "Para");
    }

    #[test]
    fn test_type_table_with_name() {
        let lua = create_test_lua();

        let result: String = lua
            .load(
                r#"
                local mt = { __name = "MyType" }
                local t = setmetatable({}, mt)
                return pandoc.utils.type(t)
            "#,
            )
            .eval()
            .unwrap();

        assert_eq!(result, "MyType");
    }

    #[test]
    fn test_type_number() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.type(42)")
            .eval()
            .unwrap();

        assert_eq!(result, "number");
    }

    // ========== sha1 tests ==========

    #[test]
    fn test_sha1_empty() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.sha1('')")
            .eval()
            .unwrap();

        // SHA1 of empty string
        assert_eq!(result, "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn test_sha1_hello() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.sha1('hello')")
            .eval()
            .unwrap();

        // SHA1 of "hello"
        assert_eq!(result, "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
    }

    // ========== normalize_date tests ==========

    #[test]
    fn test_normalize_date_iso() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.normalize_date('2024-01-15')")
            .eval()
            .unwrap();

        assert_eq!(result, "2024-01-15");
    }

    #[test]
    fn test_normalize_date_us_long() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.normalize_date('12/31/2024')")
            .eval()
            .unwrap();

        assert_eq!(result, "2024-12-31");
    }

    #[test]
    fn test_normalize_date_us_short() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.normalize_date('12/31/24')")
            .eval()
            .unwrap();

        assert_eq!(result, "2024-12-31");
    }

    #[test]
    fn test_normalize_date_day_abbrev_month() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.normalize_date('09 Nov 1989')")
            .eval()
            .unwrap();

        assert_eq!(result, "1989-11-09");
    }

    #[test]
    fn test_normalize_date_day_full_month() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.normalize_date('9 November 1989')")
            .eval()
            .unwrap();

        assert_eq!(result, "1989-11-09");
    }

    #[test]
    fn test_normalize_date_abbrev_month_dot() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.normalize_date('Nov. 9, 1989')")
            .eval()
            .unwrap();

        assert_eq!(result, "1989-11-09");
    }

    #[test]
    fn test_normalize_date_full_month_day() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.normalize_date('November 9, 1989')")
            .eval()
            .unwrap();

        assert_eq!(result, "1989-11-09");
    }

    #[test]
    fn test_normalize_date_compact() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.normalize_date('19891109')")
            .eval()
            .unwrap();

        assert_eq!(result, "1989-11-09");
    }

    #[test]
    fn test_normalize_date_year_month() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.normalize_date('198911')")
            .eval()
            .unwrap();

        assert_eq!(result, "1989-11-01");
    }

    #[test]
    fn test_normalize_date_year_only() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.normalize_date('1989')")
            .eval()
            .unwrap();

        assert_eq!(result, "1989-01-01");
    }

    #[test]
    fn test_normalize_date_invalid() {
        let lua = create_test_lua();

        let result: Value = lua
            .load("return pandoc.utils.normalize_date('not a date')")
            .eval()
            .unwrap();

        assert!(result.is_nil());
    }

    // ========== to_roman_numeral tests ==========

    #[test]
    fn test_roman_numeral_1() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.to_roman_numeral(1)")
            .eval()
            .unwrap();

        assert_eq!(result, "I");
    }

    #[test]
    fn test_roman_numeral_4() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.to_roman_numeral(4)")
            .eval()
            .unwrap();

        assert_eq!(result, "IV");
    }

    #[test]
    fn test_roman_numeral_9() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.to_roman_numeral(9)")
            .eval()
            .unwrap();

        assert_eq!(result, "IX");
    }

    #[test]
    fn test_roman_numeral_49() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.to_roman_numeral(49)")
            .eval()
            .unwrap();

        assert_eq!(result, "XLIX");
    }

    #[test]
    fn test_roman_numeral_1984() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.to_roman_numeral(1984)")
            .eval()
            .unwrap();

        assert_eq!(result, "MCMLXXXIV");
    }

    #[test]
    fn test_roman_numeral_3999() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.to_roman_numeral(3999)")
            .eval()
            .unwrap();

        assert_eq!(result, "MMMCMXCIX");
    }

    #[test]
    fn test_roman_numeral_invalid_zero() {
        let lua = create_test_lua();

        let result: mlua::Result<String> = lua
            .load("return pandoc.utils.to_roman_numeral(0)")
            .eval();

        assert!(result.is_err());
    }

    #[test]
    fn test_roman_numeral_invalid_negative() {
        let lua = create_test_lua();

        let result: mlua::Result<String> = lua
            .load("return pandoc.utils.to_roman_numeral(-1)")
            .eval();

        assert!(result.is_err());
    }

    #[test]
    fn test_roman_numeral_invalid_too_large() {
        let lua = create_test_lua();

        let result: mlua::Result<String> = lua
            .load("return pandoc.utils.to_roman_numeral(4000)")
            .eval();

        assert!(result.is_err());
    }

    // ========== Helper function unit tests ==========

    #[test]
    fn test_is_valid_date_normal() {
        assert!(is_valid_date(2024, 1, 15));
        assert!(is_valid_date(2024, 12, 31));
    }

    #[test]
    fn test_is_valid_date_leap_year() {
        assert!(is_valid_date(2024, 2, 29)); // 2024 is leap year
        assert!(!is_valid_date(2023, 2, 29)); // 2023 is not leap year
    }

    #[test]
    fn test_is_valid_date_invalid_month() {
        assert!(!is_valid_date(2024, 0, 15));
        assert!(!is_valid_date(2024, 13, 15));
    }

    #[test]
    fn test_is_valid_date_invalid_day() {
        assert!(!is_valid_date(2024, 1, 0));
        assert!(!is_valid_date(2024, 1, 32));
        assert!(!is_valid_date(2024, 4, 31)); // April has 30 days
    }

    #[test]
    fn test_is_valid_date_invalid_year() {
        assert!(!is_valid_date(1600, 1, 1)); // Too early
        assert!(!is_valid_date(10000, 1, 1)); // Too late
    }

    #[test]
    fn test_is_leap_year() {
        assert!(is_leap_year(2024)); // Divisible by 4
        assert!(!is_leap_year(2023)); // Not divisible by 4
        assert!(!is_leap_year(1900)); // Divisible by 100 but not 400
        assert!(is_leap_year(2000)); // Divisible by 400
    }

    #[test]
    fn test_month_from_abbrev() {
        assert_eq!(month_from_abbrev("jan"), Some(1));
        assert_eq!(month_from_abbrev("Jan"), Some(1));
        assert_eq!(month_from_abbrev("JAN"), Some(1));
        assert_eq!(month_from_abbrev("dec"), Some(12));
        assert_eq!(month_from_abbrev("foo"), None);
    }

    #[test]
    fn test_month_from_full_name() {
        assert_eq!(month_from_full_name("january"), Some(1));
        assert_eq!(month_from_full_name("January"), Some(1));
        assert_eq!(month_from_full_name("december"), Some(12));
        assert_eq!(month_from_full_name("foo"), None);
    }

    // ========== block_to_inlines tests ==========

    #[test]
    fn test_block_to_inlines_blockquote() {
        let lua = create_test_lua();

        let result: Table = lua
            .load("return pandoc.utils.blocks_to_inlines({pandoc.BlockQuote{pandoc.Para{pandoc.Str('quoted')}}})")
            .eval()
            .unwrap();

        assert!(result.len().unwrap() >= 1);
    }

    #[test]
    fn test_block_to_inlines_bullet_list() {
        let lua = create_test_lua();

        let result: Table = lua
            .load("return pandoc.utils.blocks_to_inlines({pandoc.BulletList{{pandoc.Plain{pandoc.Str('item')}}}})")
            .eval()
            .unwrap();

        assert!(result.len().unwrap() >= 1);
    }

    #[test]
    fn test_block_to_inlines_ordered_list() {
        let lua = create_test_lua();

        let result: Table = lua
            .load("return pandoc.utils.blocks_to_inlines({pandoc.OrderedList{{pandoc.Plain{pandoc.Str('item')}}}})")
            .eval()
            .unwrap();

        assert!(result.len().unwrap() >= 1);
    }

    #[test]
    fn test_block_to_inlines_div() {
        let lua = create_test_lua();

        let result: Table = lua
            .load("return pandoc.utils.blocks_to_inlines({pandoc.Div{pandoc.Para{pandoc.Str('content')}}})")
            .eval()
            .unwrap();

        assert!(result.len().unwrap() >= 1);
    }

    #[test]
    fn test_block_to_inlines_codeblock() {
        let lua = create_test_lua();

        // CodeBlock returns empty inlines
        let result: Table = lua
            .load("return pandoc.utils.blocks_to_inlines({pandoc.CodeBlock('code')})")
            .eval()
            .unwrap();

        assert_eq!(result.len().unwrap(), 0);
    }

    // ========== stringify block types tests ==========

    #[test]
    fn test_stringify_header() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.stringify(pandoc.Header(1, {pandoc.Str('Title')}))")
            .eval()
            .unwrap();

        assert_eq!(result, "Title");
    }

    #[test]
    fn test_stringify_codeblock() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.stringify(pandoc.CodeBlock('let x = 1'))")
            .eval()
            .unwrap();

        assert_eq!(result, "let x = 1");
    }

    #[test]
    fn test_stringify_blockquote() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.stringify(pandoc.BlockQuote{pandoc.Para{pandoc.Str('quoted')}})")
            .eval()
            .unwrap();

        assert_eq!(result, "quoted");
    }

    #[test]
    fn test_stringify_bulletlist() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.stringify(pandoc.BulletList{{pandoc.Plain{pandoc.Str('item')}}})")
            .eval()
            .unwrap();

        assert!(result.contains("item"));
    }

    #[test]
    fn test_stringify_div() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.stringify(pandoc.Div{pandoc.Para{pandoc.Str('content')}})")
            .eval()
            .unwrap();

        assert_eq!(result, "content");
    }

    #[test]
    fn test_stringify_nil_value() {
        let lua = create_test_lua();

        let result: String = lua
            .load("return pandoc.utils.stringify(nil)")
            .eval()
            .unwrap();

        assert_eq!(result, "");
    }

    // ========== get_inline_type_name tests ==========

    #[test]
    fn test_get_inline_type_name_all() {
        use crate::pandoc::*;

        assert_eq!(
            get_inline_type_name(&Inline::Str(Str {
                text: "".into(),
                source_info: SourceInfo::default()
            })),
            "Str"
        );
        assert_eq!(
            get_inline_type_name(&Inline::Space(Space {
                source_info: SourceInfo::default()
            })),
            "Space"
        );
        assert_eq!(
            get_inline_type_name(&Inline::SoftBreak(SoftBreak {
                source_info: SourceInfo::default()
            })),
            "SoftBreak"
        );
        assert_eq!(
            get_inline_type_name(&Inline::LineBreak(LineBreak {
                source_info: SourceInfo::default()
            })),
            "LineBreak"
        );
    }

    // ========== get_block_type_name tests ==========

    #[test]
    fn test_get_block_type_name_all() {
        use crate::pandoc::*;

        assert_eq!(
            get_block_type_name(&Block::Paragraph(Paragraph {
                content: vec![],
                source_info: SourceInfo::default()
            })),
            "Para"
        );
        assert_eq!(
            get_block_type_name(&Block::Plain(Plain {
                content: vec![],
                source_info: SourceInfo::default()
            })),
            "Plain"
        );
        assert_eq!(
            get_block_type_name(&Block::HorizontalRule(HorizontalRule {
                source_info: SourceInfo::default()
            })),
            "HorizontalRule"
        );
    }
}
