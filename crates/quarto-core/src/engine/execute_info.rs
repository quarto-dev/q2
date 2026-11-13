/*
 * engine/execute_info.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The execute environment Quarto 1 gives code cells (bd-8fmuyh4d, GH #1131).
 */

//! The execution environment a document's code cells see.
//!
//! Quarto 1 (`src/execute/environment.ts`, `src/resources/jupyter/notebook.py`)
//! gives every code cell these environment variables:
//!
//! - `QUARTO_PROJECT_ROOT`, `QUARTO_DOCUMENT_PATH`, `QUARTO_DOCUMENT_FILE`;
//! - `QUARTO_EXECUTE_INFO`, the path of a JSON file holding `document-path`
//!   and the resolved six-bin `format` (`docs/advanced/quarto-execute-info.qmd`
//!   in quarto-web documents it as API);
//! - `QUARTO_FIG_WIDTH`, `QUARTO_FIG_HEIGHT`, `QUARTO_FIG_DPI`,
//!   `QUARTO_FIG_FORMAT`.
//!
//! Quarto 1 sets them with `Deno.env.set` on its own process. **q2 never
//! mutates its process environment** (see `project/environment.rs`): this
//! module only computes the values and the file contents, and each engine
//! delivers them to the code it runs (knitr with `Command::envs`, Jupyter with
//! a per-document setup request inside the kernel, [`jupyter_setup_code`]).
//!
//! The `format` object is built by [`partition_format`], a port of Quarto 1's
//! `metadataAsFormat` (`src/config/metadata.ts`), the same partition the
//! Node engine host runs (`ts-packages/quarto-engine-host-node/src/
//! metadata-as-format.ts`). Its key lists are tested against
//! `ts-packages/quarto-api/src/config/index.ts` so the two cannot drift.

use std::path::Path;

use serde_json::{Map, Value};

use crate::engine::knitr::format::ExecuteConfig;

/// Environment variable naming the execute-info JSON file.
pub const QUARTO_EXECUTE_INFO: &str = "QUARTO_EXECUTE_INFO";

/// File name of the execute-info JSON inside the document's temp dir.
pub const EXECUTE_INFO_FILE_NAME: &str = "quarto-execute-info.json";

/// Q1 `kExecuteDefaultsKeys` (`src/config/constants.ts`).
pub(crate) const EXECUTE_KEYS: &[&str] = &[
    "fig-width",
    "fig-height",
    "fig-format",
    "fig-dpi",
    "fig-asp",
    "mermaid-format",
    "df-print",
    "error",
    "eval",
    "engine",
    "cache",
    "freeze",
    "echo",
    "output",
    "warning",
    "include",
    "keep-md",
    "keep-ipynb",
    "enabled",
    "ipynb",
    "daemon",
    "daemon-restart",
    "debug",
    "ipynb-filter",
    "ipynb-filters",
    "ipynb-shell-interactivity",
    "plotly-connected",
];

/// Q1 `kRenderDefaultsKeys`.
pub(crate) const RENDER_KEYS: &[&str] = &[
    "keep-tex",
    "keep-typ",
    "keep-source",
    "keep-hidden",
    "remove-hidden",
    "clear-hidden-classes",
    "variant",
    "output-ext",
    "output-divs",
    "prefer-html",
    "page-width",
    "fig-align",
    "fig-pos",
    "fig-env",
    "code-fold",
    "code-link",
    "code-line-numbers",
    "code-overflow",
    "code-tools",
    "shortcodes",
    "tbl-colwidths",
    "inline-includes",
    "preserve-yaml",
    "merge-includes",
    "self-contained-math",
    "latex-auto-mk",
    "latex-auto-install",
    "latex-min-runs",
    "latex-max-runs",
    "latex-clean",
    "latex-input-paths",
    "latex-makeindex",
    "latex-makeindex-opts",
    "latex-tlmgr-opts",
    "latex-output-dir",
    "latex-tinytex",
    "pdf-standard",
    "link-external-icon",
    "link-external-newwindow",
    "link-external-filter",
    "format-resources",
    "format-links",
    "produce-source-notebook",
    "notebook-links",
    "notebook-subarticles",
    "notebook-view",
    "notebook-view-style",
    "notebook-preserve-cells",
    "clear-cell-options",
    "html-table-processing",
    "validate-yaml",
    "canonical-url",
    "body-classes",
];

