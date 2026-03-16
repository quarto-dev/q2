# Investigation: Extension Path Resolution — Absolute vs `!path`

**Created**: 2026-03-16
**Status**: Complete
**Related**: Phase 4 (`2026-03-16-extensions-phase4-templates-partials.md`)

## Question

Currently `read_extension()` resolves filter and shortcode paths to absolute
during parsing (stored in `ExtensionFilter.path` and `Contributes.shortcodes`).
This is a different pattern from the `!path`/`ConfigValueKind::Path` approach
used by project/directory metadata, where paths stay relative and are rebased
by `adjust_paths_to_document_dir()` during metadata merge.

For Phase 4 (templates/partials), we chose to add `adjust_paths_to_document_dir`
to the extension layer in `MetadataMergeStage` rather than resolve to absolute.
This raises the question: should filters/shortcodes also use the `!path` pattern?
Or is the absolute-path approach correct for extension-owned resources?

## Investigation Steps

- [x] Trace git history of `read_extension()`
- [x] Trace git history of `adjust_paths_to_document_dir`
- [x] Check conceptual difference between execution paths vs metadata paths
- [x] Check TS Quarto for comparison

## Findings

### Git history

- `read_extension()` was introduced in `4caa9f57` with absolute path resolution
  from the start. No alternative was considered; `ext_dir.join(path_str)` was
  the original design for filters and shortcodes.
- `adjust_paths_to_document_dir()` was introduced later for `_metadata.yml`
  directory metadata support. It handles `ConfigValueKind::Path` values by
  rebasing relative paths from the metadata source dir to the document dir.

### Conceptual difference: execution vs metadata

There is a meaningful architectural distinction:

**Filters/shortcodes are executable resources.** Their paths live in dedicated
typed structs (`ExtensionFilter.path: PathBuf`, `Contributes.shortcodes:
Vec<PathBuf>`). They are consumed directly by the filter engine
(`pampa::unified_filter::apply_filters`), which needs absolute paths to locate
files on disk. These paths **never flow through the config merge pipeline**.

**Templates/partials are metadata values.** They appear as string values inside
`Contributes.formats: HashMap<String, ConfigValue>`. These values flow through
the `MergedConfig` merge pipeline in `MetadataMergeStage`, where they get
layered with project, directory, and document metadata. The `!path` /
`adjust_paths_to_document_dir` system exists precisely for this: paths that are
relative in their source context but need rebasing when merged into a different
context.

### TS Quarto comparison

TS Quarto uses the **exact same two-pattern approach**:

- **Filters**: Resolved to absolute immediately during `readExtension()` via
  `resolveFilterPath()` (`extension.ts:1113-1142`). The comment reads:
  *"Filters are expected to be absolute"*.
- **Shortcodes**: Same pattern via `resolveShortcodePath()` (`extension.ts:1064`).
- **Templates/partials**: Resolved **later** via `resolveTemplatePartialPaths()`
  (`template.ts:71-93`), which takes `inputDir` and `project` context and
  resolves paths relative to those directories. Called during rendering
  (`pandoc.ts:728`), not during extension loading.

## Conclusion

**"Absolute is correct for execution, `!path` for metadata."**

The two patterns coexist intentionally, and TS Quarto confirms this split:

| Resource type      | Storage              | Resolution timing     | Mechanism                        |
|--------------------|----------------------|-----------------------|----------------------------------|
| Filters/shortcodes | `PathBuf` in struct  | At parse time         | `ext_dir.join(path)` in `read_extension()` |
| Templates/partials | `ConfigValue` (!path)| During metadata merge | `adjust_paths_to_document_dir()` |

**No unification needed. No follow-up migration task.**
