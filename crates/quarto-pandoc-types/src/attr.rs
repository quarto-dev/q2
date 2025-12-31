/*
 * attr.rs
 * Copyright (c) 2025 Posit, PBC
 */

use hashlink::LinkedHashMap;
use quarto_source_map::SourceInfo;
use serde::{Deserialize, Serialize};

pub fn empty_attr() -> Attr {
    (String::new(), vec![], LinkedHashMap::new())
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

    /// Combine all source pieces into a single SourceInfo.
    ///
    /// This iterates through id, classes, and attributes, combining
    /// all non-None SourceInfo pieces using SourceInfo::combine().
    /// The result is a SourceInfo (either Original if contiguous, or Concat)
    /// that preserves all pieces.
    ///
    /// Returns None if no source info pieces are present.
    pub fn combine_all(&self) -> Option<SourceInfo> {
        let mut result: Option<SourceInfo> = None;

        // Add id if present
        if let Some(id_src) = &self.id {
            result = Some(id_src.clone());
        }

        // Add all class sources
        for src in self.classes.iter().flatten() {
            result = match result {
                Some(r) => Some(r.combine(src)),
                None => Some(src.clone()),
            };
        }

        // Add all key-value attribute sources
        for (key_src, val_src) in &self.attributes {
            if let Some(src) = key_src {
                result = match result {
                    Some(r) => Some(r.combine(src)),
                    None => Some(src.clone()),
                };
            }
            if let Some(src) = val_src {
                result = match result {
                    Some(r) => Some(r.combine(src)),
                    None => Some(src.clone()),
                };
            }
        }

        result
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

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::FileId;

    #[test]
    fn test_combine_all_empty() {
        // Empty AttrSourceInfo should return None
        let attr_source = AttrSourceInfo::empty();
        assert!(attr_source.combine_all().is_none());
    }

    #[test]
    fn test_combine_all_only_id() {
        // Only id present
        let attr_source = AttrSourceInfo {
            id: Some(SourceInfo::original(FileId(0), 5, 15)),
            classes: vec![],
            attributes: vec![],
        };

        let result = attr_source.combine_all();
        assert!(result.is_some());

        // Should return the id source info
        let source = result.unwrap();
        assert_eq!(source, SourceInfo::original(FileId(0), 5, 15));
    }

    #[test]
    fn test_combine_all_only_classes() {
        // Only classes present
        let attr_source = AttrSourceInfo {
            id: None,
            classes: vec![
                Some(SourceInfo::original(FileId(0), 10, 20)),
                Some(SourceInfo::original(FileId(0), 21, 30)),
            ],
            attributes: vec![],
        };

        let result = attr_source.combine_all();
        assert!(result.is_some());

        // Should combine both class sources
        let source = result.unwrap();
        // The result should be a Concat that includes both ranges
        match source {
            SourceInfo::Concat { pieces } => {
                assert_eq!(pieces.len(), 2);
            }
            SourceInfo::Original { .. } => {
                // If combine() merged them into a single range, that's also acceptable
            }
            _ => panic!("Unexpected SourceInfo variant"),
        }
    }

    #[test]
    fn test_combine_all_id_and_classes() {
        // Both id and classes present
        let attr_source = AttrSourceInfo {
            id: Some(SourceInfo::original(FileId(0), 5, 15)),
            classes: vec![
                Some(SourceInfo::original(FileId(0), 16, 25)),
                None, // Some classes may not have source info
                Some(SourceInfo::original(FileId(0), 26, 35)),
            ],
            attributes: vec![],
        };

        let result = attr_source.combine_all();
        assert!(result.is_some());

        // Should combine id and non-None classes
        let source = result.unwrap();
        match source {
            SourceInfo::Concat { pieces } => {
                // Could be 2-3 pieces depending on whether contiguous ranges were merged
                // (5-15, 16-25, 26-35 are contiguous so may merge to: 5-15, 16-35)
                assert!(pieces.len() >= 2 && pieces.len() <= 3);
            }
            SourceInfo::Original { .. } => {
                // If all ranges were contiguous and fully merged, that's also valid
            }
            _ => panic!("Unexpected SourceInfo variant"),
        }
    }

    #[test]
    fn test_combine_all_full_attributes() {
        // Full AttrSourceInfo with id, classes, and key-value attributes
        let attr_source = AttrSourceInfo {
            id: Some(SourceInfo::original(FileId(0), 5, 15)),
            classes: vec![Some(SourceInfo::original(FileId(0), 16, 25))],
            attributes: vec![
                (
                    Some(SourceInfo::original(FileId(0), 26, 30)), // key
                    Some(SourceInfo::original(FileId(0), 31, 40)), // value
                ),
                (
                    None,                                          // key without source
                    Some(SourceInfo::original(FileId(0), 41, 50)), // value
                ),
            ],
        };

        let result = attr_source.combine_all();
        assert!(result.is_some());

        // Should combine all pieces
        let source = result.unwrap();
        match source {
            SourceInfo::Concat { pieces } => {
                // Could be 2-5 pieces depending on contiguous range merging
                // (5-15, 16-25, 26-30, 31-40, 41-50 are all contiguous)
                assert!(pieces.len() >= 2 && pieces.len() <= 5);
            }
            SourceInfo::Original { .. } => {
                // If all ranges were contiguous and fully merged into one
            }
            _ => panic!("Unexpected SourceInfo variant"),
        }
    }

    #[test]
    fn test_combine_all_sparse_attributes() {
        // Some pieces missing source info (None)
        let attr_source = AttrSourceInfo {
            id: None,
            classes: vec![None, Some(SourceInfo::original(FileId(0), 10, 20)), None],
            attributes: vec![
                (None, None),
                (None, Some(SourceInfo::original(FileId(0), 30, 40))),
            ],
        };

        let result = attr_source.combine_all();
        assert!(result.is_some());

        // Should only combine non-None pieces
        let source = result.unwrap();
        match source {
            SourceInfo::Concat { pieces } => {
                // Should have 2 pieces: one class + one value
                assert_eq!(pieces.len(), 2);
            }
            SourceInfo::Original { .. } => {
                // If they were merged
            }
            _ => panic!("Unexpected SourceInfo variant"),
        }
    }
}
