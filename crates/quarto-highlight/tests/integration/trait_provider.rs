//! Tests for the `UserGrammarProvider` trait — the abstraction that lets
//! both the native `UserGrammars` (wasmtime-backed) and the browser
//! `JsUserGrammars` (JS-callback-backed, Phase 4.3) feed `annotate_pandoc`
//! through the same code path. These tests use a minimal `MockProvider`
//! to exercise the contract without depending on either concrete impl.

use std::collections::HashMap;

use quarto_highlight::{
    HighlightError, SPANS_ATTR_KEY, UserGrammarProvider, annotate_pandoc, encoding,
};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{
    Attr, AttrSourceInfo, Block, CodeBlock, ConfigValue, ConfigValueKind, MergeOp,
};
use quarto_source_map::{FileId, SourceInfo};

/// Minimal [`UserGrammarProvider`] implementation backed by a
/// `HashMap<class, fixed JSON output>`. `call_count` lets tests assert
/// whether `highlight()` was dispatched.
struct MockProvider {
    classes: HashMap<String, String>,
    call_count: usize,
}

impl MockProvider {
    fn new() -> Self {
        Self {
            classes: HashMap::new(),
            call_count: 0,
        }
    }

    fn with(mut self, class: &str, json: &str) -> Self {
        self.classes.insert(class.to_string(), json.to_string());
        self
    }
}

impl UserGrammarProvider for MockProvider {
    fn contains(&self, class: &str) -> bool {
        self.classes.contains_key(class)
    }

    fn highlight(&mut self, class: &str, _source: &str) -> Result<Option<String>, HighlightError> {
        self.call_count += 1;
        Ok(self.classes.get(class).cloned())
    }
}

// ----- AST construction helpers (mirror those in tests/annotate.rs) -----

fn attr_with_class(class: &str) -> Attr {
    use hashlink::LinkedHashMap;
    (String::new(), vec![class.to_string()], LinkedHashMap::new())
}

fn empty_source_info() -> SourceInfo {
    SourceInfo::original(FileId(0), 0, 0)
}

fn empty_attr_source() -> AttrSourceInfo {
    AttrSourceInfo::empty()
}

fn make_code_block(class: &str, text: &str) -> Block {
    Block::CodeBlock(CodeBlock {
        attr: attr_with_class(class),
        text: text.to_string(),
        source_info: empty_source_info(),
        attr_source: empty_attr_source(),
    })
}

fn empty_pandoc() -> Pandoc {
    Pandoc {
        meta: ConfigValue {
            value: ConfigValueKind::Map(vec![]),
            source_info: empty_source_info(),
            merge_op: MergeOp::default(),
        },
        blocks: vec![],
    }
}

fn get_hl_attr(attr: &Attr) -> Option<&str> {
    attr.2.get(SPANS_ATTR_KEY).map(|s| s.as_str())
}

// ----- tests -----

#[test]
fn provider_output_is_written_to_attr() {
    // A class the provider knows about: annotate_pandoc must ask the
    // provider and put its output into `data-hl-spans`.
    let fixed_json = "[[0,3,\"custom.capture\"]]";
    let mut provider = MockProvider::new().with("mylang", fixed_json);

    let mut doc = empty_pandoc();
    doc.blocks.push(make_code_block("mylang", "abc def"));

    annotate_pandoc(&mut doc, Some(&mut provider)).expect("annotate must succeed");

    let Block::CodeBlock(cb) = &doc.blocks[0] else {
        unreachable!()
    };
    assert_eq!(get_hl_attr(&cb.attr), Some(fixed_json));
    assert_eq!(provider.call_count, 1);
}

#[test]
fn provider_fallthrough_uses_builtin_registry() {
    // Provider has no entries; `python` is a built-in. annotate_pandoc
    // must fall through to the Registry and not invoke provider.highlight.
    let mut provider = MockProvider::new();

    let mut doc = empty_pandoc();
    doc.blocks
        .push(make_code_block("python", "def foo(): pass\n"));

    annotate_pandoc(&mut doc, Some(&mut provider)).expect("annotate must succeed");

    let Block::CodeBlock(cb) = &doc.blocks[0] else {
        unreachable!()
    };
    let encoded = get_hl_attr(&cb.attr).expect("built-in should have highlighted");
    let spans = encoding::decode(encoded).unwrap();
    assert!(
        spans.iter().any(|s| s.capture == "keyword"),
        "expected at least one keyword capture from the built-in python grammar; got: {spans:?}",
    );
    assert_eq!(
        provider.call_count, 0,
        "provider.highlight must not be called for a class the provider doesn't contain",
    );
}

#[test]
fn provider_takes_precedence_over_builtin_on_class_collision() {
    // When the provider claims a class that's also a built-in, the
    // provider wins — users can override built-in grammars by loading
    // a user grammar with the same class name.
    let override_json = "[[0,3,\"provider.override\"]]";
    let mut provider = MockProvider::new().with("python", override_json);

    let mut doc = empty_pandoc();
    doc.blocks.push(make_code_block("python", "def foo()"));

    annotate_pandoc(&mut doc, Some(&mut provider)).expect("annotate must succeed");

    let Block::CodeBlock(cb) = &doc.blocks[0] else {
        unreachable!()
    };
    assert_eq!(
        get_hl_attr(&cb.attr),
        Some(override_json),
        "provider must take precedence over the built-in for the same class",
    );
    assert_eq!(provider.call_count, 1);
}

#[test]
fn provider_returning_none_leaves_attr_alone() {
    // Providers may legally report `contains() == true` but then
    // `highlight()` returns `Ok(None)` (e.g. the grammar is loaded
    // but produced no spans for this source). The walker should treat
    // this like an un-highlighted block — no attribute written.
    struct NullProvider;
    impl UserGrammarProvider for NullProvider {
        fn contains(&self, class: &str) -> bool {
            class == "mylang"
        }
        fn highlight(
            &mut self,
            _class: &str,
            _source: &str,
        ) -> Result<Option<String>, HighlightError> {
            Ok(None)
        }
    }

    let mut provider = NullProvider;
    let mut doc = empty_pandoc();
    doc.blocks.push(make_code_block("mylang", "whatever"));

    annotate_pandoc(&mut doc, Some(&mut provider)).expect("annotate must succeed");

    let Block::CodeBlock(cb) = &doc.blocks[0] else {
        unreachable!()
    };
    assert!(
        get_hl_attr(&cb.attr).is_none(),
        "no attribute should be written when provider returns None",
    );
}

#[test]
fn filter_authored_spans_still_win_over_provider() {
    // The existing filter-wins rule must survive the trait change: if
    // `data-hl-spans` is already set when the walker arrives, the
    // provider is never consulted.
    let mut provider = MockProvider::new().with("python", "[[0,3,\"would-have-won\"]]");

    let mut doc = empty_pandoc();
    let mut block = make_code_block("python", "def foo()");
    if let Block::CodeBlock(cb) = &mut block {
        cb.attr
            .2
            .insert(SPANS_ATTR_KEY.to_string(), "[]".to_string());
    }
    doc.blocks.push(block);

    annotate_pandoc(&mut doc, Some(&mut provider)).expect("annotate must succeed");

    let Block::CodeBlock(cb) = &doc.blocks[0] else {
        unreachable!()
    };
    assert_eq!(get_hl_attr(&cb.attr), Some("[]"));
    assert_eq!(
        provider.call_count, 0,
        "provider must not be consulted when attr is already set",
    );
}
