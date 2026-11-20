/*
 * config_paths.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The one place a config-authored path string is turned into a
 * filesystem path. bd-nqq41zgx (GH #945). Contract:
 * claude-notes/designs/path-resolution-model.md.
 */

//! Late-binding resolution of paths written in YAML.
//!
//! A relative path in `_quarto.yml`, a `_metadata.yml`, a document's
//! front matter or an extension manifest means "relative to the
//! directory of the file that declared it" (contract rule 1), and a
//! leading `/` means "relative to the project root" (rule 2). After
//! the metadata merge the *layer* a value came from is gone, but the
//! value's own [`SourceInfo`] survives, and the merge stage registers
//! every YAML layer file in the document's [`SourceContext`]
//! (`stage/stages/metadata_merge.rs`). So the declaring directory can
//! be recovered at the point of use, and no consumer needs to pick a
//! base directory of its own.
//!
//! [`resolve_config_path`] is that resolver. Consumers call it with the
//! `ConfigValue` they read (not the string they extracted from it) and
//! a [`PathCtx`]; they never `doc_dir.join(..)` a config string
//! themselves (contract author rule 1; the `config-path-base` xtask lint,
//! bd-40luf359, enforces it in `stage/` and `transforms/`).
//!
//! Resolution order:
//!
//! 1. A [`ConfigValueKind::Path`] value was marked by one of the
//!    merge-time mechanisms (an explicit `!path`, the
//!    `FORMAT_PATH_KEYS` marking in `project/format_paths.rs`, or an
//!    extension rebase) and already holds the **pivot form**
//!    ([`pivot_form`]): the path relative to the project root with
//!    forward slashes, or an OS-absolute path when the file lies
//!    outside the project (a built-in extension extracted to a temp
//!    dir; `adjust_paths_to_project_root`, bd-9z2258af). A relative
//!    value is joined onto the project root; a rooted one is used as
//!    written. The pivot does not vary with the consuming document, so
//!    a value's spelling is stable for cache keys, `get-config` and Lua
//!    (bd-okqrjh8v; before Phase 1b the stored form was
//!    document-relative, bd-79c4do6g).
//! 2. Anything with a URL scheme or a `//host` prefix is external
//!    ([`quarto_util::is_external_url`], the sole classifier) and is
//!    returned as [`ResolvedConfigPath::External`].
//! 3. A leading `/` anchors at the project root.
//! 4. An OS-absolute path (a Windows drive or UNC prefix; on Unix this
//!    case is already step 3) is used as written.
//! 5. Otherwise the base is the declaring file's directory, recovered
//!    from the value's `SourceInfo` ([`declaring_dir`]): the registered
//!    file's directory, except for an extension manifest's **project
//!    fragment** (`contributes.project`, `contributes.metadata.project`),
//!    which is `_quarto.yml` content and resolves against the project
//!    root. The fragment is a *virtual file* of its own: at config time
//!    its spans are re-keyed to [`project_fragment_file_id`]
//!    ([`rekey_project_fragment`]) and the merge registers that id with
//!    the manifest's path and content, so diagnostics still render into
//!    `_extension.yml` while [`file_base_dir`] knows the base from the
//!    id alone (bd-0x1znhgb; before this `resolve_metadata_path`
//!    special-cased the manifest's file name, bd-74ktvzv7, which was
//!    wrong for the manifest's other fragments). When there is no usable
//!    span (a programmatic or runtime-layer value, a file not registered
//!    in the context, no context at all) the base is the **document
//!    directory**, which is what every consumer assumed before this
//!    module existed (plan decision 2, 2026-10-28).
//!
//! The result is normalized lexically (`.`/`..` collapsed) and never
//! canonicalized: the file may not exist, and a missing-file diagnostic
//! should name the path the author's declaration resolves to.
//!
//! [`resolve_config_file`] layers one policy on top, for keys whose
//! namespace mixes file paths with other names and whose consumer
//! therefore probes for existence anyway (`filters`: extension names;
//! `theme`: built-in theme names): the rule-2 candidate when it exists,
//! else — for a leading-`/` string — the legacy OS-absolute reading
//! when *that* exists, else the rule-2 candidate so a diagnostic names
//! it. This is the same effective behaviour the merge-time marking has
//! for its existence-driven keys (an unmarked `/x` falls through to the
//! consumer as OS-absolute), and it keeps Quarto 1's escape hatch of an
//! absolute path to a filter outside the project working.

use std::path::{Path, PathBuf};

use quarto_pandoc_types::{ConfigValue, ConfigValueKind};
use quarto_source_map::{FileId, SourceContext, SourceInfo};

