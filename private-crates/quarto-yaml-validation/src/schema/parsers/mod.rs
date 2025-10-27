//! Schema parser modules
//!
//! This module contains all the individual schema type parsers,
//! organized by category:
//! - primitive: Basic types (boolean, number, string, null, any)
//! - enum: Enumeration types
//! - ref: Reference types
//! - combinators: anyOf, allOf
//! - arrays: Array types
//! - objects: Object types
//! - wrappers: Schema wrappers (future)

pub(super) mod arrays;
pub(super) mod combinators;
pub(super) mod r#enum;
pub(super) mod objects;
pub(super) mod primitive;
pub(super) mod r#ref;
pub(super) mod wrappers;

// Re-export parser functions for use within the schema module
pub(super) use arrays::{parse_array_schema, parse_arrayof_schema};
pub(super) use combinators::{parse_allof_schema, parse_anyof_schema, parse_maybe_arrayof_schema};
pub(super) use r#enum::parse_enum_schema;
pub(super) use objects::{parse_object_schema, parse_record_schema};
pub(super) use primitive::{
    parse_any_schema, parse_boolean_schema, parse_null_schema, parse_number_schema,
    parse_string_schema,
};
pub(super) use r#ref::parse_ref_schema;
pub(super) use wrappers::parse_schema_wrapper;
