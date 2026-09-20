//! The `QUARTO_FILTER_PARAMS` builder: Q2's re-derivation of Q1 TypeScript's
//! `quartoFilterParams`/`layoutFilterParams`/`crossrefFilterParams` family
//! (`src/command/render/filters.ts:128-201`, `layout.ts:23`,
//! `crossref.ts:27`, all at tag `v1.11.3`), plus two keys measurement shows
//! are structurally required even though Q1's own semantics reads them as
//! N/A (`quarto-filters`) or as a caller-supplied row (`language`) — see
//! `claude-notes/plans/2026-09-18-pandoc-hybrid-P4-implementation.md`
//! Findings for Gordon, items 1 and 2.
//!
//! Deliberate seam: [`FilterParamsBuilder`] takes `&Format`/`&ProjectContext`
//! rather than the full `&RenderContext` — the pieces the builder actually
//! needs, without pulling in `ArtifactStore` or other render-only state.
//! Task 9's `PandocWriteStage` constructs these from a real `RenderContext`.
//!
//! The `crossref-<type>-title`/`-prefix` family lives in
//! [`super::crossref_params`] (Task 5) and is wired in via [`build`]; the
//! [`FilterParamsContributor`] extension point is for *callers* (P7's
//! per-format extras), not for Q2's own built-in key families.

use std::path::PathBuf;

use serde_json::{Map, Value, json};

use crate::crossref::RefTypeRegistry;
use crate::format::{Format, PipelineProfile};
use crate::language::LanguageTerms;
use crate::project::ProjectContext;

/// Extension point for callers to add or override keys after the core
/// builder runs — mirroring Q1's own spread order (`filters.ts:186`,
/// `...filterParams` applied after `...quartoFilterParams`, i.e. the
/// caller-supplied row wins on conflicts).
pub trait FilterParamsContributor {
    fn contribute(&self, blob: &mut Map<String, Value>);
}

/// The 5 docx callout-icon params (P7 Task 4). `docxCalloutImage` in the
/// vendored `callouts.lua` reads `param("icon-" .. type, nil)` for each
/// callout type and returns `nil` (icon-less, but a successful render) when
/// unset — see `resources/pandoc-filters/filters/modules/callouts.lua`.
/// `share_dir` is the directory that has a `formats/docx/<name>.png` child:
/// either the per-render extracted share tree
/// (`PandocWriteStage`'s real wiring) or the checked-in `resources/`
/// directory directly (this module's own unit test — see
/// [`format_defaults`](super::format_defaults) module docs for why the two
/// share one relative shape).
pub struct DocxCalloutIconsContributor {
    pub share_dir: PathBuf,
}

/// The 5 callout types `docxCalloutImage` looks up by
/// `"icon-" .. type` — one PNG each, vendored at
/// `resources/formats/docx/<name>.png`.
pub const DOCX_CALLOUT_ICON_NAMES: &[&str] = &["note", "tip", "warning", "caution", "important"];

impl FilterParamsContributor for DocxCalloutIconsContributor {
    fn contribute(&self, blob: &mut Map<String, Value>) {
        for name in DOCX_CALLOUT_ICON_NAMES {
            let path = self
                .share_dir
                .join("formats")
                .join("docx")
                .join(format!("{name}.png"));
            blob.insert(
                format!("icon-{name}"),
                json!(path.to_string_lossy().into_owned()),
            );
        }
    }
}

/// Builds the `QUARTO_FILTER_PARAMS` blob for one Pandoc-leg render.
pub struct FilterParamsBuilder<'a> {
    format: &'a Format,
    project: &'a ProjectContext,
    ref_type_registry: Option<&'a RefTypeRegistry>,
    language: &'a LanguageTerms,
    results_file: PathBuf,
    typst_binary: Option<&'a std::path::Path>,
    contributors: Vec<Box<dyn FilterParamsContributor>>,
}

impl<'a> FilterParamsBuilder<'a> {
    pub fn new(
        format: &'a Format,
        project: &'a ProjectContext,
        ref_type_registry: Option<&'a RefTypeRegistry>,
        language: &'a LanguageTerms,
        results_file: PathBuf,
    ) -> Self {
        Self {
            format,
            project,
            ref_type_registry,
            language,
            results_file,
            typst_binary: None,
            contributors: Vec::new(),
        }
    }