/// Q1 `kPandocDefaultsKeys`.
pub(crate) const PANDOC_KEYS: &[&str] = &[
    "to",
    "from",
    "reader",
    "writer",
    "output-file",
    "input-files",
    "defaults",
    "template",
    "standalone",
    "self-contained",
    "embed-resources",
    "variables",
    "metadata",
    "metadata-files",
    "metadata-file",
    "include-before-body",
    "include-after-body",
    "include-in-header",
    "resource-path",
    "citeproc",
    "cite-method",
    "citation-abbreviations",
    "filters",
    "file-scope",
    "data-dir",
    "verbosity",
    "log-file",
    "top-level-division",
    "abbreviations",
    "pdf-engine",
    "pdf-engine-opts",
    "pdf-engine-opt",
    "pdf-standard",
    "wrap",
    "columns",
    "dpi",
    "extract-media",
    "toc",
    "table-of-contents",
    "toc-depth",
    "number-sections",
    "number-offset",
    "shift-heading-level-by",
    "section-divs",
    "identifier-prefix",
    "title-prefix",
    "eol",
    "strip-comments",
    "indented-code-classes",
    "ascii",
    "default-image-extension",
    "highlight-style",
    "syntax-highlighting",
    "syntax-definitions",
    "syntax-definition",
    "listings",
    "reference-doc",
    "html-math-method",
    "email-obfuscation",
    "tab-stop",
    "preserve-tabs",
    "incremental",
    "slide-level",
    "epub-subdirectory",
    "epub-metadata",
    "epub-fonts",
    "epub-chapter-level",
    "epub-cover-image",
    "reference-links",
    "reference-location",
    "markdown-headings",
    "track-changes",
    "html-q-tags",
    "css",
    "ipynb-output",
    "request-headers",
    "fail-if-warnings",
    "dump-args",
    "ignore-args",
    "trace",
];

/// Q1 `kIdentifierDefaultsKeys`.
pub(crate) const IDENTIFIER_KEYS: &[&str] = &["target-format", "display-name", "extension-name"];

const BIN_IDENTIFIER: &str = "identifier";
const BIN_RENDER: &str = "render";
const BIN_EXECUTE: &str = "execute";
const BIN_PANDOC: &str = "pandoc";
const BIN_LANGUAGE: &str = "language";
const BIN_METADATA: &str = "metadata";
const BINS: [&str; 6] = [
    BIN_IDENTIFIER,
    BIN_RENDER,
    BIN_EXECUTE,
    BIN_PANDOC,
    BIN_LANGUAGE,
    BIN_METADATA,
];

/// Q1's base execute-visibility defaults (`format/formats-shared.ts`),
/// filled in for keys the document did not set (as the Node host's
/// `applyExecuteDefaults` does).
const EXECUTE_VISIBILITY_DEFAULTS: [(&str, bool); 6] = [
    ("eval", true),
    ("echo", true),
    ("output", true),
    ("warning", true),
    ("include", true),
    ("error", false),
];

/// The identity of the format being rendered, as Q1's `format.identifier`.
#[derive(Debug, Clone)]
pub struct FormatIdentity {
    /// The format as the user wrote it (`html`, `acm-html`, `dashboard`).
    pub target_format: String,
    /// The base format (`html`, `pdf`, `dashboard`).
    pub base_format: String,
    /// Human-readable name (`HTML`).
    pub display_name: String,
    /// The extension providing the format, if any.
    pub extension_name: Option<String>,
}

