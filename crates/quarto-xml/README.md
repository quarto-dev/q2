# quarto-xml

Source-tracked XML parsing for Quarto, analogous to [quarto-yaml](../quarto-yaml/).

## Overview

This crate provides XML parsing with source location tracking. It wraps [quick-xml](https://docs.rs/quick-xml) to build a tree of elements where each element, attribute, and text node tracks its byte position in the original source.

## Core Types

- `XmlWithSourceInfo` - Parsed XML document with source tracking
- `XmlElement` - Element with name, attributes, children, and source positions
- `XmlAttribute` - Attribute with separate source tracking for name and value
- `XmlChildren` - Element content (elements, text, mixed, or empty)

## Usage

```rust
use quarto_xml::parse;

let xml = parse(r#"<style version="1.0">
  <macro name="author">
    <text variable="author"/>
  </macro>
</style>"#).unwrap();

// Navigate the tree
assert_eq!(xml.root.name, "style");
assert_eq!(xml.root.get_attribute("version"), Some("1.0"));

// Get children by name
let macros = xml.root.get_children("macro");
assert_eq!(macros[0].get_attribute("name"), Some("author"));

// Access source positions for error reporting
let attr = xml.root.get_attribute_full("version").unwrap();
let start = attr.value_source.start_offset();
let end = attr.value_source.end_offset();
```

## Features

- Element positions from start tag to end tag
- Precise attribute name and value positions
- Text and CDATA content tracking
- Mixed content support
- Namespace prefix handling

## Design

See [claude-notes/plans/2025-11-26-citeproc-rust-port-design.md](../../claude-notes/plans/2025-11-26-citeproc-rust-port-design.md) for architecture details.
