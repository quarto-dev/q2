//! Phase 0 tests #3 and #12 — `GitBlameProvider` porcelain parsing
//! plus producer invariant.
//!
//! Fixtures live as **checked-in porcelain text** under
//! `tests/fixtures/attribution-blame/` so these unit tests don't
//! depend on live commit timestamps or git being installed. The
//! `REGEN.md` file in that directory documents how to refresh them.

use std::sync::Arc;

use quarto_core::attribution::{
    AttributionSourceProvider, BlameLine, BlameRun, GitBlameProvider, actor_color,
    attribution_from_porcelain, build_blame_runs, fnv1a_hex8, parse_blame_porcelain,
};

// ===========================================================================
// Phase 0 test #3 — Parses porcelain identically to TS reference
// ===========================================================================

#[test]
fn parse_single_commit_single_line() {
    let porcelain = include_str!("../fixtures/attribution-blame/single-commit.porcelain");
    let parsed = parse_blame_porcelain(porcelain);
    assert_eq!(parsed.len(), 1);
    assert_eq!(
        parsed[0],
        BlameLine {
            author: "Alice".to_string(),
            author_mail: "alice@example.com".to_string(),
            committer_time: 1_700_000_000,
        }
    );
}

/// Regression: when a commit is back-dated (`git commit --date=PAST`
/// or any rebase / cherry-pick / amend), the porcelain reports a
/// past `author-time` alongside a present `committer-time`. The
/// run's `time` field, which feeds `data-attr-time` and ultimately
/// the rendered relative-time badge, must follow committer-time so
/// the viewer surfaces "when this line was committed to the branch"
/// rather than "when its author originally wrote it back in 2023".
///
/// The porcelain block carries both `author-time 1700000000` and
/// `committer-time 1900000000`; only the latter must survive into
/// `BlameRun.time`.
#[test]
fn parse_uses_committer_time_for_run_time_even_with_backdated_author() {
    let porcelain = "\
abcdef0123456789abcdef0123456789abcdef01 1 1 1
author Alice
author-mail <alice@example.com>
author-time 1700000000
author-tz +0000
committer Alice
committer-mail <alice@example.com>
committer-time 1900000000
committer-tz +0000
summary backdated
boundary
filename doc.qmd
\thello
";
    let parsed = parse_blame_porcelain(porcelain);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].committer_time, 1_900_000_000);

    let runs = build_blame_runs(&parsed, "hello\n").expect("build runs");
    assert_eq!(runs.len(), 1);
    assert_eq!(
        runs[0].time, 1_900_000_000,
        "run.time must follow committer-time; a regression to \
         author-time (1700000000) would make back-dated commits look \
         ancient in the rendered viewer"
    );
}

#[test]
fn parse_caches_commit_metadata_across_lines_from_same_commit() {
    // The fixture has commit `aaa...` emitting both line 1 and line 2;
    // the second line record has only `<hash> 2 2` and a `\t...` body,
    // with no author block — the parser must hydrate from cache.
    let porcelain = include_str!("../fixtures/attribution-blame/multi-commit.porcelain");
    let parsed = parse_blame_porcelain(porcelain);
    assert!(parsed.len() >= 2);
    assert_eq!(parsed[0].author_mail, "alice@example.com");
    assert_eq!(parsed[1].author_mail, "alice@example.com");
    assert_eq!(parsed[0].committer_time, parsed[1].committer_time);
}

#[test]
fn parse_empty_porcelain_returns_empty_vec() {
    assert!(parse_blame_porcelain("").is_empty());
}

#[test]
fn build_runs_handles_multi_byte_utf8() {
    // 世界\n is 3+3+1 = 7 bytes.
    let blame = vec![BlameLine {
        author: "Alice".into(),
        author_mail: "alice@x".into(),
        committer_time: 1,
    }];
    let runs = build_blame_runs(&blame, "世界\n").expect("build runs");
    assert_eq!(
        runs,
        vec![BlameRun {
            byte_start: 0,
            byte_end: 7,
            actor: "alice@x".into(),
            time: 1,
        }]
    );
}

#[test]
fn build_runs_handles_text_without_trailing_newline() {
    let blame = vec![
        BlameLine {
            author: "A".into(),
            author_mail: "a@x".into(),
            committer_time: 1,
        },
        BlameLine {
            author: "B".into(),
            author_mail: "b@x".into(),
            committer_time: 2,
        },
    ];
    let runs = build_blame_runs(&blame, "foo\nbar").expect("build runs");
    assert_eq!(
        runs,
        vec![
            BlameRun {
                byte_start: 0,
                byte_end: 4,
                actor: "a@x".into(),
                time: 1,
            },
            BlameRun {
                byte_start: 4,
                byte_end: 7,
                actor: "b@x".into(),
                time: 2,
            },
        ]
    );
}

#[test]
fn build_runs_errors_on_line_count_mismatch() {
    // Empty blame vs non-empty text — must error.
    let blame: Vec<BlameLine> = Vec::new();
    let result = build_blame_runs(&blame, "hello\n");
    assert!(
        result.is_err(),
        "line-count mismatch must error, not silently accept"
    );
}