/// The resolved brand, one entry per mode, already in Q1's JSON shape.
#[derive(Debug, Clone)]
pub struct BrandModesJson {
    pub light: Value,
    /// Present only when the brand defines dark values.
    pub dark: Option<Value>,
}

/// Partition merged, lowered document metadata into Q1's six-bin `Format`.
///
/// Port of `metadataAsFormat` (stage 1 peels nested bins, stage 2 classifies
/// flat keys, then the `server`, `ipynb-filter` and `variant` normalizations).
/// Two q2 additions:
///
/// - the `language` bin also receives the resolved term table q2 keeps at
///   `quarto.language` (Q1 resolves it into `format.language` during format
///   resolution); the table is removed from the `metadata` bin so it is not
///   written twice;
/// - nothing else: identifier, execute defaults and brand are layered on by
///   [`build_execute_info`].
pub fn partition_format(metadata: &Map<String, Value>) -> Map<String, Value> {
    let mut bins: Map<String, Value> = BINS
        .iter()
        .map(|b| (b.to_string(), Value::Object(Map::new())))
        .collect();

    fn bin_mut<'a>(bins: &'a mut Map<String, Value>, name: &str) -> &'a mut Map<String, Value> {
        match bins.get_mut(name) {
            Some(Value::Object(m)) => m,
            _ => unreachable!("bins are initialized as objects"),
        }
    }

    let mut remaining: Vec<(&String, &Value)> = Vec::new();
    for (key, value) in metadata {
        if BINS.contains(&key.as_str()) {
            match value {
                Value::Bool(enabled) if key == BIN_EXECUTE => {
                    bin_mut(&mut bins, BIN_EXECUTE).insert("enabled".into(), Value::Bool(*enabled));
                }
                Value::Object(entries) => {
                    let target = bin_mut(&mut bins, key);
                    for (k, v) in entries {
                        target.insert(k.clone(), v.clone());
                    }
                }
                // Q1 drops any other scalar under a bin name.
                _ => {}
            }
        } else {
            remaining.push((key, value));
        }
    }

    for (key, value) in remaining {
        let target = if IDENTIFIER_KEYS.contains(&key.as_str()) {
            BIN_IDENTIFIER
        } else if RENDER_KEYS.contains(&key.as_str()) {
            BIN_RENDER
        } else if EXECUTE_KEYS.contains(&key.as_str()) {
            BIN_EXECUTE
        } else if PANDOC_KEYS.contains(&key.as_str()) {
            BIN_PANDOC
        } else {
            BIN_METADATA
        };
        bin_mut(&mut bins, target).insert(key.clone(), value.clone());
    }

    // q2: the resolved language table lives at `quarto.language`.
    let resolved_language = {
        let meta = bin_mut(&mut bins, BIN_METADATA);
        let table = match meta.get_mut("quarto") {
            Some(Value::Object(quarto)) => quarto.remove("language"),
            _ => None,
        };
        if matches!(meta.get("quarto"), Some(Value::Object(q)) if q.is_empty()) {
            meta.remove("quarto");
        }
        table
    };
    if let Some(Value::Object(terms)) = resolved_language {
        let language = bin_mut(&mut bins, BIN_LANGUAGE);
        for (k, v) in terms {
            language.insert(k, v);
        }
    }

    // Tail normalization 1: `server: <string>` → `{type: <string>}`.
    {
        let meta = bin_mut(&mut bins, BIN_METADATA);
        if let Some(Value::String(kind)) = meta.get("server").cloned() {
            meta.insert("server".into(), serde_json::json!({ "type": kind }));
        }
    }

    // Tail normalization 2: coalesce `ipynb-filter` into `ipynb-filters`.
    {
        let execute = bin_mut(&mut bins, BIN_EXECUTE);
        if let Some(Value::String(filter)) = execute.get("ipynb-filter").cloned() {
            let mut filters = match execute.get("ipynb-filters") {
                Some(Value::Array(existing)) => existing.clone(),
                _ => Vec::new(),
            };
            filters.push(Value::String(filter));
            execute.insert("ipynb-filters".into(), Value::Array(filters));
            execute.remove("ipynb-filter");
        }
    }

    // Tail normalization 3: expand a leading `gfm` in `variant`.
    {
        let render = bin_mut(&mut bins, BIN_RENDER);
        if let Some(Value::String(variant)) = render.get("variant").cloned()
            && let Some(rest) = variant.strip_prefix("gfm")
        {
            render.insert(
                "variant".into(),
                Value::String(format!("{GFM_COMMONMARK_VARIANT}{rest}")),
            );
        }
    }

    bins
}

