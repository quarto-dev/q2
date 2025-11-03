/*
 * attr.rs
 * Copyright (c) 2025 Posit, PBC
 */

use quarto_source_map::SourceInfo;
use serde::{Deserialize, Serialize};
use hashlink::LinkedHashMap;

pub fn empty_attr() -> Attr {
    ("".to_string(), vec![], LinkedHashMap::new())
}

pub type Attr = (String, Vec<String>, LinkedHashMap<String, String>);

pub fn is_empty_attr(attr: &Attr) -> bool {
    attr.0.is_empty() && attr.1.is_empty() && attr.2.is_empty()
}

/// Source location information for Attr components.
///
/// Attr is a tuple: (id: String, classes: Vec<String>, attributes: HashMap<String, String>)
/// This struct tracks source locations for each component:
/// - id: Source location of the id string (None if id is empty "")
/// - classes: Source locations for each class string
/// - attributes: Source locations for each key-value pair (both key and value)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttrSourceInfo {
    pub id: Option<SourceInfo>,
    pub classes: Vec<Option<SourceInfo>>,
    pub attributes: Vec<(Option<SourceInfo>, Option<SourceInfo>)>,
}

impl AttrSourceInfo {
    /// Creates an empty AttrSourceInfo with no source tracking.
    pub fn empty() -> Self {
        AttrSourceInfo {
            id: None,
            classes: Vec::new(),
            attributes: Vec::new(),
        }
    }
}

/// Source location information for Target components.
///
/// Target is a tuple: (url: String, title: String)
/// This struct tracks source locations for each component:
/// - url: Source location of the URL string (None if url is empty "")
/// - title: Source location of the title string (None if title is empty "")
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetSourceInfo {
    pub url: Option<SourceInfo>,
    pub title: Option<SourceInfo>,
}

impl TargetSourceInfo {
    /// Creates an empty TargetSourceInfo with no source tracking.
    pub fn empty() -> Self {
        TargetSourceInfo {
            url: None,
            title: None,
        }
    }
}
