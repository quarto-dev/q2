/*
 * mod.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::readers;
use crate::utils::output::VerboseOutput;
use crate::utils::tree_sitter_log_observer::TreeSitterLogObserver;
use std::io;

fn pandoc_to_json(doc: &crate::pandoc::Pandoc, context: &crate::pandoc::ast_context::ASTContext) -> Result<String, String> {
    let mut buf = Vec::new();
    match crate::writers::json::write(doc, context, &mut buf) {
        Ok(_) => {
            // Nothing to do
        }
        Err(err) => {
            return Err(format!("Unable to write as json: {:?}", err));
        }
    }

    match String::from_utf8(buf) {
        Ok(json) => Ok(json),
        Err(err) => Err(format!("Unable to convert json to string: {:?}", err)),
    }
}

pub fn qmd_to_pandoc(input: &[u8]) -> Result<(crate::pandoc::Pandoc, crate::pandoc::ast_context::ASTContext), Vec<String>> {
    let mut output = VerboseOutput::Sink(io::sink());
    readers::qmd::read(
        input,
        false,
        "<input>",
        &mut output,
        None::<fn(&[u8], &TreeSitterLogObserver, &str) -> Vec<String>>,
    )
}

pub fn parse_qmd(input: &[u8]) -> String {
    let (pandoc, context) = qmd_to_pandoc(input).unwrap();
    pandoc_to_json(&pandoc, &context).unwrap()
}