/// Q1 `kGfmCommonmarkExtensions.join("")` (`format-markdown-consts.ts`).
const GFM_COMMONMARK_VARIANT: &str = "+autolink_bare_uris+emoji+footnotes+gfm_auto_identifiers+pipe_tables+strikeout+task_lists+tex_math_dollars";

/// Everything [`build_execute_info`] needs.
#[derive(Debug, Clone)]
pub struct ExecuteInfoInput<'a> {
    /// Absolute path of the source document.
    pub document_path: &'a Path,
    /// The merged document metadata, lowered to JSON.
    pub metadata: &'a Map<String, Value>,
    pub identity: &'a FormatIdentity,
    /// The pandoc writer the format uses (`html` for a dashboard).
    pub writer: &'a str,
    /// Execute options with the engines' defaults applied (fig sizes).
    pub execute: &'a ExecuteConfig,
    pub brand: Option<&'a BrandModesJson>,
}

/// The JSON object `QUARTO_EXECUTE_INFO` points to:
/// `{"document-path": ..., "format": {six bins}}`.
pub fn build_execute_info(input: &ExecuteInfoInput<'_>) -> Value {
    let mut format = partition_format(input.metadata);

    if let Some(Value::Object(identifier)) = format.get_mut(BIN_IDENTIFIER) {
        identifier.insert(
            "target-format".into(),
            Value::String(input.identity.target_format.clone()),
        );
        identifier.insert(
            "base-format".into(),
            Value::String(input.identity.base_format.clone()),
        );
        identifier.insert(
            "display-name".into(),
            Value::String(input.identity.display_name.clone()),
        );
        if let Some(extension) = &input.identity.extension_name {
            identifier.insert("extension-name".into(), Value::String(extension.clone()));
        }
    }

    if let Some(Value::Object(execute)) = format.get_mut(BIN_EXECUTE) {
        for (key, value) in EXECUTE_VISIBILITY_DEFAULTS {
            execute.entry(key).or_insert(Value::Bool(value));
        }
        let fig = FigOptions::from_execute(input.execute);
        execute
            .entry("fig-width")
            .or_insert_with(|| number(fig.width));
        execute
            .entry("fig-height")
            .or_insert_with(|| number(fig.height));
        execute
            .entry("fig-dpi")
            .or_insert_with(|| Value::from(fig.dpi));
        execute
            .entry("fig-format")
            .or_insert_with(|| Value::String(fig.format.clone()));
    }

    if let Some(Value::Object(pandoc)) = format.get_mut(BIN_PANDOC) {
        pandoc
            .entry("to")
            .or_insert_with(|| Value::String(input.writer.to_string()));
    }

    if let (Some(brand), Some(Value::Object(render))) = (input.brand, format.get_mut(BIN_RENDER)) {
        let mut modes = Map::new();
        modes.insert("light".into(), brand.light.clone());
        if let Some(dark) = &brand.dark {
            modes.insert("dark".into(), dark.clone());
        }
        render.insert("brand".into(), Value::Object(modes));
    }

    serde_json::json!({
        "document-path": input.document_path.to_string_lossy(),
        "format": Value::Object(format),
    })
}

