# RFU-6 failure details, run `20261117-021842-GWWIN`

## Tools on PATH

| tool | path | version |
|---|---|---|
| pandoc | C:\Users\GordonWoodhull\src\q2\target\windows-run\tools\pandoc\pandoc-3.12.1\pandoc.EXE | pandoc 3.12.1 |
| typst | C:\Users\GordonWoodhull\src\q2\target\windows-run\tools\typst\typst-x86_64-pc-windows-msvc\typst.EXE | typst 0.15.1 (9dfd3a08) |
| quarto | NOT FOUND |  |
| deno | NOT FOUND |  |
| node | C:\Program Files\nodejs\node.EXE | v24.21.0 |
| julia | NOT FOUND |  |
| Rscript | NOT FOUND |  |
| python | C:\Users\GordonWoodhull\AppData\Roaming\uv\python\cpython-3.12-windows-x86_64-none\python.EXE | Python 3.12.16 |
| python3 | C:\Users\GordonWoodhull\AppData\Local\Microsoft\WindowsApps\python3.EXE | Python was not found; run without arguments to install from the Microsoft Store, or disable this shortcut from Settings > Apps > Advanced app settings > App execution aliases. |
| uv | C:\Users\GordonWoodhull\src\q2\target\windows-run\tools\uv\uv.EXE | uv 0.13.7 (316c827e0 2026-11-03 x86_64-pc-windows-msvc) |
| git | C:\Program Files\Git\cmd\git.EXE | git version 2.47.1.windows.1 |
| sh | NOT FOUND |  |
| bash | C:\Users\GordonWoodhull\AppData\Local\Microsoft\WindowsApps\bash.EXE | GNU bash, version 5.2.21(1)-release (x86_64-pc-linux-gnu) |
| cmd | C:\WINDOWS\system32\cmd.EXE |  |

Environment variables of interest: TEMP=C:\Users\GORDON~1\AppData\Local\Temp, TMP=C:\Users\GORDON~1\AppData\Local\Temp

## i8 nextest

- Summary [ 988.235s] 10511 tests run: 10419 passed (6 slow, 2 leaky), 92 failed, 41 skipped

### Failures grouped by first panic message (92 failing tests with output, 84 causes)

- **4 x** `assertion 'left == right' failed: callouts: QUARTO_FILTER_PARAMS | left: Object {"active-filters": Object {"crossref": Bool(true), "jats_subarticle": Bool(false), "normalization": Bool(true)}, "crossr`
  - modules: quarto-core::integration pandoc_request_prepare (4)
  - example: `quarto-core::integration pandoc_request_prepare::docx_request_matches_the_recorded_native_run`
  - at: `crates\quarto-core\tests\integration\pandoc_request_prepare.rs:454`
- **3 x** `called 'Result::unwrap()' on an 'Err' value: StripPrefixError(())`
  - modules: quarto-core::integration pandoc_request_projects (2), quarto-core::integration pandoc_request_extension_filters (1)
  - example: `quarto-core::integration pandoc_request_extension_filters::real_pandoc_loads_the_filter_from_the_request_alone`
  - at: `crates\quarto-core\tests\integration\pandoc_request_extension_filters.rs:211`
- **2 x** `assertion 'left == right' failed | left: [] | right: ["data/public.csv", "data/secret.csv"]`
  - modules: quarto-core glob (2)
  - example: `quarto-core glob::expand::tests::expands_a_glob_over_the_runtime`
  - at: `crates\quarto-core\src\glob\expand.rs:381`
- **2 x** `assertion 'left == right' failed | left: Some("<tmp>\\a\\_metadata.yml") | right: Some("\\\\?\\<tmp>\\a\\_metadata.yml")`
  - modules: quarto-core metadata_layers (2)
  - example: `quarto-core metadata_layers::tests::deepest_directory_layer_comes_last_and_carries_its_file`
  - at: `crates\quarto-core\src\metadata_layers.rs:606`
- **2 x** `assertion failed: matches!(result, Err(Error::HubAlreadyRunning))`
  - modules: quarto-hub storage (2)
  - example: `quarto-hub storage::tests::test_storage_manager_prevents_double_lock`
  - at: `crates\quarto-hub\src\storage.rs:733`