/// What a resolver call needs to know about where it is being called
/// from. Build one per document; it is cheap to construct.
#[derive(Clone, Copy)]
pub struct PathCtx<'a> {
    /// The document's source context, with the YAML layer files
    /// registered (`None` degrades every value to the document-dir
    /// fallback — today's behaviour).
    pub source_context: Option<&'a SourceContext>,
    /// The project root (the anchor for a leading `/`). For a
    /// single-file render, the document's directory.
    pub project_dir: &'a Path,
    /// The consuming document's directory: the base for marked
    /// (`Path`-kind) values and the fallback when provenance is absent.
    pub document_dir: &'a Path,
}

impl<'a> PathCtx<'a> {
    /// The context for resolving paths read from a document's merged
    /// metadata: the document's own directory (its project's directory
    /// when the input has no parent) is the fallback and marked-value
    /// base. The usual way to build one inside a stage or transform:
    ///
    /// ```ignore
    /// let path_ctx = PathCtx::for_document(Some(&doc.source_context), &ctx.project.dir, &ctx.document.input);
    /// ```
    pub fn for_document(
        source_context: Option<&'a SourceContext>,
        project_dir: &'a Path,
        input: &'a Path,
    ) -> Self {
        PathCtx {
            source_context,
            project_dir,
            document_dir: input.parent().unwrap_or(project_dir),
        }
    }
}

/// A config-authored path kept in its *unresolved* form so a parsed
/// type can carry it past the parse without choosing a base directory
/// (the `href_source` pattern of bd-qor9a, generalized): resolve it
/// where the `PathCtx` is known, with [`ConfigPath::resolve`].
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigPath {
    /// A `Path`-kind value: already rebased by the merge (step 1).
    Marked(String),
    /// A plain string with the span it was read from (steps 2–5).
    Raw { raw: String, source: SourceInfo },
}

impl ConfigPath {
    /// Capture a value's path string and provenance; `None` for a
    /// non-string value.
    pub fn from_value(value: &ConfigValue) -> Option<Self> {
        if let ConfigValueKind::Path(marked) = &value.value {
            return Some(ConfigPath::Marked(marked.clone()));
        }
        Some(ConfigPath::Raw {
            raw: value.as_plain_text()?,
            source: value.source_info.clone(),
        })
    }

    /// The string as the author wrote it (for diagnostics and keys).
    pub fn as_written(&self) -> &str {
        match self {
            ConfigPath::Marked(s) => s,
            ConfigPath::Raw { raw, .. } => raw,
        }
    }

    /// Resolve exactly as [`resolve_config_path`] would have resolved
    /// the value this was captured from.
    pub fn resolve(&self, ctx: &PathCtx<'_>) -> ResolvedConfigPath {
        match self {
            ConfigPath::Marked(marked) => resolve_marked(marked, ctx),
            ConfigPath::Raw { raw, source } => resolve_config_path_str(raw, source, ctx),
        }
    }
}

/// A `quarto_sass::BrandPathResolver` for the `brand:` key of `meta`:
/// quarto-sass hands back the path string it parsed out of the value,
/// and this resolves it against the file that declared the `brand:`
/// key (for a `{light:, dark:}` pair, the pair's own span: both halves
/// are written in the same file).
pub fn brand_path_resolver<'a>(
    meta: &'a ConfigValue,
    ctx: PathCtx<'a>,
) -> impl Fn(&Path) -> PathBuf + 'a {
    let source = meta.get("brand").map(|v| v.source_info.clone());
    move |path: &Path| {
        let raw = path.to_string_lossy();
        let resolved = match &source {
            Some(source) => resolve_config_path_str(&raw, source, &ctx),
            None => resolve_config_path_str(&raw, &SourceInfo::for_test(), &ctx),
        };
        resolved.file().unwrap_or_else(|| path.to_path_buf())
    }
}

/// The two terminal forms of a config-authored path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedConfigPath {
    /// A URL (RFC-3986 scheme or `//host/...`): not a filesystem path.
    External(String),
    /// A filesystem path, absolute whenever the inputs were, with
    /// `.`/`..` collapsed.
    File(PathBuf),
}

impl ResolvedConfigPath {
    /// The filesystem path, or `None` for an external URL.
    pub fn file(self) -> Option<PathBuf> {
        match self {
            ResolvedConfigPath::File(p) => Some(p),
            ResolvedConfigPath::External(_) => None,
        }
    }
}

/// Resolve a config value that denotes a path. Returns `None` when the
/// value carries no string (a map, an array, a non-string scalar).
pub fn resolve_config_path(value: &ConfigValue, ctx: &PathCtx<'_>) -> Option<ResolvedConfigPath> {
    if let ConfigValueKind::Path(marked) = &value.value {
        return Some(resolve_marked(marked, ctx));
    }
    let raw = value.as_plain_text()?;
    Some(resolve_config_path_str(&raw, &value.source_info, ctx))
}

