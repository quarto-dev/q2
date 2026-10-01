# Windows: mixed plain/verbatim spellings in prefix comparisons (bd-l0eemakd)

Found by roborev 3019 (finding 1) on the layer 3 branch `bugfix/bd-1klbq2zd-dunce-seam`. Accepted as a known limit of layer 3 on 2026-10-01; this note keeps the evidence for whoever fixes it.

## The defect

`quarto_system_runtime::canonicalize` calls `dunce::canonicalize`. On Windows it returns the plain `C:\…` form for short paths and keeps the verbatim `\\?\C:\…` form past `MAX_PATH` (260 UTF-16 units). Two paths canonicalized independently can therefore come back in different spellings even when one is under the other. `Path::starts_with` and `strip_prefix` treat the `C:` and `\\?\C:` prefixes as different components (checked with a compiled probe: `\\?\C:\project\a\doc.qmd` does not `starts_with` `C:\project`). `pathdiff::diff_paths` does not fail; it returns a `..`-led path. Before layer 3, `std::fs::canonicalize` returned the verbatim form for every path, so these comparisons matched.

It goes both ways: a short plain root with a long verbatim document, or a verbatim root (a project discovered from a long file path) with a short document that comes back plain.

Main cause: `render_to_file.rs:252` canonicalizes the input document apart from `project.dir` (`project/mod.rs:1998`). Every consumer of `document.input`, `doc.path`, the main-document `SourceContext` path, or `output_path` (when `output_dir == dir`) then compares against `project.dir` / `output_dir`.

## Why it is a known limit for now

Only paths over 260 units are affected, and Lua `io.open` already fails there on Windows (bd-9z2258af); other external tools were not checked. A fix spans ~41 sites plus a spelling-aware `diff_paths`, and without a lint any new raw comparison brings the bug back.

## At-risk sites (static sweep at `3241511b1`, Opus; central claim checked in the main session)

Prefix comparisons (`starts_with` / `strip_prefix`):

- `quarto-core/src/output_sink.rs:247, 265, 361`
- `quarto-core/src/project/mod.rs:217`; `project/orchestrator.rs:2376, 1161, 1321, 1701`
- `quarto-core/src/project_resources.rs:400, 463, 609, 849, 1306`
- `quarto-core/src/render_to_file.rs:595`; `render.rs:661`; `stage/context.rs:536`
- `quarto-core/src/stage/stages/document_profile.rs:165, 179`; `stage/stages/resource_report.rs:94`
- `quarto-core/src/transforms/navigation_active.rs:40`; `transforms/navigation_href.rs:805`; `transforms/book_cover_image.rs:115`
- `quarto-core/src/glob/provenance.rs:70`; `project/listing/feed/stage.rs:565`
- `quarto/src/commands/render.rs:371, 1341`; `preview_static.rs:220`; `publish.rs:343, 425`
- `quarto-preview/src/config.rs:410, 414`

`pathdiff::diff_paths` (wrong `..`-led result rather than a failure):

- `quarto-core/src/project/mod.rs:317`; `project/format_paths.rs:299`
- `quarto-core/src/transforms/format_css.rs:122, 135`
- `quarto-core/src/resource_resolver.rs:258, 284`
- `quarto-core/src/project/listing/post_render_upgrade/substitute.rs:345`

Other:

- `quarto-core/src/engine/capture_files.rs:132`: knitr supporting files come from R's own absolute path (plain), compared with the canonical document dir.
- Equality checks with the same hazard: `quarto/src/commands/render.rs:382` (lexical project file vs canonical target), `quarto-preview/src/static_mode/watch_policy.rs:97`.

The sweep classified 42 other production prefix comparisons as safe (both operands from the same walk, both from `std::fs::canonicalize`, relative, or VFS-only).

## Prototype helper and RED test

Not applied on any branch. `expand_literal_path_past_max_path_under_a_plain_root` fails on Windows with `OutOfProject` for a resource inside the project; the `canonical.rs` helper tests pass. Routing `project_resources.rs:609` alone is not enough for the test, since `:463` strips the prefix next.