- **1 x** `assertion 'left == right' failed | left: String("<tmp>/doc.qmd") | right: "//?/<tmp>/doc.qmd"`
  - modules: quarto::integration convert_positron_cli (1)
  - example: `quarto::integration convert_positron_cli::the_source_uri_default_is_the_documents_path_with_forward_slashes`
  - at: `crates\quarto\tests\integration\convert_positron_cli.rs:473`
- **1 x** `snapshot assertion for 'blog.stderr' failed in line 244`
  - modules: quarto::integration lmd_snapshots (1)
  - example: `quarto::integration lmd_snapshots::lmd_blog_project_renders_as_snapshotted`
  - at: `C:\Users\GordonWoodhull\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\insta-1.48.0\src\runtime.rs:719`
- **1 x** `snapshot assertion for 'desugar.stderr' failed in line 279`
  - modules: quarto::integration lmd_snapshots (1)
  - example: `quarto::integration lmd_snapshots::lmd_html_fixtures_render_as_snapshotted`
  - at: `C:\Users\GordonWoodhull\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\insta-1.48.0\src\runtime.rs:719`
- **1 x** `snapshot assertion for 'project.stderr' failed in line 261`
  - modules: quarto::integration lmd_snapshots (1)
  - example: `quarto::integration lmd_snapshots::lmd_project_renders_as_snapshotted`
  - at: `C:\Users\GordonWoodhull\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\insta-1.48.0\src\runtime.rs:719`
- **1 x** `snapshot assertion for 'errors.stderr' failed in line 293`
  - modules: quarto::integration lmd_snapshots (1)
  - example: `quarto::integration lmd_snapshots::lmd_fixtures_render_as_snapshotted`
  - at: `C:\Users\GordonWoodhull\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\insta-1.48.0\src\runtime.rs:719`
- **1 x** `assertion 'left == right' failed: quoted argument should arrive as a single argv entry | left: "--flag\r\ntwo words" | right: "--flag\ntwo words"`
  - modules: quarto::integration render_scripts_cli (1)
  - example: `quarto::integration render_scripts_cli::explicit_interpreter_command_line_with_args`
  - at: `crates\quarto\tests\integration\render_scripts_cli.rs:611`
- **1 x** `assertion 'left == right' failed: scripts must run in declaration order | left: "one\r\ntwo\r\n" | right: "one\ntwo\n"`
  - modules: quarto::integration render_scripts_cli (1)
  - example: `quarto::integration render_scripts_cli::list_form_runs_scripts_in_order`
  - at: `crates\quarto\tests\integration\render_scripts_cli.rs:579`
- **1 x** `assertion 'left == right' failed | left: "  sub/broken.qmd (revealjs) — would be skipped: failed to parse" | right: "  sub\\broken.qmd (revealjs) — would be skipped: failed to parse"`
  - modules: quarto::bin/q2 commands (1)
  - example: `quarto::bin/q2 commands::render_dry_run::tests::would_render_lines_are_project_relative`
  - at: `crates\quarto\src\commands\render_dry_run.rs:258`
- **1 x** `assertion failed: matches!(render_book_download(params()), Err(BookDownloadFailure::Busy))`
  - modules: quarto-core book_download (1)
  - example: `quarto-core book_download::tests::a_second_download_of_the_same_project_is_busy_until_the_first_ends`
  - at: `crates\quarto-core\src\book_download.rs:315`
- **1 x** `DEADLOCK DETECTED: test did not complete within timeout: Timeout`
  - modules: quarto-core engine (1)
  - example: `quarto-core engine::ts_process::tests::test_single_dial_invariant`
  - at: `crates\quarto-core\src\engine\ts_process.rs:2106`
- **1 x** `assertion 'left == right' failed | left: "C:/already/absolute" | right: "/already/absolute"`
  - modules: quarto-core extension (1)
  - example: `quarto-core extension::build::tests::file_aliases_resolve_relative_to_the_config_dir`
  - at: `crates\quarto-core\src\extension\build.rs:597`