/// Resolve a path string together with the `SourceInfo` of the value it
/// was read from. Prefer [`resolve_config_path`]; this entry point is for
/// callers that already split the two apart.
pub fn resolve_config_path_str(
    raw: &str,
    source: &SourceInfo,
    ctx: &PathCtx<'_>,
) -> ResolvedConfigPath {
    if quarto_util::is_external_url(raw) {
        return ResolvedConfigPath::External(raw.to_string());
    }
    if let Some(rest) = raw.strip_prefix('/') {
        return file(ctx.project_dir.join(rest));
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return file(path.to_path_buf());
    }
    let base = match declaring_dir(source, ctx) {
        Some(dir) => dir,
        None => {
            tracing::trace!(
                raw,
                document_dir = %ctx.document_dir.display(),
                "config path has no usable provenance; resolving against the document dir"
            );
            ctx.document_dir.to_path_buf()
        }
    };
    file(base.join(path))
}

/// A `Path`-kind value: already in pivot form (step 1 of the module
/// docs): project-root-relative, or OS-absolute for a file outside the
/// project.
fn resolve_marked(marked: &str, ctx: &PathCtx<'_>) -> ResolvedConfigPath {
    let path = Path::new(marked);
    if quarto_util::is_rooted(path) || path.is_absolute() {
        return file(path.to_path_buf());
    }
    file(ctx.project_dir.join(path))
}

/// The stored form of a marked path (`ConfigValueKind::Path`): `joined`
/// (the declaring directory joined with the authored path) normalized
/// lexically and expressed relative to `project_dir` with forward
/// slashes when it lies inside the project, else the normalized absolute
/// path. One spelling on every platform, so cache keys, `get-config`
/// output and the Lua-visible value do not depend on the OS.
///
/// `..` is collapsed lexically here, as `resolve_config_path` collapses
/// it again at resolution; a `link/..` whose `link` is a symlink to a
/// different directory therefore names the lexical parent, not the
/// symlink target's parent. Whether the file is inside the project is
/// decided on the normalized path, so a `..` that escapes the project is
/// seen as outside it (bd-9z2258af).
pub fn pivot_form(joined: &Path, project_dir: &Path) -> String {
    let project_dir = quarto_util::normalize_lexically(project_dir);
    let normalized = quarto_util::normalize_lexically(joined);
    match normalized.strip_prefix(&project_dir) {
        Ok(rel) if rel.as_os_str().is_empty() => ".".to_string(),
        Ok(rel) => quarto_util::to_forward_slashes(rel),
        Err(_) => quarto_util::to_forward_slashes(&normalized),
    }
}

/// A resolved file plus whether it exists, for consumers that probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCandidate {
    pub path: PathBuf,
    pub exists: bool,
}

/// [`resolve_config_path`] with the existence policy described in the
/// module docs, for keys whose strings are only sometimes paths.
/// Returns `None` for a URL or a non-string value.
pub fn resolve_config_file(
    value: &ConfigValue,
    ctx: &PathCtx<'_>,
    runtime: &dyn quarto_system_runtime::SystemRuntime,
) -> Option<FileCandidate> {
    resolve_config_file_with(value, ctx, |p| runtime.is_file(p).unwrap_or(false))
}

/// [`resolve_config_file`] with an explicit existence probe, for callers
/// without a `SystemRuntime` in reach (transforms) and for tests.
pub fn resolve_config_file_with(
    value: &ConfigValue,
    ctx: &PathCtx<'_>,
    is_file: impl Fn(&Path) -> bool,
) -> Option<FileCandidate> {
    let config_path = ConfigPath::from_value(value)?;
    config_path.resolve_file_with(ctx, is_file)
}

impl ConfigPath {
    /// [`resolve_config_file_with`] for a captured path.
    pub fn resolve_file_with(
        &self,
        ctx: &PathCtx<'_>,
        is_file: impl Fn(&Path) -> bool,
    ) -> Option<FileCandidate> {
        let candidate = self.resolve(ctx).file()?;
        if is_file(&candidate) {
            return Some(FileCandidate {
                path: candidate,
                exists: true,
            });
        }
        // Legacy reading of an authored leading `/`: OS-absolute. Only
        // for plain strings (a marked value is already OS-absolute when
        // rooted).
        if let ConfigPath::Raw { raw, .. } = self
            && raw.starts_with('/')
        {
            let os_absolute = quarto_util::normalize_lexically(Path::new(raw));
            if is_file(&os_absolute) {
                tracing::trace!(
                    raw,
                    "leading-`/` config path does not exist under the project root; using the OS-absolute reading"
                );
                return Some(FileCandidate {
                    path: os_absolute,
                    exists: true,
                });
            }
        }
        Some(FileCandidate {
            path: candidate,
            exists: false,
        })
    }
}

