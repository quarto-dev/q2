//! `quarto-output-extract`: compare two rendered outputs semantically.
//!
//! ```text
//! quarto-output-extract extract <file>        print the normalized extraction
//! quarto-output-extract compare <a> <b>       exit 0 if equal after normalization, 1 if not
//! quarto-output-extract fixtures              print the golden fixture manifest as JSON
//! ```
//!
//! Formats, chosen by extension: `.docx` and `.pptx` (semantic extraction: text,
//! styles, math, media names, no timestamps), `.epub` (UUIDs, timestamps and zip
//! metadata normalized; embedded bytes hashed; see `epub`), and `.typ` (compared
//! verbatim; typst source is deterministic). Consumed by the pandoc-wasm host
//! gates H0 and H3.

use std::path::Path;
use std::process::ExitCode;

fn extract(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let text = match ext {
        "docx" => quarto_output_extract::extract_docx(&bytes).map(|e| e.to_string()),
        "pptx" => quarto_output_extract::extract_pptx(&bytes).map(|e| e.to_string()),
        "epub" => quarto_output_extract::epub::extract_epub(&bytes).map(|e| e.to_string()),
        "typ" => return String::from_utf8(bytes).map_err(|e| format!("{}: {e}", path.display())),
        other => {
            return Err(format!(
                "{}: unsupported extension `{other}`",
                path.display()
            ));
        }
    };
    text.map_err(|e| format!("{}: {e}", path.display()))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["extract", file] => match extract(Path::new(file)) {
            Ok(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            Err(e) => fail(&e),
        },
        ["compare", a, b] => match (extract(Path::new(a)), extract(Path::new(b))) {
            (Ok(x), Ok(y)) if x == y => ExitCode::SUCCESS,
            (Ok(x), Ok(y)) => {
                let line = x.lines().zip(y.lines()).position(|(l, r)| l != r);
                let n = line.unwrap_or_else(|| x.lines().count().min(y.lines().count()));
                eprintln!("differ at line {}:", n + 1);
                eprintln!("  {a}: {}", x.lines().nth(n).unwrap_or("<end>"));
                eprintln!("  {b}: {}", y.lines().nth(n).unwrap_or("<end>"));
                ExitCode::from(1)
            }
            (Err(e), _) | (_, Err(e)) => fail(&e),
        },
        ["fixtures"] => {
            print!("{}", quarto_output_extract::fixtures_json());
            ExitCode::SUCCESS
        }
        _ => fail("usage: quarto-output-extract extract <file> | compare <a> <b> | fixtures"),
    }
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("{msg}");
    ExitCode::from(2)
}
