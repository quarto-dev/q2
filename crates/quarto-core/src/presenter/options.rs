/*
 * presenter/options.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * KNR-2 Phase 2C: the Quarto cell-option layer of the shared presenter.
 */

//! Resolved cell options that shape a cell's markup (KNR-2 Phase 2C, epic decision 12).
//!
//! [`CellOptions`] holds what the presenter reads: classes and attributes for the source
//! fence and the output fences, `column`, `keep-hidden`, the `code-*` and `lst-*` keys. It
//! has two sources: a Jupyter cell's resolved [`ConfigValue`] (document scope merged under
//! the cell's `#|` map) and a knitr record's options (a JSON object of the chunk's current
//! header). Both reduce to one key lookup, so a key means the same thing for either engine.
//! A cell that sets none of these keys yields [`CellOptions::default`], which renders
//! byte-identically to a cell without the layer.

use quarto_pandoc_types::config_value::ConfigValue;
use serde_json::Value;

/// Which output fence an option group targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FenceKind {
    /// `class.output` / `attr.output`: stdout and plain results.
    Output,
    /// `class.error` / `attr.error`.
    Error,
    /// `class.message` or `class.warning` (and the `attr.` forms): stderr of unknown kind,
    /// which takes both.
    Stderr,
    /// `class.message` / `attr.message`: a knitr message.
    Message,
    /// `class.warning` / `attr.warning`: a knitr warning.
    Warning,
}

/// What a stderr output is, where the producer knows (knitr: `quarto_output_kinds`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    Message,
    Warning,
}

/// Resolved options that shape cell markup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CellOptions {
    class_source: Vec<String>,
    attr_source: Vec<String>,
    class_output: Vec<String>,
    attr_output: Vec<String>,
    class_error: Vec<String>,
    attr_error: Vec<String>,
    class_stderr: Vec<String>,
    attr_stderr: Vec<String>,
    class_message: Vec<String>,
    attr_message: Vec<String>,
    class_warning: Vec<String>,
    attr_warning: Vec<String>,
    /// Parallel to the cell\'s outputs: which of them is a message or a warning. Empty when
    /// unknown (a Jupyter cell).
    output_kinds: Vec<Option<OutputKind>>,
    panel: Option<String>,
    column: Vec<String>,
    /// `layout`, `layout-nrow`, `layout-ncol`, `layout-align`, `layout-valign` that are set,
    /// in Quarto\'s forwarding order, as `(key, text)`.
    layout: Vec<(&'static str, String)>,
    keep_hidden: Option<String>,
    code_fold: Option<String>,
    code_summary: Option<String>,
    code_line_numbers: Option<String>,
    lst_cap: Option<String>,
    lst_label: Option<String>,
    /// knitr `fig.cap` (one caption, or one per figure) and `fig.subcap`. Read under knitr\'s
    /// dotted names only: Jupyter\'s `fig-cap` is handled by the Quarto crossref layer.
    fig_cap: Vec<String>,
    fig_subcap: Vec<String>,
    fig_alt: Option<String>,
    /// knitr `prompt`: `> ` before each expression, `+ ` on its continuation lines.
    prompt: bool,
    /// knitr `comment`: written before every output line (`## `); empty or unset is none.
    comment: Option<String>,
    /// knitr `collapse`: outputs merge into the source fence, with no output divs.
    collapse: bool,
    /// knitr `results`: `hide` drops printed text, `asis` writes it bare (`hold` is applied
    /// by [`crate::presenter::cell::apply_holds`]).
    results: Option<String>,
    /// Options Quarto forwards to the cell div as `key='value'` (knitr: outside `known_options`).
    unknown: Vec<(String, String)>,
    /// A `fig-` label: the id of a figure\'s output div.
    figure_label: Option<String>,
    /// A `tbl-` label: the cell\'s id (hooks.R lets only table labels through).
    table_label: Option<String>,
    /// A knitr cell: printed text keeps its trailing spaces (only the final newlines go).
    keep_trailing_space: bool,
    /// knitr `fig.show = "hide"`: plots are not shown.
    fig_show_hide: bool,
    /// knitr numeric `echo`: the expression numbers to show (negative: all but those).
    echo_selection: Option<Vec<i64>>,
    /// knitr numeric `warning` / `message`: which of the cell\'s warnings or messages to show.
    warning_selection: Option<Vec<i64>>,
    message_selection: Option<Vec<i64>>,
    /// knitr `fenced.echo`: show the chunk as written. Set when the option is true.
    fenced_echo: bool,
    /// The chunk as written: the info string (`r c1`) and its `#|` option lines. The mapper
    /// supplies it, since a record\'s code has the option lines removed.
    original_chunk: Option<(String, String)>,
}