/// A finite `f64` as a JSON number, integral values without a fraction
/// (`7`, not `7.0`), as Q1's JSON has them.
fn number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        Value::from(value as i64)
    } else {
        serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
    }
}

/// The figure options the `QUARTO_FIG_*` variables report.
#[derive(Debug, Clone, PartialEq)]
pub struct FigOptions {
    pub width: f64,
    pub height: f64,
    pub dpi: u32,
    pub format: String,
}

impl FigOptions {
    /// Read the figure options from execute options, falling back to the
    /// engines' own defaults ([`ExecuteConfig::with_defaults`]).
    pub fn from_execute(execute: &ExecuteConfig) -> Self {
        let defaults = ExecuteConfig::with_defaults();
        Self {
            width: execute.fig_width.or(defaults.fig_width).unwrap_or(7.0),
            height: execute.fig_height.or(defaults.fig_height).unwrap_or(5.0),
            dpi: execute.fig_dpi.or(defaults.fig_dpi).unwrap_or(96),
            format: execute
                .fig_format
                .clone()
                .or(defaults.fig_format)
                .unwrap_or_else(|| "png".to_string()),
        }
    }

    /// `QUARTO_FIG_*`, as Q1's `set_env_vars` (`notebook.py`) computes them:
    /// a `retina` format reports `png` at twice the dpi.
    pub fn env(&self) -> Vec<(String, String)> {
        let (dpi, format) = if self.format == "retina" {
            (self.dpi * 2, "png".to_string())
        } else {
            (self.dpi, self.format.clone())
        };
        vec![
            ("QUARTO_FIG_WIDTH".into(), number_text(self.width)),
            ("QUARTO_FIG_HEIGHT".into(), number_text(self.height)),
            ("QUARTO_FIG_DPI".into(), dpi.to_string()),
            ("QUARTO_FIG_FORMAT".into(), format),
        ]
    }
}

fn number_text(value: f64) -> String {
    match number(value) {
        Value::Number(n) => n.to_string(),
        _ => value.to_string(),
    }
}

/// `QUARTO_PROJECT_ROOT`, `QUARTO_DOCUMENT_PATH`, `QUARTO_DOCUMENT_FILE` and
/// `QUARTO_EXECUTE_INFO`.
///
/// `project_root` is the project directory, or the document's directory for a
/// lone file (Q1 uses its execute cwd there, which defaults to the same
/// directory).
pub fn path_env(
    project_root: &Path,
    document_path: &Path,
    execute_info_file: &Path,
) -> Vec<(String, String)> {
    let document_dir = document_path.parent().unwrap_or(Path::new(""));
    let document_file = document_path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    vec![
        (
            "QUARTO_PROJECT_ROOT".into(),
            project_root.to_string_lossy().into_owned(),
        ),
        (
            "QUARTO_DOCUMENT_PATH".into(),
            document_dir.to_string_lossy().into_owned(),
        ),
        ("QUARTO_DOCUMENT_FILE".into(), document_file),
        (
            QUARTO_EXECUTE_INFO.into(),
            execute_info_file.to_string_lossy().into_owned(),
        ),
    ]
}

/// Code that sets `vars` in a running kernel's environment, for the kernel
/// languages q2 knows how to address, or `None`.
///
/// Each document has its own kernel (bd-i7yz08uh), but `q2 preview` keeps a
/// document's kernel across renders, so these values cannot be given only
/// when the kernel starts. The engine runs this code as a
/// silent request before the document's first cell, the way Quarto 1's setup
/// cell sets `QUARTO_FIG_*` inside the kernel. It changes only the kernel's
/// environment, never q2's.
pub fn jupyter_setup_code(language: &str, vars: &[(String, String)]) -> Option<String> {
    let language = language.to_ascii_lowercase();
    match language.as_str() {
        "python" => {
            let entries: Vec<String> = vars
                .iter()
                .map(|(k, v)| format!("{}: {}", json_string(k), json_string(v)))
                .collect();
            Some(format!(
                "import os as __quarto_os\n__quarto_os.environ.update({{{}}})\ndel __quarto_os\n",
                entries.join(", ")
            ))
        }
        "r" => {
            let entries: Vec<String> = vars
                .iter()
                .map(|(k, v)| format!("{} = {}", json_string(k), json_string(v)))
                .collect();
            Some(format!("invisible(Sys.setenv({}))\n", entries.join(", ")))
        }
        "julia" => {
            let lines: Vec<String> = vars
                .iter()
                .map(|(k, v)| format!("ENV[{}] = {}", julia_string(k), julia_string(v)))
                .collect();
            Some(format!("{}\nnothing\n", lines.join("\n")))
        }
        _ => None,
    }
}

