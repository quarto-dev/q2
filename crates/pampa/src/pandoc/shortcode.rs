/*
 * shortcode.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * This module contains conversion functions for shortcodes.
 * The type definitions (Shortcode, ShortcodeArg) are
 * defined in quarto-pandoc-types and re-exported from the pandoc module.
 */

use crate::pandoc::location::empty_source_info;
use hashlink::LinkedHashMap;
use quarto_pandoc_types::{AttrSourceInfo, Inline, Inlines, Shortcode, ShortcodeArg, Span};

fn shortcode_value_span(str: String) -> Inline {
    let mut attr_hash = LinkedHashMap::new();
    attr_hash.insert("data-raw".to_string(), str.clone());
    attr_hash.insert("data-value".to_string(), str);
    attr_hash.insert("data-is-shortcode".to_string(), "1".to_string());

    Inline::Span(Span {
        attr: (
            String::new(),
            vec!["quarto-shortcode__-param".to_string()],
            attr_hash,
        ),
        content: vec![],
        source_info: empty_source_info(),
        attr_source: AttrSourceInfo::empty(),
    })
}

fn shortcode_key_value_span(key: String, value: String) -> Inline {
    let mut attr_hash = LinkedHashMap::new();

    // this needs to be fixed and needs to use the actual source. We'll do that when we have source mapping
    attr_hash.insert(
        "data-raw".to_string(),
        format!("{} = {}", key.clone(), value.clone()),
    );
    attr_hash.insert("data-key".to_string(), key);
    attr_hash.insert("data-value".to_string(), value);
    attr_hash.insert("data-is-shortcode".to_string(), "1".to_string());

    Inline::Span(Span {
        attr: (
            String::new(),
            vec!["quarto-shortcode__-param".to_string()],
            attr_hash,
        ),
        content: vec![],
        source_info: empty_source_info(),
        attr_source: AttrSourceInfo::empty(),
    })
}

