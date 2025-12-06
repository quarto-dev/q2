/*
 * output.rs
 * Copyright (c) 2025 Posit, PBC
 */

use std::io::{self, Write};

pub enum VerboseOutput {
    Stderr(io::Stderr),
    Sink(io::Sink),
}

impl Write for VerboseOutput {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            VerboseOutput::Stderr(stderr) => stderr.write(buf),
            VerboseOutput::Sink(sink) => sink.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            VerboseOutput::Stderr(stderr) => stderr.flush(),
            VerboseOutput::Sink(sink) => sink.flush(),
        }
    }
}