- **1 x** `assertion 'left == right' failed | left: [] | right: ["img/logo.png"]`
  - modules: quarto-core glob (1)
  - example: `quarto-core glob::expand::tests::a_literal_file_expands_to_itself`
  - at: `crates\quarto-core\src\glob\expand.rs:429`
- **1 x** `assertion 'left == right' failed | left: [] | right: ["data/fig-1.png"]`
  - modules: quarto-core glob (1)
  - example: `quarto-core glob::expand::tests::character_classes_expand`
  - at: `crates\quarto-core\src\glob\expand.rs:408`
- **1 x** `assertion 'left == right' failed | left: [] | right: ["data/public.csv"]`
  - modules: quarto-core glob (1)
  - example: `quarto-core glob::expand::tests::negation_subtracts`
  - at: `crates\quarto-core\src\glob\expand.rs:397`
- **1 x** `assertion 'left == right' failed | left: 0 | right: 3`
  - modules: quarto-core glob (1)
  - example: `quarto-core glob::expand::tests::pruning_does_not_change_results`
  - at: `crates\quarto-core\src\glob\expand.rs:480`
- **1 x** `assertion 'left == right' failed | left: [] | right: ["data/nested/deep.csv", "data/public.csv", "data/secret.csv"]`
  - modules: quarto-core glob (1)
  - example: `quarto-core glob::expand::tests::recursive_glob_crosses_directories`
  - at: `crates\quarto-core\src\glob\expand.rs:389`
- **1 x** `assertion 'left == right' failed | left: "<tmp>\\a\\b" | right: "\\\\?\\<tmp>\\a\\b"`
  - modules: quarto-core metadata_layers (1)
  - example: `quarto-core metadata_layers::tests::layered_value_returns_the_highest_layer_that_sets_the_path`
  - at: `crates\quarto-core\src\metadata_layers.rs:503`
- **1 x** `assertion 'left == right' failed | left: Some("<tmp>\\_quarto.yml") | right: Some("\\\\?\\<tmp>\\_quarto.yml")`
  - modules: quarto-core metadata_layers (1)
  - example: `quarto-core metadata_layers::tests::project_layer_carries_the_config_file`
  - at: `crates\quarto-core\src\metadata_layers.rs:649`
- **1 x** `assertion 'left == right' failed: the pivot-form Path value resolves against the project root | left: ["--reference-doc", "\\doc\\custom.docx", "--default-image-extension", "png"] | right: ["--referen`
  - modules: quarto-core pandoc_filters (1)
  - example: `quarto-core pandoc_filters::format_defaults::tests::test_reference_doc_path_forwarded`
  - at: `crates\quarto-core\src\pandoc_filters\format_defaults.rs:957`
- **1 x** `assertion failed: std::path::Path::new(results_file).is_absolute()`
  - modules: quarto-core pandoc_filters (1)
  - example: `quarto-core pandoc_filters::params::tests::test_top_level_literals`
  - at: `crates\quarto-core\src\pandoc_filters\params.rs:783`
- **1 x** `assertion failed: out.path().starts_with(root.join("sub").join(PREVIEW_OUT_REL))`
  - modules: quarto-core preview_out (1)
  - example: `quarto-core preview_out::tests::a_project_without_a_config_file_uses_the_document_directory`
  - at: `crates\quarto-core\src\preview_out.rs:201`
- **1 x** `"<tmp>\\.quarto/preview-out\\fd1f3dfd-7217-444f-bc2f-<hash>"`
  - modules: quarto-core preview_out (1)
  - example: `quarto-core preview_out::tests::creates_under_the_discovered_project_and_cleans_up_on_drop`
  - at: `crates\quarto-core\src\preview_out.rs:182`
- **1 x** `assertion 'left == right' failed: typst | left: NoBook | right: Book`
  - modules: quarto-core project (1)
  - example: `quarto-core project::book::scope::tests::a_member_follows_the_request_on_a_consolidating_format`
  - at: `crates\quarto-core\src\project\book\scope\tests.rs:241`
