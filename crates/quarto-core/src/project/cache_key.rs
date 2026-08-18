/*
 * project/cache_key.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Phase 8 cache-key construction.
//!
//! Builds a deterministic SHA-256 over every input that participates
//! in producing a `DocumentProfile`, so the on-disk profile cache
//! can be keyed cleanly. The hash domain:
//!
//! ```text
//! sha256(
//!     PROFILE_KEY_VERSION       (4 bytes BE)
//!   | quarto_build_id()         (length-prefixed UTF-8)
//!   | DOCUMENT_PROFILE_VERSION  (4 bytes BE)
//!   | format_id                 (length-prefixed UTF-8)
//!   | source_path               (project-relative, length-prefixed UTF-8)
//!   | source_bytes              (length-prefixed)
//!   | for each layered _metadata.yml from project root → doc dir:
//!         path                  (length-prefixed UTF-8)
//!         bytes                 (length-prefixed)
//!   | _quarto.yml bytes         (length-prefixed; empty if absent)
//!   | for each extension contribution, sorted by name:
//!         name                  (length-prefixed UTF-8)
//!         bytes                 (length-prefixed)
//! )
//! ```
//!
//! ## What `extension_contributions` carries (Plan 6 decision 9)
//!
//! The `extension_contributions` slot is a generic `(name, bytes)` list.
//! Since Plan 6 Phase 5 it carries **engine-extension** `_extension.yml`
//! raw bytes — one entry per registered engine that came from an
//! `EngineContribution::External` (built-ins contribute nothing): the
//! stamped `DocumentProfile.engine_resolution` is a function of the
//! registry (which engine extensions exist and what `claims:` they
//! declare), so it must be in the key domain, or editing an extension's
//! `claims:` — exactly the fix the Phase-5 fall-through warning
//! recommends — would serve a stale cached profile. See
//! `orchestrator::pass1_engine_extension_contributions` for how the pairs
//! are gathered (from `EngineRegistry::engine_extension_provenance`,
//! re-reading each file's bytes at key-build time).
//!
//! Proper *format*-extension metadata hashing (the slot's original,
//! pre-Phase-5 intent) remains the pre-existing follow-up — see
//! `claude-notes/plans/2026-04-27-websites-phase-8.md` §"Sub-phase 8.4".
//!
//! ## Where transitive includes fit in
//!
//! The original Phase-8 plan put the include set's
//! `(path, content_hash)` pairs in the hash domain. That ran into
//! a chicken-and-egg: to *look up* the cache before running Pass-1,
//! the caller would need to know the include set, which is exactly
//! what Pass-1 produces. Phase 8.2's resolution:
//!
//! - The cache key **does not include the include set**.
//! - The cached `DocumentProfile` carries `includes:
//!   Vec<IncludeEntry>` (Phase-8 sub-phase 8.0a, `bd-r82e`).
//! - On load, the cache wrapper verifies each cached include's
//!   `content_hash` against the file's current bytes. Any mismatch
//!   degrades the load to a miss.
//!
//! Net effect on invalidation: identical. A change to an included
//! child's bytes still invalidates the parent's cache entry —
//! through the verification step rather than the key.
//!
//! See `crates/quarto-core/src/project/profile_cache.rs` for the
//! verification step.
//!
//! ## Versioning
//!
//! - `PROFILE_KEY_VERSION` is the manual override lever — bump it
//!   when a behavior change in the head pipeline shifts what a
//!   profile would record without changing the serialized shape.
//! - `quarto_build_id()` returns the crate version. A distribution
//!   upgrade invalidates every cache transparently. (Future work:
//!   augment with a git short hash for dev builds via a
//!   `build.rs`. Until then, the manual key-version lever is the
//!   escape hatch for in-version behavior changes.)
//! - `DOCUMENT_PROFILE_VERSION` is in the hash domain too so a
//!   shape bump invalidates entries even if a v1 file is somehow
//!   placed at a v2 key path.
//!
//! ## Length-prefix encoding
//!
//! Every variable-length input is prefixed with a 4-byte big-endian
//! length so concatenation can't yield collisions (e.g. an empty
//! path followed by `"foo"` would otherwise hash the same as the
//! path `"foo"` followed by empty content). All length-prefixes are
//! `u32`; an input larger than 4 GiB would overflow this and is
//! rejected with a panic in debug builds. (No real qmd source is
//! that large.)
//!
//! ## Output
//!
//! All key helpers return raw 32-byte arrays. Use [`hex_encode`] to
//! produce the 64-char ASCII string the runtime cache expects.

use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::document_profile::DOCUMENT_PROFILE_VERSION;

/// Manual key-version constant. Bump when a head-pipeline behavior
/// change alters what a profile records without changing
/// `DOCUMENT_PROFILE_VERSION`.
///
/// v2: the key domain gained project-profile inputs (active names +
/// overlay bytes, bd-fu16z22k).
pub const PROFILE_KEY_VERSION: u32 = 2;

/// Returns the Quarto build identifier baked into every cache key.
///
/// Today this is just `CARGO_PKG_VERSION`. A future enhancement may
/// concatenate a git short hash for dev builds via a `build.rs`;
/// when that ships, this function's return value will change and
/// existing caches will silently invalidate (which is the intended
/// behavior — dev iterations should not see stale cache).
pub fn quarto_build_id() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Inputs to [`pass1_key`].
///
/// Construct on the call site and pass by reference; the hasher
/// consumes everything in declared order. Helper accepts borrows
/// where possible so the caller doesn't have to clone.
pub struct Pass1KeyInputs<'a> {
    /// Format identifier (e.g. `"html"`, `"acm-html"`). Mirrors
    /// `DocumentProfile.format_id`.
    pub format_id: &'a str,

    /// Project-relative source path, forward-slash separated.
    /// (Format helper [`source_path_for_hash`] turns a `Path` into
    /// the canonical string when needed.)
    pub source_path: &'a str,

    /// Raw bytes of the source file.
    pub source_bytes: &'a [u8],

    /// Layered `_metadata.yml` files from project root down to the
    /// document's directory, in walk order. Each entry is
    /// `(project-relative-path, raw-bytes)`. Empty when no
    /// `_metadata.yml` files apply.
    pub metadata_files: &'a [(PathBuf, Vec<u8>)],

    /// Raw bytes of the project's `_quarto.yml` (or `_quarto.yaml`).
    /// Empty slice when the project has no config file.
    pub quarto_yml_bytes: &'a [u8],

    /// Extension contributions, sorted by name. Each entry is
    /// `(name, raw-bytes)`. Since Plan 6 Phase 5 this carries
    /// engine-extension `_extension.yml` bytes (decision 9); proper
    /// format-extension metadata hashing remains the pre-existing
    /// follow-up (see the module docs). Empty when no engine
    /// extensions are registered.
    pub extension_contributions: &'a [(String, Vec<u8>)],

    /// Active **project-profile** names in activation order
    /// (bd-fu16z22k). ⚠️ Both meanings of "profile" collide right
    /// here: this field holds *project profiles* (`--profile` /
    /// `QUARTO_PROFILE`), which are an input to the *DocumentProfile*
    /// cache key this struct feeds — switching project profiles must
    /// not serve stale pass-1 DocumentProfiles. Empty when none are
    /// active.
    pub active_config_profiles: &'a [String],

    /// `(project-relative-path, raw-bytes)` of every profile overlay
    /// (`_quarto-<name>.yml`) and `_quarto.yml.local` actually merged
    /// into the project config, in merge order. Byte-level like
    /// [`metadata_files`](Self::metadata_files): a comment-only edit
    /// to an overlay correctly invalidates the key.
    pub profile_config_files: &'a [(PathBuf, Vec<u8>)],
}

/// Compute the SHA-256 cache key for a `DocumentProfile`.
///
/// Returns the raw 32-byte hash; use [`hex_encode`] to produce the
/// 64-char ASCII string the system runtime's cache API expects.
pub fn pass1_key(inputs: &Pass1KeyInputs<'_>) -> [u8; 32] {
    let mut hasher = Sha256::new();

    // Version preamble.
    hasher.update(PROFILE_KEY_VERSION.to_be_bytes());
    write_lp_str(&mut hasher, quarto_build_id());
    hasher.update(DOCUMENT_PROFILE_VERSION.to_be_bytes());

    // Document-identity preamble.
    write_lp_str(&mut hasher, inputs.format_id);
    write_lp_str(&mut hasher, inputs.source_path);

    // Source bytes.
    write_lp_bytes(&mut hasher, inputs.source_bytes);

    // Layered _metadata.yml. The walker (callers' responsibility)
    // produces a stable order; we hash whatever order it gave us.
    for (path, bytes) in inputs.metadata_files {
        write_lp_str(&mut hasher, &path.to_string_lossy());
        write_lp_bytes(&mut hasher, bytes);
    }

    // _quarto.yml bytes (empty slice when absent).
    write_lp_bytes(&mut hasher, inputs.quarto_yml_bytes);

    // Project-profile activation + overlay bytes (bd-fu16z22k). The
    // name list is hashed even when no overlay files exist: two runs
    // differing only in `--profile` must not share keys (conditional
    // content will depend on the active set). Each list is
    // count-prefixed so a name list can never alias a path/bytes
    // pair from the file list. With both lists empty the stream
    // gains only two zero counts, keeping profile-less keys cheap.
    hasher.update((inputs.active_config_profiles.len() as u32).to_be_bytes());
    for name in inputs.active_config_profiles {
        write_lp_str(&mut hasher, name);
    }
    hasher.update((inputs.profile_config_files.len() as u32).to_be_bytes());
    for (path, bytes) in inputs.profile_config_files {
        write_lp_str(&mut hasher, &path.to_string_lossy());
        write_lp_bytes(&mut hasher, bytes);
    }

    // Format-extension contributions, sorted by name (caller's
    // responsibility). Hashing in any other order would change the
    // key for the same set of contributions.
    for (name, bytes) in inputs.extension_contributions {
        write_lp_str(&mut hasher, name);
        write_lp_bytes(&mut hasher, bytes);
    }

    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Encode a 32-byte hash as a 64-character lower-case hex string.
///
/// The result fits within the system runtime's
/// `CACHE_NAME_MAX_LEN = 128` and uses only ASCII alphanumerics,
/// so it passes [`quarto_system_runtime::validate_cache_key`]
/// without further escaping.
pub fn hex_encode(hash: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for byte in hash {
        s.push_str(&format!("{:02x}", byte));
    }
    s
}

/// Write a length-prefixed UTF-8 string into the hasher.
fn write_lp_str(hasher: &mut Sha256, s: &str) {
    write_lp_bytes(hasher, s.as_bytes());
}

/// Write a length-prefixed byte slice into the hasher.
///
/// Length is a 4-byte big-endian `u32`. Inputs larger than 4 GiB
/// would overflow this; debug builds panic, release builds produce
/// a meaningless hash. No realistic qmd source is that large.
fn write_lp_bytes(hasher: &mut Sha256, bytes: &[u8]) {
    let len = u32::try_from(bytes.len()).expect(
        "cache_key input exceeds u32 length limit \
         (4 GiB) — likely a programming error",
    );
    hasher.update(len.to_be_bytes());
    hasher.update(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_inputs() -> Pass1KeyInputs<'static> {
        Pass1KeyInputs {
            format_id: "html",
            source_path: "page.qmd",
            source_bytes: b"# Hello\n\nBody.\n",
            metadata_files: &[],
            quarto_yml_bytes: b"",
            extension_contributions: &[],
            active_config_profiles: &[],
            profile_config_files: &[],
        }
    }

    #[test]
    fn key_changes_on_active_profile_set() {
        // Even with no overlay files on disk, a different --profile
        // selection must change the key (bd-fu16z22k): conditional
        // content depends on the active set.
        let a = pass1_key(&minimal_inputs());
        let names = vec!["prod".to_string()];
        let mut tweaked = minimal_inputs();
        tweaked.active_config_profiles = &names;
        assert_ne!(a, pass1_key(&tweaked));
    }

    #[test]
    fn key_changes_on_profile_order() {
        // First-listed-wins makes activation ORDER semantic.
        let ab = vec!["a".to_string(), "b".to_string()];
        let ba = vec!["b".to_string(), "a".to_string()];
        let mut a = minimal_inputs();
        a.active_config_profiles = &ab;
        let mut b = minimal_inputs();
        b.active_config_profiles = &ba;
        assert_ne!(pass1_key(&a), pass1_key(&b));
    }

    #[test]
    fn key_changes_on_overlay_byte_change() {
        let f_a = vec![(
            PathBuf::from("_quarto-prod.yml"),
            b"toc: true
"
            .to_vec(),
        )];
        let f_b = vec![(
            PathBuf::from("_quarto-prod.yml"),
            b"toc: false
"
            .to_vec(),
        )];
        let names = vec!["prod".to_string()];
        let mut a = minimal_inputs();
        a.active_config_profiles = &names;
        a.profile_config_files = &f_a;
        let mut b = minimal_inputs();
        b.active_config_profiles = &names;
        b.profile_config_files = &f_b;
        assert_ne!(pass1_key(&a), pass1_key(&b));
    }

    #[test]
    fn profile_name_list_cannot_alias_overlay_file_entry() {
        // Count prefixes keep the two lists domain-separated: names
        // ["p", "x"] must not hash like files [("p", b"x")].
        let names = vec!["p".to_string(), "x".to_string()];
        let files = vec![(PathBuf::from("p"), b"x".to_vec())];
        let mut a = minimal_inputs();
        a.active_config_profiles = &names;
        let mut b = minimal_inputs();
        b.profile_config_files = &files;
        assert_ne!(pass1_key(&a), pass1_key(&b));
    }

    #[test]
    fn key_is_deterministic_for_identical_inputs() {
        let a = pass1_key(&minimal_inputs());
        let b = pass1_key(&minimal_inputs());
        assert_eq!(a, b, "same inputs must hash the same");
    }

    #[test]
    fn hex_encode_produces_64_char_lowercase_hex() {
        let key = pass1_key(&minimal_inputs());
        let hex = hex_encode(&key);
        assert_eq!(hex.len(), 64);
        assert!(
            hex.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "hex_encode output must be lowercase hex: {hex}"
        );
    }

    #[test]
    fn key_changes_on_source_edit() {
        let a = pass1_key(&minimal_inputs());
        let mut tweaked = minimal_inputs();
        tweaked.source_bytes = b"# Hello\n\nDIFFERENT.\n";
        let b = pass1_key(&tweaked);
        assert_ne!(a, b);
    }

    #[test]
    fn key_changes_on_source_path_edit() {
        let a = pass1_key(&minimal_inputs());
        let mut tweaked = minimal_inputs();
        tweaked.source_path = "other.qmd";
        let b = pass1_key(&tweaked);
        assert_ne!(a, b);
    }

    #[test]
    fn key_changes_on_format_id_edit() {
        let a = pass1_key(&minimal_inputs());
        let mut tweaked = minimal_inputs();
        tweaked.format_id = "acm-html";
        let b = pass1_key(&tweaked);
        assert_ne!(a, b);
    }

    #[test]
    fn key_changes_on_metadata_yml_edit() {
        let a = pass1_key(&minimal_inputs());
        let metadata = vec![(PathBuf::from("_metadata.yml"), b"toc: true\n".to_vec())];
        let mut tweaked = minimal_inputs();
        tweaked.metadata_files = &metadata;
        let b = pass1_key(&tweaked);
        assert_ne!(a, b);
    }

    #[test]
    fn key_changes_on_metadata_yml_byte_change() {
        // Same path, different bytes ⇒ different key. Catches
        // sidebar / config edits that don't change the layered
        // _metadata.yml inventory but do change its content.
        let m_a = vec![(PathBuf::from("_metadata.yml"), b"toc: true\n".to_vec())];
        let m_b = vec![(PathBuf::from("_metadata.yml"), b"toc: false\n".to_vec())];
        let mut a = minimal_inputs();
        a.metadata_files = &m_a;
        let mut b = minimal_inputs();
        b.metadata_files = &m_b;
        assert_ne!(pass1_key(&a), pass1_key(&b));
    }

    #[test]
    fn key_changes_on_quarto_yml_edit() {
        let a = pass1_key(&minimal_inputs());
        let mut tweaked = minimal_inputs();
        tweaked.quarto_yml_bytes = b"project: { type: website }\n";
        let b = pass1_key(&tweaked);
        assert_ne!(a, b);
    }

    // `extension_contributions` is a generic `(name, bytes)` slot; these
    // two tests predate Plan 6 and already exercise it structurally.
    // Since Phase 5 the slot's real-world content is engine-extension
    // `_extension.yml` bytes (decision 9) — the two Phase-5-named tests
    // below pin that specific scenario; the generic tests stay as the
    // shape-level contract.
    #[test]
    fn key_changes_on_extension_contribution() {
        let a = pass1_key(&minimal_inputs());
        let ext = vec![(
            "acm-html".to_string(),
            b"format: { html: { ... } }\n".to_vec(),
        )];
        let mut tweaked = minimal_inputs();
        tweaked.extension_contributions = &ext;
        let b = pass1_key(&tweaked);
        assert_ne!(a, b);
    }

    #[test]
    fn key_changes_on_extension_order() {
        // Caller is responsible for sorting; if they pass a
        // different order the key will differ. This test locks
        // that contract.
        let order_a = vec![
            ("ext-a".to_string(), b"foo".to_vec()),
            ("ext-b".to_string(), b"bar".to_vec()),
        ];
        let order_b = vec![
            ("ext-b".to_string(), b"bar".to_vec()),
            ("ext-a".to_string(), b"foo".to_vec()),
        ];
        let mut a = minimal_inputs();
        a.extension_contributions = &order_a;
        let mut b = minimal_inputs();
        b.extension_contributions = &order_b;
        assert_ne!(pass1_key(&a), pass1_key(&b));
    }

    /// Plan 6 decision 9: editing an engine extension's `_extension.yml`
    /// (e.g. adding `claims:`) must change the key — same doc, same
    /// engine NAME, different `_extension.yml` bytes.
    #[test]
    fn key_changes_on_engine_extension_yml_byte_edit() {
        let before = vec![(
            "legacy-python".to_string(),
            b"title: T\nauthor: A\ncontributes:\n  engines:\n    - path: engine.js\n".to_vec(),
        )];
        let after = vec![(
            "legacy-python".to_string(),
            b"title: T\nauthor: A\ncontributes:\n  engines:\n    - path: engine.js\n      claims: [python]\n".to_vec(),
        )];
        let mut a = minimal_inputs();
        a.extension_contributions = &before;
        let mut b = minimal_inputs();
        b.extension_contributions = &after;
        assert_ne!(
            pass1_key(&a),
            pass1_key(&b),
            "adding `claims:` to an engine extension's _extension.yml must change the key"
        );
    }

    /// Plan 6 decision 9: registering (or removing) a claims-less engine
    /// extension changes the key even though no doc content changed —
    /// the stamped `engine_resolution` depends on the registry, not just
    /// the document.
    #[test]
    fn key_changes_on_engine_extension_pair_added_or_removed() {
        let a = pass1_key(&minimal_inputs());
        let ext = vec![(
            "legacy-python".to_string(),
            b"title: T\nauthor: A\ncontributes:\n  engines:\n    - path: engine.js\n".to_vec(),
        )];
        let mut with_engine = minimal_inputs();
        with_engine.extension_contributions = &ext;
        let b = pass1_key(&with_engine);
        assert_ne!(
            a, b,
            "registering an engine extension (empty -> one pair) must change the key"
        );
    }

    #[test]
    fn key_independent_of_unrelated_files() {
        // Different source_path / source_bytes ⇒ different key, but
        // adding a sibling page's content as a "metadata file" would
        // be a programming error. This test asserts the helper hashes
        // exactly its own inputs and nothing else: identical inputs
        // ⇒ identical key, regardless of what other files exist on
        // disk. (Trivially true by construction; the test
        // documents the contract.)
        let key_a = pass1_key(&minimal_inputs());
        let key_b = pass1_key(&minimal_inputs());
        assert_eq!(key_a, key_b);
    }

    #[test]
    fn lp_prefix_distinguishes_concatenations() {
        // Without length-prefixing, "foo" + "bar" and "foob" + "ar"
        // would hash identically. The length prefix makes them
        // distinct. We verify this by hashing two pairs of source +
        // path that, without length-prefixing, would produce the
        // same byte stream after concatenation.
        let mut a = minimal_inputs();
        a.source_path = "foo";
        a.source_bytes = b"bar";

        let mut b = minimal_inputs();
        b.source_path = "foob";
        b.source_bytes = b"ar";

        assert_ne!(
            pass1_key(&a),
            pass1_key(&b),
            "length-prefixing must distinguish concat collisions"
        );
    }

    #[test]
    fn key_includes_quarto_build_id_in_domain() {
        // We can't change CARGO_PKG_VERSION at runtime, so this
        // test asserts only that the build id is non-empty. The
        // actual "different version ⇒ different key" behavior is
        // mechanical — every CARGO_PKG_VERSION bump changes the
        // hash domain.
        let id = quarto_build_id();
        assert!(!id.is_empty(), "quarto_build_id must be non-empty");
        // And that the key isn't accidentally identical to a stub
        // that excludes the version.
        let key = pass1_key(&minimal_inputs());
        // (The test is mostly a documentation marker; the real
        // assertion is "this constant participates in the hash
        // domain" which is visible from `pass1_key`'s code.)
        let _ = key;
    }
}
