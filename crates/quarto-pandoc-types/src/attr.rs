/*
 * attr.rs
 * Copyright (c) 2025 Posit, PBC
 */

use hashlink::LinkedHashMap;
use quarto_source_map::SourceInfo;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::LazyLock;

/// The contents of an [`Attr`]: `(id, classes, key-value attributes)`.
pub type AttrTuple = (String, Vec<String>, LinkedHashMap<String, String>);

/// Pandoc's `Attr`: an id, a list of classes, and ordered key-value pairs.
///
/// **Layout.** Most nodes that can carry an `Attr` carry an empty one (92% of
/// them on the `claude-notes/plans` corpus), so the empty case is stored as a
/// null pointer and only a non-empty `Attr` owns a heap [`AttrTuple`]. That
/// makes this type one word instead of 104 bytes. See
/// `claude-notes/plans/2026-11-12-ast-string-storage-study.md`.
///
/// **Use.** `Attr` dereferences to [`AttrTuple`], so it reads and writes like
/// the tuple it used to be: `attr.0`, `attr.1.push(..)`, `attr.2.get(..)`.
/// Build one from a tuple with `.into()` (or [`Attr::from`]) and take it apart
/// with [`Attr::into_tuple`].
///
/// Mutable access (`DerefMut`) to an empty `Attr` allocates its tuple, even if
/// nothing ends up being written. That costs memory but never changes
/// meaning: equality, `Debug` and serde all look at the contents, so an
/// allocated-but-empty `Attr` is indistinguishable from an unallocated one.
#[derive(Clone, Default)]
pub struct Attr {
    // Named (not a tuple struct) so that `attr.0` reaches the tuple through
    // `Deref` instead of this field.
    inner: Option<Box<AttrTuple>>,
}

static EMPTY_ATTR: LazyLock<AttrTuple> =
    LazyLock::new(|| (String::new(), Vec::new(), LinkedHashMap::new()));

fn is_empty_tuple(t: &AttrTuple) -> bool {
    t.0.is_empty() && t.1.is_empty() && t.2.is_empty()
}

impl Attr {
    /// Consumes the `Attr`, returning `(id, classes, attributes)`.
    pub fn into_tuple(self) -> AttrTuple {
        match self.inner {
            Some(boxed) => *boxed,
            None => (String::new(), Vec::new(), LinkedHashMap::new()),
        }
    }
}

impl Deref for Attr {
    type Target = AttrTuple;
    fn deref(&self) -> &AttrTuple {
        match &self.inner {
            Some(boxed) => boxed,
            None => &EMPTY_ATTR,
        }
    }
}

impl DerefMut for Attr {
    fn deref_mut(&mut self) -> &mut AttrTuple {
        self.inner.get_or_insert_with(Box::default)
    }
}

impl From<AttrTuple> for Attr {
    fn from(tuple: AttrTuple) -> Self {
        Attr {
            inner: (!is_empty_tuple(&tuple)).then(|| Box::new(tuple)),
        }
    }
}

impl From<Attr> for AttrTuple {
    fn from(attr: Attr) -> Self {
        attr.into_tuple()
    }
}

impl PartialEq for Attr {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl PartialEq<AttrTuple> for Attr {
    fn eq(&self, other: &AttrTuple) -> bool {
        **self == *other
    }
}

impl fmt::Debug for Attr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}

impl Serialize for Attr {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (**self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Attr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        AttrTuple::deserialize(deserializer).map(Attr::from)
    }
}

pub fn empty_attr() -> Attr {
    Attr::default()
}

/// Whether an attr has no id, no classes and no key-value pairs.
///
/// Takes the tuple so it accepts both `&Attr` (through deref coercion) and a
/// bare `&AttrTuple`.
pub fn is_empty_attr(attr: &AttrTuple) -> bool {
    is_empty_tuple(attr)
}

