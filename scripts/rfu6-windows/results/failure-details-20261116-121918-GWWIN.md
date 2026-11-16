# RFU-6 failure details, run `20261116-121918-GWWIN`

## Tools on PATH

| tool | path | version |
|---|---|---|
| pandoc | NOT FOUND |  |
| typst | NOT FOUND |  |
| quarto | NOT FOUND |  |
| deno | NOT FOUND |  |
| node | C:\Program Files\nodejs\node.EXE | v24.21.0 |
| julia | NOT FOUND |  |
| Rscript | NOT FOUND |  |
| python | C:\Users\GordonWoodhull\AppData\Roaming\uv\python\cpython-3.12-windows-x86_64-none\python.EXE | Python 3.12.16 |
| python3 | C:\Users\GordonWoodhull\AppData\Local\Microsoft\WindowsApps\python3.EXE | Python was not found; run without arguments to install from the Microsoft Store, or disable this shortcut from Settings > Apps > Advanced app settings > App execution aliases. |
| uv | C:\Users\GordonWoodhull\.local\bin\uv.EXE | uv 0.13.7 (316c827e0 2026-11-03 x86_64-pc-windows-msvc) |
| git | C:\Program Files\Git\cmd\git.EXE | git version 2.47.1.windows.1 |
| sh | NOT FOUND |  |
| bash | C:\Users\GordonWoodhull\AppData\Local\Microsoft\WindowsApps\bash.EXE | GNU bash, version 5.2.21(1)-release (x86_64-pc-linux-gnu) |
| cmd | C:\WINDOWS\system32\cmd.EXE |  |

Environment variables of interest: TEMP=C:\Users\GORDON~1\AppData\Local\Temp, TMP=C:\Users\GORDON~1\AppData\Local\Temp

## i8 nextest

- Summary [ 962.966s] 10181 tests run: 9844 passed (20 slow, 2 leaky), 337 failed, 41 skipped

### Failures grouped by first panic message (337 failing tests with output, 157 causes)

- **107 x** `pandoc must be installed and on PATH to run the 'L'-tier pandoc_transport tests (see crates/quarto-core/src/pandoc_filters/harness.rs): Error { kind: NotFound, message: "program not found" }`
  - modules: quarto-core::integration pandoc_shim (48), quarto-core::integration pandoc_transport (17), quarto-core::integration pandoc_shim_goldens (9), quarto-core::integration typst_generic_font_families (9), quarto-core::integration crossref_external_mode_matrix (8), quarto-core::integration crossref_numbering_matrix (5)
  - example: `quarto-core::integration book_numbering_lua::appendix_chapter_attribute_is_inert_for_non_epub_targets`
  - at: `crates\quarto-core\src\pandoc_filters\harness.rs:454`
- **23 x** `typst render should succeed: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pandoc binary could not be f`
  - modules: quarto-core::integration pandoc_typst_writer (18), quarto-core::integration pandoc_typst_compile (5)
  - example: `quarto-core::integration pandoc_typst_compile::output_ext_from_project_config_applies_and_document_overrides`
  - at: `crates\quarto-core\tests\integration\pandoc_typst_compile.rs:288`
- **22 x** `assertion 'left == right' failed: ProjectRenderSummary { outputs: [], pass1_failures: [], pass2_failures: [FileFailure { input: "<tmp>", error: "Error [Q-20-1]: Pandoc Not Found\nThe pandoc binary cou`
  - modules: quarto-core::integration book_citations (4), quarto-core::integration book_single_file_merge (4), quarto-core::integration book_epub (3), quarto-core::integration book_numbering_pipeline (3), quarto-core::integration book_chapter_images (2), quarto-core::integration book_appendix_letter_parity (2)
  - example: `quarto-core::integration book_chapter_images::epub_book_subdir_chapter_images_are_packaged`
  - at: `crates\quarto-core\tests\integration\book_chapter_images.rs:79`
- **6 x** `pandoc render should succeed: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pandoc binary could not be `
  - modules: quarto-core::integration pandoc_execute_defaults (6)
  - example: `quarto-core::integration pandoc_execute_defaults::document_execute_wins_over_format_default`
  - at: `crates\quarto-core\tests\integration\pandoc_execute_defaults.rs:132`