fn file(path: PathBuf) -> ResolvedConfigPath {
    ResolvedConfigPath::File(quarto_util::normalize_lexically(&path))
}

/// The [`FileId`] of an extension manifest's **project fragment**: the
/// `contributes.project` / `contributes.metadata.project` content of
/// `manifest`, which is merged into the project config and means what
/// it would mean in `_quarto.yml`.
///
/// The fragment is registered as a virtual file with the manifest's
/// path and content (so a diagnostic anchored in it renders a snippet
/// into `_extension.yml`), but under this id rather than the
/// manifest's own `quarto_yaml::file_id_for_filename`, so that
/// [`file_base_dir`] can tell the fragment's values (base: the project
/// root) from the manifest's `contributes.metadata` /
/// `contributes.formats` values (base: the extension directory).
pub fn project_fragment_file_id(manifest: &Path) -> FileId {
    quarto_yaml::file_id_for_filename(&format!(
        "{}#project",
        quarto_util::to_forward_slashes(manifest)
    ))
}

/// Re-key every span in a project fragment from the manifest's own
/// id to [`project_fragment_file_id`]. Called once, when the fragment
/// is merged into the project config (`project/mod.rs`).
pub fn rekey_project_fragment(fragment: &mut ConfigValue, manifest: &Path) {
    let from = quarto_yaml::file_id_for_filename(&manifest.to_string_lossy());
    let to = project_fragment_file_id(manifest);
    if from == to {
        return;
    }
    let map = move |id: FileId| if id == from { to } else { id };
    rekey_config_value(fragment, &map);
}

fn rekey_config_value(value: &mut ConfigValue, map: &impl Fn(FileId) -> FileId) {
    value.source_info.remap_file_ids(map);
    match &mut value.value {
        ConfigValueKind::Map(entries) => {
            for entry in entries {
                entry.key_source.remap_file_ids(map);
                rekey_config_value(&mut entry.value, map);
            }
        }
        ConfigValueKind::Array(items) => {
            for item in items {
                rekey_config_value(item, map);
            }
        }
        _ => {}
    }
}

/// The directory the values of the registered file `id` (at `path`)
/// resolve against: the project root for a project fragment
/// ([`project_fragment_file_id`]), else the file's own directory.
///
/// `None` when the path has no usable (rooted) directory — an
/// in-memory `<anonymous>` document, a bare file name.
pub fn file_base_dir(id: FileId, path: &str, project_dir: &Path) -> Option<PathBuf> {
    let path = Path::new(path);
    if id == project_fragment_file_id(path) {
        return Some(project_dir.to_path_buf());
    }
    let dir = path.parent()?;
    if dir.as_os_str().is_empty() || !quarto_util::is_rooted(dir) {
        return None;
    }
    Some(dir.to_path_buf())
}

