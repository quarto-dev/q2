<!--

## Quarto-Hub Changelog

Changelog entry format:

### YYYY-MM-DD

- [`<hash>`](https://github.com/quarto-dev/kyoto/commits/<hash>): One-sentence description

Group commits by date under level-three headers. Entries within each date should
be in reverse chronological order (latest first).

-->

### 2026-02-01

- [`a5261499`](https://github.com/quarto-dev/kyoto/commits/a5261499): Resolve meta shortcodes in document outline (headers like `# {{< meta title >}}` now show resolved values)
- [`6c300f58`](https://github.com/quarto-dev/kyoto/commits/6c300f58): Add meta shortcode resolution to rendering pipeline

### 2026-01-29

- [`f80ccc58`](https://github.com/quarto-dev/kyoto/commits/f80ccc58): Add SCSS resources versioning for cache invalidation
- [`976c4a9b`](https://github.com/quarto-dev/kyoto/commits/976c4a9b): Add SCSS styling for editorial marks
- [`dfcb6b90`](https://github.com/quarto-dev/kyoto/commits/dfcb6b90): Add collapsible sections to OutlinePanel
- [`03fa5765`](https://github.com/quarto-dev/kyoto/commits/03fa5765): Add collapsible nested folder tree to FileSidebar
- [`0c6ed3e6`](https://github.com/quarto-dev/kyoto/commits/0c6ed3e6): Restrict preview and QMD features to .qmd files only
- [`56e61953`](https://github.com/quarto-dev/kyoto/commits/56e61953): Add deep linking support with URL-based file navigation and multi-tab support
- [`1dcd6bae`](https://github.com/quarto-dev/kyoto/commits/1dcd6bae): Add WASM end-to-end tests for compute_theme_content_hash
- [`d4160a0c`](https://github.com/quarto-dev/kyoto/commits/d4160a0c): Implement content-based merkle hash for SASS cache keys to fix stale CSS when editing custom themes
- [`9715102c`](https://github.com/quarto-dev/kyoto/commits/9715102c): Fix custom SCSS theme file resolution by passing document path through rendering pipeline

### 2026-01-28

- [`bad2aab6`](https://github.com/quarto-dev/kyoto/commits/bad2aab6): Add TOC rendering support to hub-client
- [`8867e5dc`](https://github.com/quarto-dev/kyoto/commits/8867e5dc): Fix theme changes not updating preview and reduce flash of unstyled content

### 2026-01-27

- [`7053e539`](https://github.com/quarto-dev/kyoto/commits/7053e539): Add bootstrap-test-fixtures command and generate initial E2E fixtures
- [`d470b1b3`](https://github.com/quarto-dev/kyoto/commits/d470b1b3): Add utility tests (stripAnsi, diagnosticToMonaco) and E2E fixture script
- [`d7e55db9`](https://github.com/quarto-dev/kyoto/commits/d7e55db9): Add testing infrastructure with mock utilities and Playwright E2E setup

### 2026-01-26

- [`fe5d0523`](https://github.com/quarto-dev/kyoto/commits/fe5d0523): Add support for SCSS compilation

### 2026-01-22

- [`efb6ac6e`](https://github.com/quarto-dev/kyoto/commits/efb6ac6e): Divert ctrl/cmd+s to pop up a toast instead of triggering browser save dialog
- [`ee0c6ce0`](https://github.com/quarto-dev/kyoto/commits/ee0c6ce0): Fix paste handling to prevent Monaco snippet expansion artifacts

### 2026-01-20

- [`2977a1db`](https://github.com/quarto-dev/kyoto/commits/2977a1db): Fix OutlinePanel crashes and flash on refresh
- [`2e73417`](https://github.com/quarto-dev/kyoto/commits/2e73417): Add LSP infrastructure and document outline panel

### 2026-01-16

- [`45da7f5`](https://github.com/quarto-dev/kyoto/commits/45da7f5): Fix cursor jump bug during rapid typing by switching Monaco to uncontrolled mode
- [`f371d82`](https://github.com/quarto-dev/kyoto/commits/f371d82): Fix preview link handling for external links and cross-document anchors
- [`f447654`](https://github.com/quarto-dev/kyoto/commits/f447654): Add persistent user preferences with zod validation (scroll sync, error overlay)
- [`4b9db07`](https://github.com/quarto-dev/kyoto/commits/4b9db07): Update browser tab title and favicon for Quarto Hub

### 2026-01-14

- [`fb347c8`](https://github.com/quarto-dev/kyoto/commits/fb347c8): Extract Automerge schema and sync client into reusable packages

### 2026-01-12

- [`55ade12`](https://github.com/quarto-dev/kyoto/commits/55ade12): Add Create New Project feature with project type selection
- [`1c52e8e`](https://github.com/quarto-dev/kyoto/commits/1c52e8e): Rename 'Add New Project' to 'Connect to Project'

### 2026-01-10

- [`6c429f3`](https://github.com/quarto-dev/kyoto/commits/6c429f3): Fix scroll sync rescrolling when editing documents with images
- [`e1801af`](https://github.com/quarto-dev/kyoto/commits/e1801af): Add internal drag-drop from Files pane to editor for images and qmd links
- [`7fed669`](https://github.com/quarto-dev/kyoto/commits/7fed669): Fix race condition in image drop markdown insertion
- [`eed9975`](https://github.com/quarto-dev/kyoto/commits/eed9975): Add drag-drop image upload to Monaco editor with markdown insertion
- [`81fed79`](https://github.com/quarto-dev/kyoto/commits/81fed79): Add More Information modal and refactor markdown viewer
- [`b0ddb29`](https://github.com/quarto-dev/kyoto/commits/b0ddb29): Add changelog view to About tab
- [`e6f742c`](https://github.com/quarto-dev/kyoto/commits/e6f742c): Refactor navigation to VS Code-style collapsible sidebar sections

### 2026-01-09

- [`bafe8d0`](https://github.com/quarto-dev/kyoto/commits/bafe8d0): Add file rename support
- [`50a6ef1`](https://github.com/quarto-dev/kyoto/commits/50a6ef1): Add file management UI with sidebar and upload dialog
- [`8e49c2b`](https://github.com/quarto-dev/kyoto/commits/8e49c2b): Add VFS binary file reading for preview images
- [`1709572`](https://github.com/quarto-dev/kyoto/commits/1709572): Add binary document support to hub-client

### 2026-01-08

- [`9660689`](https://github.com/quarto-dev/kyoto/commits/9660689): Fix preview not updating after undo or identical HTML renders
- [`5f61597`](https://github.com/quarto-dev/kyoto/commits/5f61597): Retain last good preview when markdown syntax errors occur

### 2026-01-07

- [`27feb31`](https://github.com/quarto-dev/kyoto/commits/27feb31): Add git commit hash display to project selector page
- [`2541f22`](https://github.com/quarto-dev/kyoto/commits/2541f22): Fix race condition in automerge sync causing document unavailable errors

### 2026-01-06

- [`806703b`](https://github.com/quarto-dev/kyoto/commits/806703b): Add PipelineStage abstraction for unified async render pipeline