    /// The `typst` binary path, if known, feeding
    /// `quarto-environment.paths.Typst`. Not required — Route R's Pandoc leg
    /// doesn't need `typst` for docx/pptx; the key still needs a (possibly
    /// empty) value because `init.lua`'s accessor indexes `.paths` directly.
    pub fn with_typst_binary(mut self, path: Option<&'a std::path::Path>) -> Self {
        self.typst_binary = path;
        self
    }

    /// Registers a caller-supplied contributor (P7's per-format extras).
    /// Contributors run after every core key family, in registration order,
    /// and may override a core key.
    pub fn with_contributor(mut self, contributor: Box<dyn FilterParamsContributor>) -> Self {
        self.contributors.push(contributor);
        self
    }

    pub fn build(&self) -> Value {
        let mut blob = Map::new();

        insert_format_identifier(&mut blob, self.format);
        insert_active_filters(&mut blob, self.format);
        insert_quarto_filters(&mut blob);
        insert_project_keys(&mut blob, self.project);
        insert_language(&mut blob, self.language);
        if let Some(registry) = self.ref_type_registry {
            super::crossref_params::insert_crossref_title_prefix_family(
                &mut blob,
                registry,
                self.language,
            );
        }
        insert_numbering_params(&mut blob);
        insert_crossref_numbering_mode(&mut blob, self.format);
        insert_top_level_literals(&mut blob, &self.results_file, self.typst_binary);

        for contributor in &self.contributors {
            contributor.contribute(&mut blob);
        }

        Value::Object(blob)
    }
}

/// `format-identifier: { base-format, target-format }` — `filters.ts:663`'s
/// `options.format.identifier`/`options.format.formatExtras`-derived pair,
/// re-derived from Q2's own [`Format`]. `base-format` is the underlying
/// pandoc writer name (`output_extension`, e.g. `"docx"`); `target-format`
/// is the possibly-extended format string (`Format::target_format`, e.g.
/// `"acm-docx"`).
fn insert_format_identifier(blob: &mut Map<String, Value>, format: &Format) {
    blob.insert(
        "format-identifier".to_string(),
        json!({
            "base-format": format.output_extension,
            "target-format": format.target_format,
        }),
    );
}

/// `enable-crossref`, `output-divs`, `active-filters`, `page-width` —
/// structural literals from the Task 4 worked example, with `output-divs`
/// and `page-width` overridable per format
/// ([`super::format_defaults::format_pandoc_defaults`], P7 Task 4 Finding
/// 1). Neither is a pandoc CLI flag — both are read from
/// `QUARTO_FILTER_PARAMS` by the vendored layout Lua (`page-width` by
/// `wp.lua`'s `wpPageWidth()`). Q2's crossref/normalization/AST-pipeline
/// features are always active for the Pandoc leg, so `enable-crossref`/
/// `active-filters` stay constants, not derived from render options.
fn insert_active_filters(blob: &mut Map<String, Value>, format: &Format) {
    let defaults = super::format_defaults::format_pandoc_defaults(&format.output_extension);
    blob.insert("enable-crossref".to_string(), json!(true));
    blob.insert(
        "output-divs".to_string(),
        json!(defaults.output_divs.unwrap_or(true)),
    );
    blob.insert(
        "active-filters".to_string(),
        json!({
            "normalization": true,
            "crossref": true,
            "jats_subarticle": false,
        }),
    );
    if let Some(page_width) = defaults.page_width {
        blob.insert("page-width".to_string(), json!(page_width));
    }
}

/// `quarto-filters: { entryPoints: [] }` — structurally required
/// (`main.lua:735` `inject_user_filters_at_entry_points`,
/// `ast/emulatedfilter.lua:45` indexes `.entryPoints` with no default) even
/// though Q2 runs user filters itself and never populates entry points; see
/// Findings for Gordon, item 2.
fn insert_quarto_filters(blob: &mut Map<String, Value>) {
    blob.insert("quarto-filters".to_string(), json!({ "entryPoints": [] }));
}

