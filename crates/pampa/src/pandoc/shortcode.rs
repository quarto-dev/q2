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
