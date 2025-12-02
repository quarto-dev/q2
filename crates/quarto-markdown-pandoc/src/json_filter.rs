/*
 * json_filter.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * JSON filter support for quarto-markdown-pandoc.
 *
 * JSON filters are external processes that receive the Pandoc AST as JSON on stdin
 * and output the modified AST as JSON on stdout. This provides language-agnostic
 * extensibility.
 */

use crate::pandoc::Pandoc;
use crate::pandoc::ast_context::ASTContext;
use crate::readers;
use crate::writers;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Errors that can occur during JSON filter execution
#[derive(Debug)]
pub enum JsonFilterError {
    /// Failed to spawn the filter process
    SpawnFailed(std::path::PathBuf, std::io::Error),
    /// Filter exited with a non-zero status
    NonZeroExit(std::path::PathBuf, i32),
    /// Filter produced invalid UTF-8 output
    InvalidUtf8Output(std::path::PathBuf),
    /// Failed to parse JSON output from filter
    JsonParseError(std::path::PathBuf, String),
    /// Failed to serialize document to JSON
    SerializationError(String),
    /// Failed to write to filter stdin
    StdinWriteError(std::path::PathBuf, std::io::Error),
}

impl std::fmt::Display for JsonFilterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JsonFilterError::SpawnFailed(path, err) => {
                write!(f, "Failed to spawn filter '{}': {}", path.display(), err)
            }
            JsonFilterError::NonZeroExit(path, code) => {
                write!(f, "Filter '{}' exited with status {}", path.display(), code)
            }
            JsonFilterError::InvalidUtf8Output(path) => {
                write!(
                    f,
                    "Filter '{}' produced invalid UTF-8 output",
                    path.display()
                )
            }
            JsonFilterError::JsonParseError(path, err) => {
                write!(
                    f,
                    "Failed to parse JSON output from filter '{}': {}",
                    path.display(),
                    err
                )
            }
            JsonFilterError::SerializationError(err) => {
                write!(f, "Failed to serialize document to JSON: {}", err)
            }
            JsonFilterError::StdinWriteError(path, err) => {
                write!(
                    f,
                    "Failed to write to filter '{}' stdin: {}",
                    path.display(),
                    err
                )
            }
        }
    }
}

impl std::error::Error for JsonFilterError {}

/// Apply a JSON filter to a Pandoc document.
///
/// The filter receives the document as JSON on stdin and produces the modified
/// document as JSON on stdout. The filter is invoked as a subprocess with the
/// target format as the first argument.
///
/// # Arguments
///
/// * `pandoc` - The Pandoc document to filter
/// * `context` - The AST context (used for serialization)
/// * `filter_path` - Path to the filter executable
/// * `target_format` - Target format passed as first argument to the filter
///
/// # Returns
///
/// A tuple of the filtered Pandoc document and updated ASTContext.
pub fn apply_json_filter(
    pandoc: &Pandoc,
    context: &ASTContext,
    filter_path: &Path,
    target_format: &str,
) -> Result<(Pandoc, ASTContext), JsonFilterError> {
    // 1. Serialize document to JSON (including source locations - our format is a
    // superset of Pandoc's, so filters that don't understand source info will ignore it,
    // and filters that preserve it allow us to maintain source tracking through the pipeline)
    let mut json_buf = Vec::new();
    let json_config = writers::json::JsonConfig {
        include_inline_locations: true,
    };
    writers::json::write_with_config(pandoc, context, &mut json_buf, &json_config).map_err(
        |diags| {
            let msgs: Vec<String> = diags.iter().map(|d| d.title.clone()).collect();
            JsonFilterError::SerializationError(msgs.join("; "))
        },
    )?;

    // 2. Spawn the filter subprocess
    let mut child = Command::new(filter_path)
        .arg(target_format)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit()) // Pass through filter stderr to user
        .spawn()
        .map_err(|e| JsonFilterError::SpawnFailed(filter_path.to_owned(), e))?;

    // 3. Write JSON to filter's stdin
    {
        let stdin = child.stdin.as_mut().expect("Failed to get stdin handle");
        stdin
            .write_all(&json_buf)
            .map_err(|e| JsonFilterError::StdinWriteError(filter_path.to_owned(), e))?;
    }
    // stdin is dropped here, signaling EOF to the filter

    // 4. Wait for filter to complete and capture output
    let output = child
        .wait_with_output()
        .map_err(|e| JsonFilterError::SpawnFailed(filter_path.to_owned(), e))?;

    // 5. Check exit status
    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        return Err(JsonFilterError::NonZeroExit(filter_path.to_owned(), code));
    }

    // 6. Parse the output JSON
    let json_output = String::from_utf8(output.stdout)
        .map_err(|_| JsonFilterError::InvalidUtf8Output(filter_path.to_owned()))?;

    let (filtered_pandoc, filtered_context) = readers::json::read(&mut json_output.as_bytes())
        .map_err(|e| JsonFilterError::JsonParseError(filter_path.to_owned(), e.to_string()))?;

    Ok((filtered_pandoc, filtered_context))
}