- **1 x** `assertion 'left == right' failed: html Book | left: NoBook | right: Chapter`
  - modules: quarto-core project (1)
  - example: `quarto-core project::book::scope::tests::a_member_is_a_chapter_on_a_format_with_no_whole_book_output`
  - at: `crates\quarto-core\src\project\book\scope\tests.rs:257`
- **1 x** `assertion 'left == right' failed | left: Dir("<tmp>\\a\\local") | right: Dir("\\\\?\\<tmp>\\a\\local")`
  - modules: quarto-core record (1)
  - example: `quarto-core record::source_key::tests::a_directory_resolves_against_the_declaring_layer`
  - at: `crates\quarto-core\src\record\source_key.rs:643`
- **1 x** `'execute: record-sources': the directory './nope' is outside the project; only directories inside the project are accepted (a leading '/' means the project root). Copy or link a cache from elsewhere i`
  - modules: quarto-core record (1)
  - example: `quarto-core record::source_key::tests::directories_outside_the_project_or_missing_are_errors`
  - at: `crates\quarto-core\src\record\source_key.rs:687`
- **1 x** `figure-html\plot-<hash>.svg`
  - modules: quarto-core transforms (1)
  - example: `quarto-core transforms::hephaestus::tests::artifact_path_is_content_and_size_addressed`
  - at: `crates\quarto-core\src\transforms\hephaestus.rs:556`
- **1 x** `assertion failed: typ.contains("EDITED unsaved sentence")`
  - modules: quarto-core::integration book_download (1)
  - example: `quarto-core::integration book_download::the_active_chapter_comes_from_the_editor_and_the_others_from_disk`
  - at: `crates\quarto-core\tests\integration\book_download.rs:325`
- **1 x** `assertion 'left == right' failed | left: 1 | right: 0`
  - modules: quarto-core::integration book_download (1)
  - example: `quarto-core::integration book_download::a_chapters_captures_are_spliced_and_its_plot_lands_in_the_request_directory`
  - at: `crates\quarto-core\tests\integration\book_download.rs:372`
- **1 x** `native book output changed for 'epub-basic': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\epub-basic.txt against C:\Users\<user>\src\q2\crates\quarto-core\tests`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::epub_basic`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'epub-citations': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\epub-citations.txt against C:\Users\<user>\src\q2\crates\quarto-co`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::epub_citations`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-missing-bib': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-missing-bib.txt against C:\Users\<user>\src\q2\crates\qua`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_missing_bibliography_message`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-missing-csl': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-missing-csl.txt against C:\Users\<user>\src\q2\crates\qua`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_missing_csl_message`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'epub-partpage-image': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\epub-partpage-image.txt against C:\Users\<user>\src\q2\crates`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::epub_partpage_image`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-basic': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-basic.txt against C:\Users\<user>\src\q2\crates\quarto-core\tes`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_basic`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-caption-image': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-caption-image.txt against C:\Users\<user>\src\q2\crates`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_caption_image`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-citations': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-citations.txt against C:\Users\<user>\src\q2\crates\quarto-`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_citations`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-citations-csl': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-citations-csl.txt against C:\Users\<user>\src\q2\crates`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_citations_csl`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-citations-margin': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-citations-margin.txt against C:\Users\<user>\src\q2\`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_citations_margin`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'epub-rich': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\epub-rich.txt against C:\Users\<user>\src\q2\crates\quarto-core\tests/i`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::epub_rich`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-partpage-image': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-partpage-image.txt against C:\Users\<user>\src\q2\crat`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_partpage_image`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-smoke-orange-book-lang': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-smoke-orange-book-lang.txt against C:\Users\<u`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_smoke_orange_book_lang`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-smoke-override-orange-book': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-smoke-override-orange-book.txt against C:\`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_smoke_override_orange_book`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-rich': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-rich.txt against C:\Users\<user>\src\q2\crates\quarto-core\tests`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_rich`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-smoke-orange-book': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-smoke-orange-book.txt against C:\Users\<user>\src\q`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_smoke_orange_book`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-smoke-orange-book-margin': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-smoke-orange-book-margin.txt against C:\User`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_smoke_orange_book_margin`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `a warning names the record location: ["Warning: Execution record not written\nthe execution record copy requested by 'record: true' could not be written: Stage 'engine-execution' failed: creating <tmp`
  - modules: quarto-core::integration engine_record_stage (1)
  - example: `quarto-core::integration engine_record_stage::an_unwritable_record_location_warns_and_the_render_succeeds`
  - at: `crates\quarto-core\tests\integration\engine_record_stage.rs:848`
