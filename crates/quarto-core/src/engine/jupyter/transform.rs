/*
 * engine/jupyter/transform.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * JupyterEngine AST transform implementation.
 */

//! AST transform implementation for Jupyter execution.
//!
//! This module implements the `AstTransform` trait for the Jupyter engine,
//! allowing code cells in the Pandoc AST to be executed via Jupyter kernels.

use std::path::PathBuf;
use std::sync::Arc;

use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::{Inline, Str};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;

use super::daemon::{JupyterDaemon, daemon};
use super::execute::CellOutput;
use super::kernelspec::is_jupyter_language;
use super::output::{OutputOptions, outputs_to_blocks};
use super::session::SessionKey;

/// Jupyter engine AST transform.
///
/// Transforms the AST by executing code blocks via Jupyter kernels
/// and replacing them with output blocks.
pub struct JupyterTransform {
    /// The daemon managing kernel sessions.
    daemon: Arc<JupyterDaemon>,
}

/// Information about an inline expression to evaluate.
#[derive(Debug, Clone)]
struct InlineExpr {
    /// Block index
    block_idx: usize,
    /// Inline index within the block's content
    inline_idx: usize,
    /// Language (e.g., "python")
    language: String,
    /// The expression code
    code: String,
}

impl JupyterTransform {
    /// Create a new Jupyter transform using the global daemon.
    pub fn new() -> Self {
        Self { daemon: daemon() }
    }

    /// Create a Jupyter transform with a specific daemon.
    pub fn with_daemon(daemon: Arc<JupyterDaemon>) -> Self {
        Self { daemon }
    }

    /// Extract executable code blocks from the AST.
    ///
    /// Returns a list of (index, language, code) tuples.
    fn extract_code_cells(ast: &Pandoc) -> Vec<(usize, String, String)> {
        let mut cells = Vec::new();

        for (idx, block) in ast.blocks.iter().enumerate() {
            if let Block::CodeBlock(cb) = block {
                // Check if this is an executable code block
                // by looking at the classes (second element of attr tuple)
                let (_, classes, _) = &cb.attr;

                for class in classes {
                    // Handle both plain language and Quarto syntax {python}
                    let lang = class.trim_matches(|c| c == '{' || c == '}');
                    if is_jupyter_language(lang) {
                        cells.push((idx, lang.to_lowercase(), cb.text.clone()));
                        break; // Only take first matching language
                    }
                }
            }
        }

        cells
    }

    /// Determine the kernel to use from the code cells.
    fn determine_kernel(cells: &[(usize, String, String)]) -> Option<String> {
        // For now, use the first language found and map to kernel name
        cells
            .first()
            .map(|(_, lang, _)| Self::language_to_kernel(lang))
    }

    /// Map a language name to a kernel name.
    fn language_to_kernel(lang: &str) -> String {
        match lang {
            "python" => "python3".to_string(),
            "julia" => "julia-1.9".to_string(), // Common default
            "r" => "ir".to_string(),
            other => other.to_string(),
        }
    }

    /// Execute all code cells and collect outputs.
    async fn execute_cells(
        &self,
        key: &SessionKey,
        cells: &[(usize, String, String)],
    ) -> Result<Vec<(usize, Vec<Block>)>> {
        let mut results = Vec::new();

        for (idx, _lang, code) in cells {
            // Execute the code
            let exec_result = self
                .daemon
                .execute_in_session(key, code)
                .await
                .ok_or_else(|| crate::error::QuartoError::other("Kernel session not found"))?
                .map_err(|e| crate::error::QuartoError::other(format!("Execution error: {}", e)))?;

            // Convert outputs to AST blocks
            let options = OutputOptions::default();
            let output_blocks = outputs_to_blocks(&exec_result.outputs, &options);

            results.push((*idx, output_blocks));
        }

        Ok(results)
    }

    /// Replace code blocks with their outputs in the AST.
    fn replace_with_outputs(ast: &mut Pandoc, outputs: Vec<(usize, Vec<Block>)>) {
        // Process in reverse order to maintain correct indices
        let mut sorted_outputs = outputs;
        sorted_outputs.sort_by(|a, b| b.0.cmp(&a.0));

        for (idx, output_blocks) in sorted_outputs {
            if idx < ast.blocks.len() {
                // Remove the code block
                ast.blocks.remove(idx);

                // Insert output blocks at the same position
                for (i, block) in output_blocks.into_iter().enumerate() {
                    ast.blocks.insert(idx + i, block);
                }
            }
        }
    }