/// The directory a value resolves against, from the file its span
/// points into ([`file_base_dir`]): the file's directory, or the
/// project root for an extension's project fragment.
///
/// Returns `None` for programmatic sentinels (`config-default`,
/// `programmatic-config`, `unknown`), for spans with no single byte
/// range (`Concat`, anchor-less `Generated`), for a `FileId` the
/// context does not know, and for a registered path that is not
/// rooted (an in-memory `<anonymous>` document, a bare file name): in
/// each case the caller falls back to the document directory.
pub fn declaring_dir(source: &SourceInfo, ctx: &PathCtx<'_>) -> Option<PathBuf> {
    if let SourceInfo::Generated(g) = source
        && g.by.is_programmatic_sentinel()
    {
        return None;
    }
    let (file_id, _, _) = source.resolve_byte_range()?;
    let file_id = FileId(file_id);
    let source_file = ctx.source_context?.get_file(file_id)?;
    file_base_dir(file_id, &source_file.path, ctx.project_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::By;
    use std::sync::Arc;
    use yaml_rust2::Yaml;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    /// A context whose document is `/project/docs/page.qmd` and whose
    /// `_quarto.yml` and `docs/_metadata.yml` are registered.
    struct Fixture {
        sc: SourceContext,
        quarto_yml: FileId,
        metadata_yml: FileId,
        document: FileId,
        anonymous: FileId,
    }

    fn fixture() -> Fixture {
        let mut sc = SourceContext::new();
        let document = sc.add_file("/project/docs/page.qmd".into(), Some(String::new()));
        let quarto_yml = sc.add_file("/project/_quarto.yml".into(), Some(String::new()));
        let metadata_yml = sc.add_file("/project/docs/_metadata.yml".into(), Some(String::new()));
        let anonymous = sc.add_file("<anonymous>".into(), Some(String::new()));
        Fixture {
            sc,
            quarto_yml,
            metadata_yml,
            document,
            anonymous,
        }
    }

    /// Owned base dirs so a `PathCtx` can borrow them for a whole test.
    struct Dirs {
        project: PathBuf,
        document: PathBuf,
    }

    fn dirs(project: &str, document: &str) -> Dirs {
        Dirs {
            project: p(project),
            document: p(document),
        }
    }

    fn ctx<'a>(f: &'a Fixture, d: &'a Dirs) -> PathCtx<'a> {
        PathCtx {
            source_context: Some(&f.sc),
            project_dir: &d.project,
            document_dir: &d.document,
        }
    }

    fn span(file_id: FileId) -> SourceInfo {
        SourceInfo::Original {
            file_id,
            start_offset: 0,
            end_offset: 1,
        }
    }

    fn scalar(s: &str, source: SourceInfo) -> ConfigValue {
        ConfigValue::new_scalar(Yaml::String(s.into()), source)
    }

    fn resolve(raw: &str, source: SourceInfo, ctx: &PathCtx<'_>) -> ResolvedConfigPath {
        resolve_config_path(&scalar(raw, source), ctx).expect("string value")
    }

    #[test]
    fn quarto_yml_value_resolves_against_the_project_root() {
        let f = fixture();
        let d = dirs("/project", "/project/docs");
        let c = ctx(&f, &d);
        assert_eq!(
            resolve("hello.lua", span(f.quarto_yml), &c),
            ResolvedConfigPath::File(p("/project/hello.lua"))
        );
    }

    #[test]
    fn metadata_yml_value_resolves_against_its_own_dir() {
        let f = fixture();
        let d = dirs("/project", "/project/docs/deep");
        let c = ctx(&f, &d);
        assert_eq!(
            resolve("../shared.lua", span(f.metadata_yml), &c),
            ResolvedConfigPath::File(p("/project/shared.lua"))
        );
    }

    #[test]
    fn document_value_resolves_against_the_document_dir() {
        let f = fixture();
        let d = dirs("/project", "/project/docs");
        let c = ctx(&f, &d);
        assert_eq!(
            resolve("./local.lua", span(f.document), &c),
            ResolvedConfigPath::File(p("/project/docs/local.lua"))
        );
    }

    #[test]
    fn substring_span_resolves_through_its_parent() {
        let f = fixture();
        let d = dirs("/project", "/project/docs");
        let c = ctx(&f, &d);
        let nested = SourceInfo::Substring {
            parent: Arc::new(span(f.quarto_yml)),
            start_offset: 0,
            end_offset: 1,
        };
        assert_eq!(
            resolve("hello.lua", nested, &c),
            ResolvedConfigPath::File(p("/project/hello.lua"))
        );
    }

    #[test]
    fn leading_slash_anchors_at_the_project_root_for_every_declarer() {
        let f = fixture();
        let d = dirs("/project", "/project/docs");
        let c = ctx(&f, &d);
        for source in [span(f.quarto_yml), span(f.document), SourceInfo::for_test()] {
            assert_eq!(
                resolve("/hello.lua", source, &c),
                ResolvedConfigPath::File(p("/project/hello.lua"))
            );
        }
    }

    #[test]
    fn external_urls_are_classified_not_joined() {
        let f = fixture();
        let d = dirs("/project", "/project/docs");
        let c = ctx(&f, &d);
        for url in [
            "https://example.com/x.lua",
            "//cdn/x.lua",
            "data:text/plain,x",
        ] {
            assert_eq!(
                resolve(url, span(f.quarto_yml), &c),
                ResolvedConfigPath::External(url.to_string())
            );
        }
    }

    /// bd-0x1znhgb: an extension manifest's project fragment is a
    /// virtual file of its own — same path and bytes, its own id — whose
    /// values resolve against the project root, while the manifest's
    /// other fragments keep resolving against the extension directory.
    #[test]
    fn project_fragment_resolves_against_the_project_root() {
        let f = fixture();
        let d = dirs("/project", "/project/docs/deep");
        let c = ctx(&f, &d);
        let manifest = "/project/_extensions/x/_extension.yml";
        let plain = quarto_yaml::file_id_for_filename(manifest);
        let fragment = project_fragment_file_id(Path::new(manifest));
        assert_ne!(plain, fragment);
        let mut sc = f.sc.clone();
        sc.add_file_with_id(plain, manifest.to_string(), Some(String::new()));
        sc.add_file_with_id(fragment, manifest.to_string(), Some(String::new()));
        let c = PathCtx {
            source_context: Some(&sc),
            ..c
        };
        assert_eq!(
            resolve("index.qmd", span(fragment), &c),
            ResolvedConfigPath::File(p("/project/index.qmd")),
            "a project-fragment value means what it would in _quarto.yml"
        );
        assert_eq!(
            resolve("scripts/x.lua", span(plain), &c),
            ResolvedConfigPath::File(p("/project/_extensions/x/scripts/x.lua")),
            "a contributes.metadata value names a bundled file"
        );
    }

    #[test]
    fn rekey_project_fragment_moves_every_span_to_the_fragment_id() {
        let manifest = Path::new("/project/_extensions/x/_extension.yml");
        let plain = quarto_yaml::file_id_for_filename(&manifest.to_string_lossy());
        let other = FileId(7);
        let leaf = |id: FileId| ConfigValue::new_string("v", SourceInfo::original(id, 0, 1));
        let mut value = ConfigValue::new_map(
            vec![
                quarto_pandoc_types::ConfigMapEntry {
                    key: "a".into(),
                    key_source: SourceInfo::original(plain, 0, 1),
                    value: leaf(plain),
                },
                quarto_pandoc_types::ConfigMapEntry {
                    key: "b".into(),
                    key_source: SourceInfo::original(other, 0, 1),
                    value: ConfigValue {
                        value: ConfigValueKind::Array(vec![leaf(plain), leaf(other)]),
                        source_info: SourceInfo::original(plain, 0, 1),
                        merge_op: Default::default(),
                    },
                },
            ],
            SourceInfo::original(plain, 0, 1),
        );
        rekey_project_fragment(&mut value, manifest);
        let fragment = project_fragment_file_id(manifest);
        let id_of = |v: &ConfigValue| v.source_info.root_file_id().unwrap();
        assert_eq!(id_of(&value), fragment);
        assert_eq!(id_of(value.get("a").unwrap()), fragment);
        let ConfigValueKind::Map(entries) = &value.value else {
            panic!("map")
        };
        assert_eq!(entries[0].key_source.root_file_id(), Some(fragment));
        assert_eq!(
            entries[1].key_source.root_file_id(),
            Some(other),
            "other files untouched"
        );
        let ConfigValueKind::Array(items) = &value.get("b").unwrap().value else {
            panic!("array")
        };
        assert_eq!(id_of(&items[0]), fragment);
        assert_eq!(id_of(&items[1]), other);
    }

    #[test]
    fn programmatic_sentinel_falls_back_to_the_document_dir() {
        let f = fixture();
        let d = dirs("/project", "/project/docs");
        let c = ctx(&f, &d);
        for by in [
            By::config_default(),
            By::programmatic_config(),
            By::unknown(),
        ] {
            let source = SourceInfo::generated(by);
            assert_eq!(
                resolve("x.lua", source, &c),
                ResolvedConfigPath::File(p("/project/docs/x.lua"))
            );
        }
    }

    #[test]
    fn unregistered_file_id_falls_back_to_the_document_dir() {
        let f = fixture();
        let d = dirs("/project", "/project/docs");
        let c = ctx(&f, &d);
        assert_eq!(
            resolve("x.lua", span(FileId(usize::MAX - 7)), &c),
            ResolvedConfigPath::File(p("/project/docs/x.lua"))
        );
    }

    #[test]
    fn unrooted_registered_path_falls_back_to_the_document_dir() {
        let f = fixture();
        let d = dirs("/project", "/project/docs");
        let c = ctx(&f, &d);
        assert_eq!(
            resolve("x.lua", span(f.anonymous), &c),
            ResolvedConfigPath::File(p("/project/docs/x.lua"))
        );
    }

    #[test]
    fn no_source_context_falls_back_to_the_document_dir() {
        let f = fixture();
        let d = dirs("/project", "/project/docs");
        let c = PathCtx {
            source_context: None,
            project_dir: &d.project,
            document_dir: &d.document,
        };
        assert_eq!(
            resolve("x.lua", span(f.quarto_yml), &c),
            ResolvedConfigPath::File(p("/project/docs/x.lua"))
        );
    }

    #[test]
    fn marked_path_value_is_joined_onto_the_project_root() {
        let f = fixture();
        let d = dirs("/project", "/project/docs");
        let c = ctx(&f, &d);
        // Marked by the merge in pivot form (project-root-relative); its
        // span still points at the declaring file, which must be ignored.
        let marked = ConfigValue {
            value: ConfigValueKind::Path("lib/hello.lua".into()),
            source_info: span(f.document),
            merge_op: Default::default(),
        };
        assert_eq!(
            resolve_config_path(&marked, &c),
            Some(ResolvedConfigPath::File(p("/project/lib/hello.lua")))
        );
    }

    #[test]
    fn pivot_form_is_normalized_project_relative_inside_and_absolute_outside() {
        let project = p("/project");
        assert_eq!(
            pivot_form(&p("/project/styles.css"), &project),
            "styles.css"
        );
        assert_eq!(
            pivot_form(&p("/project/docs/../shared/a.css"), &project),
            "shared/a.css"
        );
        assert_eq!(
            pivot_form(&p("/project/docs/../../escaped.css"), &project),
            "/escaped.css"
        );
        assert_eq!(pivot_form(&p("/tmp/ext/f.lua"), &project), "/tmp/ext/f.lua");
        assert_eq!(
            pivot_form(&p("/project/docs/./local.css"), &project),
            "docs/local.css"
        );
        assert_eq!(pivot_form(&p("/project"), &project), ".");
    }

    #[test]
    fn marked_rooted_value_is_used_as_written() {
        let f = fixture();
        let d = dirs("/project", "/project/docs");
        let c = ctx(&f, &d);
        // A built-in extension extracted outside the project is rebased
        // to an OS-absolute `Path` by `adjust_paths_to_document_dir`.
        let abs = ConfigValue {
            value: ConfigValueKind::Path("/tmp/ext/f.lua".into()),
            source_info: SourceInfo::for_test(),
            merge_op: Default::default(),
        };
        assert_eq!(
            resolve_config_path(&abs, &c),
            Some(ResolvedConfigPath::File(p("/tmp/ext/f.lua")))
        );
    }

    /// Runtime whose `is_file` answers from a fixed set.
    struct FileSet(std::collections::HashSet<PathBuf>);

    impl FileSet {
        fn of(paths: &[&str]) -> Self {
            Self(paths.iter().map(|s| p(s)).collect())
        }
    }

    #[async_trait::async_trait]
    impl quarto_system_runtime::SystemRuntime for FileSet {
        fn file_read(
            &self,
            _path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn file_write(
            &self,
            _path: &std::path::Path,
            _contents: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn path_exists(
            &self,
            path: &std::path::Path,
            _kind: Option<quarto_system_runtime::PathKind>,
        ) -> quarto_system_runtime::RuntimeResult<bool> {
            Ok(self.0.contains(path))
        }
        fn canonicalize(
            &self,
            path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(path.to_path_buf())
        }
        fn path_metadata(
            &self,
            _path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata> {
            unimplemented!()
        }
        fn file_copy(
            &self,
            _src: &std::path::Path,
            _dst: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn path_rename(
            &self,
            _old: &std::path::Path,
            _new: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn file_remove(&self, _path: &std::path::Path) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_create(
            &self,
            _path: &std::path::Path,
            _recursive: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_remove(
            &self,
            _path: &std::path::Path,
            _recursive: bool,
        ) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn dir_list(
            &self,
            _path: &std::path::Path,
        ) -> quarto_system_runtime::RuntimeResult<Vec<PathBuf>> {
            Ok(vec![])
        }
        fn cwd(&self) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/"))
        }
        fn temp_dir(
            &self,
            _template: &str,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::TempDir> {
            Ok(quarto_system_runtime::TempDir::new(PathBuf::from(
                "/nonexistent/q2-mock-temp-dir",
            )))
        }
        fn exec_pipe(
            &self,
            _command: &str,
            _args: &[&str],
            _stdin: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            Ok(vec![])
        }
        fn exec_command(
            &self,
            _command: &str,
            _args: &[&str],
            _stdin: Option<&[u8]>,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput> {
            Ok(quarto_system_runtime::CommandOutput {
                code: 0,
                stdout: vec![],
                stderr: vec![],
            })
        }
        fn env_get(&self, _name: &str) -> quarto_system_runtime::RuntimeResult<Option<String>> {
            Ok(None)
        }
        fn env_all(
            &self,
        ) -> quarto_system_runtime::RuntimeResult<std::collections::HashMap<String, String>>
        {
            Ok(std::collections::HashMap::new())
        }
        async fn fetch_url(
            &self,
            _url: &str,
        ) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
            Err(quarto_system_runtime::RuntimeError::NotSupported(
                "test".to_string(),
            ))
        }
        fn os_name(&self) -> &'static str {
            "test"
        }
        fn arch(&self) -> &'static str {
            "test"
        }
        fn cpu_time(&self) -> quarto_system_runtime::RuntimeResult<u64> {
            Ok(0)
        }
        fn xdg_dir(
            &self,
            _kind: quarto_system_runtime::XdgDirKind,
            _subpath: Option<&std::path::Path>,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            Ok(PathBuf::from("/xdg"))
        }
        fn stdout_write(&self, _data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
        fn stderr_write(&self, _data: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            Ok(())
        }
    }

    #[test]
    fn resolve_config_file_prefers_the_project_root_reading() {
        let f = fixture();
        let d = dirs("/project", "/project/docs");
        let c = ctx(&f, &d);
        let rt = FileSet::of(&["/project/hello.lua", "/hello.lua"]);
        assert_eq!(
            resolve_config_file(&scalar("/hello.lua", span(f.quarto_yml)), &c, &rt),
            Some(FileCandidate {
                path: p("/project/hello.lua"),
                exists: true
            })
        );
    }

    #[test]
    fn resolve_config_file_falls_back_to_the_os_absolute_reading() {
        let f = fixture();
        let d = dirs("/project", "/project/docs");
        let c = ctx(&f, &d);
        let rt = FileSet::of(&["/opt/filters/shared.lua"]);
        assert_eq!(
            resolve_config_file(
                &scalar("/opt/filters/shared.lua", span(f.quarto_yml)),
                &c,
                &rt
            ),
            Some(FileCandidate {
                path: p("/opt/filters/shared.lua"),
                exists: true
            })
        );
    }

    #[test]
    fn resolve_config_file_reports_the_rule_2_candidate_when_nothing_exists() {
        let f = fixture();
        let d = dirs("/project", "/project/docs");
        let c = ctx(&f, &d);
        let rt = FileSet::of(&[]);
        assert_eq!(
            resolve_config_file(&scalar("/nope.lua", span(f.quarto_yml)), &c, &rt),
            Some(FileCandidate {
                path: p("/project/nope.lua"),
                exists: false
            })
        );
        assert_eq!(
            resolve_config_file(&scalar("nope.lua", span(f.quarto_yml)), &c, &rt),
            Some(FileCandidate {
                path: p("/project/nope.lua"),
                exists: false
            })
        );
    }

    #[test]
    fn non_string_values_resolve_to_none() {
        let f = fixture();
        let d = dirs("/project", "/project/docs");
        let c = ctx(&f, &d);
        let map = ConfigValue::new_map(vec![], SourceInfo::for_test());
        assert_eq!(resolve_config_path(&map, &c), None);
    }

    #[test]
    fn result_is_lexically_normalized() {
        let f = fixture();
        let d = dirs("/project", "/project/docs");
        let c = ctx(&f, &d);
        assert_eq!(
            resolve("./a/../b/./x.lua", span(f.quarto_yml), &c),
            ResolvedConfigPath::File(p("/project/b/x.lua"))
        );
    }

    #[test]
    fn for_document_falls_back_to_the_project_dir_without_a_parent() {
        let f = fixture();
        let project = p("/project");
        let c = PathCtx::for_document(Some(&f.sc), &project, Path::new("page.qmd"));
        assert_eq!(c.document_dir, Path::new(""));
        let c = PathCtx::for_document(Some(&f.sc), &project, Path::new("/project/docs/page.qmd"));
        assert_eq!(c.document_dir, Path::new("/project/docs"));
    }

    #[test]
    fn config_path_round_trips_both_forms() {
        let f = fixture();
        let d = dirs("/project", "/project/docs");
        let c = ctx(&f, &d);
        let raw = ConfigPath::from_value(&scalar("x.lua", span(f.quarto_yml))).unwrap();
        assert_eq!(raw.as_written(), "x.lua");
        assert_eq!(
            raw.resolve(&c),
            ResolvedConfigPath::File(p("/project/x.lua"))
        );
        let marked = ConfigPath::from_value(&ConfigValue {
            value: ConfigValueKind::Path("x.lua".into()),
            source_info: span(f.quarto_yml),
            merge_op: Default::default(),
        })
        .unwrap();
        assert_eq!(marked, ConfigPath::Marked("x.lua".into()));
        assert_eq!(
            marked.resolve(&c),
            ResolvedConfigPath::File(p("/project/x.lua"))
        );
        assert_eq!(
            ConfigPath::from_value(&ConfigValue::new_map(vec![], SourceInfo::for_test())),
            None
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_drive_paths_are_used_as_written() {
        let f = fixture();
        let d = dirs("C:\\project", "C:\\project\\docs");
        let c = ctx(&f, &d);
        assert_eq!(
            resolve("D:\\filters\\x.lua", span(f.quarto_yml), &c),
            ResolvedConfigPath::File(p("D:\\filters\\x.lua"))
        );
    }
}
