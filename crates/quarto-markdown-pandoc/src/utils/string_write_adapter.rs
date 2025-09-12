/*
 * string_write_adapter.rs
 * Copyright (c) 2025 Posit, PBC
 */

use std::io::{self, Write};

pub struct StringWriteAdapter<'a> {
    string: &'a mut String,
}

impl<'a> StringWriteAdapter<'a> {
    pub fn new(string: &'a mut String) -> Self {
        Self { string }
    }
}

impl<'a> Write for StringWriteAdapter<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Convert bytes to string, handling potential UTF-8 errors
        match std::str::from_utf8(buf) {
            Ok(s) => {
                self.string.push_str(s);
                Ok(buf.len())
            }
            Err(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid UTF-8 sequence",
            )),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        // Nothing to flush for a String
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_string_write_adapter() {
        let mut s = String::new();
        {
            let mut adapter = StringWriteAdapter::new(&mut s);

            // Test writing
            adapter.write_all(b"Hello, ").unwrap();
            adapter.write_all(b"World!").unwrap();
        }

        assert_eq!(s, "Hello, World!");
    }

    #[test]
    fn test_as_dyn_write() {
        let mut s = String::new();
        {
            let mut adapter = StringWriteAdapter::new(&mut s);
            let writer: &mut dyn Write = &mut adapter;

            writeln!(writer, "Line 1").unwrap();
            writeln!(writer, "Line 2").unwrap();
        }

        assert_eq!(s, "Line 1\nLine 2\n");
    }
}
