/*
 * mod.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::readers;
use crate::utils::output::VerboseOutput;
use crate::utils::tree_sitter_log_observer::TreeSitterLogObserver;
use std::io;

fn pandoc_to_json(doc: &crate::pandoc::Pandoc) -> Result<String, String> {
    let mut buf = Vec::new();
    match crate::writers::json::write(doc, &mut buf) {
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

pub fn qmd_to_pandoc(input: &[u8]) -> Result<crate::pandoc::Pandoc, Vec<String>> {
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
    pandoc_to_json(&qmd_to_pandoc(input).unwrap()).unwrap()
}
