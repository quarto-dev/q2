//! The wire types of a [`PandocRequest`] (schema: `schemas/pandoc-request.schema.json`).
//!
//! JSON cannot carry bytes. A byte field is a base64 string in the JSON
//! form (the schema annotates it `contentEncoding: base64`, and the golden
//! stores it that way); the object the wasm entry point hands to JS carries
//! `Uint8Array`s instead.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::constants::constants;

/// Bumped on any breaking change to the request shape; the host refuses a
/// version it does not know.
pub const REQUEST_SCHEMA_VERSION: u32 = 1;

/// What the request asks the host to run. Reserved so that PDF can later be
/// a second job fed by the first (`kind` other than `pandoc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RequestKind {
    #[default]
    Pandoc,
}

/// What runs after pandoc. `None` for docx, pptx, epub and typst source;
/// `CompileTypst` (the `pdf` request) means the host compiles `output_path`
/// (a `.typ`) to a PDF, after prepending [`super::typst_date_prelude`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RequestPost {
    #[default]
    None,
    CompileTypst,
}

/// A file the worker mounts at `path`, with its bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestFile {
    /// `/`-normalized absolute path.
    pub path: String,
    #[serde(with = "base64_bytes")]
    pub bytes: Vec<u8>,
}

/// Everything one pandoc run needs, as data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PandocRequest {
    pub schema_version: u32,
    #[serde(default)]
    pub kind: RequestKind,
    /// Computed by Rust ([`PandocRequest::compute_job_id`]); opaque to TS.
    pub job_id: String,
    /// The pandoc writer (`docx`, ...).
    pub writer: String,
    /// The whole argv, `argv[0]` (`pandoc`) included. No RTS options.
    pub argv: Vec<String>,
    /// The environment, fixed at init.
    pub env: BTreeMap<String, String>,
    /// Files created by this request (input JSON, the empty dependency
    /// file, staged templates). Not the share tree.
    pub files: Vec<RequestFile>,
    /// Directories that must exist on their own (empty ones).
    pub dirs: Vec<String>,
    /// Document resources (images, reference docs): paths and bytes.
    pub resource_refs: Vec<RequestFile>,
    /// The temp root; reserved in wasm mode.
    pub share_root: String,
    /// Where the share tree is mounted (`<share_root>/pandoc-share`, the
    /// value of `QUARTO_SHARE_PATH`).
    pub share_tree_path: String,
    pub doc_dir: String,
    pub project_root: String,
    pub output_path: String,
    /// Passed back to `classify_pandoc_completion`.
    pub stage_name: String,
    /// The input JSON path, passed back to `classify_pandoc_completion`.
    pub json_path: String,
    #[serde(default)]
    pub post: RequestPost,
    pub expected_pandoc_wasm_sha256: String,
    /// SHA-256 over the sorted `(path, bytes)` entries of the share tree.
    pub share_tree_version: String,
    /// Echo of `render_pandoc_request`'s input (`None` for a `.typ` download).
    pub typst_available_fonts: Option<Vec<String>>,
}

impl PandocRequest {
    /// First 16 hex digits of sha256 over the canonical JSON (keys sorted,
    /// no whitespace) of `{schema_version, tool, argv[1..], env minus
    /// SOURCE_DATE_EPOCH, files, resource_refs, share_tree}`, with `files`
    /// and `resource_refs` as `{path: sha256(bytes)}`. `SOURCE_DATE_EPOCH` is
    /// excluded so the id repeats across click times; `resource_refs` is
    /// included because an image's bytes change the output as much as an
    /// input file's.
    ///
    /// The id must not depend on where the render's scratch files happen to
    /// live, so `share_root` is replaced by the constant share root in every
    /// string that is hashed (argv, env values, the decoded params blob, file
    /// paths and text file contents). On wasm the root is already the
    /// constant, and the replacement is the identity.
    pub fn compute_job_id(&self) -> String {
        use serde_json::{Value, json};

        let placeholder = constants().share_root.as_str();
        let rooted = |s: &str| replace_root(s, &self.share_root, placeholder);

        let hashes = |list: &[RequestFile]| -> Value {
            let map: BTreeMap<String, String> = list
                .iter()
                .map(|f| {
                    let digest = match std::str::from_utf8(&f.bytes) {
                        Ok(text) => Sha256::digest(rooted(text).as_bytes()),
                        Err(_) => Sha256::digest(&f.bytes),
                    };
                    (rooted(&f.path), hex::encode(digest))
                })
                .collect();
            json!(map)
        };
        let env: BTreeMap<String, String> = self
            .env
            .iter()
            .filter(|(k, _)| k.as_str() != "SOURCE_DATE_EPOCH")
            .map(|(k, v)| {
                let value = if k == "QUARTO_FILTER_PARAMS" {
                    use base64::Engine as _;
                    base64::engine::general_purpose::STANDARD
                        .decode(v)
                        .ok()
                        .and_then(|b| String::from_utf8(b).ok())
                        .map_or_else(|| v.clone(), |text| rooted(&text))
                } else {
                    rooted(v)
                };
                (k.clone(), value)
            })
            .collect();
        let argv: Vec<String> = self
            .argv
            .get(1..)
            .unwrap_or_default()
            .iter()
            .map(|a| rooted(a))
            .collect();
        let mut value = json!({
            "schema_version": self.schema_version,
            "tool": self.expected_pandoc_wasm_sha256,
            "argv": argv,
            "env": env,
            "files": hashes(&self.files),
            "resource_refs": hashes(&self.resource_refs),
            "share_tree": self.share_tree_version,
        });
        // Only when set, so ids of requests without a post step are unchanged:
        // `pdf` and `typst` share argv but not what the host does next.
        if self.post != RequestPost::None {
            value["post"] = json!(self.post);
        }
        let mut canonical = String::new();
        write_canonical(&value, &mut canonical);
        hex::encode(Sha256::digest(canonical.as_bytes()))[..16].to_string()
    }
}

/// Replace `root` by `placeholder`, also in its backslash and JSON-escaped
/// backslash spellings (Windows paths inside JSON text).
fn replace_root(text: &str, root: &str, placeholder: &str) -> String {
    if root.is_empty() || root == placeholder {
        return text.to_string();
    }
    let backslash = root.replace('/', "\\");
    let escaped = root.replace('/', "\\\\");
    text.replace(&escaped, placeholder)
        .replace(&backslash, placeholder)
        .replace(root, placeholder)
}

/// Canonical JSON: object keys sorted, no whitespace.
fn write_canonical(value: &serde_json::Value, out: &mut String) {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(k.clone()).to_string());
                out.push(':');
                write_canonical(&map[k], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

mod base64_bytes {
    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .map_err(serde::de::Error::custom)
    }
}
