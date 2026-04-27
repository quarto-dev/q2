/*
 * project/dependency_graph.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Phase 8 cross-document dependency graph.
//!
//! Records, for every project document, the set of *other*
//! documents whose profile content can affect this document's
//! rendered output. Used by Mode B (subset render) to walk
//! transitive dependencies and decide which sibling profiles need
//! to be loaded for the user-named targets to render correctly.
//!
//! ## Edge sources
//!
//! Edges come from four channels (see
//! `claude-notes/plans/2026-04-27-websites-phase-8.md` Decision 5):
//!
//! | Channel | Edge contributed |
//! |---|---|
//! | Sidebar co-membership | for each sidebar with N members, the complete subgraph among those N pages (every member depends on every other) |
//! | Prev/next neighbors | each member → its previous and next sibling in the resolved sidebar order |
//! | Body-link targets | source page → every project-relative `.qmd` it links to (from `DocumentProfile.body_link_targets`, populated by `LinkResolutionStage`) |
//! | User-declared | `DocumentProfile.nav_dependencies` (from `meta.project.nav-dependencies`); each path becomes a forward edge |
//!
//! ## Reverse edges
//!
//! [`ProjectDependencyGraph`] ships a reverse-edge index alongside
//! the forward edges. Mode B's "what other pages need to be
//! profiled to render `targets` correctly?" query walks the
//! forward edges (`for each target t: traverse t → t.deps`).
//! Other queries (e.g. "if page Q has `always-render: true`, does
//! it need to join the render set when target T is rendered?")
//! traverse reverse edges (Q is implicitly added if Q ∈ deps_of(T)
//! transitively). Building both alongside is `O(E)` work — five
//! extra lines on top of the forward build.
//!
//! ## What this module does NOT do
//!
//! - It does **not** decide which pages get rendered. That's
//!   Mode A vs Mode B logic, owned by the orchestrator.
//! - It does **not** consult `force_render` flags. Those live on
//!   the resulting graph as data; the orchestrator interprets them.
//! - It does **not** emit diagnostics for unresolved
//!   `nav_dependencies` paths. The graph builder silently drops
//!   them; if surfacing matters, the orchestrator iterates and
//!   warns. (Future bd issue.)

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use quarto_pandoc_types::ConfigValue;

use crate::project::index::ProjectIndex;
use crate::project::sidebar_membership::resolve_sidebar_membership;

/// Directed dependency graph plus its reverse index and a set of
/// always-render pages.
#[derive(Debug, Default, Clone)]
pub struct ProjectDependencyGraph {
    /// For each source path, the set of source paths whose profile
    /// content can affect this page's rendered output. Forward
    /// edges (`A → B` means: rendering A reads B's profile).
    pub edges: HashMap<PathBuf, BTreeSet<PathBuf>>,

    /// Reverse index. `reverse_edges[B]` is the set of A's such
    /// that A → B in `edges`. Built in one O(E) pass alongside
    /// forward edges.
    pub reverse_edges: HashMap<PathBuf, BTreeSet<PathBuf>>,

    /// Pages with `project.always-render: true` in their merged
    /// metadata. Mode B consults this to decide which always-render
    /// pages get implicitly pulled into the render set when their
    /// reverse dependents are user-named targets.
    pub force_render: BTreeSet<PathBuf>,
}

impl ProjectDependencyGraph {
    /// Build the dependency graph from a fully-populated
    /// [`ProjectIndex`] and the project's merged metadata.
    ///
    /// `meta` is read for `website.sidebar` to produce the
    /// sidebar co-membership and prev/next edges. Per-page
    /// metadata (body links, nav-dependencies, always-render) is
    /// read off each [`DocumentProfile`].
    ///
    /// Diagnostics from sidebar `auto:` expansion are collected in
    /// `diagnostics`. If `auto:` resolution surfaces problems
    /// (unresolved paths, etc.), they appear here; the graph is
    /// built regardless.
    pub fn build(
        index: &ProjectIndex,
        meta: &ConfigValue,
        diagnostics: &mut Vec<quarto_error_reporting::DiagnosticMessage>,
    ) -> Self {
        let mut edges: HashMap<PathBuf, BTreeSet<PathBuf>> = HashMap::new();
        let mut reverse_edges: HashMap<PathBuf, BTreeSet<PathBuf>> = HashMap::new();
        let mut force_render: BTreeSet<PathBuf> = BTreeSet::new();

        // Helper to add a directed edge plus its reverse mirror.
        // Self-edges (a page depending on itself) are silently
        // dropped; they'd be no-ops in every consumer.
        let mut add_edge = |from: &Path, to: &Path| {
            if from == to {
                return;
            }
            edges
                .entry(from.to_path_buf())
                .or_default()
                .insert(to.to_path_buf());
            reverse_edges
                .entry(to.to_path_buf())
                .or_default()
                .insert(from.to_path_buf());
        };

        // === Sidebar co-membership + prev/next ===
        let sidebars = resolve_sidebar_membership(meta, index, diagnostics);
        for sidebar in &sidebars {
            // Co-membership: every member depends on every other.
            // O(N²) edges per sidebar; sidebars are tens of entries
            // in practice, so this is fine.
            for a in &sidebar.members {
                for b in &sidebar.members {
                    if a != b {
                        add_edge(a, b);
                    }
                }
            }
            // Prev/next neighbors are already a subset of the
            // co-membership set above, so they don't add new edges.
            // Keeping the conceptual channel separate in case we
            // ever want different behavior (e.g. weighting).
        }

        // === Per-profile contributions ===
        for profile in index.profiles() {
            let from = &profile.source_path;

            // `always-render` flag — collected here once per pass
            // over profiles.
            if profile.always_render {
                force_render.insert(from.clone());
            }

            // Body-link edges: source page → every linked .qmd
            // target. Targets that don't resolve to a project
            // page were already filtered out by
            // LinkResolutionStage (it consults the index).
            for target in &profile.body_link_targets {
                if index.lookup_by_source(target).is_some() {
                    add_edge(from, target);
                }
            }

            // User-declared nav-dependencies: same shape as
            // body links but explicit. Drop any that don't
            // resolve to a project document — the graph
            // builder is silent here (a future diagnostic
            // pass at the orchestrator level can surface
            // them).
            for target in &profile.nav_dependencies {
                if index.lookup_by_source(target).is_some() {
                    add_edge(from, target);
                }
            }
        }

        Self {
            edges,
            reverse_edges,
            force_render,
        }
    }

    /// Forward dependency closure: starting from `targets`,
    /// gather every page reachable through the forward edges.
    /// Used by Mode B's "what other profiles do I need to load
    /// for `targets` to render correctly?" query.
    ///
    /// Returns the closure as a `BTreeSet` so iteration is
    /// deterministic across runs. The targets themselves are
    /// included in the result (a target depends on itself in the
    /// trivial sense — and a caller iterating "which profiles
    /// to consult" wants the targets too).
    ///
    /// Termination: monotone (each iteration only adds, never
    /// removes), bounded by the size of the index. Cyclic
    /// dependencies (legal when a user's `nav_dependencies`
    /// declarations form a cycle) terminate cleanly because the
    /// closure stops growing once every reachable page is in the
    /// set.
    pub fn forward_closure<I, P>(&self, targets: I) -> BTreeSet<PathBuf>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut closure: BTreeSet<PathBuf> = BTreeSet::new();
        let mut frontier: Vec<PathBuf> = Vec::new();
        for t in targets {
            let p = t.as_ref().to_path_buf();
            if closure.insert(p.clone()) {
                frontier.push(p);
            }
        }
        while let Some(p) = frontier.pop() {
            if let Some(deps) = self.edges.get(&p) {
                for d in deps {
                    if closure.insert(d.clone()) {
                        frontier.push(d.clone());
                    }
                }
            }
        }
        closure
    }

    /// Reverse closure: starting from `targets`, gather every page
    /// that reaches them through the *reverse* edges. Used by
    /// Mode B's `always-render` augmentation: a page Q with
    /// `always-render: true` joins `targets` if any of Q's
    /// dependents (i.e. any page that reaches Q via the forward
    /// graph) is in `targets`.
    ///
    /// Returns a `BTreeSet`. Targets themselves are included.
    pub fn reverse_closure<I, P>(&self, targets: I) -> BTreeSet<PathBuf>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut closure: BTreeSet<PathBuf> = BTreeSet::new();
        let mut frontier: Vec<PathBuf> = Vec::new();
        for t in targets {
            let p = t.as_ref().to_path_buf();
            if closure.insert(p.clone()) {
                frontier.push(p);
            }
        }
        while let Some(p) = frontier.pop() {
            if let Some(deps) = self.reverse_edges.get(&p) {
                for d in deps {
                    if closure.insert(d.clone()) {
                        frontier.push(d.clone());
                    }
                }
            }
        }
        closure
    }

    /// Augment `targets` with every `always-render` page whose
    /// reverse closure intersects `targets`. The result is the
    /// effective render set for Mode B.
    ///
    /// Algorithm (Phase 8 plan §"Mode B with `always-render`
    /// siblings" data flow):
    ///
    /// ```text
    /// reachable = reverse_closure(targets)
    /// implicit = { q ∈ force_render | q ∈ reachable }
    /// effective = targets ∪ implicit
    /// ```
    ///
    /// Note this is **not** transitive in the always-render set:
    /// if q1 forces q2 forces q3, only q1's direct connection to
    /// targets matters. The recursive form would need a fixpoint
    /// iteration; v1 keeps it linear and matches the plan's
    /// semantics ("always-render pulls in pages whose dependents
    /// are user-named").
    pub fn augment_targets_with_always_render<I, P>(&self, targets: I) -> BTreeSet<PathBuf>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let target_set: BTreeSet<PathBuf> = targets
            .into_iter()
            .map(|p| p.as_ref().to_path_buf())
            .collect();
        let reachable = self.reverse_closure(target_set.iter());

        let mut effective = target_set;
        for q in &self.force_render {
            if reachable.contains(q) {
                effective.insert(q.clone());
            }
        }
        effective
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_source_map::SourceInfo;

    fn config_map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let map_entries: Vec<ConfigMapEntry> = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::default(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(map_entries, SourceInfo::default())
    }

    fn s(x: &str) -> ConfigValue {
        ConfigValue::new_string(x, SourceInfo::default())
    }

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, SourceInfo::default())
    }

    /// Build an index of `n` profiles with default fields and the
    /// given source paths.
    fn make_index(paths: &[&str]) -> ProjectIndex {
        let profiles: Vec<DocumentProfile> = paths
            .iter()
            .map(|p| DocumentProfile {
                source_path: PathBuf::from(p),
                output_href: p.replace(".qmd", ".html"),
                format_id: "html".to_string(),
                title: Some(p.to_string()),
                ..DocumentProfile::default()
            })
            .collect();
        ProjectIndex::new(profiles)
    }

    /// Configure a sidebar with the given members and add it to
    /// `meta.website.sidebar`.
    fn add_sidebar(meta: &mut ConfigValue, members: &[&str]) {
        let entries: Vec<ConfigValue> = members.iter().map(|m| s(m)).collect();
        let sidebar = config_map(vec![("contents", arr(entries))]);
        meta.insert_path(&["website", "sidebar"], sidebar);
    }

    #[test]
    fn empty_meta_yields_empty_graph() {
        let index = make_index(&["a.qmd", "b.qmd"]);
        let meta = ConfigValue::default();
        let mut diags = Vec::new();
        let g = ProjectDependencyGraph::build(&index, &meta, &mut diags);
        assert!(g.edges.is_empty());
        assert!(g.reverse_edges.is_empty());
        assert!(g.force_render.is_empty());
    }

    #[test]
    fn sidebar_co_membership_complete_subgraph() {
        let index = make_index(&["a.qmd", "b.qmd", "c.qmd"]);
        let mut meta = ConfigValue::default();
        add_sidebar(&mut meta, &["a.qmd", "b.qmd", "c.qmd"]);

        let mut diags = Vec::new();
        let g = ProjectDependencyGraph::build(&index, &meta, &mut diags);

        // 3 pages × 2 other = 6 directed edges.
        let total_edges: usize = g.edges.values().map(|s| s.len()).sum();
        assert_eq!(total_edges, 6, "every page should depend on every other");

        // Each page has both other pages in its dep set.
        for &p in &["a.qmd", "b.qmd", "c.qmd"] {
            let deps = g.edges.get(Path::new(p)).expect("page in graph");
            assert_eq!(deps.len(), 2);
            for &q in &["a.qmd", "b.qmd", "c.qmd"] {
                if p != q {
                    assert!(deps.contains(Path::new(q)));
                }
            }
        }

        // Reverse edges mirror forward edges exactly.
        for (from, deps) in &g.edges {
            for to in deps {
                assert!(
                    g.reverse_edges
                        .get(to)
                        .map(|s| s.contains(from))
                        .unwrap_or(false),
                    "reverse edge missing: {to:?} → {from:?}"
                );
            }
        }
    }

    #[test]
    fn body_link_targets_become_edges() {
        let mut profiles = vec![DocumentProfile {
            source_path: PathBuf::from("a.qmd"),
            output_href: "a.html".to_string(),
            format_id: "html".to_string(),
            title: Some("A".to_string()),
            body_link_targets: vec![PathBuf::from("b.qmd")],
            ..DocumentProfile::default()
        }];
        profiles.push(DocumentProfile {
            source_path: PathBuf::from("b.qmd"),
            output_href: "b.html".to_string(),
            format_id: "html".to_string(),
            title: Some("B".to_string()),
            ..DocumentProfile::default()
        });
        let index = ProjectIndex::new(profiles);

        let mut diags = Vec::new();
        let g = ProjectDependencyGraph::build(&index, &ConfigValue::default(), &mut diags);

        let a_deps = g
            .edges
            .get(Path::new("a.qmd"))
            .expect("a depends on something");
        assert!(a_deps.contains(Path::new("b.qmd")));
        // No incoming for `a.qmd`; `b.qmd` has reverse edge from `a.qmd`.
        assert!(!g.edges.contains_key(Path::new("b.qmd")));
        assert!(
            g.reverse_edges
                .get(Path::new("b.qmd"))
                .unwrap()
                .contains(Path::new("a.qmd"))
        );
    }

    #[test]
    fn nav_dependencies_become_edges() {
        let profiles = vec![
            DocumentProfile {
                source_path: PathBuf::from("foo.qmd"),
                output_href: "foo.html".to_string(),
                format_id: "html".to_string(),
                title: Some("Foo".to_string()),
                nav_dependencies: vec![PathBuf::from("a.qmd"), PathBuf::from("b.qmd")],
                ..DocumentProfile::default()
            },
            DocumentProfile {
                source_path: PathBuf::from("a.qmd"),
                output_href: "a.html".to_string(),
                format_id: "html".to_string(),
                title: Some("A".to_string()),
                ..DocumentProfile::default()
            },
            DocumentProfile {
                source_path: PathBuf::from("b.qmd"),
                output_href: "b.html".to_string(),
                format_id: "html".to_string(),
                title: Some("B".to_string()),
                ..DocumentProfile::default()
            },
        ];
        let index = ProjectIndex::new(profiles);

        let mut diags = Vec::new();
        let g = ProjectDependencyGraph::build(&index, &ConfigValue::default(), &mut diags);

        let foo_deps = g.edges.get(Path::new("foo.qmd")).unwrap();
        assert!(foo_deps.contains(Path::new("a.qmd")));
        assert!(foo_deps.contains(Path::new("b.qmd")));
    }

    #[test]
    fn unresolvable_nav_dependency_is_silently_dropped() {
        let profiles = vec![DocumentProfile {
            source_path: PathBuf::from("foo.qmd"),
            output_href: "foo.html".to_string(),
            format_id: "html".to_string(),
            title: Some("Foo".to_string()),
            nav_dependencies: vec![PathBuf::from("missing.qmd")],
            ..DocumentProfile::default()
        }];
        let index = ProjectIndex::new(profiles);

        let mut diags = Vec::new();
        let g = ProjectDependencyGraph::build(&index, &ConfigValue::default(), &mut diags);

        // No edges (the only declared dependency doesn't resolve).
        assert!(
            g.edges.is_empty(),
            "missing target should not create an edge"
        );
    }

    #[test]
    fn always_render_pages_collected_in_force_render() {
        let profiles = vec![
            DocumentProfile {
                source_path: PathBuf::from("vol.qmd"),
                output_href: "vol.html".to_string(),
                format_id: "html".to_string(),
                title: Some("Volatile".to_string()),
                always_render: true,
                ..DocumentProfile::default()
            },
            DocumentProfile {
                source_path: PathBuf::from("stable.qmd"),
                output_href: "stable.html".to_string(),
                format_id: "html".to_string(),
                title: Some("Stable".to_string()),
                ..DocumentProfile::default()
            },
        ];
        let index = ProjectIndex::new(profiles);

        let mut diags = Vec::new();
        let g = ProjectDependencyGraph::build(&index, &ConfigValue::default(), &mut diags);

        assert_eq!(g.force_render.len(), 1);
        assert!(g.force_render.contains(Path::new("vol.qmd")));
    }

    #[test]
    fn forward_closure_walks_transitively() {
        // a → b → c → d. Closure starting at {a} = {a, b, c, d}.
        let profiles = vec![
            DocumentProfile {
                source_path: PathBuf::from("a.qmd"),
                output_href: "a.html".to_string(),
                format_id: "html".to_string(),
                title: Some("A".to_string()),
                body_link_targets: vec![PathBuf::from("b.qmd")],
                ..DocumentProfile::default()
            },
            DocumentProfile {
                source_path: PathBuf::from("b.qmd"),
                output_href: "b.html".to_string(),
                format_id: "html".to_string(),
                title: Some("B".to_string()),
                body_link_targets: vec![PathBuf::from("c.qmd")],
                ..DocumentProfile::default()
            },
            DocumentProfile {
                source_path: PathBuf::from("c.qmd"),
                output_href: "c.html".to_string(),
                format_id: "html".to_string(),
                title: Some("C".to_string()),
                body_link_targets: vec![PathBuf::from("d.qmd")],
                ..DocumentProfile::default()
            },
            DocumentProfile {
                source_path: PathBuf::from("d.qmd"),
                output_href: "d.html".to_string(),
                format_id: "html".to_string(),
                title: Some("D".to_string()),
                ..DocumentProfile::default()
            },
        ];
        let index = ProjectIndex::new(profiles);

        let mut diags = Vec::new();
        let g = ProjectDependencyGraph::build(&index, &ConfigValue::default(), &mut diags);

        let closure = g.forward_closure([Path::new("a.qmd")]);
        assert_eq!(closure.len(), 4);
        for p in &["a.qmd", "b.qmd", "c.qmd", "d.qmd"] {
            assert!(closure.contains(Path::new(p)), "closure missing {p}");
        }
    }

    #[test]
    fn forward_closure_terminates_on_cycles() {
        // a → b, b → a. forward_closure({a}) = {a, b}.
        let profiles = vec![
            DocumentProfile {
                source_path: PathBuf::from("a.qmd"),
                output_href: "a.html".to_string(),
                format_id: "html".to_string(),
                title: Some("A".to_string()),
                body_link_targets: vec![PathBuf::from("b.qmd")],
                ..DocumentProfile::default()
            },
            DocumentProfile {
                source_path: PathBuf::from("b.qmd"),
                output_href: "b.html".to_string(),
                format_id: "html".to_string(),
                title: Some("B".to_string()),
                body_link_targets: vec![PathBuf::from("a.qmd")],
                ..DocumentProfile::default()
            },
        ];
        let index = ProjectIndex::new(profiles);

        let mut diags = Vec::new();
        let g = ProjectDependencyGraph::build(&index, &ConfigValue::default(), &mut diags);

        let closure = g.forward_closure([Path::new("a.qmd")]);
        assert_eq!(closure.len(), 2);
    }

    #[test]
    fn reverse_closure_finds_dependents() {
        // a → b, c → b. reverse_closure({b}) = {a, b, c}.
        let profiles = vec![
            DocumentProfile {
                source_path: PathBuf::from("a.qmd"),
                output_href: "a.html".to_string(),
                format_id: "html".to_string(),
                title: Some("A".to_string()),
                body_link_targets: vec![PathBuf::from("b.qmd")],
                ..DocumentProfile::default()
            },
            DocumentProfile {
                source_path: PathBuf::from("b.qmd"),
                output_href: "b.html".to_string(),
                format_id: "html".to_string(),
                title: Some("B".to_string()),
                ..DocumentProfile::default()
            },
            DocumentProfile {
                source_path: PathBuf::from("c.qmd"),
                output_href: "c.html".to_string(),
                format_id: "html".to_string(),
                title: Some("C".to_string()),
                body_link_targets: vec![PathBuf::from("b.qmd")],
                ..DocumentProfile::default()
            },
        ];
        let index = ProjectIndex::new(profiles);

        let mut diags = Vec::new();
        let g = ProjectDependencyGraph::build(&index, &ConfigValue::default(), &mut diags);

        let closure = g.reverse_closure([Path::new("b.qmd")]);
        assert_eq!(closure.len(), 3);
        for p in &["a.qmd", "b.qmd", "c.qmd"] {
            assert!(closure.contains(Path::new(p)));
        }
    }

    #[test]
    fn augment_pulls_in_always_render_dependents() {
        // q (always_render: true) → x. user-targets = {x}.
        // augmented = {x, q}.
        let profiles = vec![
            DocumentProfile {
                source_path: PathBuf::from("q.qmd"),
                output_href: "q.html".to_string(),
                format_id: "html".to_string(),
                title: Some("Q".to_string()),
                always_render: true,
                body_link_targets: vec![PathBuf::from("x.qmd")],
                ..DocumentProfile::default()
            },
            DocumentProfile {
                source_path: PathBuf::from("x.qmd"),
                output_href: "x.html".to_string(),
                format_id: "html".to_string(),
                title: Some("X".to_string()),
                ..DocumentProfile::default()
            },
        ];
        let index = ProjectIndex::new(profiles);

        let mut diags = Vec::new();
        let g = ProjectDependencyGraph::build(&index, &ConfigValue::default(), &mut diags);

        let augmented = g.augment_targets_with_always_render([Path::new("x.qmd")]);
        assert_eq!(augmented.len(), 2);
        assert!(augmented.contains(Path::new("x.qmd")));
        assert!(augmented.contains(Path::new("q.qmd")));
    }

    #[test]
    fn augment_does_not_pull_unrelated_always_render_pages() {
        // q always_render but doesn't reach x via any edge.
        // augmented({x}) = {x}.
        let profiles = vec![
            DocumentProfile {
                source_path: PathBuf::from("q.qmd"),
                output_href: "q.html".to_string(),
                format_id: "html".to_string(),
                title: Some("Q".to_string()),
                always_render: true,
                ..DocumentProfile::default()
            },
            DocumentProfile {
                source_path: PathBuf::from("x.qmd"),
                output_href: "x.html".to_string(),
                format_id: "html".to_string(),
                title: Some("X".to_string()),
                ..DocumentProfile::default()
            },
        ];
        let index = ProjectIndex::new(profiles);

        let mut diags = Vec::new();
        let g = ProjectDependencyGraph::build(&index, &ConfigValue::default(), &mut diags);

        let augmented = g.augment_targets_with_always_render([Path::new("x.qmd")]);
        assert_eq!(augmented.len(), 1);
        assert!(augmented.contains(Path::new("x.qmd")));
        assert!(!augmented.contains(Path::new("q.qmd")));
    }

    #[test]
    fn forward_closure_includes_targets_themselves() {
        let index = make_index(&["a.qmd", "b.qmd"]);
        let mut diags = Vec::new();
        let g = ProjectDependencyGraph::build(&index, &ConfigValue::default(), &mut diags);

        // No edges, so the closure starting at {a} contains just {a}.
        let closure = g.forward_closure([Path::new("a.qmd")]);
        assert_eq!(closure.len(), 1);
        assert!(closure.contains(Path::new("a.qmd")));
    }

    #[test]
    fn graph_is_empty_for_unrelated_pages() {
        // a, b, c all unrelated → no edges, no force_render.
        let index = make_index(&["a.qmd", "b.qmd", "c.qmd"]);
        let mut diags = Vec::new();
        let g = ProjectDependencyGraph::build(&index, &ConfigValue::default(), &mut diags);

        assert!(g.edges.is_empty());
        assert!(g.reverse_edges.is_empty());
        assert!(g.force_render.is_empty());

        let closure = g.forward_closure([Path::new("a.qmd")]);
        assert_eq!(closure.len(), 1);
    }
}