- **1 x** `the error must name the declaration-resolved path \\?\<tmp>\nope.lua, not \\?\<tmp>\sub\nope.lua; got: Error: Failed to read filter '<tmp>\nope.lua': I/O error: The system cannot find the file specifi`
  - modules: quarto-core::integration filter_paths (1)
  - example: `quarto-core::integration filter_paths::missing_project_filter_names_the_declaration_resolved_path`
  - at: `crates\quarto-core\tests\integration\filter_paths.rs:301`
- **1 x** `probe-ext-meta: file id must be registered`
  - modules: quarto-core::integration front_matter_provenance (1)
  - example: `quarto-core::integration front_matter_provenance::merged_front_matter_root_file_ids_identify_the_documents_own_keys`
  - at: `crates\quarto-core\tests\integration\front_matter_provenance.rs:74`
- **1 x** `assertion 'left == right' failed | left: ["Q-5-48"] | right: ["Q-5-42"]`
  - modules: quarto-core::integration format_resolution (1)
  - example: `quarto-core::integration format_resolution::the_scope_notices_are_emitted_once_on_both_dispatch_paths`
  - at: `crates\quarto-core\tests\integration\format_resolution.rs:369`
- **1 x** `1 golden mismatch(es): | C:\Users\<user>\src\q2\crates\quarto-core\tests/fixtures/jupyter-markdown-goldens\twenty-cells.preview.md: differs | --- expected`
  - modules: quarto-core::integration jupyter_markdown_goldens (1)
  - example: `quarto-core::integration jupyter_markdown_goldens::legacy_capture_payload_is_byte_identical_after_flip`
  - at: `crates\quarto-core\tests\integration\jupyter_markdown_goldens.rs:382`
- **1 x** `2 golden mismatch(es): | C:\Users\<user>\src\q2\crates\quarto-core\tests/fixtures/jupyter-markdown-goldens\twenty-cells.md: differs | --- expected`
  - modules: quarto-core::integration jupyter_markdown_goldens (1)
  - example: `quarto-core::integration jupyter_markdown_goldens::jupyter_markdown_matches_goldens`
  - at: `crates\quarto-core\tests\integration\jupyter_markdown_goldens.rs:206`
- **1 x** `assertion 'left == right' failed | left: Some("<tmp>\\_quarto.yml") | right: Some("_quarto.yml")`
  - modules: quarto-core::integration metadata_layers (1)
  - example: `quarto-core::integration metadata_layers::probe_in_the_project`
  - at: `crates\quarto-core\tests\integration\metadata_layers.rs:175`
- **1 x** `loading orange-book.lua as a user filter must not crash: Error running filter 'C:\Users\<user>\AppData\Local\Temp\quarto-pandoc-share-Dua1cz\filters\main.lua': | Couldn't load 'init.lua': | ...Temp\qu`
  - modules: quarto-core::integration orange_book_lua (1)
  - example: `quarto-core::integration orange_book_lua::orange_book_lua_loads_without_crashing_the_filter_chain`
  - at: `crates\quarto-core\tests\integration\orange_book_lua.rs:68`
- **1 x** `real part-divider content must render without crashing: Error running filter 'C:\Users\<user>\AppData\Local\Temp\quarto-pandoc-share-uMskuf\filters\main.lua': | Couldn't load 'init.lua': | ...Temp\qua`
  - modules: quarto-core::integration orange_book_lua (1)
  - example: `quarto-core::integration orange_book_lua::orange_book_lua_transforms_a_real_part_divider_via_pandocs_main_lua_chain`
  - at: `crates\quarto-core\tests\integration\orange_book_lua.rs:99`
