/*
 * lua_conformance.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Pandoc Lua API conformance suite (strand bd-grkrb9nj).
 *
 * Runs the test files vendored from pandoc-lua-marshal (Pandoc's own
 * Lua marshaling tests) inside pampa's production filter environment
 * and compares the outcome against the expected-failure list. See
 * tests/lua-conformance/README.md for the layout, vendoring policy,
 * and the xfail ratchet semantics.
 */

#![cfg(feature = "lua-filter")]
#![cfg(not(target_arch = "wasm32"))]

use mlua::{Lua, Table, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn conformance_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lua-conformance")
}

#[derive(Debug)]
struct CaseResult {
    /// Stable id: `<file>::<group>::…::<test name>`
    id: String,
    passed: bool,
    message: Option<String>,
}

/// Build the production filter environment and prepare it for the
/// upstream suite: preload the vendored `tasty` module and replicate
/// the upstream driver's globals (constructors as bare globals, `List`,
/// enum constants as strings) via prelude.lua.
fn conformance_lua_env(script_name: &str) -> Lua {
    let runtime: Arc<dyn pampa::lua::SystemRuntime> = Arc::new(pampa::lua::NativeRuntime::new());
    let script_path = conformance_dir().join("upstream").join(script_name);
    let lua = pampa::lua::create_filter_environment(runtime, "html", &script_path, None)
        .expect("failed to create filter environment");

    let tasty_src = std::fs::read_to_string(conformance_dir().join("tasty.lua"))
        .expect("failed to read vendored tasty.lua");
    let tasty: Value = lua
        .load(&tasty_src)
        .set_name("tasty.lua")
        .eval()
        .expect("failed to evaluate vendored tasty.lua");
    let package: Table = lua
        .globals()
        .get("package")
        .expect("package library missing");
    let loaded: Table = package.get("loaded").expect("package.loaded missing");
    loaded.set("tasty", tasty).expect("failed to preload tasty");

    let prelude_src = std::fs::read_to_string(conformance_dir().join("prelude.lua"))
        .expect("failed to read prelude.lua");
    lua.load(&prelude_src)
        .set_name("prelude.lua")
        .exec()
        .expect("failed to execute prelude.lua");

    lua
}

/// Stringify a Lua value with `tostring` (error payloads are not
/// always strings).
fn lua_tostring(lua: &Lua, value: &Value) -> String {
    let tostring: mlua::Function = lua.globals().get("tostring").expect("tostring missing");
    tostring.call::<mlua::String>(value.clone()).map_or_else(
        |_| "<unprintable error>".to_string(),
        |s| s.to_string_lossy(),
    )
}

/// Flatten a tasty result tree.
///
/// Executing an upstream file already ran every test (tasty's
/// `test_case` pcall-executes its callback at tree construction time);
/// each node is `{name = ..., result = true | error-string | list}`.
fn collect_results(lua: &Lua, prefix: &str, nodes: &Table, out: &mut Vec<CaseResult>) {
    for node in nodes.sequence_values::<Table>() {
        let node = match node {
            Ok(t) => t,
            Err(e) => {
                out.push(CaseResult {
                    id: format!("{prefix}::<malformed test tree>"),
                    passed: false,
                    message: Some(e.to_string()),
                });
                continue;
            }
        };
        let name: String = node
            .get::<Option<String>>("name")
            .ok()
            .flatten()
            .unwrap_or_else(|| "<unnamed>".to_string());
        let id = format!("{prefix}::{name}");
        let result: Value = node.get("result").unwrap_or(Value::Nil);
        match result {
            Value::Boolean(true) => out.push(CaseResult {
                id,
                passed: true,
                message: None,
            }),
            Value::Table(subtree) => collect_results(lua, &id, &subtree, out),
            other => {
                let message = lua_tostring(lua, &other);
                out.push(CaseResult {
                    id,
                    passed: false,
                    message: Some(message),
                });
            }
        }
    }
}

/// Run one vendored upstream file, returning the flattened results.
/// A file-level error (the chunk itself fails to execute) is reported
/// as a single failed pseudo-case so the ratchet can track it.
fn run_upstream_file(file_name: &str) -> Vec<CaseResult> {
    let lua = conformance_lua_env(file_name);
    let src = std::fs::read_to_string(conformance_dir().join("upstream").join(file_name))
        .unwrap_or_else(|e| panic!("failed to read {file_name}: {e}"));

    let mut results = Vec::new();
    match lua.load(&src).set_name(file_name).eval::<Value>() {
        Ok(Value::Table(tree)) => collect_results(&lua, file_name, &tree, &mut results),
        Ok(other) => results.push(CaseResult {
            id: format!("{file_name}::<file>"),
            passed: false,
            message: Some(format!(
                "expected the test file to return a table, got {}",
                other.type_name()
            )),
        }),
        Err(e) => results.push(CaseResult {
            id: format!("{file_name}::<file>"),
            passed: false,
            message: Some(format!("file-level error: {e}")),
        }),
    }
    results
}

