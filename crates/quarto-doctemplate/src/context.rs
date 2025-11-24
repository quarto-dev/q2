/*
 * context.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Template value and context types.
//!
//! This module defines the types used to represent template variable values
//! and the context in which templates are evaluated.
//!
//! **Important**: These types are independent of Pandoc AST types. Conversion
//! from Pandoc's `MetaValue` to `TemplateValue` happens in the writer layer.

use std::collections::HashMap;

/// A value that can be used in template evaluation.
///
/// This mirrors the value types supported by Pandoc's doctemplates library.
#[derive(Debug, Clone, PartialEq)]
pub enum TemplateValue {
    /// A string value.
    String(String),

    /// A boolean value.
    Bool(bool),

    /// A list of values.
    List(Vec<TemplateValue>),

    /// A map of string keys to values.
    Map(HashMap<String, TemplateValue>),

    /// A null/missing value.
    Null,
}

impl TemplateValue {
    /// Check if this value is "truthy" for conditional evaluation.
    ///
    /// Truthiness rules (matching Pandoc):
    /// - Any non-empty map is truthy
    /// - Any array containing at least one truthy value is truthy
    /// - Any non-empty string is truthy (even "false")
    /// - Boolean true is truthy
    /// - Everything else is falsy
    pub fn is_truthy(&self) -> bool {
        match self {
            TemplateValue::Bool(b) => *b,
            TemplateValue::String(s) => !s.is_empty(),
            TemplateValue::List(items) => items.iter().any(|v| v.is_truthy()),
            TemplateValue::Map(m) => !m.is_empty(),
            TemplateValue::Null => false,
        }
    }

    /// Get a nested field by path.
    ///
    /// For example, `get_path(&["employee", "salary"])` on a Map containing
    /// `{"employee": {"salary": 50000}}` returns the salary value.
    pub fn get_path(&self, path: &[&str]) -> Option<&TemplateValue> {
        if path.is_empty() {
            return Some(self);
        }

        match self {
            TemplateValue::Map(m) => {
                let first = path[0];
                m.get(first).and_then(|v| v.get_path(&path[1..]))
            }
            _ => None,
        }
    }

    /// Render this value as a string for output.
    ///
    /// - String: returned as-is
    /// - Bool: "true" or "" (empty for false)
    /// - List: concatenation of rendered elements
    /// - Map: "true"
    /// - Null: ""
    pub fn render(&self) -> String {
        match self {
            TemplateValue::String(s) => s.clone(),
            TemplateValue::Bool(true) => "true".to_string(),
            TemplateValue::Bool(false) => String::new(),
            TemplateValue::List(items) => items.iter().map(|v| v.render()).collect(),
            TemplateValue::Map(_) => "true".to_string(),
            TemplateValue::Null => String::new(),
        }
    }
}

impl Default for TemplateValue {
    fn default() -> Self {
        TemplateValue::Null
    }
}

/// A context for template evaluation containing variable bindings.
#[derive(Debug, Clone, Default)]
pub struct TemplateContext {
    /// Variable bindings at this level.
    variables: HashMap<String, TemplateValue>,

    /// Parent context for nested scopes (e.g., inside for loops).
    parent: Option<Box<TemplateContext>>,
}

impl TemplateContext {
    /// Create a new empty context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a variable into the context.
    pub fn insert(&mut self, key: impl Into<String>, value: TemplateValue) {
        self.variables.insert(key.into(), value);
    }

    /// Get a variable from the context, checking parent scopes.
    pub fn get(&self, key: &str) -> Option<&TemplateValue> {
        self.variables
            .get(key)
            .or_else(|| self.parent.as_ref().and_then(|p| p.get(key)))
    }

    /// Get a variable by path (e.g., "employee.salary").
    pub fn get_path(&self, path: &[&str]) -> Option<&TemplateValue> {
        if path.is_empty() {
            return None;
        }

        self.get(path[0]).and_then(|v| v.get_path(&path[1..]))
    }

    /// Create a child context for a nested scope (e.g., for loop iteration).
    ///
    /// The child context inherits access to parent variables.
    pub fn child(&self) -> TemplateContext {
        TemplateContext {
            variables: HashMap::new(),
            parent: Some(Box::new(self.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truthiness() {
        assert!(TemplateValue::Bool(true).is_truthy());
        assert!(!TemplateValue::Bool(false).is_truthy());

        assert!(TemplateValue::String("hello".to_string()).is_truthy());
        assert!(TemplateValue::String("false".to_string()).is_truthy()); // "false" string is truthy!
        assert!(!TemplateValue::String("".to_string()).is_truthy());

        assert!(TemplateValue::List(vec![TemplateValue::Bool(true)]).is_truthy());
        assert!(!TemplateValue::List(vec![TemplateValue::Bool(false)]).is_truthy());
        assert!(!TemplateValue::List(vec![]).is_truthy());

        let mut map = HashMap::new();
        map.insert("key".to_string(), TemplateValue::Null);
        assert!(TemplateValue::Map(map).is_truthy()); // Non-empty map is truthy

        assert!(!TemplateValue::Map(HashMap::new()).is_truthy());
        assert!(!TemplateValue::Null.is_truthy());
    }

    #[test]
    fn test_get_path() {
        let mut inner = HashMap::new();
        inner.insert(
            "salary".to_string(),
            TemplateValue::String("50000".to_string()),
        );

        let mut outer = HashMap::new();
        outer.insert("employee".to_string(), TemplateValue::Map(inner));

        let value = TemplateValue::Map(outer);

        assert_eq!(
            value.get_path(&["employee", "salary"]),
            Some(&TemplateValue::String("50000".to_string()))
        );
        assert_eq!(value.get_path(&["employee", "name"]), None);
        assert_eq!(value.get_path(&["nonexistent"]), None);
    }

    #[test]
    fn test_context_scoping() {
        let mut parent = TemplateContext::new();
        parent.insert("x", TemplateValue::String("parent_x".to_string()));
        parent.insert("y", TemplateValue::String("parent_y".to_string()));

        let mut child = parent.child();
        child.insert("x", TemplateValue::String("child_x".to_string()));

        // Child shadows parent for 'x'
        assert_eq!(
            child.get("x"),
            Some(&TemplateValue::String("child_x".to_string()))
        );
        // Child inherits 'y' from parent
        assert_eq!(
            child.get("y"),
            Some(&TemplateValue::String("parent_y".to_string()))
        );
        // Parent unchanged
        assert_eq!(
            parent.get("x"),
            Some(&TemplateValue::String("parent_x".to_string()))
        );
    }
}