impl CellOptions {
    /// Build from a Jupyter cell's resolved options (`None`: nothing was set).
    pub fn from_config(resolved: Option<&ConfigValue>) -> Self {
        Self::from_lookup(
            &|key| {
                resolved
                    .and_then(|c| c.get(key))
                    .map_or_else(Vec::new, config_strings)
            },
            &|key| {
                resolved
                    .and_then(|c| c.get(key))
                    .and_then(|v| json_text(&config_to_json(v)))
            },
        )
    }

    /// Build from a knitr record's options object.
    pub fn from_knitr(options: &Value) -> Self {
        let mut me = Self::from_lookup(
            &|key| options.get(key).map_or_else(Vec::new, json_strings),
            &|key| options.get(key).and_then(json_text),
        );
        me.keep_trailing_space = true;
        me
    }

    /// [`Self::from_knitr`] plus Quarto\'s forwarding of unknown options (hooks.R
    /// `unknown_opts`): each key of `options` that is not in `known` (the record\'s
    /// `known_options`) becomes a cell attribute. `None` filters nothing, since without the
    /// list every key would look unknown.
    pub fn from_knitr_known(options: &Value, known: Option<&[String]>) -> Self {
        let mut me = Self::from_knitr(options);
        let (Some(known), Some(map)) = (known, options.as_object()) else {
            return me;
        };
        for (key, value) in map {
            if known.iter().any(|k| k == key) || key.starts_with('.') || value.is_null() {
                continue;
            }
            let text = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            me.unknown.push((key.clone(), text.replace('\'', "\\'")));
        }
        me
    }

