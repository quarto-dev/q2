// YAML validation for Quarto
//
// This crate provides schema-based validation for YAML content,
// with support for Quarto's simplified JSON Schema subset.

pub mod diagnostic;
pub mod error;
pub mod schema;
pub mod validator;

pub use diagnostic::{PathSegment, SourceRange, ValidationDiagnostic};
pub use error::{ValidationError, ValidationResult};
pub use schema::{Schema, SchemaRegistry, merge_object_schemas};
pub use validator::{ValidationContext, validate};

#[cfg(test)]
mod tests;