- **1 x** `loading a synthetic pre-render filter must not crash: Error running filter 'C:\Users\<user>\AppData\Local\Temp\quarto-pandoc-share-CKjJYA\filters\main.lua': | Couldn't load 'init.lua': | ...Temp\quart`
  - modules: quarto-core::integration orange_book_lua (1)
  - example: `quarto-core::integration orange_book_lua::pre_render_entry_point_round_trips_through_pandocs_main_lua_chain`
  - at: `crates\quarto-core\tests\integration\orange_book_lua.rs:174`
- **1 x** `typst render into \\?\<tmp>\.quarto\preview-out\t failed: Error [Q-21-3]: typst compile exited with exit code: 1: | error: failed to load file (path contains invalid component '"C:"') | ┌─ \\?\<tmp>\.`
  - modules: quarto-core::integration output_outside_source_images (1)
  - example: `quarto-core::integration output_outside_source_images::typst_finds_source_images_in_the_request_directory`
  - at: `crates\quarto-core\tests\integration\output_outside_source_images.rs:87`
- **1 x** `keyed from the VFS root`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::a_book_below_the_vfs_root_keys_its_captures_from_that_root`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:831`
- **1 x** `[]`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::a_captured_figure_in_a_subdirectory_chapter_mounts_under_its_own_directory`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:1213`
- **1 x** `chapter 2's recorded output`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::a_chapters_capture_shows_there_and_never_in_an_identical_cell_elsewhere`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:1122`
- **1 x** `a diagnostic naming chapter two`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::a_path_key_that_is_not_a_chapter_is_ignored_and_a_corrupt_one_degrades_that_chapter`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:1351`
- **1 x** `{"blocks":[{"a":{"classes":[],"id":null,"kvs":[]},"c":[1,["one",[],[]],[{"c":"One","s":1,"t":"Str"}]],"s":0,"t":"Header"},{"c":[{"c":"Before.","s":3,"t":"Str"}],"s":2,"t":"Para"},{"a":{"classes":[5],"`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::chapter_scope_prefers_the_given_blob_and_falls_back_to_the_map`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:1250`
- **1 x** `assertion 'left == right' failed | left: ["C:/Users/GordonWoodhull/src/q2/target/tmp/q2-r9-books-kmOiGA/project/index.qmd", "C:/Users/GordonWoodhull/src/q2/target/tmp/q2-r9-books-kmOiGA/project/one.qm`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::the_resolver_reports_a_book_chapter_with_the_chapter_keys_in_book_order`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:1498`
- **1 x** `assertion 'left == right' failed | left: ["C:/Users/GordonWoodhull/src/q2/target/tmp/q2-r9-books-PtZsD1/project/index.qmd", "C:/Users/GordonWoodhull/src/q2/target/tmp/q2-r9-books-PtZsD1/project/one.qm`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::the_resolver_says_a_non_chapter_page_is_not_a_chapter_and_other_projects_have_no_book`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:1521`
- **1 x** `chapter 1's include is seeded`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::engine_includes_in_a_capture_count_only_for_the_first_chapter`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:1176`
- **1 x** `assertion 'left == right' failed | left: ["-f", "json", "-t", "docx", "--data-dir", "\"<tmp>\\root\\pandoc-share\\pandoc\\datadir\"", "-L", "\"<tmp>\\root\\pandoc-share\\filters\\main.lua\"", "--resou`
  - modules: quarto-core::integration pandoc_request_execute (1)
  - example: `quarto-core::integration pandoc_request_execute::execute_applies_dirs_files_share_tree_argv_and_env`
  - at: `crates\quarto-core\tests\integration\pandoc_request_execute.rs:203`
- **1 x** `assertion 'left == right' failed | left: "\"<tmp>\\tmp\\pandoc-share\\pandoc\\datadir\"" | right: "<tmp>/tmp/pandoc-share/pandoc/datadir"`
  - modules: quarto-core::integration pandoc_request_execute (1)
  - example: `quarto-core::integration pandoc_request_execute::stage_runs_the_prepared_request_through_the_native_executor`
  - at: `crates\quarto-core\tests\integration\pandoc_request_execute.rs:152`