/// Source location information for Attr components.
///
/// Attr is a tuple: (id: String, classes: Vec<String>, attributes: HashMap<String, String>)
/// This struct tracks source locations for each component:
/// - id: Source location of the id string (None if id is empty "")
/// - classes: Source locations for each class string
/// - attributes: Source locations for each key-value pair (key and value)
///
/// **The key and value slots are not the same kind of span** (since D1,
/// `claude-notes/plans/2026-08-20-provenance-2-consumers/`). The key slot is
/// always a raw, quote-inclusive node span. The value slot is **content
/// provenance**: quotes excluded, escapes collapsed, produced by
/// `pandoc::treesitter_utils::text_helpers::extract_quoted_text`. For a
/// value with no escapes the two kinds coincide up to quote-trimming and
/// the span is `Original`. For a value with at least one collapsed escape
/// there is no affine map from content back to source, so the span is a
/// top-level `Concat` of `Original` leaves instead (the decoder builds via
/// `ProvenanceBuilder::in_file`, so there is no `Substring` wrapper) — and
/// `SourceInfo::resolve_byte_range` returns `None` for that shape
/// (`quarto-source-map` 0.1.3 `source_info.rs:403`), where it previously
/// returned a drifting `Some` against the old quote-inclusive span. A
/// consumer that needs the byte range of an escaped value must resolve it
/// piecewise (walk the `Concat`'s pieces, or use the `map_offset` pair)
/// rather than reach for `resolve_byte_range`.
///
/// **Positional-alignment invariant** (added 2026-05-22, Plan 6):
/// `attributes[i]` is the `(key_src, val_src)` for the i-th entry in
/// `Attr.2` (`LinkedHashMap<String,String>`) in **insertion order**.
/// Consumers that index into `attributes` by key position (e.g. to
/// recover the source range of a value before `kvs.remove(key)`) rely
/// on this lockstep.
///
/// This invariant holds in the parser's main path but is **broken by
/// two preexisting code paths** (tracked separately):
/// - **bd-3aolj** — `commonmark_attribute.rs:41-49` (duplicate-key
///   handling: `LinkedHashMap::insert` updates in place while
///   `attr_source.attributes.push` always appends).
/// - **bd-1e6a5** — caption-attr-into-table merge in `section.rs` and
///   `postprocess.rs` (same root cause when caption + table keys
///   overlap).
///
/// Until those fix-ups land, indexing consumers should guard with a
/// runtime length check (`kvs.len() == attr_source.attributes.len()`)
/// plus a `debug_assert_eq!` and fall back to `None` (or whatever
/// `Option<SourceInfo>`-aware behavior the consumer prefers) on
/// mismatch so production never panics on misaligned input. See
/// `quarto-core/src/transforms/theorem.rs` and `proof.rs` for the
/// canonical fall-back-to-`None` pattern.
///
/// **Layout.** Like [`Attr`], this is one word: the empty case (no source
/// tracking, which is what an empty `Attr` has) is a null pointer, and only a
/// non-empty value owns a heap [`AttrSourceParts`]. It dereferences to
/// `AttrSourceParts`, so `attr_source.id`, `.classes` and `.attributes` read
/// and write as before. Build one with [`AttrSourceInfo::new`] or from an
/// `AttrSourceParts` with `.into()`. As with `Attr`, mutable access to an
/// empty value allocates, and equality, `Debug` and serde look only at the
/// contents.
#[derive(Clone, Default)]
pub struct AttrSourceInfo {
    inner: Option<Box<AttrSourceParts>>,
}

/// The contents of an [`AttrSourceInfo`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AttrSourceParts {
    pub id: Option<SourceInfo>,
    pub classes: Vec<Option<SourceInfo>>,
    pub attributes: Vec<(Option<SourceInfo>, Option<SourceInfo>)>,
}

static EMPTY_ATTR_SOURCE: AttrSourceParts = AttrSourceParts {
    id: None,
    classes: Vec::new(),
    attributes: Vec::new(),
};