/// `crossref-index-file`, present only for a real (non-single-file) project.
/// Mirrors `filters.ts:663`'s unconditional dereference of
/// `options.project.isSingleFile` — Q2's [`ProjectContext::is_single_file`]
/// already models the same synthetic-project value Q1's TS side requires,
/// so no separate "synthetic project" type is needed.
fn insert_project_keys(blob: &mut Map<String, Value>, project: &ProjectContext) {
    if !project.is_single_file {
        let index_file = project.dir.join(".quarto").join("crossref-index.json");
        blob.insert(
            "crossref-index-file".to_string(),
            json!(index_file.to_string_lossy()),
        );
    }
}

/// `language: { ...all resolved terms... }` — structurally required
/// (`layout/manuscript.lua:29-30` indexes `param("language", nil)`
/// unconditionally at construction time). Q2 already vendors the full
/// upstream locale set (`resources/language/_language*.yml`), so
/// [`LanguageTerms`] needs no new data; see Findings for Gordon, item 1.
fn insert_language(blob: &mut Map<String, Value>, language: &LanguageTerms) {
    let mut obj = Map::new();
    for (key, entry) in language.iter() {
        obj.insert(key.to_string(), json!(entry.value));
    }
    blob.insert("language".to_string(), Value::Object(obj));
}

/// `number-sections`, `number-offset`, `number-depth` — required for API
/// completeness (Q1's own `quartoFilterParams` always includes them) but
/// inert under external crossref mode, since the only Lua that reads them
/// beyond the safe `param(key, default)` calls already scattered through
/// the tree (`quarto_crossref_filters`, gated at `main.lua:718`) never runs.
/// See the design doc's number-sections decision (2026-09-18) and the
/// plan's Missing-test pass item 4.
fn insert_numbering_params(blob: &mut Map<String, Value>) {
    blob.insert("number-sections".to_string(), json!(false));
    blob.insert("number-offset".to_string(), json!([] as [i64; 0]));
    blob.insert("number-depth".to_string(), json!(6));
}

/// `crossref-numbering: "external"` — Pandoc-leg profiles only (P6 Task 1).
///
/// P5's wire-format shim already assigns every Route-R node's `.order` from
/// Q2's own `CrossrefIndexTransform` before this render starts. Without this
/// key, Q1's own `quarto_crossref_filters` auto-indexer group
/// (`main.lua:718`'s `assignCrossrefNumbers` predicate, gated by P3's
/// upstream patch) stays active and unconditionally *overwrites* that
/// pre-assigned order — `crossref_theorems()`/`crossref_figures()`/
/// `crossref_callouts()` all call `add_crossref`, which always calls
/// `indexNextOrder` with no idempotency check against an existing value.
/// For a flat single-element fixture the two counters happen to agree (both
/// start from 1 in document order), which is why this went unnoticed until
/// a discriminating fixture was built — but for any real multi-element
/// document, or a `CrossrefResolvedRef` citing a Route-R element's
/// Q2-resolved number, the two can silently disagree (bd-fzqykm0n).
///
/// Every non-Pandoc profile omits the key entirely, deliberately not
/// `"quarto"` — emitting the literal Q1 default would make
/// `param("crossref-numbering", "quarto")`'s own default branch
/// unreachable and untested (P3's companion already logged that trade-off
/// for the value pair this key introduces).
fn insert_crossref_numbering_mode(blob: &mut Map<String, Value>, format: &Format) {
    if matches!(
        PipelineProfile::from_format(&format.target_format),
        PipelineProfile::Pandoc(_)
    ) {
        blob.insert("crossref-numbering".to_string(), json!("external"));
    }
}