/// A parsed xfail list: expected-failure ids, plus which of them are
/// registered permanent divergences (bd-9p2686pc). An entry is a
/// divergence when its trailing comment starts with `DIVERGENCE` —
/// e.g. `test-x.lua::case # DIVERGENCE: q2 raises Q-11-2`. Divergence
/// entries must have a matching record in
/// `tests/lua-conformance/divergences.md` (enforced by
/// `divergence_xfails_are_registered`), and an unexpected PASS on one
/// is reported differently: it means q2 now matches pandoc and the
/// registry entry itself is stale.
pub(crate) struct XfailList {
    /// id -> is_divergence
    entries: std::collections::BTreeMap<String, bool>,
}

impl XfailList {
    pub(crate) fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    pub(crate) fn is_divergence(&self, id: &str) -> bool {
        self.entries.get(id).copied().unwrap_or(false)
    }

    pub(crate) fn divergence_ids(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .filter(|(_, d)| **d)
            .map(|(id, _)| id.as_str())
    }
}

/// Parse xfail content: one test id per line; `#` starts a comment
/// (standalone or trailing); blank lines ignored; a trailing comment
/// starting with `DIVERGENCE` marks the entry as a permanent
/// divergence. Shared with the differential suite
/// (`lua_differential.rs`).
pub(crate) fn parse_xfail(content: &str) -> XfailList {
    let mut entries = std::collections::BTreeMap::new();
    for line in content.lines() {
        let (id, comment) = match line.split_once('#') {
            Some((id, comment)) => (id.trim(), comment.trim()),
            None => (line.trim(), ""),
        };
        if id.is_empty() {
            continue;
        }
        entries.insert(id.to_string(), comment.starts_with("DIVERGENCE"));
    }
    XfailList { entries }
}

pub(crate) fn load_xfail_file(path: &Path) -> XfailList {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    parse_xfail(&content)
}

/// Check that every `# DIVERGENCE` xfail entry has a record in the
/// divergence registry (matched by literal id containment). Returns
/// the unregistered ids.
pub(crate) fn unregistered_divergences(xfail: &XfailList, registry: &str) -> Vec<String> {
    xfail
        .divergence_ids()
        .filter(|id| !registry.contains(*id))
        .map(String::from)
        .collect()
}

/// Check one file's results against the xfail list (the ratchet).
///
/// Fails on unexpected failures (regressions) and on unexpected passes
/// (progress — remove the xfail line so the ratchet only tightens).
///
/// Maintenance affordance: set `LUA_CONFORMANCE_DUMP=1` to print every
/// failing id in xfail-ready format instead of asserting (used to
/// regenerate the baseline; run with `--no-capture`).
fn check_against_xfail(file_name: &str) {
    // Vendored files vary widely in size; 10 is right for the big
    // three, smaller files pass their own floor.
    check_against_xfail_min(file_name, 10)
}

fn check_against_xfail_min(file_name: &str, min_cases: usize) {
    let results = run_upstream_file(file_name);
    let xfail = load_xfail_file(&conformance_dir().join("xfail.txt"));

    if std::env::var("LUA_CONFORMANCE_DUMP").is_ok() {
        for r in &results {
            if !r.passed {
                let msg = r
                    .message
                    .as_deref()
                    .unwrap_or("")
                    .replace('\n', " ")
                    .trim()
                    .to_string();
                let msg = if msg.chars().count() > 120 {
                    format!("{}…", msg.chars().take(120).collect::<String>())
                } else {
                    msg
                };
                println!("{} # {}", r.id, msg);
            }
        }
        let (pass, fail): (Vec<_>, Vec<_>) = results.iter().partition(|r| r.passed);
        println!(
            "-- {file_name}: {} passed, {} failed, {} total",
            pass.len(),
            fail.len(),
            results.len()
        );
        return;
    }

    let unexpected_failures: Vec<&CaseResult> = results
        .iter()
        .filter(|r| !r.passed && !xfail.contains(&r.id))
        .collect();
    let unexpected_passes: Vec<&CaseResult> = results
        .iter()
        .filter(|r| r.passed && xfail.contains(&r.id))
        .collect();

    let mut report = String::new();
    if !unexpected_failures.is_empty() {
        report.push_str(&format!(
            "\n{} unexpected FAILURE(s) (regressions — not in xfail.txt):\n",
            unexpected_failures.len()
        ));
        for r in &unexpected_failures {
            report.push_str(&format!(
                "  {}\n    {}\n",
                r.id,
                r.message
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .replace('\n', "\n    ")
            ));
        }
    }
    let (divergence_passes, progress_passes): (Vec<&&CaseResult>, Vec<&&CaseResult>) =
        unexpected_passes
            .iter()
            .partition(|r| xfail.is_divergence(&r.id));
    if !progress_passes.is_empty() {
        report.push_str(&format!(
            "\n{} unexpected PASS(es) (progress! remove these lines from xfail.txt):\n",
            progress_passes.len()
        ));
        for r in &progress_passes {
            report.push_str(&format!("  {}\n", r.id));
        }
    }
    if !divergence_passes.is_empty() {
        report.push_str(&format!(
            "\n{} DIVERGENCE entry/entries passed — q2 now matches pandoc here; remove the \
             xfail line AND the corresponding divergences.md entry:\n",
            divergence_passes.len()
        ));
        for r in &divergence_passes {
            report.push_str(&format!("  {}\n", r.id));
        }
    }
    assert!(
        report.is_empty(),
        "lua conformance ratchet violated for {file_name}:{report}\n\
         (xfail list: crates/pampa/tests/lua-conformance/xfail.txt)"
    );

    // Sanity: the harness must have actually run a non-trivial number
    // of cases; a collapse to a single file-level error must never
    // hide behind a matching xfail entry count.
    let total = results.len();
    assert!(
        total > min_cases,
        "suspiciously few conformance cases ({total}) ran for {file_name} — harness broken?"
    );
}

