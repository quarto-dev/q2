/*
 * meta.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::block::Blocks;
use crate::inline::Inlines;
use hashlink::LinkedHashMap;
use serde::{Deserialize, Serialize};

// Pandoc's MetaValue notably does not support numbers or nulls, so we don't either
// https://pandoc.org/lua-filters.html#type-metavalue
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MetaValue {
    MetaString(String),
    MetaBool(bool),
    MetaInlines(Inlines),
    MetaBlocks(Blocks),
    MetaList(Vec<MetaValue>),
    MetaMap(LinkedHashMap<String, MetaValue>),
}

impl Default for MetaValue {
    fn default() -> Self {
        MetaValue::MetaMap(LinkedHashMap::new())
    }
}

pub type Meta = LinkedHashMap<String, MetaValue>;