/// Apply multiple JSON filters in sequence.
///
/// Filters are applied in the order they appear in the slice. The output of
/// each filter becomes the input to the next.
pub fn apply_json_filters(
    pandoc: Pandoc,
    context: ASTContext,
    filter_paths: &[std::path::PathBuf],
    target_format: &str,
) -> Result<(Pandoc, ASTContext), JsonFilterError> {
    let mut current_pandoc = pandoc;
    let mut current_context = context;

    for filter_path in filter_paths {
        let (new_pandoc, new_context) = apply_json_filter(
            &current_pandoc,
            &current_context,
            filter_path,
            target_format,
        )?;
        current_pandoc = new_pandoc;
        current_context = new_context;
    }

    Ok((current_pandoc, current_context))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn create_identity_filter(dir: &TempDir) -> std::path::PathBuf {
        let filter_path = dir.path().join("identity.py");
        fs::write(
            &filter_path,
            r#"#!/usr/bin/env python3
import sys
import json

doc = json.load(sys.stdin)
json.dump(doc, sys.stdout)
"#,
        )
        .unwrap();
        // Make executable
        let mut perms = fs::metadata(&filter_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&filter_path, perms).unwrap();
        filter_path
    }

    fn create_uppercase_filter(dir: &TempDir) -> std::path::PathBuf {
        let filter_path = dir.path().join("uppercase.py");
        fs::write(
            &filter_path,
            r#"#!/usr/bin/env python3
import sys
import json

def uppercase_strs(obj):
    if isinstance(obj, dict):
        if obj.get('t') == 'Str':
            obj['c'] = obj['c'].upper()
        else:
            for v in obj.values():
                uppercase_strs(v)
    elif isinstance(obj, list):
        for item in obj:
            uppercase_strs(item)
    return obj

doc = json.load(sys.stdin)
json.dump(uppercase_strs(doc), sys.stdout)
"#,
        )
        .unwrap();
        // Make executable
        let mut perms = fs::metadata(&filter_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&filter_path, perms).unwrap();
        filter_path
    }

    fn create_failing_filter(dir: &TempDir) -> std::path::PathBuf {
        let filter_path = dir.path().join("failing.py");
        fs::write(
            &filter_path,
            r#"#!/usr/bin/env python3
import sys
sys.exit(1)
"#,
        )
        .unwrap();
        // Make executable
        let mut perms = fs::metadata(&filter_path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&filter_path, perms).unwrap();
        filter_path
    }

    #[test]
    fn test_identity_filter() {
        let dir = TempDir::new().unwrap();
        let filter_path = create_identity_filter(&dir);

        // Create a simple document
        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![crate::pandoc::Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![crate::pandoc::Inline::Str(crate::pandoc::Str {
                    text: "Hello".to_string(),
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let (filtered, _) = apply_json_filter(&pandoc, &context, &filter_path, "html").unwrap();

        // The identity filter should preserve the document
        match &filtered.blocks[0] {
            crate::pandoc::Block::Paragraph(p) => match &p.content[0] {
                crate::pandoc::Inline::Str(s) => {
                    assert_eq!(s.text, "Hello");
                }
                _ => panic!("Expected Str inline"),
            },
            _ => panic!("Expected Paragraph block"),
        }
    }

    #[test]
    fn test_uppercase_filter() {
        let dir = TempDir::new().unwrap();
        let filter_path = create_uppercase_filter(&dir);

        // Create a simple document
        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![crate::pandoc::Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![crate::pandoc::Inline::Str(crate::pandoc::Str {
                    text: "hello world".to_string(),
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let (filtered, _) = apply_json_filter(&pandoc, &context, &filter_path, "html").unwrap();

        // The uppercase filter should convert text to uppercase
        match &filtered.blocks[0] {
            crate::pandoc::Block::Paragraph(p) => match &p.content[0] {
                crate::pandoc::Inline::Str(s) => {
                    assert_eq!(s.text, "HELLO WORLD");
                }
                _ => panic!("Expected Str inline"),
            },
            _ => panic!("Expected Paragraph block"),
        }
    }

    #[test]
    fn test_failing_filter() {
        let dir = TempDir::new().unwrap();
        let filter_path = create_failing_filter(&dir);

        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![],
        };
        let context = ASTContext::new();

        let result = apply_json_filter(&pandoc, &context, &filter_path, "html");

        assert!(result.is_err());
        match result.unwrap_err() {
            JsonFilterError::NonZeroExit(path, code) => {
                assert_eq!(path, filter_path);
                assert_eq!(code, 1);
            }
            err => panic!("Expected NonZeroExit error, got: {:?}", err),
        }
    }

    #[test]
    fn test_multiple_filters() {
        let dir = TempDir::new().unwrap();
        let identity = create_identity_filter(&dir);
        let uppercase = create_uppercase_filter(&dir);

        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![crate::pandoc::Block::Paragraph(crate::pandoc::Paragraph {
                content: vec![crate::pandoc::Inline::Str(crate::pandoc::Str {
                    text: "hello".to_string(),
                    source_info: quarto_source_map::SourceInfo::default(),
                })],
                source_info: quarto_source_map::SourceInfo::default(),
            })],
        };
        let context = ASTContext::new();

        let (filtered, _) =
            apply_json_filters(pandoc, context, &[identity, uppercase], "html").unwrap();

        match &filtered.blocks[0] {
            crate::pandoc::Block::Paragraph(p) => match &p.content[0] {
                crate::pandoc::Inline::Str(s) => {
                    assert_eq!(s.text, "HELLO");
                }
                _ => panic!("Expected Str inline"),
            },
            _ => panic!("Expected Paragraph block"),
        }
    }

    #[test]
    fn test_nonexistent_filter() {
        let pandoc = Pandoc {
            meta: crate::pandoc::MetaValueWithSourceInfo::MetaMap {
                entries: vec![],
                source_info: quarto_source_map::SourceInfo::default(),
            },
            blocks: vec![],
        };
        let context = ASTContext::new();

        let result = apply_json_filter(
            &pandoc,
            &context,
            Path::new("/nonexistent/filter.py"),
            "html",
        );

        assert!(result.is_err());
        match result.unwrap_err() {
            JsonFilterError::SpawnFailed(_, _) => {}
            err => panic!("Expected SpawnFailed error, got: {:?}", err),
        }
    }
}