```diff
diff --git a/crates/quarto-core/src/project_resources.rs b/crates/quarto-core/src/project_resources.rs
index e8a21203d..a6b996408 100644
--- a/crates/quarto-core/src/project_resources.rs
+++ b/crates/quarto-core/src/project_resources.rs
@@ -1411,6 +1411,41 @@ mod tests {
         );
     }
 
+    /// A resource past `MAX_PATH` canonicalizes verbatim (`\\?\C:\…`)
+    /// while the short project root canonicalizes plain; it is still
+    /// inside the project and keeps its project-relative output path.
+    #[cfg(windows)]
+    #[test]
+    fn expand_literal_path_past_max_path_under_a_plain_root() {
+        use std::os::windows::ffi::OsStrExt;
+        let temp = TempDir::new().unwrap();
+        let root = quarto_system_runtime::canonicalize(temp.path()).unwrap();
+        assert!(
+            !root.to_string_lossy().starts_with(r"\\?\"),
+            "test setup: the TEMP root must canonicalize plain: {}",
+            root.display()
+        );
+        let mut relative = PathBuf::new();
+        while root.join(&relative).as_os_str().encode_wide().count() <= 300 {
+            relative.push("a".repeat(40));
+        }
+        relative.push("data.txt");
+        touch(&root.join(&relative));
+        let pattern = crate::glob::path_to_forward_slashes(&relative);
+
+        let resolved = expand_patterns(
+            &root,
+            &root,
+            &[raw(&pattern)],
+            &rt(),
+            || ResourceOrigin::ProjectMetadata,
+            ResourceScope::Project,
+        )
+        .unwrap();
+        assert_eq!(resolved.len(), 1);
+        assert_eq!(resolved[0].output_relative, pattern);
+    }
+
     /// D3: `!` excludes. Before bd-mt7a6uc4 this entry fell through
     /// to the literal-path branch and **aborted the render** with
     /// "Declared resource '<root>/!data/secret.csv' does not exist on
diff --git a/crates/quarto-system-runtime/src/canonical.rs b/crates/quarto-system-runtime/src/canonical.rs
index 1b2b3b56a..c7b30effc 100644
--- a/crates/quarto-system-runtime/src/canonical.rs
+++ b/crates/quarto-system-runtime/src/canonical.rs
@@ -58,10 +58,86 @@ pub fn canonicalize_deepest_existing(path: &Path) -> PathBuf {
     result
 }
 
+/// Whether `path` lies under `base`, comparing canonical paths whatever
+/// their spelling.
+///
+/// [`canonicalize`] can return a plain root and a verbatim descendant
+/// (one past `MAX_PATH`), and `Path::starts_with` treats the `C:` and
+/// `\\?\C:` prefixes as different components. Use this, not
+/// `Path::starts_with`, when both operands are canonical paths.
+pub fn path_starts_with(path: &Path, base: &Path) -> bool {
+    comparison_form(path).starts_with(comparison_form(base))
+}
+
+/// `path` relative to `base`, comparing canonical paths whatever their
+/// spelling, like [`path_starts_with`].
+pub fn path_strip_prefix(path: &Path, base: &Path) -> Option<PathBuf> {
+    comparison_form(path)
+        .strip_prefix(comparison_form(base))
+        .ok()
+        .map(Path::to_path_buf)
+}
+
+/// `path` with a Windows verbatim disk or UNC prefix respelled plain. For
+/// comparisons only: the result may not name the file to every API.
+fn comparison_form(path: &Path) -> std::borrow::Cow<'_, Path> {
+    #[cfg(windows)]
+    {
+        use std::path::{Component, Prefix};
+        let mut components = path.components();
+        if let Some(Component::Prefix(prefix)) = components.next() {
+            let plain = match prefix.kind() {
+                Prefix::VerbatimDisk(drive) => format!("{}:", drive as char),
+                Prefix::VerbatimUNC(server, share) => format!(
+                    r"\\{}\{}",
+                    server.to_string_lossy(),
+                    share.to_string_lossy()
+                ),
+                _ => return std::borrow::Cow::Borrowed(path),
+            };
+            let mut out = PathBuf::from(plain);
+            out.push(components.as_path());
+            return std::borrow::Cow::Owned(out);
+        }
+    }
+    std::borrow::Cow::Borrowed(path)
+}
+
 #[cfg(test)]
 mod tests {
     use super::*;
 
+    #[cfg(windows)]
+    #[test]
+    fn verbatim_descendant_is_under_its_plain_root() {
+        let root = Path::new(r"C:\project");
+        let long = Path::new(r"\\?\C:\project\sub\doc.qmd");
+        assert!(path_starts_with(long, root));
+        assert!(path_starts_with(root, Path::new(r"\\?\c:\project")));
+        assert_eq!(
+            path_strip_prefix(long, root),
+            Some(PathBuf::from(r"sub\doc.qmd"))
+        );
+        assert!(path_starts_with(
+            Path::new(r"\\?\UNC\server\share\p\doc.qmd"),
+            Path::new(r"\\server\share\p")
+        ));
+        assert!(!path_starts_with(long, Path::new(r"D:\project")));
+        assert!(!path_starts_with(long, Path::new(r"C:\proj")));
+    }
+
+    #[test]
+    fn plain_paths_compare_as_std_does() {
+        let root = Path::new("/project");
+        assert!(path_starts_with(Path::new("/project/a/b"), root));
+        assert!(!path_starts_with(Path::new("/projectx/a"), root));
+        assert_eq!(
+            path_strip_prefix(Path::new("/project/a/b"), root),
+            Some(PathBuf::from("a/b"))
+        );
+        assert_eq!(path_strip_prefix(Path::new("/other"), root), None);
+    }
+
     #[test]
     fn deepest_existing_reappends_the_missing_tail() {
         let temp = tempfile::TempDir::new().unwrap();
diff --git a/crates/quarto-system-runtime/src/lib.rs b/crates/quarto-system-runtime/src/lib.rs
index 800e83315..1730de9cf 100644
--- a/crates/quarto-system-runtime/src/lib.rs
+++ b/crates/quarto-system-runtime/src/lib.rs
@@ -55,7 +55,9 @@ pub use traits::{
 };
 pub use vfs::{VfsWriteStats, VirtualFileSystem};
 
-pub use canonical::{canonicalize, canonicalize_deepest_existing};
+pub use canonical::{
+    canonicalize, canonicalize_deepest_existing, path_starts_with, path_strip_prefix,
+};
 
 // Re-export runtime implementations based on target
 #[cfg(not(target_arch = "wasm32"))]
```