impl AttrSourceParts {
    fn is_empty(&self) -> bool {
        self.id.is_none() && self.classes.is_empty() && self.attributes.is_empty()
    }
}

impl Deref for AttrSourceInfo {
    type Target = AttrSourceParts;
    fn deref(&self) -> &AttrSourceParts {
        match &self.inner {
            Some(boxed) => boxed,
            None => &EMPTY_ATTR_SOURCE,
        }
    }
}

impl DerefMut for AttrSourceInfo {
    fn deref_mut(&mut self) -> &mut AttrSourceParts {
        self.inner.get_or_insert_with(Box::default)
    }
}

impl From<AttrSourceParts> for AttrSourceInfo {
    fn from(parts: AttrSourceParts) -> Self {
        AttrSourceInfo {
            inner: (!parts.is_empty()).then(|| Box::new(parts)),
        }
    }
}

impl PartialEq for AttrSourceInfo {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl fmt::Debug for AttrSourceInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Print under the type's own name, as the derived impl did.
        f.debug_struct("AttrSourceInfo")
            .field("id", &self.id)
            .field("classes", &self.classes)
            .field("attributes", &self.attributes)
            .finish()
    }
}

impl Serialize for AttrSourceInfo {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (**self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AttrSourceInfo {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        AttrSourceParts::deserialize(deserializer).map(AttrSourceInfo::from)
    }
}

impl AttrSourceInfo {
    /// Creates an empty AttrSourceInfo with no source tracking.
    pub fn empty() -> Self {
        AttrSourceInfo::default()
    }

    /// Consumes the AttrSourceInfo, returning its components.
    pub fn into_parts(self) -> AttrSourceParts {
        self.inner.map(|boxed| *boxed).unwrap_or_default()
    }

    /// Creates an AttrSourceInfo from its three components.
    pub fn new(
        id: Option<SourceInfo>,
        classes: Vec<Option<SourceInfo>>,
        attributes: Vec<(Option<SourceInfo>, Option<SourceInfo>)>,
    ) -> Self {
        AttrSourceParts {
            id,
            classes,
            attributes,
        }
        .into()
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
/// - url: Source location of the URL string (None if url is empty ""). This
///   is a raw, quote-inclusive node span — the URL is never decoded.
/// - title: Source location of the title string (None if title is empty
///   ""). Since D1 (`claude-notes/plans/2026-08-20-provenance-2-consumers/`)
///   this is **content provenance**, not a raw span: quotes excluded,
///   escapes collapsed, produced by the same
///   `extract_quoted_text`/`unescape_punctuation` decoder as
///   `AttrSourceInfo.attributes[i].1` (see that struct's doc for the
///   key/value-asymmetry note, which applies here identically between
///   `url` and `title`). An escaped title's span is a top-level `Concat`
///   of `Original` leaves, for which `resolve_byte_range` returns `None` —
///   resolve it piecewise instead.
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
        let attr_source =
            AttrSourceInfo::new(Some(SourceInfo::original(FileId(0), 5, 15)), vec![], vec![]);

        let result = attr_source.combine_all();
        assert!(result.is_some());

        // Should return the id source info
        let source = result.unwrap();
        assert_eq!(source, SourceInfo::original(FileId(0), 5, 15));
    }

