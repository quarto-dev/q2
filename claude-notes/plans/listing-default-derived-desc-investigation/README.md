# Investigation artifacts for bd-listing-default-no-derived-desc-m0wrr8ty

- `repro/` — minimal website: default listing over two prose-only posts. Render with
  `cargo run --bin q2 -- render claude-notes/plans/listing-default-derived-desc-investigation/repro`.
  At HEAD (0.25.0 / 87c0e21a): `_site/index.html` has zero `listing-description` and zero
  `listing-reading-time`; cached profiles have `listing_item: null`.
- `spike-output-index.html` — host page rendered with `ListingItemInfoStage` added to the
  Pass-1 head pipeline in `orchestrator.rs::pass1_profile_single_file_live` (spike reverted).
  Shows derived, truncated descriptions and reading times for both items.

Plan: `../2026-08-20-listing-default-derived-description.md`.