- **5 x** `q2 render --to epub should succeed; stderr: Rendering single file: <tmp>\f.qmd | error: while rendering <tmp>\f.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration render_pandoc_formats_e2e (5)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_render_epub`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:243`
- **4 x** `assertion 'left == right' failed: callouts: QUARTO_FILTER_PARAMS | left: Object {"active-filters": Object {"crossref": Bool(true), "jats_subarticle": Bool(false), "normalization": Bool(true)}, "crossr`
  - modules: quarto-core::integration pandoc_request_prepare (4)
  - example: `quarto-core::integration pandoc_request_prepare::docx_request_matches_the_recorded_native_run`
  - at: `crates\quarto-core\tests\integration\pandoc_request_prepare.rs:454`
- **4 x** `native render should succeed: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pandoc binary could not be `
  - modules: quarto-core::integration shortcode_pandoc_escapes (4)
  - example: `quarto-core::integration shortcode_pandoc_escapes::escaped_bare_brand_in_code_span_stays_literal`
  - at: `crates\quarto-core\tests\integration\shortcode_pandoc_escapes.rs:41`
- **3 x** `docx render should succeed: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pandoc binary could not be fo`
  - modules: quarto-core::integration pandoc_render_to_file (3)
  - example: `quarto-core::integration pandoc_render_to_file::render_document_to_file_docx_embeds_a_relatively_referenced_image`
  - at: `crates\quarto-core\tests\integration\pandoc_render_to_file.rs:168`
- **3 x** `called 'Result::unwrap()' on an 'Err' value: StripPrefixError(())`
  - modules: quarto-core::integration pandoc_request_projects (2), quarto-core::integration pandoc_request_extension_filters (1)
  - example: `quarto-core::integration pandoc_request_extension_filters::real_pandoc_loads_the_filter_from_the_request_alone`
  - at: `crates\quarto-core\tests\integration\pandoc_request_extension_filters.rs:207`
- **3 x** `expected 200, got 422 Unprocessable Entity: {"diagnostics":[{"$schema":"https://quarto.org/schemas/v1/json-diagnostic.json","kind":"error","title":"Pandoc Not Found","code":"Q-20-1","problem":"The pan`
  - modules: quarto-preview render_download (3)
  - example: `quarto-preview render_download::tests::falls_back_to_the_disk_copy_without_content`
  - at: `crates\quarto-preview\src\render_download.rs:162`
- **2 x** `Rendering project: <tmp> (type: book) | error: while rendering <tmp>\one.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration book_scope_cli_e2e (1), quarto::integration format_resolution_cli (1)
  - example: `quarto::integration book_scope_cli_e2e::typst_chapter_alone_writes_just_that_chapter`
  - at: `crates\quarto\tests\integration\book_scope_cli_e2e.rs:185`
- **2 x** `Rendering project: <tmp> (type: book) | error: while rendering <tmp>\two.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration format_resolution_cli (2)
  - example: `quarto::integration format_resolution_cli::naming_every_chapter_when_one_says_typst_renders_each_on_its_own`
  - at: `crates\quarto\tests\integration\format_resolution_cli.rs:311`
- **2 x** `q2 render --to docx should succeed; stderr: Rendering single file: <tmp>\img.qmd | error: while rendering <tmp>\img.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration render_pandoc_formats_e2e (2)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_docx_image_staged`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:600`
- **2 x** `assertion 'left == right' failed | left: [] | right: ["data/public.csv", "data/secret.csv"]`
  - modules: quarto-core glob (2)
  - example: `quarto-core glob::expand::tests::expands_a_glob_over_the_runtime`
  - at: `crates\quarto-core\src\glob\expand.rs:381`
- **2 x** `assertion 'left == right' failed | left: Some("<tmp>\\a\\_metadata.yml") | right: Some("\\\\?\\<tmp>\\a\\_metadata.yml")`
  - modules: quarto-core metadata_layers (2)
  - example: `quarto-core metadata_layers::tests::deepest_directory_layer_comes_last_and_carries_its_file`
  - at: `crates\quarto-core\src\metadata_layers.rs:606`
- **2 x** `pandoc must be installed and on PATH to run this test: Error { kind: NotFound, message: "program not found" }`
  - modules: quarto-core::integration conditional_content_pandoc (2)
  - example: `quarto-core::integration conditional_content_pandoc::resolved_conditional_content_renders_correctly_to_docx`
  - at: `crates\quarto-core\tests\integration\conditional_content_pandoc.rs:44`
- **2 x** `01-inline-marks.qmd to docx: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pandoc binary could not be f`
  - modules: quarto-core::integration editorial_marks_ooxml (2)
  - example: `quarto-core::integration editorial_marks_ooxml::a_project_that_still_lists_the_extension_renders_the_same_docx`
  - at: `crates\quarto-core\tests\integration\editorial_marks_ooxml.rs:48`
- **2 x** `docx render of crossrefs/all-docx.qmd must succeed: Error [Q-20-1]: Pandoc Not Found | The pandoc binary could not be found on PATH, and QUARTO_PANDOC is not set.`
  - modules: quarto-core::integration pandoc_goldens (2)
  - example: `quarto-core::integration pandoc_goldens::test_accepted_divergences_match_native_parity_reference`
  - at: `crates\quarto-core\tests\integration\pandoc_goldens.rs:123`
- **2 x** `plain render should succeed: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pandoc binary could not be f`
  - modules: quarto-core::integration pandoc_render_to_file (2)
  - example: `quarto-core::integration pandoc_render_to_file::render_document_to_file_source_override_replaces_the_disk_copy`
  - at: `crates\quarto-core\tests\integration\pandoc_render_to_file.rs:375`
- **2 x** `assertion failed: matches!(result, Err(Error::HubAlreadyRunning))`
  - modules: quarto-hub storage (2)
  - example: `quarto-hub storage::tests::test_storage_manager_prevents_double_lock`
  - at: `crates\quarto-hub\src\storage.rs:733`
- **1 x** `Rendering project: <tmp> (type: website) | error: while rendering <tmp>\index.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration components_render (1)
  - example: `quarto::integration components_render::markdown_output_sees_no_binding`
  - at: `crates\quarto\tests\integration\components_render.rs:205`
- **1 x** `q2 render --to docx should succeed; stderr: Rendering single file: <tmp>\cond.qmd | error: while rendering <tmp>\cond.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration conditional_content_pandoc_e2e (1)
  - example: `quarto::integration conditional_content_pandoc_e2e::e2e_docx_content_hidden_gating`
  - at: `crates\quarto\tests\integration\conditional_content_pandoc_e2e.rs:87`
- **1 x** `q2 render --to pptx should succeed; stderr: Rendering single file: <tmp>\cond.qmd | error: while rendering <tmp>\cond.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration conditional_content_pandoc_e2e (1)
  - example: `quarto::integration conditional_content_pandoc_e2e::e2e_pptx_content_hidden_gating`
  - at: `crates\quarto\tests\integration\conditional_content_pandoc_e2e.rs:130`
- **1 x** `assertion 'left == right' failed | left: String("<tmp>/doc.qmd") | right: "//?/<tmp>/doc.qmd"`
  - modules: quarto::integration convert_positron_cli (1)
  - example: `quarto::integration convert_positron_cli::the_source_uri_default_is_the_documents_path_with_forward_slashes`
  - at: `crates\quarto\tests\integration\convert_positron_cli.rs:473`
- **1 x** `Rendering project: <tmp> (type: book) | error: while rendering <tmp>\notes.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration format_resolution_cli (1)
  - example: `quarto::integration format_resolution_cli::several_files_in_a_typst_book_each_render_alone_in_input_order`
  - at: `crates\quarto\tests\integration\format_resolution_cli.rs:595`
- **1 x** `no markdown output for fragments: The system cannot find the file specified. (os error 2) | exit: ExitStatus(ExitStatus(1)) | stdout:`
  - modules: quarto::integration lmd_snapshots (1)
  - example: `quarto::integration lmd_snapshots::lmd_fixtures_render_as_snapshotted`
  - at: `crates\quarto\tests\integration\lmd_snapshots.rs:102`
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
- **1 x** `q2 render --to docx should succeed; stderr: Rendering project: <tmp> (type: website) | error: while rendering <tmp>\index.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration project_pandoc_gate_e2e (1)
  - example: `quarto::integration project_pandoc_gate_e2e::e2e_docx_project_no_sitemap`
  - at: `crates\quarto\tests\integration\project_pandoc_gate_e2e.rs:66`
- **1 x** `q2 render --to docx should succeed; stderr: Rendering single file: <tmp>\meta.qmd | error: while rendering <tmp>\meta.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_docx_meta_title`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:852`
- **1 x** `assertion failed: output.status.success()`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_docx_output_is_not_html`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:224`
- **1 x** `expected the missing filename on stderr: Rendering single file: <tmp>\f.qmd | error: while rendering <tmp>\f.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_missing_reference_doc_diagnosed`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:742`
- **1 x** `q2 render (no --to) should succeed; stderr: Rendering single file: <tmp>\multi.qmd | Warning [Q-20-8]: Multiple formats in one render are not yet supported | 'format:' declares more than one format, a`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_multi_format_warns`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:132`
- **1 x** `q2 render --to pptx should succeed; stderr: Rendering single file: <tmp>\meta.qmd | error: while rendering <tmp>\meta.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_pptx_meta_title`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:888`
- **1 x** `failed to build the base reference doc`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_reference_doc_forwarded`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:647`
- **1 x** `q2 render --to chunkedhtml should succeed; stderr: Rendering single file: <tmp>\f.qmd | error: while rendering <tmp>\f.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_render_chunkedhtml`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:1152`
- **1 x** `q2 render --to commonmark should succeed; stderr: Rendering single file: <tmp>\f.qmd | error: while rendering <tmp>\f.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_render_commonmark`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:944`
- **1 x** `q2 render --to commonmark_x should succeed; stderr: Rendering single file: <tmp>\f.qmd | error: while rendering <tmp>\f.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_render_commonmark_x`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:1084`
- **1 x** `q2 render --to djot should succeed; stderr: Rendering single file: <tmp>\f.qmd | error: while rendering <tmp>\f.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_render_djot`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:1120`
- **1 x** `q2 render --to docx should succeed; stderr: Rendering single file: <tmp>\f.qmd | error: while rendering <tmp>\f.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_render_docx`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:57`
- **1 x** `q2 render default.qmd --to epub should succeed; stderr: Rendering single file: <tmp>\default.qmd | error: while rendering <tmp>\default.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_render_epub_chapter_level`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:360`
- **1 x** `q2 render --to gfm should succeed; stderr: Rendering single file: <tmp>\f.qmd | error: while rendering <tmp>\f.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_render_gfm`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:917`
- **1 x** `q2 render --to gfm should succeed; stderr: Rendering single file: <tmp>\esc.qmd | error: while rendering <tmp>\esc.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_render_gfm_shortcode_round_trip`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:1016`
- **1 x** `q2 render --to markdown_strict should succeed; stderr: Rendering single file: <tmp>\f.qmd | error: while rendering <tmp>\f.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_render_markdown_strict`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:1050`
- **1 x** `q2 render --to odt should succeed; stderr: Rendering single file: <tmp>\f.qmd | error: while rendering <tmp>\f.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_render_odt`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:980`
- **1 x** `q2 render --to pptx should succeed; stderr: Rendering single file: <tmp>\f.qmd | error: while rendering <tmp>\f.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_render_pptx`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:95`
- **1 x** `q2 render --to s5 should succeed; stderr: Rendering single file: <tmp>\f.qmd | error: while rendering <tmp>\f.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_render_s5`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:1237`
- **1 x** `q2 render --to slidy should succeed; stderr: Rendering single file: <tmp>\f.qmd | error: while rendering <tmp>\f.qmd | Error [Q-20-1]: Pandoc Not Found`
  - modules: quarto::integration render_pandoc_formats_e2e (1)
  - example: `quarto::integration render_pandoc_formats_e2e::e2e_render_slidy`
  - at: `crates\quarto\tests\integration\render_pandoc_formats_e2e.rs:1201`
- **1 x** `assertion 'left == right' failed: quoted argument should arrive as a single argv entry | left: "--flag\r\ntwo words" | right: "--flag\ntwo words"`
  - modules: quarto::integration render_scripts_cli (1)
  - example: `quarto::integration render_scripts_cli::explicit_interpreter_command_line_with_args`
  - at: `crates\quarto\tests\integration\render_scripts_cli.rs:611`
- **1 x** `assertion 'left == right' failed: scripts must run in declaration order | left: "one\r\ntwo\r\n" | right: "one\ntwo\n"`
  - modules: quarto::integration render_scripts_cli (1)
  - example: `quarto::integration render_scripts_cli::list_form_runs_scripts_in_order`
  - at: `crates\quarto\tests\integration\render_scripts_cli.rs:579`
- **1 x** `render failed: | Rendering single file: <tmp>\t.qmd | error: while rendering <tmp>\t.qmd`
  - modules: quarto::integration typst_html_table_css (1)
  - example: `quarto::integration typst_html_table_css::style_block_rules_reach_typst_table_cells`
  - at: `crates\quarto\tests\integration\typst_html_table_css.rs:30`
- **1 x** `assertion failed: matches!(render_book_download(params()), Err(BookDownloadFailure::Busy))`
  - modules: quarto-core book_download (1)
  - example: `quarto-core book_download::tests::a_second_download_of_the_same_project_is_busy_until_the_first_ends`
  - at: `crates\quarto-core\src\book_download.rs:315`
- **1 x** `execute succeeds: RuntimeNotFound { engine: "julia", runtime: "julia" }`
  - modules: quarto-core engine (1)
  - example: `quarto-core engine::ts_engine::handshake_tests::an_engine_named_julia_that_does_not_acknowledge_is_not_presented_by_core`
  - at: `crates\quarto-core\src\engine\ts_engine.rs:3771`
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
- **1 x** `"<tmp>\\.quarto/preview-out\\1bf1f3aa-53a4-472f-bb7b-<hash>"`
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
- **1 x** `typst must be on PATH to run this test suite`
  - modules: quarto-core stage (1)
  - example: `quarto-core stage::stages::typst_compile::tests::test_discover_available_typst_fonts_finds_vendored_font_awesome`
  - at: `crates\quarto-core\src\stage\stages\typst_compile.rs:836`
- **1 x** `figure-html\plot-<hash>.svg`
  - modules: quarto-core transforms (1)
  - example: `quarto-core transforms::hephaestus::tests::artifact_path_is_content_and_size_addressed`
  - at: `crates\quarto-core\src\transforms\hephaestus.rs:556`
- **1 x** `assertion 'left == right' failed | left: [] | right: ["one.typ"]`
  - modules: quarto-core::integration book_named_files (1)
  - example: `quarto-core::integration book_named_files::a_chapter_rendered_alone_shuts_the_engines_down_once`
  - at: `crates\quarto-core\tests\integration\book_named_files.rs:108`
- **1 x** `assertion 'left == right' failed | left: [] | right: ["index.typ"]`
  - modules: quarto-core::integration book_named_files (1)
  - example: `quarto-core::integration book_named_files::a_failing_file_does_not_discard_the_others_and_fail_fast_stops_the_loop`
  - at: `crates\quarto-core\tests\integration\book_named_files.rs:143`
- **1 x** `plain typst render with top-level-division: chapter should compile to a real PDF: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, pro`
  - modules: quarto-core::integration book_numbering_pipeline (1)
  - example: `quarto-core::integration book_numbering_pipeline::typst_top_level_division_alone_does_not_reset_figure_numbering`
  - at: `crates\quarto-core\tests\integration\book_numbering_pipeline.rs:234`
- **1 x** `assertion 'left == right' failed | left: ["two.html"] | right: ["one.typ", "two.html", "notes.typ"]`
  - modules: quarto-core::integration book_named_files (1)
  - example: `quarto-core::integration book_named_files::several_files_render_each_on_their_own_in_one_run`
  - at: `crates\quarto-core\tests\integration\book_named_files.rs:118`
- **1 x** `native book output changed for 'epub-basic': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\epub-basic.txt against C:\Users\<user>\src\q2\crates\quarto-core\tests`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::epub_basic`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'epub-citations': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\epub-citations.txt against C:\Users\<user>\src\q2\crates\quarto-co`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::epub_citations`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-basic': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-basic.txt against C:\Users\<user>\src\q2\crates\quarto-core\tes`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_basic`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'epub-partpage-image': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\epub-partpage-image.txt against C:\Users\<user>\src\q2\crates`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::epub_partpage_image`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-caption-image': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-caption-image.txt against C:\Users\<user>\src\q2\crates`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_caption_image`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-missing-bib': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-missing-bib.txt against C:\Users\<user>\src\q2\crates\qua`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_missing_bibliography_message`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-citations': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-citations.txt against C:\Users\<user>\src\q2\crates\quarto-`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_citations`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-citations-csl': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-citations-csl.txt against C:\Users\<user>\src\q2\crates`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_citations_csl`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-missing-csl': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-missing-csl.txt against C:\Users\<user>\src\q2\crates\qua`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_missing_csl_message`
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
- **1 x** `native book output changed for 'typst-smoke-override-orange-book': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-smoke-override-orange-book.txt against C:\`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_smoke_override_orange_book`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-smoke-orange-book-lang': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-smoke-orange-book-lang.txt against C:\Users\<u`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_smoke_orange_book_lang`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-rich': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-rich.txt against C:\Users\<user>\src\q2\crates\quarto-core\tests`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_rich`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `assertion 'left == right' failed: exactly one merged output, not one per chapter: ProjectRenderSummary { outputs: [], pass1_failures: [], pass2_failures: [FileFailure { input: "<tmp>", error: "Error [`
  - modules: quarto-core::integration book_single_file_merge (1)
  - example: `quarto-core::integration book_single_file_merge::typst_book_merges_all_chapters_into_one_compiled_pdf`
  - at: `crates\quarto-core\tests\integration\book_single_file_merge.rs:141`
- **1 x** `native book output changed for 'typst-smoke-orange-book': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-smoke-orange-book.txt against C:\Users\<user>\src\q`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_smoke_orange_book`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `native book output changed for 'typst-smoke-orange-book-margin': diff C:\Users\<user>\src\q2\crates\quarto-core\tests/integration/book_refactor_guard\typst-smoke-orange-book-margin.txt against C:\User`
  - modules: quarto-core::integration book_refactor_guard (1)
  - example: `quarto-core::integration book_refactor_guard::typst_smoke_orange_book_margin`
  - at: `crates\quarto-core\tests\integration\book_refactor_guard.rs:378`
- **1 x** `unexpected failures: pass1=[] pass2=[("<tmp>\\sub\\page.qmd", "Error [Q-20-1]: Pandoc Not Found\nThe pandoc binary could not be found on PATH, and QUARTO_PANDOC is not set.\n")]`
  - modules: quarto-core::integration config_path_consumers (1)
  - example: `quarto-core::integration config_path_consumers::project_typst_template_partials_reach_subdir_pages`
  - at: `crates\quarto-core\tests\integration\config_path_consumers.rs:74`
- **1 x** `case0.qmd to docx: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pandoc binary could not be found on PA`
  - modules: quarto-core::integration editorial_marks_ooxml (1)
  - example: `quarto-core::integration editorial_marks_ooxml::commented_ranges_survive_word_and_read_back_as_the_equal_range_shape`
  - at: `crates\quarto-core\tests\integration\editorial_marks_ooxml.rs:48`
- **1 x** `06-pptx-marks.qmd to pptx: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pandoc binary could not be fou`
  - modules: quarto-core::integration editorial_marks_ooxml (1)
  - example: `quarto-core::integration editorial_marks_ooxml::the_extensions_pptx_example_produces_its_runs`
  - at: `crates\quarto-core\tests\integration\editorial_marks_ooxml.rs:48`
- **1 x** `track-changes-docx.qmd to docx: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pandoc binary could not b`
  - modules: quarto-core::integration editorial_marks_ooxml (1)
  - example: `quarto-core::integration editorial_marks_ooxml::the_round_trip_through_import_is_stable_for_the_import_fixtures`
  - at: `crates\quarto-core\tests\integration\editorial_marks_ooxml.rs:48`
- **1 x** `shapes.qmd to docx: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pandoc binary could not be found on P`
  - modules: quarto-core::integration editorial_marks_ooxml (1)
  - example: `quarto-core::integration editorial_marks_ooxml::the_shapes_that_dont_invert_come_back_equal_at_the_qmd_level`
  - at: `crates\quarto-core\tests\integration\editorial_marks_ooxml.rs:48`
- **1 x** `a warning names the record location: ["Warning: Execution record not written\nthe execution record copy requested by 'record: true' could not be written: Stage 'engine-execution' failed: creating <tmp`
  - modules: quarto-core::integration engine_record_stage (1)
  - example: `quarto-core::integration engine_record_stage::an_unwritable_record_location_warns_and_the_render_succeeds`
  - at: `crates\quarto-core\tests\integration\engine_record_stage.rs:848`
- **1 x** `docx render: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pandoc binary could not be found on PATH, an`
  - modules: quarto-core::integration fig_alt (1)
  - example: `quarto-core::integration fig_alt::docx_descr_comes_from_fig_alt`
  - at: `crates\quarto-core\tests\integration\fig_alt.rs:307`
- **1 x** `typst render: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pandoc binary could not be found on PATH, a`
  - modules: quarto-core::integration fig_alt (1)
  - example: `quarto-core::integration fig_alt::typst_image_alt_comes_from_fig_alt`
  - at: `crates\quarto-core\tests\integration\fig_alt.rs:360`
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
- **1 x** `unexpected failures: pass1=[] pass2=[FileFailure { input: "<tmp>\\sub\\deep.qmd", error: "Error [Q-5-7]: Resource copy failed\nCould not copy '<tmp>\\sub\\../img.png' to '<tmp>\\_site\\img.png'.\nℹ Th`
  - modules: quarto-core::integration lightbox_pipeline (1)
  - example: `quarto-core::integration lightbox_pipeline::nested_page_links_the_project_runtime`
  - at: `crates\quarto-core\tests\integration\lightbox_pipeline.rs:70`
- **1 x** `1 golden mismatch(es): | C:\Users\<user>\src\q2\crates\quarto-core\tests/fixtures/jupyter-markdown-goldens\twenty-cells.preview.md: differs | --- expected`
  - modules: quarto-core::integration jupyter_markdown_goldens (1)
  - example: `quarto-core::integration jupyter_markdown_goldens::legacy_capture_payload_is_byte_identical_after_flip`
  - at: `crates\quarto-core\tests\integration\jupyter_markdown_goldens.rs:382`
- **1 x** `14 golden mismatch(es): | C:\Users\<user>\src\q2\crates\quarto-core\tests/fixtures/jupyter-markdown-goldens\ceded-ojs.record.json: differs | --- expected`
  - modules: quarto-core::integration jupyter_markdown_goldens (1)
  - example: `quarto-core::integration jupyter_markdown_goldens::jupyter_markdown_matches_goldens`
  - at: `crates\quarto-core\tests\integration\jupyter_markdown_goldens.rs:206`
- **1 x** `docx render must succeed with html-math-method: mathml: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The p`
  - modules: quarto-core::integration math_mode_pipeline (1)
  - example: `quarto-core::integration math_mode_pipeline::mathml_method_is_ignored_for_docx`
  - at: `crates\quarto-core\tests\integration\math_mode_pipeline.rs:98`
- **1 x** `assertion 'left == right' failed | left: Some("<tmp>\\_quarto.yml") | right: Some("_quarto.yml")`
  - modules: quarto-core::integration metadata_layers (1)
  - example: `quarto-core::integration metadata_layers::probe_in_the_project`
  - at: `crates\quarto-core\tests\integration\metadata_layers.rs:175`
- **1 x** `142 smoke-all test(s) failed (see output above)`
  - modules: quarto::integration smoke_all (1)
  - example: `quarto::integration smoke_all::smoke_all`
  - at: `crates\quarto\tests\integration\smoke_all.rs:141`
- **1 x** `pptx render of crossrefs/all-docx.qmd must succeed: Error [Q-20-1]: Pandoc Not Found | The pandoc binary could not be found on PATH, and QUARTO_PANDOC is not set.`
  - modules: quarto-core::integration pandoc_goldens (1)
  - example: `quarto-core::integration pandoc_goldens::test_fixtures_match_q1_golden`
  - at: `crates\quarto-core\tests\integration\pandoc_goldens.rs:123`
- **1 x** `docx render of smoke-all/mermaid/backticks.qmd must succeed: Error [Q-20-1]: Pandoc Not Found | The pandoc binary could not be found on PATH, and QUARTO_PANDOC is not set.`
  - modules: quarto-core::integration pandoc_goldens (1)
  - example: `quarto-core::integration pandoc_goldens::test_mermaid_fixture_preserves_diagram_source_text`
  - at: `crates\quarto-core\tests\integration\pandoc_goldens.rs:123`
- **1 x** `gfm render should succeed: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pandoc binary could not be fou`
  - modules: quarto-core::integration pandoc_long_tail_formats (1)
  - example: `quarto-core::integration pandoc_long_tail_formats::gfm_shortcode_round_trip`
  - at: `crates\quarto-core\tests\integration\pandoc_long_tail_formats.rs:175`
- **1 x** `textile render should succeed: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pandoc binary could not be`
  - modules: quarto-core::integration pandoc_long_tail_formats (1)
  - example: `quarto-core::integration pandoc_long_tail_formats::textile_fresh_baseline_snapshot`
  - at: `crates\quarto-core\tests\integration\pandoc_long_tail_formats.rs:137`
- **1 x** `render --to odt failed: Error [Q-20-1]: Pandoc Not Found | The pandoc binary could not be found on PATH, and QUARTO_PANDOC is not set.`
  - modules: quarto-core::integration pandoc_long_tail_formats (1)
  - example: `quarto-core::integration pandoc_long_tail_formats::tier_a_smoke_all_variants`
  - at: `crates\quarto-core\tests\integration\pandoc_long_tail_formats.rs:79`
- **1 x** `render --to djot failed: Error [Q-20-1]: Pandoc Not Found | The pandoc binary could not be found on PATH, and QUARTO_PANDOC is not set.`
  - modules: quarto-core::integration pandoc_long_tail_formats (1)
  - example: `quarto-core::integration pandoc_long_tail_formats::tier_c_smoke_all_variants`
  - at: `crates\quarto-core\tests\integration\pandoc_long_tail_formats.rs:253`
- **1 x** `render --to s5 failed: Error [Q-20-1]: Pandoc Not Found | The pandoc binary could not be found on PATH, and QUARTO_PANDOC is not set.`
  - modules: quarto-core::integration pandoc_long_tail_formats (1)
  - example: `quarto-core::integration pandoc_long_tail_formats::tier_d_smoke_all_variants`
  - at: `crates\quarto-core\tests\integration\pandoc_long_tail_formats.rs:378`
- **1 x** `typst: the engine stage succeeds: StageError { stage: "engine-execution", diagnostics: [DiagnosticMessage { code: None, title: "Execution failed in marimo: @quarto/api: pandoc unavailable (no pandoc p`
  - modules: quarto-core::integration marimo_records (1)
  - example: `quarto-core::integration marimo_records::marimo_record_tags_static_format_for_pdf`
  - at: `crates\quarto-core\tests\integration\marimo_records.rs:173`
- **1 x** `keyed from the VFS root`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::a_book_below_the_vfs_root_keys_its_captures_from_that_root`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:819`
- **1 x** `native chapter-alone epub: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pandoc binary could not be fou`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::chapter_alone_epub_request_equals_native`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:539`
- **1 x** `[]`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::a_captured_figure_in_a_subdirectory_chapter_mounts_under_its_own_directory`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:1201`
- **1 x** `native chapter-alone render: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pandoc binary could not be f`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::chapter_alone_typst_request_equals_native`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:372`
- **1 x** `{"blocks":[{"a":{"classes":[],"id":null,"kvs":[]},"c":[1,["one",[],[]],[{"c":"One","s":1,"t":"Str"}]],"s":0,"t":"Header"},{"c":[{"c":"Before.","s":3,"t":"Str"}],"s":2,"t":"Para"},{"a":{"classes":[5],"`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::chapter_scope_prefers_the_given_blob_and_falls_back_to_the_map`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:1238`
- **1 x** `chapter 2's recorded output`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::a_chapters_capture_shows_there_and_never_in_an_identical_cell_elsewhere`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:1110`
- **1 x** `assertion 'left == right' failed | left: ["C:/Users/GordonWoodhull/src/q2/target/tmp/q2-r9-books-gILLUJ/index.qmd", "C:/Users/GordonWoodhull/src/q2/target/tmp/q2-r9-books-gILLUJ/one.qmd", "C:/Users/Go`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::the_resolver_reports_a_book_chapter_with_the_chapter_keys_in_book_order`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:1486`
- **1 x** `a diagnostic naming chapter two`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::a_path_key_that_is_not_a_chapter_is_ignored_and_a_corrupt_one_degrades_that_chapter`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:1339`
- **1 x** `assertion 'left == right' failed | left: ["C:/Users/GordonWoodhull/src/q2/target/tmp/q2-r9-books-LCujXs/index.qmd", "C:/Users/GordonWoodhull/src/q2/target/tmp/q2-r9-books-LCujXs/one.qmd", "C:/Users/Go`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::the_resolver_says_a_non_chapter_page_is_not_a_chapter_and_other_projects_have_no_book`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:1509`
- **1 x** `assertion 'left == right' failed | left: ["-f", "json", "-t", "docx", "--data-dir", "\"<tmp>\\root\\pandoc-share\\pandoc\\datadir\"", "-L", "\"<tmp>\\root\\pandoc-share\\filters\\main.lua\"", "--resou`
  - modules: quarto-core::integration pandoc_request_execute (1)
  - example: `quarto-core::integration pandoc_request_execute::execute_applies_dirs_files_share_tree_argv_and_env`
  - at: `crates\quarto-core\tests\integration\pandoc_request_execute.rs:203`
- **1 x** `assertion 'left == right' failed | left: "\"<tmp>\\tmp\\pandoc-share\\pandoc\\datadir\"" | right: "<tmp>/tmp/pandoc-share/pandoc/datadir"`
  - modules: quarto-core::integration pandoc_request_execute (1)
  - example: `quarto-core::integration pandoc_request_execute::stage_runs_the_prepared_request_through_the_native_executor`
  - at: `crates\quarto-core\tests\integration\pandoc_request_execute.rs:152`
- **1 x** `chapter 1's include is seeded`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::engine_includes_in_a_capture_count_only_for_the_first_chapter`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:1164`
- **1 x** `an absolute path is left in the input JSON`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::the_parity_books_input_json_matches_its_golden_and_holds_no_absolute_path`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:1449`
- **1 x** `assertion 'left == right' failed: ProjectRenderSummary { outputs: [], pass1_failures: [], pass2_failures: [FileFailure { input: "C:\\Users\\GordonWoodhull\\src\\q2\\target\\tmp\\q2-r9-books-bUSHko", e`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::whole_book_epub_request_equals_native`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:469`
- **1 x** `assertion 'left == right' failed: ProjectRenderSummary { outputs: [], pass1_failures: [], pass2_failures: [FileFailure { input: "C:\\Users\\GordonWoodhull\\src\\q2\\target\\tmp\\q2-r9-books-upQLfs", e`
  - modules: quarto-core::integration pandoc_request_books (1)
  - example: `quarto-core::integration pandoc_request_books::whole_book_typst_request_equals_native`
  - at: `crates\quarto-core\tests\integration\pandoc_request_books.rs:310`
- **1 x** `C:/Users/GordonWoodhull/src/q2/target/tmp/q2-r7-projects-xYdE1B/ref.docx vs //?/C:/Users/GordonWoodhull/src/q2/target/tmp/q2-r7-projects-xYdE1B/ref.docx`
  - modules: quarto-core::integration pandoc_request_projects (1)
  - example: `quarto-core::integration pandoc_request_projects::project_level_reference_doc_resolves_against_the_declaring_file`
  - at: `crates\quarto-core\tests\integration\pandoc_request_projects.rs:210`
- **1 x** `typst render with a margin note should compile hermetically from the vendored package cache, with no network fallback: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title:`
  - modules: quarto-core::integration pandoc_typst_compile (1)
  - example: `quarto-core::integration pandoc_typst_compile::render_document_to_file_typst_margin_note_compiles_without_network`
  - at: `crates\quarto-core\tests\integration\pandoc_typst_compile.rs:213`
- **1 x** `typst render with a crossref'd figure should compile to a real PDF: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Mar`
  - modules: quarto-core::integration pandoc_typst_compile (1)
  - example: `quarto-core::integration pandoc_typst_compile::render_document_to_file_typst_crossref_document_compiles_to_pdf`
  - at: `crates\quarto-core\tests\integration\pandoc_typst_compile.rs:260`
- **1 x** `typst render should succeed and produce a real PDF: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pando`
  - modules: quarto-core::integration pandoc_typst_compile (1)
  - example: `quarto-core::integration pandoc_typst_compile::render_document_to_file_typst_compiles_to_real_pdf`
  - at: `crates\quarto-core\tests\integration\pandoc_typst_compile.rs:62`
- **1 x** `explicit Typst format should override multi-format frontmatter: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdow`
  - modules: quarto-core::integration pandoc_typst_compile (1)
  - example: `quarto-core::integration pandoc_typst_compile::render_to_file_typst_overrides_multiple_frontmatter_formats`
  - at: `crates\quarto-core\tests\integration\pandoc_typst_compile.rs:108`
- **1 x** `typst render with template-partials should succeed: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pando`
  - modules: quarto-core::integration pandoc_typst_template_partials (1)
  - example: `quarto-core::integration pandoc_typst_template_partials::typst_template_partials_shadow_vendored_partials`
  - at: `crates\quarto-core\tests\integration\pandoc_typst_template_partials.rs:67`
- **1 x** `expected a clean render (the image-copy-ordering bug surfaces as a pass2 failure): pass1=[] pass2=[FileFailure { input: "<tmp>\\doc.qmd", error: "Error [Q-20-1]: Pandoc Not Found\nThe pandoc binary co`
  - modules: quarto-core::integration pandoc_typst_resource_copy (1)
  - example: `quarto-core::integration pandoc_typst_resource_copy::typst_project_copies_image_to_output_dir_before_compiling`
  - at: `crates\quarto-core\tests\integration\pandoc_typst_resource_copy.rs:110`
- **1 x** `assertion failed: kept.starts_with(private.path())`
  - modules: quarto-core::integration pipeline_temp_dir_cleanup (1)
  - example: `quarto-core::integration pipeline_temp_dir_cleanup::failed_pandoc_json_outlives_the_pipeline_temp_dir`
  - at: `crates\quarto-core\tests\integration\pipeline_temp_dir_cleanup.rs:134`
- **1 x** `assertion 'left == right' failed | left: 0 | right: 1`
  - modules: quarto-core::integration pipeline_temp_dir_cleanup (1)
  - example: `quarto-core::integration pipeline_temp_dir_cleanup::lazy_temp_dir_is_removed_when_the_context_drops`
  - at: `crates\quarto-core\tests\integration\pipeline_temp_dir_cleanup.rs:73`
- **1 x** `typst render should succeed instead of crashing in modules/mediabag.lua: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Som`
  - modules: quarto-core::integration pandoc_typst_writer (1)
  - example: `quarto-core::integration pandoc_typst_writer::render_document_to_file_typst_resolves_data_uri_image_via_mediabag`
  - at: `crates\quarto-core\tests\integration\pandoc_typst_writer.rs:847`
- **1 x** `<!DOCTYPE html> | <html> | <head>`
  - modules: quarto-core::integration render_overlay (1)
  - example: `quarto-core::integration render_overlay::overlay_file_is_read_in_place_of_the_disk_file`
  - at: `crates\quarto-core\tests\integration\render_overlay.rs:106`
- **1 x** `chapter render: Parse(ParseError { diagnostics: [DiagnosticMessage { code: Some("Q-20-1"), title: "Pandoc Not Found", kind: Error, problem: Some(Markdown("The pandoc binary could not be found on PATH,`
  - modules: quarto-core::integration render_overlay (1)
  - example: `quarto-core::integration render_overlay::the_nested_chapter_alone_pipeline_sees_the_overlay`
  - at: `crates\quarto-core\tests\integration\render_overlay.rs:145`
- **1 x** `unexpected failures: pass1=[] pass2=[FileFailure { input: "<tmp>\\one\\index.qmd", error: "Error [Q-20-1]: Pandoc Not Found\nThe pandoc binary could not be found on PATH, and QUARTO_PANDOC is not set.`
  - modules: quarto-core::integration website_post_render_format_gate (1)
  - example: `quarto-core::integration website_post_render_format_gate::docx_project_survives_alias_collision`
  - at: `crates\quarto-core\tests\integration\website_post_render_format_gate.rs:88`
- **1 x** `unexpected failures: pass1=[] pass2=[FileFailure { input: "<tmp>\\index.qmd", error: "Error [Q-20-1]: Pandoc Not Found\nThe pandoc binary could not be found on PATH, and QUARTO_PANDOC is not set.\n", `
  - modules: quarto-core::integration website_post_render_format_gate (1)
  - example: `quarto-core::integration website_post_render_format_gate::docx_project_writes_no_sitemap_robots_or_alias_stub`
  - at: `crates\quarto-core\tests\integration\website_post_render_format_gate.rs:88`
- **1 x** `no record to publish from <tmp>: Nothing; engine: Unavailable("engine is not in the record sources")`
  - modules: quarto-hub-provider::integration execute (1)
  - example: `quarto-hub-provider::integration execute::publishing_an_imported_record_needs_no_consent_and_runs_no_engine`
  - at: `crates\quarto-hub-provider\tests\integration\execute.rs:717`
- **1 x** `called 'Result::unwrap()' on an 'Err' value: "no record to publish from <tmp>: Nothing; engine: Unavailable(\"engine is not in the record sources\")"`
  - modules: quarto-hub-provider::integration execute (1)
  - example: `quarto-hub-provider::integration execute::execute_document_with_an_imported_source_publishes_without_consent_or_engine`
  - at: `crates\quarto-hub-provider\tests\integration\execute.rs:809`
- **1 x** `assertion 'left == right' failed: captures: [EngineCapture { engine_name: "test-figure", input_qmd: "---\nengine: test-figure\nexecute:\n  engine: test-figure\nformat: q2-preview\nquarto:\n  language:`
  - modules: quarto-hub-provider::integration execute (1)
  - example: `quarto-hub-provider::integration execute::a_figure_reported_under_the_materialized_root_is_captured`
  - at: `crates\quarto-hub-provider\tests\integration\execute.rs:266`
- **1 x** `assertion failed: matches!(provider.execute_document("doc.qmd").await.unwrap(), | ExecOutcome::Executed(_))`
  - modules: quarto-hub-provider::integration execute (1)
  - example: `quarto-hub-provider::integration execute::a_guest_key_may_narrow_a_wider_host_list`
  - at: `crates\quarto-hub-provider\tests\integration\execute.rs:961`
- **1 x** `Cannot start a runtime from within a runtime. This happens because a function (like 'block_on') attempted to block the current thread while the thread is being used to drive asynchronous tasks.`
  - modules: quarto-preview capture_driver (1)
  - example: `quarto-preview capture_driver::tests::a_record_only_cache_entry_is_published`
  - at: `C:\Users\GordonWoodhull\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\tokio-1.53.1\src\runtime\scheduler\multi_thread\mod.rs:91`
- **1 x** `assertion 'left == right' failed | left: "C:\\Users\\GordonWoodhull\\AppData\\Local\\Temp\\p9-test-h6nqMF\\one.qmd" | right: "\\\\?\\C:\\Users\\GordonWoodhull\\AppData\\Local\\Temp\\p9-test-h6nqMF\\on`
  - modules: quarto-preview route_tests (1)
  - example: `quarto-preview route_tests::a_book_request_reaches_the_book_seam_with_every_capture`
  - at: `crates\quarto-preview\src\route_tests.rs:769`
- **1 x** `the request directory is under the project: "C:\\Users\\GordonWoodhull\\AppData\\Local\\Temp\\p9-test-1taJq5\\.quarto/preview-out\\17d5a481-2020-4c49-a54f-<hash>"`
  - modules: quarto-preview route_tests (1)
  - example: `quarto-preview route_tests::pdf_is_inline_with_stats_and_the_typst_pdf_target`
  - at: `crates\quarto-preview\src\route_tests.rs:327`
- **1 x** `assertion 'left == right' failed: pre-render script should run exactly once at boot with QUARTO_PROJECT_RENDER_ALL=1 | left: "1\r\n" | right: "1\n"`
  - modules: quarto-preview::integration render_scripts_boot (1)
  - example: `quarto-preview::integration render_scripts_boot::pre_render_scripts_run_once_at_boot`
  - at: `crates\quarto-preview\tests\integration\render_scripts_boot.rs:136`

### Failures per module

| failing | module |
|---:|---|
| 48 | quarto-core::integration pandoc_shim |
| 26 | quarto::integration render_pandoc_formats_e2e |
| 19 | quarto-core::integration pandoc_typst_writer |
| 17 | quarto-core::integration book_refactor_guard |
| 17 | quarto-core::integration pandoc_transport |
| 13 | quarto-core::integration pandoc_request_books |
| 9 | quarto-core::integration pandoc_shim_goldens |
| 9 | quarto-core::integration pandoc_typst_compile |
| 9 | quarto-core::integration typst_generic_font_families |
| 8 | quarto-core::integration crossref_external_mode_matrix |
| 7 | quarto-core glob |
| 6 | quarto-core::integration editorial_marks_ooxml |
| 6 | quarto-core::integration pandoc_execute_defaults |
| 5 | quarto-core::integration book_single_file_merge |
| 5 | quarto-core::integration crossref_numbering_matrix |
| 5 | quarto-core::integration pandoc_long_tail_formats |
| 5 | quarto-core::integration pandoc_render_to_file |
| 4 | quarto::integration format_resolution_cli |
| 4 | quarto::integration lmd_snapshots |
| 4 | quarto-core metadata_layers |
| 4 | quarto-core::integration book_citations |
| 4 | quarto-core::integration book_numbering_pipeline |
| 4 | quarto-core::integration pandoc_goldens |
| 4 | quarto-core::integration pandoc_request_prepare |
| 4 | quarto-core::integration shortcode_pandoc_escapes |
| 4 | quarto-hub-provider::integration execute |
| 3 | quarto-core::integration book_epub |
| 3 | quarto-core::integration book_named_files |
| 3 | quarto-core::integration book_numbering_lua |
| 3 | quarto-core::integration crossref_number_parity |
| 3 | quarto-core::integration orange_book_lua |
| 3 | quarto-core::integration pandoc_request_projects |
| 3 | quarto-preview render_download |
| 2 | quarto::integration conditional_content_pandoc_e2e |
| 2 | quarto::integration render_scripts_cli |
| 2 | quarto-core engine |
| 2 | quarto-core pandoc_filters |
| 2 | quarto-core preview_out |
| 2 | quarto-core project |
| 2 | quarto-core record |
| 2 | quarto-core::integration book_chapter_images |
| 2 | quarto-core::integration book_appendix_letter_parity |
| 2 | quarto-core::integration conditional_content_pandoc |
| 2 | quarto-core::integration fig_alt |
| 2 | quarto-core::integration jupyter_markdown_goldens |
| 2 | quarto-core::integration pandoc_request_execute |
| 2 | quarto-core::integration pipeline_temp_dir_cleanup |
| 2 | quarto-core::integration render_overlay |
| 2 | quarto-core::integration website_post_render_format_gate |
| 2 | quarto-hub storage |
| 2 | quarto-preview route_tests |
| 1 | quarto::integration book_scope_cli_e2e |
| 1 | quarto::integration components_render |
| 1 | quarto::integration convert_positron_cli |
| 1 | quarto::integration project_pandoc_gate_e2e |
| 1 | quarto::integration typst_html_table_css |
| 1 | quarto-core book_download |
| 1 | quarto-core extension |
| 1 | quarto-core stage |
| 1 | quarto-core transforms |
| 1 | quarto-core::integration book_cross_chapter_links |
| 1 | quarto-core::integration book_numbering_torture |
| 1 | quarto-core::integration book_part_appendix |
| 1 | quarto-core::integration book_theorem_crossref |
| 1 | quarto-core::integration crossref_custom_passthrough |
| 1 | quarto-core::integration config_path_consumers |
| 1 | quarto-core::integration engine_record_stage |
| 1 | quarto-core::integration filter_paths |
| 1 | quarto-core::integration front_matter_provenance |
| 1 | quarto-core::integration format_resolution |
| 1 | quarto-core::integration lightbox_pipeline |
| 1 | quarto-core::integration math_mode_pipeline |
| 1 | quarto-core::integration metadata_layers |
| 1 | quarto::integration smoke_all |
| 1 | quarto-core::integration marimo_records |
| 1 | quarto-core::integration pandoc_request_extension_filters |
| 1 | quarto-core::integration pandoc_shim_typst_crossref |
| 1 | quarto-core::integration pandoc_typst_template_partials |
| 1 | quarto-core::integration pandoc_typst_resource_copy |
| 1 | quarto-preview capture_driver |
| 1 | quarto-preview::integration render_scripts_boot |

## The RFU-6 findings: full output of the failing tests

### i1

- Summary [   1.533s] 31 tests run: 30 passed, 1 failed, 8294 skipped

#### `quarto-hub-provider::integration execute::a_figure_reported_under_the_materialized_root_is_captured`

```text
  stdout ───

    running 1 test
    test execute::a_figure_reported_under_the_materialized_root_is_captured ... FAILED

    failures:

    failures:
        execute::a_figure_reported_under_the_materialized_root_is_captured

    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 23 filtered out; finished in 0.92s

  stderr ───

    thread 'execute::a_figure_reported_under_the_materialized_root_is_captured' (9276) panicked at crates\quarto-hub-provider\tests\integration\execute.rs:266:5:
    assertion `left == right` failed: captures: [EngineCapture { engine_name: "test-figure", input_qmd: "---\nengine: test-figure\nexecute:\n  engine: test-figure\nformat: q2-preview\nquarto:\n  language:\n    announcement-close: Close announcement\n    appendix-attribution-bibtex: \"BibTeX citation ...[clipped 10497 chars]
      left: []
     right: ["doc_files/fig.png"]
    note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

```

### i4

- Summary [  14.209s] 22 tests run: 21 passed, 1 failed, 781 skipped

#### `quarto::integration convert_positron_cli::the_source_uri_default_is_the_documents_path_with_forward_slashes`

```text
  stdout ───

    running 1 test
    test convert_positron_cli::the_source_uri_default_is_the_documents_path_with_forward_slashes ... FAILED

    failures:

    failures:
        convert_positron_cli::the_source_uri_default_is_the_documents_path_with_forward_slashes

    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 513 filtered out; finished in 1.40s

  stderr ───

    thread 'convert_positron_cli::the_source_uri_default_is_the_documents_path_with_forward_slashes' (14828) panicked at crates\quarto\tests\integration\convert_positron_cli.rs:473:5:
    assertion `left == right` failed
      left: String("C:/Users/GordonWoodhull/AppData/Local/Temp/.tmpr9D6kf/doc.qmd")
     right: "//?/C:/Users/GordonWoodhull/AppData/Local/Temp/.tmpr9D6kf/doc.qmd"
    note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

```

### i6

- Summary [   9.753s] 84 tests run: 83 passed, 1 failed, 8544 skipped

#### `quarto-preview capture_driver::tests::a_record_only_cache_entry_is_published`

```text
  stdout ───

    running 1 test
    test capture_driver::tests::a_record_only_cache_entry_is_published ... FAILED

    failures:

    failures:
        capture_driver::tests::a_record_only_cache_entry_is_published

    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 249 filtered out; finished in 6.67s

  stderr ───

    thread 'capture_driver::tests::a_record_only_cache_entry_is_published' (16804) panicked at C:\Users\GordonWoodhull\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\tokio-1.53.1\src\runtime\scheduler\multi_thread\mod.rs:91:9:
    Cannot start a runtime from within a runtime. This happens because a function (like `block_on`) attempted to block the current thread while the thread is being used to drive asynchronous tasks.
    note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

```