// ===========================================================================
// Phase 0 test #12 — GitBlameProvider producer invariant
// ===========================================================================
//
// Every actor referenced by `runs` has an entry in `identities`,
// each entry's `display_name` equals the mail-local-part, and `color`
// equals `actor_color(fnv1a_hex8(email))`. Pin the deterministic
// colour for a known email so a future refactor of `fnv1a_hex8` can't
// silently shift hues.

#[test]
fn fnv1a_hex8_is_deterministic_and_well_distributed() {
    // Sanity: two arbitrary strings hash differently.
    let h_alice = fnv1a_hex8("alice@example.com");
    let h_bob = fnv1a_hex8("bob@example.com");
    assert_ne!(h_alice, h_bob);
    assert_eq!(h_alice.len(), 8);
    assert!(
        h_alice.chars().all(|c| c.is_ascii_hexdigit()),
        "fnv1a_hex8 output must be lowercase hex"
    );
    // Stability: calling twice with the same input gives the same answer.
    assert_eq!(h_alice, fnv1a_hex8("alice@example.com"));
}

#[test]
fn actor_color_is_deterministic_and_returns_a_hex_palette_entry() {
    let c = actor_color("aabbccdd");
    assert!(
        c.starts_with('#') && c.len() == 7,
        "actor_color must return a hex string from the Tol Muted palette; got: {c}"
    );
    assert_eq!(c, actor_color("aabbccdd"), "deterministic");
}

#[test]
fn gitblame_provider_constructs_as_trait_object() {
    // Pin: GitBlameProvider implements AttributionSourceProvider so
    // the dyn-trait construction in RenderContext::attribution_provider
    // works.
    let provider = GitBlameProvider::new();
    let _typed: Arc<dyn AttributionSourceProvider> = Arc::new(provider);
}

#[test]
fn gitblame_single_author_fixture_satisfies_producer_invariant() {
    // `single-commit.porcelain` blames a one-line file (`hello\n`)
    // to alice@example.com.
    let porcelain = include_str!("../fixtures/attribution-blame/single-commit.porcelain");
    let data = attribution_from_porcelain(porcelain, "hello\n").expect("assemble");

    let alice: Arc<str> = Arc::from("alice@example.com");
    // Every actor referenced by runs has an identity entry.
    for run in data.runs.as_slice() {
        assert!(
            data.identities.contains_key(&run.actor),
            "producer invariant violated: actor {:?} missing from identities",
            run.actor
        );
    }
    let id = data.identities.get(&alice).expect("alice identity present");
    assert_eq!(id.display_name, "alice");
    // Pin the deterministic colour for alice@example.com so a future
    // refactor of fnv1a_hex8 or the Tol Muted palette can't silently
    // shift the assignment.
    assert_eq!(id.color, "#117733");
}

#[test]
fn gitblame_multi_author_fixture_satisfies_producer_invariant() {
    // `multi-commit.porcelain` blames a four-line file:
    //   line1\n            -> alice@example.com
    //   世界\n             -> alice@example.com
    //   line3\n            -> bob@example.com
    //   line4\n            -> bob@example.com
    let porcelain = include_str!("../fixtures/attribution-blame/multi-commit.porcelain");
    let source = "line1\n世界\nline3\nline4\n";
    let data = attribution_from_porcelain(porcelain, source).expect("assemble");

    // Producer invariant: every distinct actor in runs has an
    // identity entry.
    let mut distinct_actors: Vec<String> = data
        .runs
        .as_slice()
        .iter()
        .map(|r| r.actor.to_string())
        .collect();
    distinct_actors.sort();
    distinct_actors.dedup();
    assert_eq!(
        distinct_actors,
        vec![
            "alice@example.com".to_string(),
            "bob@example.com".to_string()
        ]
    );
    for run in data.runs.as_slice() {
        assert!(
            data.identities.contains_key(&run.actor),
            "producer invariant violated: actor {:?} missing from identities",
            run.actor
        );
    }

    // Each entry's display_name equals the mail-local-part and color
    // equals actor_color(fnv1a_hex8(email)).
    for (actor, identity) in data.identities.iter() {
        let actor_str: &str = actor;
        let expected_local = actor_str
            .split_once('@')
            .map(|(l, _)| l.to_string())
            .unwrap_or_else(|| actor_str.to_string());
        assert_eq!(identity.display_name, expected_local);
        assert_eq!(identity.color, actor_color(&fnv1a_hex8(actor_str)));
    }

    // Pin alice and bob colours so a future refactor of fnv1a_hex8,
    // actor_color, or the Tol Muted palette ordering can't silently
    // shift the per-actor assignment.
    let alice: Arc<str> = Arc::from("alice@example.com");
    let bob: Arc<str> = Arc::from("bob@example.com");
    assert_eq!(data.identities.get(&alice).expect("alice").color, "#117733");
    assert_eq!(data.identities.get(&bob).expect("bob").color, "#CC6677");

    // Arc-interning invariant: every run's actor Arc<str> is
    // pointer-equal to the corresponding identity-map key.
    for run in data.runs.as_slice() {
        let (k, _v) = data
            .identities
            .get_key_value(&run.actor)
            .expect("identity present");
        assert!(
            Arc::ptr_eq(&run.actor, k),
            "actor Arc<str> in run must be ptr-eq to identity-map key"
        );
    }
}