/// `results-file`, `execution-engine`, `quarto-environment` — top-level
/// literals from the Task 4 worked example. `execution-engine` is a
/// placeholder constant: by the time a document reaches the Pandoc leg, any
/// real computation (`knitr`/`jupyter`) has already executed upstream, and
/// `main.lua:244`'s only consumer (`param("execution-engine") ==
/// "knitr"`) degrades safely to `false` for any other value — so
/// `"markdown"` (Q2's pass-through, non-computational default) is correct
/// for every render this epic's scope covers. `quarto-environment.paths`'s
/// `Rscript`/`TinyTexBinDir` have no Q2 equivalent yet (irrelevant to
/// docx/pptx); both accessors in `init.lua` are called lazily, not at load
/// time, so an empty string is safe.
fn insert_top_level_literals(
    blob: &mut Map<String, Value>,
    results_file: &std::path::Path,
    typst_binary: Option<&std::path::Path>,
) {
    blob.insert(
        "results-file".to_string(),
        json!(results_file.to_string_lossy()),
    );
    blob.insert("execution-engine".to_string(), json!("markdown"));
    blob.insert(
        "quarto-environment".to_string(),
        json!({
            "paths": {
                "Rscript": "",
                "TinyTexBinDir": "",
                "Typst": typst_binary.map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
            }
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use crate::crossref::RefTypeRegistry;
    use crate::language::resolve_language;
    use crate::project::{DocumentInfo, ProjectContext};

    fn fixture_project(is_single_file: bool) -> ProjectContext {
        ProjectContext {
            dir: PathBuf::from("/project"),
            is_single_file,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: PathBuf::from("/project"),
            ..Default::default()
        }
    }

    fn fixture_language() -> LanguageTerms {
        resolve_language("en", &[])
    }

    fn fixture_builder<'a>(
        format: &'a Format,
        project: &'a ProjectContext,
        registry: &'a RefTypeRegistry,
        language: &'a LanguageTerms,
    ) -> FilterParamsBuilder<'a> {
        FilterParamsBuilder::new(
            format,
            project,
            Some(registry),
            language,
            PathBuf::from("/tmp/quarto-pandoc-results.json"),
        )
    }

    /// T4.1: the built blob's *key set* (not values — values include
    /// absolute temp paths and the 111-key language bag, which would make a
    /// value snapshot unstable) is snapshotted.
    ///
    /// Revert hunk: removing any one contributor call in `build` (e.g.
    /// `insert_quarto_filters`) changes the sorted key list and this
    /// snapshot goes RED.
    #[test]
    fn test_params_blob_key_set_snapshot() {
        let format = Format::docx();
        let project = fixture_project(true);
        let registry = RefTypeRegistry::builtin();
        let language = fixture_language();
        let builder = fixture_builder(&format, &project, &registry, &language);

        let blob = builder.build();
        let obj = blob.as_object().expect("blob is a JSON object");
        let mut keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();

        insta::assert_debug_snapshot!(keys);
    }

    /// T4.2: `crossref-index-file` is absent for a single-file "project"
    /// and present otherwise — both polarities, which is what makes this a
    /// discriminator rather than a presence check.
    ///
    /// Revert hunk: removing the `if !project.is_single_file` guard (always
    /// inserting the key) makes the single-file half RED.
    #[test]
    fn test_single_file_project_omits_crossref_index() {
        let format = Format::docx();
        let registry = RefTypeRegistry::builtin();
        let language = fixture_language();

        let single = fixture_project(true);
        let blob = fixture_builder(&format, &single, &registry, &language).build();
        assert!(
            !blob
                .as_object()
                .unwrap()
                .contains_key("crossref-index-file")
        );

        let multi = fixture_project(false);
        let blob = fixture_builder(&format, &multi, &registry, &language).build();
        assert!(
            blob.as_object()
                .unwrap()
                .contains_key("crossref-index-file")
        );
    }

    /// T4.3: `quarto-filters.entryPoints` is an empty JSON array
    /// (type-checked, not just non-null).
    ///
    /// Revert hunk: removing `insert_quarto_filters`'s call makes the
    /// `blob["quarto-filters"]` index panic (key absent).
    #[test]
    fn test_quarto_filters_is_an_empty_entrypoints_bag() {
        let format = Format::docx();
        let project = fixture_project(true);
        let registry = RefTypeRegistry::builtin();
        let language = fixture_language();
        let blob = fixture_builder(&format, &project, &registry, &language).build();

        let entry_points = blob["quarto-filters"]["entryPoints"]
            .as_array()
            .expect("entryPoints should be an array");
        assert_eq!(entry_points.len(), 0);
    }

    /// T4.5: the `language` bag has `>= 100` keys (111 today; `>= 100`
    /// avoids reddening on a routine locale addition) and contains two
    /// named keys known to be indexed unconditionally downstream
    /// (`manuscript.lua`, `authors.lua`).
    ///
    /// Revert hunk: removing `insert_language`'s call makes the
    /// `blob["language"]` index panic (key absent).
    #[test]
    fn test_language_bag_is_complete() {
        let format = Format::docx();
        let project = fixture_project(true);
        let registry = RefTypeRegistry::builtin();
        let language = fixture_language();
        let blob = fixture_builder(&format, &project, &registry, &language).build();

        let lang_obj = blob["language"].as_object().expect("language is an object");
        assert!(
            lang_obj.len() >= 100,
            "expected >= 100 language keys, got {}",
            lang_obj.len()
        );
        assert!(lang_obj.contains_key("source-notebooks-prefix"));
        assert!(lang_obj.contains_key("title-block-author-single"));
    }

    /// T4.8: keys deferred to P7 or later are not stubbed out early.
    ///
    /// Revert hunk: adding a stub contributor call for any of these keys
    /// (e.g. `ipynb-title-block-template`) makes this RED.
    #[test]
    fn test_deferred_keys_are_not_stubbed() {
        let format = Format::docx();
        let project = fixture_project(true);
        let registry = RefTypeRegistry::builtin();
        let language = fixture_language();
        let blob = fixture_builder(&format, &project, &registry, &language).build();
        let obj = blob.as_object().unwrap();

        for key in [
            "ipynb-title-block-template",
            "jats-subarticle-id",
            "notebook-context",
            "cites-index-file",
            "quarto-custom-format",
            "reference-location",
        ] {
            assert!(!obj.contains_key(key), "unexpected key {key} in blob");
        }
    }

    /// T4.9: the inert numbering params are present for API completeness
    /// (design decision 2026-09-18) even though nothing currently reads
    /// them under external crossref mode.
    ///
    /// Revert hunk: removing `insert_numbering_params`'s call makes
    /// `blob["number-depth"]` index panic (key absent).
    #[test]
    fn test_inert_numbering_params_are_still_emitted() {
        let format = Format::docx();
        let project = fixture_project(true);
        let registry = RefTypeRegistry::builtin();
        let language = fixture_language();
        let blob = fixture_builder(&format, &project, &registry, &language).build();
        let obj = blob.as_object().unwrap();

        assert!(obj.contains_key("number-sections"));
        assert!(obj.contains_key("number-offset"));
        assert!(obj.contains_key("number-depth"));
    }

    /// T4.10: a registered contributor's keys reach the built blob, and can
    /// override a core key — matching Q1's spread order (`filters.ts:186`).
    ///
    /// Revert hunk: removing the contributor loop in `build` makes the
    /// presence assertion RED; moving the loop *before* the core inserts
    /// makes the override assertion RED.
    #[test]
    fn test_format_contributor_extension_point() {
        struct Probe;
        impl FilterParamsContributor for Probe {
            fn contribute(&self, blob: &mut Map<String, Value>) {
                blob.insert("t4-probe".to_string(), json!(1));
                blob.insert("execution-engine".to_string(), json!("overridden"));
            }
        }

        let format = Format::docx();
        let project = fixture_project(true);
        let registry = RefTypeRegistry::builtin();
        let language = fixture_language();
        let blob = FilterParamsBuilder::new(
            &format,
            &project,
            Some(&registry),
            &language,
            PathBuf::from("/tmp/quarto-pandoc-results.json"),
        )
        .with_contributor(Box::new(Probe))
        .build();

        assert_eq!(blob["t4-probe"], json!(1));
        assert_eq!(blob["execution-engine"], json!("overridden"));
    }

    /// P6 T1.1: `crossref-numbering: "external"` is present for a Pandoc
    /// profile, suppressing Q1's own crossref auto-indexer (`main.lua:718`'s
    /// `assignCrossrefNumbers` predicate, P3) so it never clobbers the
    /// wire-format shim's pre-assigned `.order` (bd-fzqykm0n).
    ///
    /// Revert hunk: removing the `crossref-numbering` insertion under the
    /// `PipelineProfile::Pandoc(_)` arm makes this RED (key absent).
    #[test]
    fn test_pandoc_profile_sets_external_crossref_numbering() {
        let format = Format::docx();
        let project = fixture_project(true);
        let registry = RefTypeRegistry::builtin();
        let language = fixture_language();
        let blob = fixture_builder(&format, &project, &registry, &language).build();

        assert_eq!(blob["crossref-numbering"], json!("external"));
    }

    /// P6 T1.2: non-Pandoc profiles (native HTML render/preview) never see
    /// the key at all — not `"quarto"` — so Q1's own
    /// `param("crossref-numbering", "quarto")` default stays the reachable,
    /// tested code path. This builder is only ever wired into the Pandoc
    /// leg in production (`PandocWriteStage`), so this row exercises the
    /// function directly with a non-Pandoc `Format` to prove the gate, not
    /// just the insert.
    ///
    /// Revert hunk: hoisting the insertion out of the `Pandoc(_)`-only gate
    /// (making it unconditional) makes this RED.
    #[test]
    fn test_non_pandoc_profile_omits_crossref_numbering() {
        let project = fixture_project(true);
        let registry = RefTypeRegistry::builtin();
        let language = fixture_language();

        for target_format in ["html", "q2-preview"] {
            let format = Format::from_format_string(target_format)
                .unwrap_or_else(|e| panic!("failed to build Format for {target_format}: {e}"));
            let blob = fixture_builder(&format, &project, &registry, &language).build();
            assert!(
                !blob.as_object().unwrap().contains_key("crossref-numbering"),
                "expected no crossref-numbering key for {target_format}, got {blob}"
            );
        }
    }

    /// T4.11: `results-file` is an absolute path and `quarto-environment`
    /// carries the three expected `paths` keys.
    ///
    /// Revert hunk: removing `insert_top_level_literals`'s call makes the
    /// `blob["results-file"]` index panic (key absent).
    #[test]
    fn test_top_level_literals() {
        let format = Format::docx();
        let project = fixture_project(true);
        let registry = RefTypeRegistry::builtin();
        let language = fixture_language();
        let blob = fixture_builder(&format, &project, &registry, &language).build();

        let results_file = blob["results-file"].as_str().unwrap();
        assert!(std::path::Path::new(results_file).is_absolute());

        let paths = blob["quarto-environment"]["paths"]
            .as_object()
            .expect("quarto-environment.paths should be an object");
        assert!(paths.contains_key("Rscript"));
        assert!(paths.contains_key("TinyTexBinDir"));
        assert!(paths.contains_key("Typst"));
    }

    /// P7 T4.1: `output-divs`/`page-width` are per-format
    /// (`format_defaults::format_pandoc_defaults`), not the flat constant
    /// they used to be.
    #[test]
    fn test_output_divs_and_page_width_are_per_format() {
        let project = fixture_project(true);
        let registry = RefTypeRegistry::builtin();
        let language = fixture_language();

        let docx = Format::docx();
        let blob = fixture_builder(&docx, &project, &registry, &language).build();
        assert_eq!(blob["output-divs"], json!(true));
        assert_eq!(blob["page-width"], json!(6.5));

        let pptx = Format::from_format_string("pptx").expect("pptx format");
        let blob = fixture_builder(&pptx, &project, &registry, &language).build();
        assert_eq!(blob["output-divs"], json!(false));
        assert!(
            blob.as_object().unwrap().get("page-width").is_none(),
            "pptx has no page-width entry in Task 4's table"
        );
    }

    /// P7 T4.8: all 5 docx callout-icon params are present and each points
    /// at an existing file under `resources/formats/docx/`. The failure
    /// this guards is silent: `docxCalloutImage` returns `nil` when unset,
    /// the render still succeeds, and callouts simply have no icons.
    #[test]
    fn test_docx_callout_icons_present() {
        let resources_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources");
        let contributor = DocxCalloutIconsContributor {
            share_dir: resources_dir,
        };
        let mut blob = Map::new();
        contributor.contribute(&mut blob);

        for name in DOCX_CALLOUT_ICON_NAMES {
            let key = format!("icon-{name}");
            let path_str = blob
                .get(&key)
                .unwrap_or_else(|| panic!("missing {key}"))
                .as_str()
                .unwrap_or_else(|| panic!("{key} is not a string"));
            assert!(
                Path::new(path_str).is_file(),
                "{key} names a file that does not exist: {path_str}"
            );
        }
    }
}