- **1 x** `C:/Users/GordonWoodhull/src/q2/target/tmp/q2-r7-projects-7nrA7F/ref.docx vs //?/C:/Users/GordonWoodhull/src/q2/target/tmp/q2-r7-projects-7nrA7F/ref.docx`
  - modules: quarto-core::integration pandoc_request_projects (1)
  - example: `quarto-core::integration pandoc_request_projects::project_level_reference_doc_resolves_against_the_declaring_file`
  - at: `crates\quarto-core\tests\integration\pandoc_request_projects.rs:210`
- **1 x** `expected the retained temp JSON at "C" to still exist`
  - modules: quarto-core::integration pandoc_shim (1)
  - example: `quarto-core::integration pandoc_shim::test_proof_missing_type_surfaces_lua_traceback`
  - at: `crates\quarto-core\tests\integration\pandoc_shim.rs:2317`
- **1 x** `assertion failed: kept.starts_with(private.path())`
  - modules: quarto-core::integration pipeline_temp_dir_cleanup (1)
  - example: `quarto-core::integration pipeline_temp_dir_cleanup::failed_pandoc_json_outlives_the_pipeline_temp_dir`
  - at: `crates\quarto-core\tests\integration\pipeline_temp_dir_cleanup.rs:134`
- **1 x** `assertion 'left == right' failed | left: 0 | right: 1`
  - modules: quarto-core::integration pipeline_temp_dir_cleanup (1)
  - example: `quarto-core::integration pipeline_temp_dir_cleanup::lazy_temp_dir_is_removed_when_the_context_drops`
  - at: `crates\quarto-core\tests\integration\pipeline_temp_dir_cleanup.rs:73`
- **1 x** `<!DOCTYPE html> | <html> | <head>`
  - modules: quarto-core::integration render_overlay (1)
  - example: `quarto-core::integration render_overlay::overlay_file_is_read_in_place_of_the_disk_file`
  - at: `crates\quarto-core\tests\integration\render_overlay.rs:106`
- **1 x** `default: "<tmp>\\.quarto/preview-out\\f6599a5a-8b9e-4abb-a64a-<hash>"`
  - modules: quarto-core::integration typst_render_foundations (1)
  - example: `quarto-core::integration typst_render_foundations::typst_request_directory_renders_images_in_every_project_shape`
  - at: `crates\quarto-core\tests\integration\typst_render_foundations.rs:408`
- **1 x** `assertion 'left == right' failed | left: "C:\\Users\\GordonWoodhull\\AppData\\Local\\Temp\\p9-test-JOIxwF\\one.qmd" | right: "\\\\?\\C:\\Users\\GordonWoodhull\\AppData\\Local\\Temp\\p9-test-JOIxwF\\on`
  - modules: quarto-preview route_tests (1)
  - example: `quarto-preview route_tests::a_book_request_reaches_the_book_seam_with_every_capture`
  - at: `crates\quarto-preview\src\route_tests.rs:769`
- **1 x** `the request directory is under the project: "C:\\Users\\GordonWoodhull\\AppData\\Local\\Temp\\p9-test-a5On35\\.quarto/preview-out\\4a033d24-b820-4a51-af52-<hash>"`
  - modules: quarto-preview route_tests (1)
  - example: `quarto-preview route_tests::pdf_is_inline_with_stats_and_the_typst_pdf_target`
  - at: `crates\quarto-preview\src\route_tests.rs:327`
- **1 x** `the plot is referenced`
  - modules: quarto-preview::integration book_downloads (1)
  - example: `quarto-preview::integration book_downloads::a_chapters_recorded_plot_reaches_the_book`
  - at: `crates\quarto-preview\tests\integration\book_downloads.rs:221`
- **1 x** `assertion 'left == right' failed: pre-render script should run exactly once at boot with QUARTO_PROJECT_RENDER_ALL=1 | left: "1\r\n" | right: "1\n"`
  - modules: quarto-preview::integration render_scripts_boot (1)
  - example: `quarto-preview::integration render_scripts_boot::pre_render_scripts_run_once_at_boot`
  - at: `crates\quarto-preview\tests\integration\render_scripts_boot.rs:137`