pub fn shortcode_to_span(shortcode: Shortcode) -> Span {
    let mut attr_hash = LinkedHashMap::new();
    let mut content: Inlines = vec![shortcode_value_span(shortcode.name)];
    for arg in shortcode.positional_args {
        match arg {
            ShortcodeArg::String(text) => {
                content.push(shortcode_value_span(text));
            }
            ShortcodeArg::Number(num) => {
                content.push(shortcode_value_span(num.to_string()));
            }
            ShortcodeArg::Boolean(b) => {
                content.push(shortcode_value_span(if b {
                    "true".to_string()
                } else {
                    "false".to_string()
                }));
            }
            ShortcodeArg::Shortcode(inner_shortcode) => {
                content.push(Inline::Span(shortcode_to_span(inner_shortcode)));
            }
            ShortcodeArg::KeyValue(spec) => {
                for (key, value) in spec {
                    match value {
                        ShortcodeArg::String(text) => {
                            content.push(shortcode_key_value_span(key, text));
                        }
                        ShortcodeArg::Number(num) => {
                            content.push(shortcode_key_value_span(key, num.to_string()));
                        }
                        ShortcodeArg::Boolean(b) => {
                            content.push(shortcode_key_value_span(
                                key,
                                if b {
                                    "true".to_string()
                                } else {
                                    "false".to_string()
                                },
                            ));
                        }
                        ShortcodeArg::Shortcode(_) => {
                            eprintln!("PANIC - Quarto doesn't support nested shortcodes");
                            std::process::exit(1);
                        }
                        _ => {
                            panic!("Unexpected ShortcodeArg type in shortcode: {:?}", value);
                        }
                    }
                }
            }
        }
    }
    // Process keyword arguments from the keyword_args HashMap
    for (key, value) in shortcode.keyword_args {
        match value {
            ShortcodeArg::String(text) => {
                content.push(shortcode_key_value_span(key, text));
            }
            ShortcodeArg::Number(num) => {
                content.push(shortcode_key_value_span(key, num.to_string()));
            }
            ShortcodeArg::Boolean(b) => {
                content.push(shortcode_key_value_span(
                    key,
                    if b {
                        "true".to_string()
                    } else {
                        "false".to_string()
                    },
                ));
            }
            ShortcodeArg::Shortcode(_) => {
                eprintln!("PANIC - Quarto doesn't support nested shortcodes in keyword args");
                std::process::exit(1);
            }
            ShortcodeArg::KeyValue(_) => {
                eprintln!("PANIC - KeyValue shouldn't appear in keyword_args HashMap");
                std::process::exit(1);
            }
        }
    }
    attr_hash.insert("data-is-shortcode".to_string(), "1".to_string());
    Span {
        attr: (
            String::new(),
            vec!["quarto-shortcode__".to_string()],
            attr_hash,
        ),
        content,
        source_info: empty_source_info(),
        attr_source: AttrSourceInfo::empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::{Space, Str};
    use std::collections::HashMap;

    fn si() -> quarto_source_map::SourceInfo {
        quarto_source_map::SourceInfo::default()
    }

    /// Helper to extract the data-value attribute from a Span inline
    fn get_span_data_value(inline: &Inline) -> Option<&str> {
        if let Inline::Span(span) = inline {
            span.attr.2.get("data-value").map(|s| s.as_str())
        } else {
            None
        }
    }

    /// Helper to extract the data-key attribute from a Span inline
    fn get_span_data_key(inline: &Inline) -> Option<&str> {
        if let Inline::Span(span) = inline {
            span.attr.2.get("data-key").map(|s| s.as_str())
        } else {
            None
        }
    }

    #[test]
    fn test_shortcode_name_only() {
        // Test a shortcode with just a name, no arguments
        let sc = Shortcode {
            is_escaped: false,
            name: "meta".to_string(),
            positional_args: vec![],
            keyword_args: HashMap::new(),
            source_info: si(),
        };

        let span = shortcode_to_span(sc);

        // Should have class "quarto-shortcode__"
        assert!(span.attr.1.contains(&"quarto-shortcode__".to_string()));
        // First content should be the name
        assert_eq!(span.content.len(), 1);
        assert_eq!(get_span_data_value(&span.content[0]), Some("meta"));
    }

    #[test]
    fn test_positional_string_arg() {
        let sc = Shortcode {
            is_escaped: false,
            name: "meta".to_string(),
            positional_args: vec![ShortcodeArg::String("title".to_string())],
            keyword_args: HashMap::new(),
            source_info: si(),
        };

        let span = shortcode_to_span(sc);

        // Content: [name, string_arg]
        assert_eq!(span.content.len(), 2);
        assert_eq!(get_span_data_value(&span.content[0]), Some("meta"));
        assert_eq!(get_span_data_value(&span.content[1]), Some("title"));
    }

    #[test]
    fn test_positional_number_arg() {
        let sc = Shortcode {
            is_escaped: false,
            name: "index".to_string(),
            positional_args: vec![ShortcodeArg::Number(42.5)],
            keyword_args: HashMap::new(),
            source_info: si(),
        };

        let span = shortcode_to_span(sc);

        assert_eq!(span.content.len(), 2);
        assert_eq!(get_span_data_value(&span.content[0]), Some("index"));
        assert_eq!(get_span_data_value(&span.content[1]), Some("42.5"));
    }

    #[test]
    fn test_positional_boolean_true() {
        let sc = Shortcode {
            is_escaped: false,
            name: "flag".to_string(),
            positional_args: vec![ShortcodeArg::Boolean(true)],
            keyword_args: HashMap::new(),
            source_info: si(),
        };

        let span = shortcode_to_span(sc);

        assert_eq!(span.content.len(), 2);
        assert_eq!(get_span_data_value(&span.content[1]), Some("true"));
    }

    #[test]
    fn test_positional_boolean_false() {
        // Tests the `false` branch of positional Boolean handling
        let sc = Shortcode {
            is_escaped: false,
            name: "flag".to_string(),
            positional_args: vec![ShortcodeArg::Boolean(false)],
            keyword_args: HashMap::new(),
            source_info: si(),
        };

        let span = shortcode_to_span(sc);

        assert_eq!(span.content.len(), 2);
        assert_eq!(get_span_data_value(&span.content[1]), Some("false"));
    }

    #[test]
    fn test_nested_shortcode() {
        // Tests ShortcodeArg::Shortcode in positional args
        let inner = Shortcode {
            is_escaped: false,
            name: "inner".to_string(),
            positional_args: vec![],
            keyword_args: HashMap::new(),
            source_info: si(),
        };

        let outer = Shortcode {
            is_escaped: false,
            name: "outer".to_string(),
            positional_args: vec![ShortcodeArg::Shortcode(inner)],
            keyword_args: HashMap::new(),
            source_info: si(),
        };

        let span = shortcode_to_span(outer);

        // Content: [outer_name, inner_span]
        assert_eq!(span.content.len(), 2);
        assert_eq!(get_span_data_value(&span.content[0]), Some("outer"));

        // Second item should be a nested span
        if let Inline::Span(inner_span) = &span.content[1] {
            // Inner span should have its own name
            assert_eq!(inner_span.content.len(), 1);
            assert_eq!(get_span_data_value(&inner_span.content[0]), Some("inner"));
        } else {
            panic!("Expected nested Span for inner shortcode");
        }
    }

    #[test]
    fn test_keyvalue_positional_with_string() {
        // Tests ShortcodeArg::KeyValue with String value
        let mut kv = HashMap::new();
        kv.insert(
            "format".to_string(),
            ShortcodeArg::String("html".to_string()),
        );

        let sc = Shortcode {
            is_escaped: false,
            name: "embed".to_string(),
            positional_args: vec![ShortcodeArg::KeyValue(kv)],
            keyword_args: HashMap::new(),
            source_info: si(),
        };

        let span = shortcode_to_span(sc);

        // Content: [name, key_value_span]
        assert_eq!(span.content.len(), 2);
        assert_eq!(get_span_data_key(&span.content[1]), Some("format"));
        assert_eq!(get_span_data_value(&span.content[1]), Some("html"));
    }

    #[test]
    fn test_keyvalue_positional_with_number() {
        // Tests ShortcodeArg::KeyValue with Number value
        let mut kv = HashMap::new();
        kv.insert("width".to_string(), ShortcodeArg::Number(800.0));

        let sc = Shortcode {
            is_escaped: false,
            name: "image".to_string(),
            positional_args: vec![ShortcodeArg::KeyValue(kv)],
            keyword_args: HashMap::new(),
            source_info: si(),
        };

        let span = shortcode_to_span(sc);

        assert_eq!(span.content.len(), 2);
        assert_eq!(get_span_data_key(&span.content[1]), Some("width"));
        assert_eq!(get_span_data_value(&span.content[1]), Some("800"));
    }

    #[test]
    fn test_keyvalue_positional_with_boolean_true() {
        // Tests ShortcodeArg::KeyValue with Boolean(true) value
        let mut kv = HashMap::new();
        kv.insert("enabled".to_string(), ShortcodeArg::Boolean(true));

        let sc = Shortcode {
            is_escaped: false,
            name: "feature".to_string(),
            positional_args: vec![ShortcodeArg::KeyValue(kv)],
            keyword_args: HashMap::new(),
            source_info: si(),
        };

        let span = shortcode_to_span(sc);

        assert_eq!(span.content.len(), 2);
        assert_eq!(get_span_data_key(&span.content[1]), Some("enabled"));
        assert_eq!(get_span_data_value(&span.content[1]), Some("true"));
    }

    #[test]
    fn test_keyvalue_positional_with_boolean_false() {
        // Tests ShortcodeArg::KeyValue with Boolean(false) value
        let mut kv = HashMap::new();
        kv.insert("enabled".to_string(), ShortcodeArg::Boolean(false));

        let sc = Shortcode {
            is_escaped: false,
            name: "feature".to_string(),
            positional_args: vec![ShortcodeArg::KeyValue(kv)],
            keyword_args: HashMap::new(),
            source_info: si(),
        };

        let span = shortcode_to_span(sc);

        assert_eq!(span.content.len(), 2);
        assert_eq!(get_span_data_key(&span.content[1]), Some("enabled"));
        assert_eq!(get_span_data_value(&span.content[1]), Some("false"));
    }

    #[test]
    fn test_keyword_arg_string() {
        // Tests keyword_args with String value
        let mut kwargs = HashMap::new();
        kwargs.insert(
            "format".to_string(),
            ShortcodeArg::String("pdf".to_string()),
        );

        let sc = Shortcode {
            is_escaped: false,
            name: "export".to_string(),
            positional_args: vec![],
            keyword_args: kwargs,
            source_info: si(),
        };

        let span = shortcode_to_span(sc);

        // Content: [name, keyword_span]
        assert_eq!(span.content.len(), 2);
        assert_eq!(get_span_data_key(&span.content[1]), Some("format"));
        assert_eq!(get_span_data_value(&span.content[1]), Some("pdf"));
    }

    #[test]
    fn test_keyword_arg_number() {
        let mut kwargs = HashMap::new();
        kwargs.insert("scale".to_string(), ShortcodeArg::Number(1.5));

        let sc = Shortcode {
            is_escaped: false,
            name: "render".to_string(),
            positional_args: vec![],
            keyword_args: kwargs,
            source_info: si(),
        };

        let span = shortcode_to_span(sc);

        assert_eq!(span.content.len(), 2);
        assert_eq!(get_span_data_key(&span.content[1]), Some("scale"));
        assert_eq!(get_span_data_value(&span.content[1]), Some("1.5"));
    }

    #[test]
    fn test_keyword_arg_boolean_true() {
        let mut kwargs = HashMap::new();
        kwargs.insert("verbose".to_string(), ShortcodeArg::Boolean(true));

        let sc = Shortcode {
            is_escaped: false,
            name: "run".to_string(),
            positional_args: vec![],
            keyword_args: kwargs,
            source_info: si(),
        };

        let span = shortcode_to_span(sc);

        assert_eq!(span.content.len(), 2);
        assert_eq!(get_span_data_key(&span.content[1]), Some("verbose"));
        assert_eq!(get_span_data_value(&span.content[1]), Some("true"));
    }

    #[test]
    fn test_keyword_arg_boolean_false() {
        // Tests keyword_args with Boolean(false) value
        let mut kwargs = HashMap::new();
        kwargs.insert("verbose".to_string(), ShortcodeArg::Boolean(false));

        let sc = Shortcode {
            is_escaped: false,
            name: "run".to_string(),
            positional_args: vec![],
            keyword_args: kwargs,
            source_info: si(),
        };

        let span = shortcode_to_span(sc);

        assert_eq!(span.content.len(), 2);
        assert_eq!(get_span_data_key(&span.content[1]), Some("verbose"));
        assert_eq!(get_span_data_value(&span.content[1]), Some("false"));
    }

    #[test]
    fn test_mixed_args() {
        // Test with multiple positional and keyword args
        let mut kwargs = HashMap::new();
        kwargs.insert(
            "output".to_string(),
            ShortcodeArg::String("html".to_string()),
        );

        let sc = Shortcode {
            is_escaped: false,
            name: "convert".to_string(),
            positional_args: vec![
                ShortcodeArg::String("input.md".to_string()),
                ShortcodeArg::Boolean(true),
            ],
            keyword_args: kwargs,
            source_info: si(),
        };

        let span = shortcode_to_span(sc);

        // Content: [name, pos_string, pos_bool, keyword]
        assert_eq!(span.content.len(), 4);
        assert_eq!(get_span_data_value(&span.content[0]), Some("convert"));
        assert_eq!(get_span_data_value(&span.content[1]), Some("input.md"));
        assert_eq!(get_span_data_value(&span.content[2]), Some("true"));
        assert_eq!(get_span_data_key(&span.content[3]), Some("output"));
        assert_eq!(get_span_data_value(&span.content[3]), Some("html"));
    }

    #[test]
    fn test_escaped_shortcode() {
        // Test that escaped flag is preserved (though currently not used in output)
        let sc = Shortcode {
            is_escaped: true,
            name: "raw".to_string(),
            positional_args: vec![],
            keyword_args: HashMap::new(),
            source_info: si(),
        };

        let span = shortcode_to_span(sc);

        // Basic structure should still work
        assert!(span.attr.1.contains(&"quarto-shortcode__".to_string()));
        assert_eq!(span.content.len(), 1);
    }

    #[test]
    fn test_get_span_data_value_with_non_span() {
        // Test that get_span_data_value returns None for non-Span inlines
        let str_inline = Inline::Str(Str {
            text: "hello".to_string(),
            source_info: si(),
        });
        assert_eq!(get_span_data_value(&str_inline), None);

        let space_inline = Inline::Space(Space { source_info: si() });
        assert_eq!(get_span_data_value(&space_inline), None);
    }

    #[test]
    fn test_get_span_data_key_with_non_span() {
        // Test that get_span_data_key returns None for non-Span inlines
        let str_inline = Inline::Str(Str {
            text: "world".to_string(),
            source_info: si(),
        });
        assert_eq!(get_span_data_key(&str_inline), None);

        let space_inline = Inline::Space(Space { source_info: si() });
        assert_eq!(get_span_data_key(&space_inline), None);
    }
}