    /// Extract inline expressions from the AST.
    ///
    /// Looks for Code inlines like `{python} 1+1` or `{r} x` in Paragraph blocks.
    fn extract_inline_expressions(ast: &Pandoc) -> Vec<InlineExpr> {
        let mut exprs = Vec::new();

        for (block_idx, block) in ast.blocks.iter().enumerate() {
            // Only look in paragraphs for now
            if let Block::Paragraph(para) = block {
                for (inline_idx, inline) in para.content.iter().enumerate() {
                    if let Inline::Code(code) = inline {
                        // Check if it starts with {language}
                        if let Some(expr) = Self::parse_inline_expression(&code.text) {
                            if is_jupyter_language(&expr.0) {
                                exprs.push(InlineExpr {
                                    block_idx,
                                    inline_idx,
                                    language: expr.0,
                                    code: expr.1,
                                });
                            }
                        }
                    }
                }
            }
        }

        exprs
    }

    /// Parse an inline expression like `{python} 1+1` or `python 1+1`.
    ///
    /// Returns (language, expression) if successful.
    fn parse_inline_expression(text: &str) -> Option<(String, String)> {
        let text = text.trim();

        // Try {language} syntax first
        if text.starts_with('{') {
            if let Some(close_brace) = text.find('}') {
                let lang = text[1..close_brace].trim().to_lowercase();
                let expr = text[close_brace + 1..].trim().to_string();
                if !lang.is_empty() && !expr.is_empty() {
                    return Some((lang, expr));
                }
            }
        }

        // Try plain `python expr` syntax (common in Quarto)
        let parts: Vec<&str> = text.splitn(2, char::is_whitespace).collect();
        if parts.len() == 2 {
            let lang = parts[0].trim().to_lowercase();
            let expr = parts[1].trim().to_string();
            if !lang.is_empty() && !expr.is_empty() {
                return Some((lang, expr));
            }
        }

        None
    }

    /// Execute inline expressions and return their results.
    async fn execute_inline_expressions(
        &self,
        key: &SessionKey,
        exprs: &[InlineExpr],
    ) -> Result<Vec<String>> {
        let mut results = Vec::new();

        for expr in exprs {
            // Execute the expression silently and capture the result
            let exec_result = self
                .daemon
                .execute_in_session(key, &expr.code)
                .await
                .ok_or_else(|| crate::error::QuartoError::other("Kernel session not found"))?
                .map_err(|e| {
                    crate::error::QuartoError::other(format!("Inline execution error: {}", e))
                })?;

            // Extract text/plain from the result
            let result_text = exec_result
                .outputs
                .iter()
                .find_map(|output| match output {
                    CellOutput::ExecuteResult { data, .. } => data
                        .get("text/plain")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim_matches('\'').trim_matches('"').to_string()),
                    CellOutput::Stream { name, text } if name == "stdout" => {
                        Some(text.trim().to_string())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| "".to_string());

            results.push(result_text);
        }

        Ok(results)
    }

    /// Replace inline expressions with their results in the AST.
    fn replace_inline_expressions(ast: &mut Pandoc, exprs: &[InlineExpr], results: &[String]) {
        // Group by block index and process in reverse order
        let mut replacements: Vec<(usize, usize, String)> = exprs
            .iter()
            .zip(results.iter())
            .map(|(expr, result)| (expr.block_idx, expr.inline_idx, result.clone()))
            .collect();

        // Sort by block_idx desc, then inline_idx desc to process in reverse order
        replacements.sort_by(|a, b| {
            if a.0 != b.0 {
                b.0.cmp(&a.0)
            } else {
                b.1.cmp(&a.1)
            }
        });

        for (block_idx, inline_idx, result) in replacements {
            if let Some(Block::Paragraph(para)) = ast.blocks.get_mut(block_idx) {
                if inline_idx < para.content.len() {
                    // Replace the Code inline with a Str inline
                    para.content[inline_idx] = Inline::Str(Str {
                        text: result,
                        source_info: SourceInfo::default(),
                    });
                }
            }
        }
    }
}

impl Default for JupyterTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl AstTransform for JupyterTransform {
    fn name(&self) -> &str {
        "jupyter"
    }

    fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Check if execution is enabled
        if !ctx.options.execute {
            tracing::debug!("Jupyter execution disabled, skipping");
            return Ok(());
        }

        // Extract code cells and inline expressions
        let cells = Self::extract_code_cells(ast);
        let inline_exprs = Self::extract_inline_expressions(ast);

        if cells.is_empty() && inline_exprs.is_empty() {
            tracing::debug!("No executable Jupyter code cells or inline expressions found");
            return Ok(());
        }

        tracing::info!(
            cell_count = cells.len(),
            inline_count = inline_exprs.len(),
            "Found Jupyter code cells and inline expressions"
        );

