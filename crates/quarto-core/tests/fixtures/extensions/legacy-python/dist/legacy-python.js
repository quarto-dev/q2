// crates/quarto-core/tests/fixtures/extensions/legacy-python/dist/legacy-python.js
//
// Trivial stub for the Plan 6 Phase 5 claims-less-engine fixture. This
// extension declares NO `claims:` in _extension.yml, so
// `resolve_engines_pass1`'s no-load predicate treats it as "would-load"
// for any language it isn't covered by a metadata claim table for —
// which means every test exercising this fixture is designed to never
// reach the point of actually loading this module (Pass-1 either lifts
// via a claim table or falls through before any load happens; Pass-2
// execution is intentionally never exercised by these tests). This file
// is registration-bait only: `build_engine_registry`'s bundle-exists
// check requires SOME file at this path.
throw new Error(
  "legacy-python.js must never be loaded — Plan 6 Phase 5 fixture tests " +
    "only exercise the no-load Pass-1 path",
);
