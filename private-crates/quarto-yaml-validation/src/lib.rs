// YAML validation for Quarto
//
// This crate provides schema-based validation for YAML content,
// with support for Quarto's simplified JSON Schema subset.

pub mod error;
pub mod schema;
pub mod validator;

pub use error::{ValidationError, ValidationResult};
pub use schema::{Schema, SchemaRegistry};
pub use validator::{ValidationContext, validate};

#[cfg(test)]
mod tests;