/// A JSON string literal, which is also a valid Python and R string literal
/// (both accept JSON's escapes).
fn json_string(s: &str) -> String {
    Value::String(s.to_string()).to_string()
}

/// A Julia string literal: JSON escaping plus `$`, which Julia interpolates.
fn julia_string(s: &str) -> String {
    json_string(s).replace('$', "\\$")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn map(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(m) => m,
            _ => panic!("not an object"),
        }
    }

    #[test]
    fn flat_keys_are_classified_into_their_bins() {
        let bins = partition_format(&map(json!({
            "fig-width": 8,
            "keep-tex": true,
            "toc": true,
            "display-name": "Custom",
            "title": "Hello",
        })));
        assert_eq!(bins["execute"]["fig-width"], json!(8));
        assert_eq!(bins["render"]["keep-tex"], json!(true));
        assert_eq!(bins["pandoc"]["toc"], json!(true));
        assert_eq!(bins["identifier"]["display-name"], json!("Custom"));
        assert_eq!(bins["metadata"]["title"], json!("Hello"));
        assert_eq!(bins["language"], json!({}));
    }

    #[test]
    fn nested_bins_are_peeled_and_execute_false_disables() {
        let bins = partition_format(&map(json!({
            "execute": false,
            "render": {"code-fold": true},
            "metadata": {"x": 1},
        })));
        assert_eq!(bins["execute"]["enabled"], json!(false));
        assert_eq!(bins["render"]["code-fold"], json!(true));
        assert_eq!(bins["metadata"]["x"], json!(1));
    }

    #[test]
    fn nested_execute_map_merges_into_the_execute_bin() {
        let bins = partition_format(&map(json!({"execute": {"echo": false, "fig-width": 4}})));
        assert_eq!(bins["execute"], json!({"echo": false, "fig-width": 4}));
    }

    #[test]
    fn resolved_language_table_moves_to_the_language_bin() {
        let bins = partition_format(&map(json!({
            "quarto": {"language": {"callout-tip-title": "Tip"}, "other": 1},
        })));
        assert_eq!(bins["language"]["callout-tip-title"], json!("Tip"));
        assert_eq!(bins["metadata"]["quarto"], json!({"other": 1}));
    }

    #[test]
    fn a_quarto_key_holding_only_language_is_dropped_from_metadata() {
        let bins = partition_format(&map(json!({"quarto": {"language": {"a": "b"}}})));
        assert!(bins["metadata"].get("quarto").is_none());
    }

    #[test]
    fn q1_tail_normalizations_apply() {
        let bins = partition_format(&map(json!({
            "server": "shiny",
            "ipynb-filter": "f.py",
            "variant": "gfm+emoji",
        })));
        assert_eq!(bins["metadata"]["server"], json!({"type": "shiny"}));
        assert_eq!(bins["execute"]["ipynb-filters"], json!(["f.py"]));
        assert!(bins["execute"].get("ipynb-filter").is_none());
        assert_eq!(
            bins["render"]["variant"],
            json!(format!("{GFM_COMMONMARK_VARIANT}+emoji"))
        );
    }

    /// The key lists are a copy of `ts-packages/quarto-api/src/config/index.ts`;
    /// both port Q1's `constants.ts`, and the Node host partitions with the TS
    /// copy, so a drift would give TS engines and the execute-info file
    /// different `format` objects.
    #[test]
    fn key_lists_match_the_typescript_copy() {
        let ts = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../ts-packages/quarto-api/src/config/index.ts"),
        )
        .expect("read quarto-api config/index.ts");
        for (name, rust) in [
            ("kExecuteDefaultsKeys", EXECUTE_KEYS),
            ("kRenderDefaultsKeys", RENDER_KEYS),
            ("kPandocDefaultsKeys", PANDOC_KEYS),
            ("kIdentifierDefaultsKeys", IDENTIFIER_KEYS),
        ] {
            let start = ts
                .find(&format!("export const {name}"))
                .unwrap_or_else(|| panic!("{name} not found"));
            let body = &ts[start..];
            // `= [`, not the `[]` of the `readonly string[]` annotation.
            let open = body.find("= [").expect("list open") + 3;
            let close = body.find("];").expect("list close");
            let ts_keys: Vec<&str> = body[open..close]
                .split(',')
                .map(|s| s.trim().trim_matches('"'))
                .filter(|s| !s.is_empty())
                .collect();
            assert_eq!(ts_keys, rust, "{name} differs from the TypeScript copy");
        }
    }

    fn identity() -> FormatIdentity {
        FormatIdentity {
            target_format: "html".into(),
            base_format: "html".into(),
            display_name: "HTML".into(),
            extension_name: None,
        }
    }

    #[test]
    fn execute_info_has_document_path_identifier_defaults_and_writer() {
        let meta = map(json!({"execute": {"echo": false}}));
        let identity = identity();
        let execute = ExecuteConfig::with_defaults();
        let info = build_execute_info(&ExecuteInfoInput {
            document_path: Path::new("/p/doc.qmd"),
            metadata: &meta,
            identity: &identity,
            writer: "html",
            execute: &execute,
            brand: None,
        });
        assert_eq!(info["document-path"], json!("/p/doc.qmd"));
        let format = &info["format"];
        assert_eq!(
            format["identifier"],
            json!({"target-format": "html", "base-format": "html", "display-name": "HTML"})
        );
        // The document's choice wins; absent keys get Q1's base defaults.
        assert_eq!(format["execute"]["echo"], json!(false));
        assert_eq!(format["execute"]["eval"], json!(true));
        assert_eq!(format["execute"]["error"], json!(false));
        assert_eq!(format["execute"]["fig-width"], json!(7));
        assert_eq!(format["execute"]["fig-height"], json!(5));
        assert_eq!(format["execute"]["fig-dpi"], json!(96));
        assert_eq!(format["execute"]["fig-format"], json!("png"));
        assert_eq!(format["pandoc"]["to"], json!("html"));
        assert!(format["render"].get("brand").is_none());
    }

    #[test]
    fn execute_info_carries_both_brand_modes() {
        let meta = Map::new();
        let identity = identity();
        let execute = ExecuteConfig::with_defaults();
        let brand = BrandModesJson {
            light: json!({"data": {"color": {"background": "#fff"}}}),
            dark: Some(json!({"data": {"color": {"background": "#000"}}})),
        };
        let info = build_execute_info(&ExecuteInfoInput {
            document_path: Path::new("/p/doc.qmd"),
            metadata: &meta,
            identity: &identity,
            writer: "html",
            execute: &execute,
            brand: Some(&brand),
        });
        let modes = &info["format"]["render"]["brand"];
        assert_eq!(modes["light"]["data"]["color"]["background"], json!("#fff"));
        assert_eq!(modes["dark"]["data"]["color"]["background"], json!("#000"));
    }

    #[test]
    fn light_only_brand_has_no_dark_entry() {
        let meta = Map::new();
        let identity = identity();
        let execute = ExecuteConfig::with_defaults();
        let brand = BrandModesJson {
            light: json!({"data": {}}),
            dark: None,
        };
        let info = build_execute_info(&ExecuteInfoInput {
            document_path: Path::new("/p/doc.qmd"),
            metadata: &meta,
            identity: &identity,
            writer: "html",
            execute: &execute,
            brand: Some(&brand),
        });
        assert!(info["format"]["render"]["brand"].get("dark").is_none());
    }

    #[test]
    fn fig_env_reports_defaults_and_document_values() {
        let defaults = FigOptions::from_execute(&ExecuteConfig::with_defaults()).env();
        assert_eq!(
            defaults,
            vec![
                ("QUARTO_FIG_WIDTH".to_string(), "7".to_string()),
                ("QUARTO_FIG_HEIGHT".to_string(), "5".to_string()),
                ("QUARTO_FIG_DPI".to_string(), "96".to_string()),
                ("QUARTO_FIG_FORMAT".to_string(), "png".to_string()),
            ]
        );
        let mut custom = ExecuteConfig::with_defaults();
        custom.fig_width = Some(5.5);
        custom.fig_format = Some("svg".into());
        let env = FigOptions::from_execute(&custom).env();
        assert_eq!(env[0].1, "5.5");
        assert_eq!(env[3].1, "svg");
    }

    #[test]
    fn retina_reports_png_at_double_dpi() {
        let mut retina = ExecuteConfig::with_defaults();
        retina.fig_format = Some("retina".into());
        let env = FigOptions::from_execute(&retina).env();
        assert_eq!(env[2], ("QUARTO_FIG_DPI".to_string(), "192".to_string()));
        assert_eq!(env[3], ("QUARTO_FIG_FORMAT".to_string(), "png".to_string()));
    }

    #[test]
    fn path_env_names_project_document_directory_file_and_info() {
        let root = PathBuf::from("proj");
        let doc = root.join("sub").join("doc.qmd");
        let info = PathBuf::from("tmp").join(EXECUTE_INFO_FILE_NAME);
        let env = path_env(&root, &doc, &info);
        assert_eq!(
            env,
            vec![
                (
                    "QUARTO_PROJECT_ROOT".to_string(),
                    root.display().to_string()
                ),
                (
                    "QUARTO_DOCUMENT_PATH".to_string(),
                    root.join("sub").display().to_string()
                ),
                ("QUARTO_DOCUMENT_FILE".to_string(), "doc.qmd".to_string()),
                (
                    "QUARTO_EXECUTE_INFO".to_string(),
                    info.display().to_string()
                ),
            ]
        );
    }

    fn vars() -> Vec<(String, String)> {
        vec![
            ("A".to_string(), "x \"q\" \\ $y".to_string()),
            ("B".to_string(), "é\n".to_string()),
        ]
    }

    #[test]
    fn python_setup_updates_os_environ_with_escaped_literals() {
        let code = jupyter_setup_code("Python", &vars()).unwrap();
        assert_eq!(
            code,
            "import os as __quarto_os\n__quarto_os.environ.update({\"A\": \"x \\\"q\\\" \\\\ $y\", \"B\": \"é\\n\"})\ndel __quarto_os\n"
        );
    }

    #[test]
    fn r_setup_calls_sys_setenv_invisibly() {
        let code = jupyter_setup_code("R", &vars()).unwrap();
        assert_eq!(
            code,
            "invisible(Sys.setenv(\"A\" = \"x \\\"q\\\" \\\\ $y\", \"B\" = \"é\\n\"))\n"
        );
    }

    #[test]
    fn julia_setup_escapes_interpolation() {
        let code = jupyter_setup_code("julia", &vars()).unwrap();
        assert_eq!(
            code,
            "ENV[\"A\"] = \"x \\\"q\\\" \\\\ \\$y\"\nENV[\"B\"] = \"é\\n\"\nnothing\n"
        );
    }

    #[test]
    fn unknown_kernel_languages_get_no_setup_code() {
        assert_eq!(jupyter_setup_code("bash", &vars()), None);
    }
}