    #[test]
    fn test_combine_all_only_classes() {
        // Only classes present
        let attr_source = AttrSourceInfo::new(
            None,
            vec![
                Some(SourceInfo::original(FileId(0), 10, 20)),
                Some(SourceInfo::original(FileId(0), 21, 30)),
            ],
            vec![],
        );

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
        let attr_source = AttrSourceInfo::new(
            Some(SourceInfo::original(FileId(0), 5, 15)),
            vec![
                Some(SourceInfo::original(FileId(0), 16, 25)),
                None, // Some classes may not have source info
                Some(SourceInfo::original(FileId(0), 26, 35)),
            ],
            vec![],
        );

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
        let attr_source = AttrSourceInfo::new(
            Some(SourceInfo::original(FileId(0), 5, 15)),
            vec![Some(SourceInfo::original(FileId(0), 16, 25))],
            vec![
                (
                    Some(SourceInfo::original(FileId(0), 26, 30)), // key
                    Some(SourceInfo::original(FileId(0), 31, 40)), // value
                ),
                (
                    None,                                          // key without source
                    Some(SourceInfo::original(FileId(0), 41, 50)), // value
                ),
            ],
        );

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
        let attr_source = AttrSourceInfo::new(
            None,
            vec![None, Some(SourceInfo::original(FileId(0), 10, 20)), None],
            vec![
                (None, None),
                (None, Some(SourceInfo::original(FileId(0), 30, 40))),
            ],
        );

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

#[cfg(test)]
mod layout_tests {
    use super::*;

    fn attr(id: &str, classes: &[&str]) -> Attr {
        (
            id.to_string(),
            classes.iter().map(|c| c.to_string()).collect(),
            LinkedHashMap::new(),
        )
            .into()
    }

    #[test]
    fn attr_and_attr_source_are_one_word() {
        assert_eq!(std::mem::size_of::<Attr>(), std::mem::size_of::<usize>());
        assert_eq!(
            std::mem::size_of::<AttrSourceInfo>(),
            std::mem::size_of::<usize>()
        );
    }

    #[test]
    fn empty_attr_does_not_allocate() {
        assert!(empty_attr().inner.is_none());
        assert!(
            Attr::from((String::new(), vec![], LinkedHashMap::new()))
                .inner
                .is_none()
        );
        assert!(AttrSourceInfo::empty().inner.is_none());
        assert!(AttrSourceInfo::new(None, vec![], vec![]).inner.is_none());
    }

    #[test]
    fn attr_reads_and_writes_like_a_tuple() {
        let mut a = empty_attr();
        assert!(a.0.is_empty() && a.1.is_empty() && a.2.is_empty());
        assert!(is_empty_attr(&a));
        a.1.push("cls".to_string());
        a.0 = "id".to_string();
        a.2.insert("k".to_string(), "v".to_string());
        assert!(!is_empty_attr(&a));
        let (id, classes, kvs) = a.into_tuple();
        assert_eq!(id, "id");
        assert_eq!(classes, vec!["cls".to_string()]);
        assert_eq!(kvs.get("k").map(String::as_str), Some("v"));
    }

    #[test]
    fn allocated_but_empty_equals_unallocated() {
        let mut touched = empty_attr();
        let _ = &mut touched.1; // DerefMut allocates
        assert!(touched.inner.is_some());
        assert_eq!(touched, empty_attr());
        assert!(is_empty_attr(&touched));
        assert_eq!(format!("{touched:?}"), format!("{:?}", empty_attr()));
        assert_eq!(
            serde_json::to_string(&touched).unwrap(),
            serde_json::to_string(&empty_attr()).unwrap()
        );

        let mut src = AttrSourceInfo::empty();
        src.classes.clear();
        assert!(src.inner.is_some());
        assert_eq!(src, AttrSourceInfo::empty());
    }

    #[test]
    fn debug_and_serde_match_the_old_shapes() {
        let a = attr("x", &["a", "b"]);
        let tuple: AttrTuple = a.clone().into_tuple();
        assert_eq!(format!("{a:?}"), format!("{tuple:?}"));
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(json, serde_json::to_string(&tuple).unwrap());
        assert_eq!(serde_json::from_str::<Attr>(&json).unwrap(), a);

        let src = AttrSourceInfo::new(Some(SourceInfo::for_test()), vec![None], vec![]);
        assert!(format!("{src:?}").starts_with("AttrSourceInfo { id: Some("));
        let json = serde_json::to_string(&src).unwrap();
        assert_eq!(json, serde_json::to_string(&*src).unwrap());
        assert_eq!(serde_json::from_str::<AttrSourceInfo>(&json).unwrap(), src);
    }
}
