# Merge new-react-renderer-stuff into main

## Overview
Merge the React renderer and slide functionality from `new-react-renderer-stuff` branch into `main`, preserving both old and new functionality where conflicts exist.

## Branch Comparison
- 15 commits ahead in new-react-renderer-stuff
- Main changes: React rendering pipeline, slides support, thumbnails, cursor sync
- 7 new files (mostly React components/hooks)
- 11 modified files (Rust WASM, React components, styles)

## Work Items

### Phase 1: Analysis
- [ ] Review modified Rust files for conflicts
- [ ] Review modified TypeScript/React files for conflicts
- [ ] Identify areas needing feature flags

### Phase 2: Merge Strategy
- [ ] Attempt git merge to see automatic conflict resolution
- [ ] Handle conflicts in Rust files (pipeline.rs, WASM entry points)
- [ ] Handle conflicts in React files (Editor.tsx, etc.)
- [ ] Add feature flags where old/new functionality differs

### Phase 3: Testing & Validation
- [ ] Build Rust workspace (`cargo build --workspace`)
- [ ] Build WASM module
- [ ] Build hub-client
- [ ] Test basic functionality
- [ ] Verify both rendering paths work

### Phase 4: Cleanup
- [ ] Update changelog if needed
- [ ] Commit merge results