- **1 x** `timed out after 20s: the re-execute of the reverted text to run the engine and rewrite the copy`
  - modules: quarto-preview::integration live_reexecute (1)
  - example: `quarto-preview::integration live_reexecute::the_cache_does_not_answer_a_re_execute_under_engine_only_sources`
  - at: `crates\quarto-preview\tests\integration\live_reexecute.rs:171`
- **1 x** `13 smoke-all test(s) failed (see output above)`
  - modules: quarto::integration smoke_all (1)
  - example: `quarto::integration smoke_all::smoke_all`
  - at: `crates\quarto\tests\integration\smoke_all.rs:141`

### Failures per module

| failing | module |
|---:|---|
| 17 | quarto-core::integration book_refactor_guard |
| 8 | quarto-core::integration pandoc_request_books |
| 7 | quarto-core glob |
| 4 | quarto::integration lmd_snapshots |
| 4 | quarto-core metadata_layers |
| 4 | quarto-core::integration pandoc_request_prepare |
| 3 | quarto-core::integration orange_book_lua |
| 3 | quarto-core::integration pandoc_request_projects |
| 2 | quarto::integration render_scripts_cli |
| 2 | quarto-core pandoc_filters |
| 2 | quarto-core preview_out |
| 2 | quarto-core project |
| 2 | quarto-core record |
| 2 | quarto-core::integration book_download |
| 2 | quarto-core::integration jupyter_markdown_goldens |
| 2 | quarto-core::integration pandoc_request_execute |
| 2 | quarto-core::integration pipeline_temp_dir_cleanup |
| 2 | quarto-hub storage |
| 2 | quarto-preview route_tests |
| 1 | quarto::integration convert_positron_cli |
| 1 | quarto::bin/q2 commands |
| 1 | quarto-core book_download |
| 1 | quarto-core engine |
| 1 | quarto-core extension |
| 1 | quarto-core transforms |
| 1 | quarto-core::integration engine_record_stage |
| 1 | quarto-core::integration filter_paths |
| 1 | quarto-core::integration front_matter_provenance |
| 1 | quarto-core::integration format_resolution |
| 1 | quarto-core::integration metadata_layers |
| 1 | quarto-core::integration output_outside_source_images |
| 1 | quarto-core::integration pandoc_request_extension_filters |
| 1 | quarto-core::integration pandoc_shim |
| 1 | quarto-core::integration render_overlay |
| 1 | quarto-core::integration typst_render_foundations |
| 1 | quarto-preview::integration book_downloads |
| 1 | quarto-preview::integration render_scripts_boot |
| 1 | quarto-preview::integration live_reexecute |
| 1 | quarto::integration smoke_all |

## The RFU-6 findings: full output of the failing tests

### i1

- Summary [   1.760s] 31 tests run: 31 passed, 8520 skipped

no failing tests with output in this log

### i4

- Summary [   7.288s] 22 tests run: 21 passed, 1 failed, 853 skipped

#### `quarto::integration convert_positron_cli::the_source_uri_default_is_the_documents_path_with_forward_slashes`

```text
  stdout ───

    running 1 test
    test convert_positron_cli::the_source_uri_default_is_the_documents_path_with_forward_slashes ... FAILED

    failures:

    failures:
        convert_positron_cli::the_source_uri_default_is_the_documents_path_with_forward_slashes

    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 566 filtered out; finished in 1.17s

  stderr ───

    thread 'convert_positron_cli::the_source_uri_default_is_the_documents_path_with_forward_slashes' (30924) panicked at crates\quarto\tests\integration\convert_positron_cli.rs:473:5:
    assertion `left == right` failed
      left: String("C:/Users/GordonWoodhull/AppData/Local/Temp/.tmpXrG0Ww/doc.qmd")
     right: "//?/C:/Users/GordonWoodhull/AppData/Local/Temp/.tmpXrG0Ww/doc.qmd"
    note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

```

### i6

- Summary [   8.001s] 84 tests run: 84 passed, 8784 skipped

no failing tests with output in this log