    /// The one place that decides what each key means. `get` returns the key's values as
    /// strings: empty when unset or null, one entry for a scalar, several for a list. `text`
    /// returns a key\'s value as one string, with a list written as compact JSON (`layout`).
    fn from_lookup(
        get: &dyn Fn(&str) -> Vec<String>,
        text: &dyn Fn(&str) -> Option<String>,
    ) -> Self {
        let words = |key: &str| -> Vec<String> {
            get(key)
                .iter()
                .flat_map(|v| v.split_whitespace().map(str::to_string).collect::<Vec<_>>())
                .collect()
        };
        let first = |key: &str| get(key).into_iter().next();
        let stderr_key = |prefix: &str| {
            let mut v = words(&format!("{prefix}.message"));
            v.extend(words(&format!("{prefix}.warning")));
            v
        };
        let collapse = get("collapse").first().map(String::as_str) == Some("true");
        Self {
            class_source: words("class.source"),
            attr_source: get("attr.source"),
            class_output: words("class.output"),
            attr_output: get("attr.output"),
            class_error: words("class.error"),
            attr_error: get("attr.error"),
            class_stderr: stderr_key("class"),
            attr_stderr: {
                let mut v = get("attr.message");
                v.extend(get("attr.warning"));
                v
            },
            class_message: words("class.message"),
            attr_message: get("attr.message"),
            class_warning: words("class.warning"),
            attr_warning: get("attr.warning"),
            output_kinds: Vec::new(),
            panel: first("panel"),
            column: get("column"),
            layout: [
                "layout",
                "layout-nrow",
                "layout-ncol",
                "layout-align",
                "layout-valign",
            ]
            .into_iter()
            .filter_map(|k| text(k).map(|v| (k, v)))
            .collect(),
            keep_hidden: first("keep-hidden"),
            code_fold: first("code-fold"),
            code_summary: first("code-summary"),
            code_line_numbers: first("code-line-numbers"),
            lst_cap: first("lst-cap"),
            lst_label: first("lst-label"),
            fig_cap: get("fig.cap"),
            // `fig.subcap: true` means empty subcaptions (Q1 hooks.R:827).
            fig_subcap: match get("fig.subcap").as_slice() {
                [t] if t == "true" => vec![String::new()],
                other => other.to_vec(),
            },
            fig_alt: first("fig.alt").filter(|a| !a.is_empty()),
            prompt: first("prompt").as_deref() == Some("true"),
            // Quarto\'s opts hook (hooks.R): `collapse` with no `comment` sets it to `##`.
            comment: match (first("comment"), collapse) {
                (Some(c), _) => Some(c).filter(|c| !c.is_empty()),
                (None, true) => Some("##".to_string()),
                (None, false) => None,
            },
            collapse,
            results: first("results"),
            keep_trailing_space: false,
            unknown: Vec::new(),
            figure_label: first("label")
                .filter(|l| l.trim_start_matches('#').starts_with("fig-"))
                .map(|l| l.trim_start_matches('#').to_string()),
            table_label: first("label")
                .filter(|l| l.trim_start_matches('#').starts_with("tbl-"))
                .map(|l| l.trim_start_matches('#').to_string()),
            fig_show_hide: first("fig.show").as_deref() == Some("hide"),
            echo_selection: echo_selection(&get("echo")),
            warning_selection: echo_selection(&get("warning")),
            message_selection: echo_selection(&get("message")),
            fenced_echo: first("fenced.echo").as_deref() == Some("true"),
            original_chunk: None,
        }
    }

    /// Text appended inside the `.cell` div's braces (` .column-margin keep-hidden='true'`).
    pub fn cell_div_attrs(&self) -> String {
        let mut out = String::new();
        if let Some(p) = &self.panel {
            out.push_str(&format!(" .panel-{p}"));
        }
        for c in &self.column {
            out.push_str(&format!(" .column-{c}"));
        }
        for (key, value) in &self.layout {
            out.push_str(&format!(" {key}=\"{value}\""));
        }
        if let Some(v) = &self.keep_hidden {
            out.push_str(&format!(" keep-hidden='{v}'"));
        }
        for (key, value) in &self.unknown {
            out.push_str(&format!(" {key}='{value}'"));
        }
        out
    }

    /// `#fig-label ` for an output div, or nothing (hooks.R `output_label`).
    pub fn figure_id(&self) -> String {
        self.figure_label
            .as_ref()
            .map_or_else(String::new, |l| format!("#{l} "))
    }

    /// `#tbl-label ` ahead of `.cell`, or nothing.
    pub fn cell_div_id(&self) -> String {
        self.table_label
            .as_ref()
            .map_or_else(String::new, |l| format!("#{l} "))
    }

    /// The alt text of the `index`th figure (from 0) of the cell: the subcaptions win over the
    /// captions, each recycled (Q1\'s `figure_cap`), and empty when neither is set.
    pub fn figure_cap(&self, index: usize) -> String {
        let list = if self.fig_subcap.is_empty() {
            &self.fig_cap
        } else {
            &self.fig_subcap
        };
        if list.is_empty() {
            String::new()
        } else {
            list[index % list.len()].clone()
        }
    }

    /// Attributes written after an image (`{fig-alt='...'}`), or an empty string.
    pub fn image_attrs(&self) -> String {
        self.image_attrs_with(None)
    }