#[test]
fn lua_conformance_attr() {
    check_against_xfail("test-attr.lua");
}

#[test]
fn lua_conformance_inline() {
    check_against_xfail("test-inline.lua");
}

#[test]
fn lua_conformance_block() {
    check_against_xfail("test-block.lua");
}

#[test]
fn lua_conformance_citation() {
    check_against_xfail("test-citation.lua");
}

#[test]
fn lua_conformance_listattributes() {
    check_against_xfail("test-listattributes.lua");
}

#[test]
fn lua_conformance_metavalue() {
    check_against_xfail_min("test-metavalue.lua", 1);
}

#[test]
fn lua_conformance_pandoc() {
    check_against_xfail("test-pandoc.lua");
}

#[test]
fn lua_conformance_simpletable() {
    check_against_xfail_min("test-simpletable.lua", 1);
}

#[test]
fn lua_conformance_table() {
    check_against_xfail("test-table.lua");
}

#[test]
fn lua_conformance_cell() {
    check_against_xfail("test-cell.lua");
}

// ============================================================================
// Xfail parsing + divergence registry consistency (bd-9p2686pc)
// ============================================================================

#[test]
fn parse_xfail_distinguishes_divergences() {
    let list = parse_xfail(
        "# a standalone comment\n\
         \n\
         test-a.lua::case one # plain observed-message comment\n\
         test-b.lua::case two # DIVERGENCE: q2 raises Q-11-2\n\
         test-c.lua::bare\n",
    );
    assert!(list.contains("test-a.lua::case one"));
    assert!(!list.is_divergence("test-a.lua::case one"));
    assert!(list.contains("test-b.lua::case two"));
    assert!(list.is_divergence("test-b.lua::case two"));
    assert!(list.contains("test-c.lua::bare"));
    assert!(!list.is_divergence("test-c.lua::bare"));
    assert!(!list.contains("# a standalone comment"));
    assert_eq!(
        list.divergence_ids().collect::<Vec<_>>(),
        vec!["test-b.lua::case two"]
    );
}

#[test]
fn unregistered_divergences_flags_missing_registry_entries() {
    let list = parse_xfail(
        "test-a.lua::registered # DIVERGENCE: in the registry\n\
         test-b.lua::missing # DIVERGENCE: not in the registry\n\
         test-c.lua::plain-xfail # not a divergence, never checked\n",
    );
    let registry = "…prose… `test-a.lua::registered` …prose…";
    assert_eq!(
        unregistered_divergences(&list, registry),
        vec!["test-b.lua::missing".to_string()]
    );
}

/// The live consistency check: every `# DIVERGENCE` entry in either
/// ratchet's xfail list must appear (as a literal id) in
/// tests/lua-conformance/divergences.md.
#[test]
fn divergence_xfails_are_registered() {
    let registry = std::fs::read_to_string(conformance_dir().join("divergences.md"))
        .expect("failed to read divergences.md");
    for xfail_path in [
        conformance_dir().join("xfail.txt"),
        conformance_dir().join("differential/xfail.txt"),
    ] {
        let missing = unregistered_divergences(&load_xfail_file(&xfail_path), &registry);
        assert!(
            missing.is_empty(),
            "DIVERGENCE xfail entries in {} lack a divergences.md record: {missing:#?}",
            xfail_path.display()
        );
    }
}