        // Determine kernel from cells or inline expressions
        let kernel_name = Self::determine_kernel(&cells)
            .or_else(|| {
                inline_exprs
                    .first()
                    .map(|e| Self::language_to_kernel(&e.language))
            })
            .ok_or_else(|| {
                crate::error::QuartoError::other("Could not determine Jupyter kernel")
            })?;

        // Get working directory from document
        let working_dir = ctx
            .document
            .input
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.project.dir.clone());

        // Create session key
        let key = SessionKey::new(&kernel_name, working_dir.clone());

        // Run async execution
        let daemon = self.daemon.clone();
        let cells_clone = cells.clone();
        let inline_exprs_clone = inline_exprs.clone();

        let (outputs, inline_results) = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Start or get kernel session
                daemon
                    .get_or_start_session(&kernel_name, &working_dir)
                    .await
                    .map_err(|e| {
                        crate::error::QuartoError::other(format!("Failed to start kernel: {}", e))
                    })?;

                // Execute all code cells
                let cell_outputs = if !cells_clone.is_empty() {
                    self.execute_cells(&key, &cells_clone).await?
                } else {
                    Vec::new()
                };

                // Execute inline expressions
                let inline_results = if !inline_exprs_clone.is_empty() {
                    self.execute_inline_expressions(&key, &inline_exprs_clone)
                        .await?
                } else {
                    Vec::new()
                };

                Ok::<_, crate::error::QuartoError>((cell_outputs, inline_results))
            })
        })?;

        // Replace code blocks with outputs
        if !outputs.is_empty() {
            Self::replace_with_outputs(ast, outputs);
        }

        // Replace inline expressions with results
        if !inline_results.is_empty() {
            Self::replace_inline_expressions(ast, &inline_exprs, &inline_results);
        }

        tracing::info!("Jupyter execution complete");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigValue;
    use quarto_pandoc_types::attr::AttrSourceInfo;
    use quarto_pandoc_types::block::{CodeBlock, Paragraph};
    use quarto_pandoc_types::inline::{Inline, Str};
    use quarto_source_map::SourceInfo;

    fn make_code_block(lang: &str, code: &str) -> Block {
        Block::CodeBlock(CodeBlock {
            attr: (
                String::new(),
                vec![format!("{{{}}}", lang)],
                Default::default(),
            ),
            text: code.to_string(),
            source_info: SourceInfo::default(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn make_paragraph(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![Inline::Str(Str {
                text: text.to_string(),
                source_info: SourceInfo::default(),
            })],
            source_info: SourceInfo::default(),
        })
    }

    #[test]
    fn test_extract_code_cells_empty() {
        let ast = Pandoc {
            meta: ConfigValue::new_map(vec![], SourceInfo::default()),
            blocks: vec![make_paragraph("Hello")],
        };

        let cells = JupyterTransform::extract_code_cells(&ast);
        assert!(cells.is_empty());
    }

    #[test]
    fn test_extract_code_cells_python() {
        let ast = Pandoc {
            meta: ConfigValue::new_map(vec![], SourceInfo::default()),
            blocks: vec![
                make_paragraph("Intro"),
                make_code_block("python", "print('hello')"),
                make_paragraph("Middle"),
                make_code_block("python", "x = 1 + 1"),
            ],
        };

        let cells = JupyterTransform::extract_code_cells(&ast);
        assert_eq!(cells.len(), 2);
        assert_eq!(
            cells[0],
            (1, "python".to_string(), "print('hello')".to_string())
        );
        assert_eq!(cells[1], (3, "python".to_string(), "x = 1 + 1".to_string()));
    }

    #[test]
    fn test_extract_code_cells_non_jupyter() {
        let ast = Pandoc {
            meta: ConfigValue::new_map(vec![], SourceInfo::default()),
            blocks: vec![
                make_code_block("rust", "fn main() {}"),
                make_code_block("javascript", "console.log('hi')"),
            ],
        };

        let cells = JupyterTransform::extract_code_cells(&ast);
        assert!(cells.is_empty());
    }

    #[test]
    fn test_determine_kernel_python() {
        let cells = vec![(0, "python".to_string(), "code".to_string())];
        assert_eq!(
            JupyterTransform::determine_kernel(&cells),
            Some("python3".to_string())
        );
    }

    #[test]
    fn test_determine_kernel_julia() {
        let cells = vec![(0, "julia".to_string(), "code".to_string())];
        assert_eq!(
            JupyterTransform::determine_kernel(&cells),
            Some("julia-1.9".to_string())
        );
    }

    #[test]
    fn test_transform_name() {
        let transform = JupyterTransform::new();
        assert_eq!(transform.name(), "jupyter");
    }
}