    /// [`Self::image_attrs`] plus a `width` (knitr `include_graphics` writes one).
    pub fn image_attrs_with(&self, width: Option<f64>) -> String {
        let mut attrs = Vec::new();
        if let Some(a) = &self.fig_alt {
            attrs.push(format!("fig-alt='{}'", a.replace('\'', "\\'")));
        }
        if let Some(w) = width {
            attrs.push(format!("width={w}"));
        }
        if attrs.is_empty() {
            String::new()
        } else {
            format!("{{{}}}", attrs.join(" "))
        }
    }

    /// `code` with knitr\'s `prompt` applied (`knitr:::line_prompt`): each expression, which
    /// starts at the line indices in `starts`, gets `> ` and its continuation lines `+ `.
    /// Without `starts` the code is one expression. Unchanged unless `prompt` is set.
    pub fn prompt_source(&self, code: &str, starts: &[usize]) -> String {
        if !self.prompt {
            return code.to_string();
        }
        code.lines()
            .enumerate()
            .map(|(i, line)| {
                let first = if starts.is_empty() {
                    i == 0
                } else {
                    starts.contains(&i)
                };
                format!("{}{line}", if first { "> " } else { "+ " })
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// `text` with knitr\'s `comment` before every line (`knitr:::comment_out`, which also
    /// prefixes empty interior lines). Unchanged when `comment` is unset or empty.
    pub fn comment_out(&self, text: &str) -> String {
        match &self.comment {
            None => text.to_string(),
            Some(prefix) => text
                .lines()
                .map(|l| format!("{prefix} {l}"))
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// Attach which outputs are messages and which are warnings (parallel to the cell\'s
    /// outputs). The mapper supplies it, as the record\'s `quarto_output_kinds`.
    pub fn with_output_kinds(mut self, kinds: Vec<Option<OutputKind>>) -> Self {
        self.output_kinds = kinds;
        self
    }

    /// The numeric `warning` selection, when set.
    pub fn warning_selection(&self) -> Option<&[i64]> {
        self.warning_selection.as_deref()
    }

    /// The numeric `message` selection, when set.
    pub fn message_selection(&self) -> Option<&[i64]> {
        self.message_selection.as_deref()
    }

    /// The kinds, parallel to the outputs (empty when unknown).
    pub fn output_kinds(&self) -> &[Option<OutputKind>] {
        &self.output_kinds
    }

    /// The kind of the `index`th output of the cell, when known.
    pub fn output_kind(&self, index: usize) -> Option<OutputKind> {
        self.output_kinds.get(index).copied().flatten()
    }

    /// Attach the chunk as written: its info string and `#|` option lines (the lines that
    /// only say `echo: fenced` are dropped, as in Q1). Used by `fenced.echo`.
    pub fn with_original_chunk(mut self, params: &str, option_lines: &str) -> Self {
        let yaml: Vec<&str> = option_lines
            .lines()
            .filter(|l| {
                let t = l.trim_start_matches("#|").trim();
                !(l.starts_with("#|") && t.replace(' ', "") == "echo:fenced")
            })
            .collect();
        self.original_chunk = Some((params.to_string(), yaml.join("\n")));
        self
    }

    /// The `(info string, option lines)` to show for `fenced.echo`, when both are known.
    pub fn fenced_echo_chunk(&self) -> Option<&(String, String)> {
        self.original_chunk.as_ref().filter(|_| self.fenced_echo)
    }

    /// `text` without its trailing newlines; a Jupyter cell loses all trailing whitespace,
    /// a knitr cell keeps the spaces of its last line.
    pub fn trim_output<'a>(&self, text: &'a str) -> &'a str {
        if self.keep_trailing_space {
            text.trim_end_matches(['\n', '\r'])
        } else {
            text.trim_end()
        }
    }

    /// `fig.show = "hide"`: plots are dropped.
    pub fn fig_show_hide(&self) -> bool {
        self.fig_show_hide
    }

    /// `results = "hide"`: printed text is dropped (messages and plots stay).
    pub fn results_hide(&self) -> bool {
        self.results.as_deref() == Some("hide")
    }

    /// `results = "asis"`: printed text is written as is, outside any div.
    pub fn results_asis(&self) -> bool {
        self.results.as_deref() == Some("asis")
    }

    /// Whether the `.cell` wrapper is written: not for `results = "asis"` unless the cell
    /// has attributes or a caption that need it (hooks.R `needCell`).
    pub fn needs_cell_div(&self) -> bool {
        !self.results_asis() || !self.cell_div_attrs().is_empty() || !self.fig_cap.is_empty()
    }

    /// knitr's numeric `echo`, when set: which source expressions to show.
    pub fn echo_selection(&self) -> Option<&[i64]> {
        self.echo_selection.as_deref()
    }

    /// Whether outputs merge into the source fence (knitr `collapse`).
    pub fn collapse(&self) -> bool {
        self.collapse
    }

    /// The `#id ` prefix of the source fence's braces (`lst-label`), with its trailing space.
    pub fn source_id_prefix(&self) -> String {
        self.lst_label
            .as_deref()
            .map(|l| format!("#{l} "))
            .unwrap_or_default()
    }

    /// Classes written between the language and `.cell-code` (` .foo .bar`).
    pub fn source_classes(&self) -> String {
        self.class_source.iter().map(|c| format!(" .{c}")).collect()
    }

    /// Attribute text written after `.cell-code` (` code-fold="true"`).
    pub fn source_attrs(&self) -> String {
        let mut out: String = self.attr_source.iter().map(|a| format!(" {a}")).collect();
        for (key, value) in [
            ("code-fold", &self.code_fold),
            ("code-summary", &self.code_summary),
            ("code-line-numbers", &self.code_line_numbers),
            ("lst-cap", &self.lst_cap),
        ] {
            if let Some(v) = value {
                out.push_str(&format!(" {key}=\"{v}\""));
            }
        }
        out
    }

    /// The braces' content for an output fence of `kind`, or `None` for a bare fence.
    pub fn fence_attrs(&self, kind: FenceKind) -> Option<String> {
        let (classes, attrs) = match kind {
            FenceKind::Output => (&self.class_output, &self.attr_output),
            FenceKind::Error => (&self.class_error, &self.attr_error),
            FenceKind::Stderr => (&self.class_stderr, &self.attr_stderr),
            FenceKind::Message => (&self.class_message, &self.attr_message),
            FenceKind::Warning => (&self.class_warning, &self.attr_warning),
        };
        let parts: Vec<String> = classes
            .iter()
            .map(|c| format!(".{c}"))
            .chain(attrs.iter().cloned())
            .collect();
        (!parts.is_empty()).then(|| parts.join(" "))
    }
}

/// knitr's numeric `echo` (`echo = c(1, 3)`, `echo = -1`): the expression numbers, or `None`
/// for the logical values and for anything that is not a whole list of numbers.
fn echo_selection(values: &[String]) -> Option<Vec<i64>> {
    if values.is_empty() {
        return None;
    }
    values
        .iter()
        .map(|v| {
            v.parse::<f64>()
                .ok()
                .filter(|f| f.fract() == 0.0)
                .map(|f| f as i64)
        })
        .collect()
}

/// A knitr option value as strings: a string, a list of them, or a scalar's JSON text.
fn json_strings(v: &Value) -> Vec<String> {
    match v {
        Value::Null => vec![],
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a.iter().flat_map(json_strings).collect(),
        other => vec![other.to_string()],
    }
}

/// A value as one string: a string as is, a list or map as compact JSON, a scalar as its
/// JSON text, null as nothing.
fn json_text(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

/// A resolved `ConfigValue` as JSON, for [`json_text`].
pub(crate) fn config_to_json(v: &ConfigValue) -> Value {
    if let Some(items) = v.as_array() {
        return Value::Array(items.iter().map(config_to_json).collect());
    }
    if let Some(b) = v.as_bool() {
        return Value::Bool(b);
    }
    if let Some(i) = v.as_int() {
        return Value::from(i);
    }
    v.as_plain_text().map_or(Value::Null, Value::String)
}

/// The same for a resolved `ConfigValue`.
fn config_strings(v: &ConfigValue) -> Vec<String> {
    if let Some(items) = v.as_array() {
        return items.iter().flat_map(config_strings).collect();
    }
    if v.is_null() {
        return vec![];
    }
    if let Some(b) = v.as_bool() {
        return vec![b.to_string()];
    }
    if let Some(i) = v.as_int() {
        return vec![i.to_string()];
    }
    v.as_plain_text().into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_options::options_to_config;
    use serde_json::json;

    fn config(yaml: &str) -> ConfigValue {
        let parsed = quarto_yaml::parse(yaml).expect("test YAML parses");
        options_to_config(parsed).0
    }

    /// A knitr cell differs from the default in one way only: it keeps trailing spaces.
    fn inert(mut o: CellOptions) -> CellOptions {
        o.keep_trailing_space = false;
        o
    }

    #[test]
    fn default_options_are_inert() {
        let o = CellOptions::default();
        assert_eq!(o.cell_div_attrs(), "");
        assert_eq!(o.source_id_prefix(), "");
        assert_eq!(o.source_classes(), "");
        assert_eq!(o.source_attrs(), "");
        for k in [FenceKind::Output, FenceKind::Error, FenceKind::Stderr] {
            assert_eq!(o.fence_attrs(k), None);
        }
        assert_eq!(CellOptions::from_config(None), o);
        assert_eq!(inert(CellOptions::from_knitr(&json!({"echo": true}))), o);
        assert_eq!(CellOptions::from_config(Some(&config("echo: false"))), o);
    }

    #[test]
    fn knitr_class_and_attr_for_source() {
        let o = CellOptions::from_knitr(&json!({
            "class.source": "a b",
            "attr.source": ["data-x=\"1\"", ".c"],
        }));
        assert_eq!(o.source_classes(), " .a .b");
        assert_eq!(o.source_attrs(), " data-x=\"1\" .c");
    }

    #[test]
    fn knitr_output_fences() {
        let o = CellOptions::from_knitr(&json!({
            "class.output": ["o1", "o2"],
            "attr.output": "foo=bar",
            "class.error": "e",
            "class.warning": "w",
        }));
        assert_eq!(
            o.fence_attrs(FenceKind::Output).as_deref(),
            Some(".o1 .o2 foo=bar")
        );
        assert_eq!(o.fence_attrs(FenceKind::Error).as_deref(), Some(".e"));
        assert_eq!(o.fence_attrs(FenceKind::Stderr).as_deref(), Some(".w"));
    }

    #[test]
    fn knitr_cell_level_keys() {
        let o = CellOptions::from_knitr(&json!({
            "column": "margin",
            "keep-hidden": true,
            "code-fold": true,
            "code-summary": "Show",
            "code-line-numbers": "1,3",
            "lst-cap": "Listing",
            "lst-label": "lst-a",
        }));
        assert_eq!(o.cell_div_attrs(), " .column-margin keep-hidden='true'");
        assert_eq!(o.source_id_prefix(), "#lst-a ");
        assert_eq!(
            o.source_attrs(),
            " code-fold=\"true\" code-summary=\"Show\" code-line-numbers=\"1,3\" lst-cap=\"Listing\""
        );
    }

    #[test]
    fn jupyter_and_knitr_agree_on_the_same_keys() {
        let yaml = "class.source: a b\nattr.output: foo=bar\ncolumn: margin\ncode-fold: true\nlst-label: lst-a\n";
        let json = json!({
            "class.source": "a b",
            "attr.output": "foo=bar",
            "column": "margin",
            "code-fold": true,
            "lst-label": "lst-a",
        });
        assert_eq!(
            CellOptions::from_config(Some(&config(yaml))),
            inert(CellOptions::from_knitr(&json))
        );
    }

    #[test]
    fn jupyter_list_values() {
        let o = CellOptions::from_config(Some(&config("class.output: [x, y]\n")));
        assert_eq!(o.fence_attrs(FenceKind::Output).as_deref(), Some(".x .y"));
    }

    #[test]
    fn panel_class_comes_before_column_class() {
        let o = CellOptions::from_knitr(&json!({"column": "margin", "panel": "tabset"}));
        assert_eq!(o.cell_div_attrs(), " .panel-tabset .column-margin");
    }

    #[test]
    fn layout_keys_are_forwarded_in_quarto_order() {
        let o = CellOptions::from_knitr(&json!({
            "layout-valign": "top",
            "layout-ncol": 2,
            "layout-align": "center",
            "layout-nrow": "1",
        }));
        assert_eq!(
            o.cell_div_attrs(),
            " layout-nrow=\"1\" layout-ncol=\"2\" layout-align=\"center\" layout-valign=\"top\""
        );
    }

    #[test]
    fn layout_attrs_follow_classes_and_precede_keep_hidden() {
        let o = CellOptions::from_knitr(&json!({
            "panel": "tabset",
            "layout-ncol": 2,
            "keep-hidden": true,
        }));
        assert_eq!(
            o.cell_div_attrs(),
            " .panel-tabset layout-ncol=\"2\" keep-hidden='true'"
        );
    }

    #[test]
    fn a_layout_list_is_written_as_compact_json() {
        let o = CellOptions::from_knitr(&json!({"layout": [[1, 1], [1]]}));
        assert_eq!(o.cell_div_attrs(), " layout=\"[[1,1],[1]]\"");
        let s = CellOptions::from_knitr(&json!({"layout": "[[1,2]]"}));
        assert_eq!(s.cell_div_attrs(), " layout=\"[[1,2]]\"");
    }

    #[test]
    fn jupyter_and_knitr_agree_on_panel_and_layout() {
        let yaml = "panel: tabset\ncolumn: margin\nlayout-ncol: 2\nlayout: [[1, 1], [1]]\n";
        let json = json!({
            "panel": "tabset",
            "column": "margin",
            "layout-ncol": 2,
            "layout": [[1, 1], [1]],
        });
        let from_yaml = CellOptions::from_config(Some(&config(yaml)));
        assert_eq!(from_yaml, inert(CellOptions::from_knitr(&json)));
        assert_eq!(
            from_yaml.cell_div_attrs(),
            " .panel-tabset .column-margin layout=\"[[1,1],[1]]\" layout-ncol=\"2\""
        );
    }

    #[test]
    fn null_values_are_unset() {
        let o = CellOptions::from_knitr(&json!({"class.source": null, "column": null}));
        assert_eq!(inert(o), CellOptions::default());
    }

    #[test]
    fn numeric_echo_is_a_selection_and_logical_echo_is_not() {
        let sel = |v: Value| CellOptions::from_knitr(&json!({"echo": v}));
        assert_eq!(sel(json!([1, 3])).echo_selection(), Some(&[1, 3][..]));
        assert_eq!(sel(json!(2)).echo_selection(), Some(&[2][..]));
        assert_eq!(sel(json!([-1])).echo_selection(), Some(&[-1][..]));
        assert_eq!(sel(json!(true)).echo_selection(), None);
        assert_eq!(sel(json!(false)).echo_selection(), None);
        assert_eq!(CellOptions::from_knitr(&json!({})).echo_selection(), None);
    }
}
