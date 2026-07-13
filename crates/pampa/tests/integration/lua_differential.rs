/*
 * lua_differential.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Track-2 Lua conformance: differential testing against a real pandoc
 * binary (strand bd-grkrb9nj).
 *
 * Each case under tests/lua-conformance/differential/cases/<name>/ is
 * an (input.md, filter.lua) pair plus a committed oracle.json snapshot
 * of what the pinned pandoc version produces for
 * `pandoc -f markdown input.md -L filter.lua -t json`. This test runs
 * the same pair through the real pampa binary
 * (`pampa input.md -F filter.lua -t json`) and compares the two ASTs
 * after normalizing away q2's source-tracking extensions.
 *
 * Oracle snapshots are regenerated locally with regen-oracles.sh (CI
 * never needs pandoc). Expected divergences live in
 * differential/xfail.txt with the same ratchet semantics as Track 1:
 * unexpected failures AND unexpected passes both fail.
 */

#![cfg(feature = "lua-filter")]
#![cfg(not(target_arch = "wasm32"))]

use serde_json::Value as Json;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::lua_conformance::load_xfail_file;

fn differential_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lua-conformance/differential")
}

/// Strip q2's source-tracking extensions so the AST is comparable with
/// pandoc's JSON:
/// - top-level `astContext`,
/// - the 4th (q2) component of `pandoc-api-version`,
/// - `s` (source id) and `a` (attr source structure) members of AST
///   nodes (objects carrying a string `t` tag — meta maps with
///   user-chosen `s`/`a` keys are not touched because their containers
///   have no `t` tag),
/// - `citationIdS` (citation id source) on citation objects (which
///   carry no `t` tag, so they need their own rule; keyed off the
///   `citationId` member).
fn normalize_nodes(v: &mut Json) {
    match v {
        Json::Object(map) => {
            if map.get("t").is_some_and(|t| t.is_string()) {
                map.remove("s");
                map.remove("a");
            }
            if map.contains_key("citationId") {
                map.remove("citationIdS");
            }
            for (_k, val) in map.iter_mut() {
                normalize_nodes(val);
            }
        }
        Json::Array(arr) => {
            for val in arr.iter_mut() {
                normalize_nodes(val);
            }
        }
        _ => {}
    }
}

fn normalize_doc(mut v: Json) -> Json {
    if let Json::Object(map) = &mut v {
        map.remove("astContext");
        if let Some(Json::Array(ver)) = map.get_mut("pandoc-api-version") {
            ver.truncate(3);
        }
    }
    normalize_nodes(&mut v);
    v
}

/// Collect up to `limit` JSON-pointer-ish paths where the two values
/// differ, for readable failure messages.
fn diff_paths(a: &Json, b: &Json, path: &str, out: &mut Vec<String>, limit: usize) {
    if out.len() >= limit {
        return;
    }
    match (a, b) {
        (Json::Object(ma), Json::Object(mb)) => {
            for (k, va) in ma {
                match mb.get(k) {
                    Some(vb) => diff_paths(va, vb, &format!("{path}/{k}"), out, limit),
                    None => out.push(format!("{path}/{k}: present in pandoc, missing in pampa")),
                }
            }
            for k in mb.keys() {
                if !ma.contains_key(k) {
                    out.push(format!("{path}/{k}: missing in pandoc, present in pampa"));
                }
            }
        }
        (Json::Array(aa), Json::Array(ab)) => {
            if aa.len() != ab.len() {
                out.push(format!(
                    "{path}: array length {} (pandoc) vs {} (pampa)",
                    aa.len(),
                    ab.len()
                ));
            }
            for (i, (va, vb)) in aa.iter().zip(ab.iter()).enumerate() {
                diff_paths(va, vb, &format!("{path}/{i}"), out, limit);
            }
        }
        _ => {
            if a != b {
                out.push(format!("{path}: {a} (pandoc) vs {b} (pampa)"));
            }
        }
    }
}

struct CaseOutcome {
    name: String,
    passed: bool,
    message: Option<String>,
}

fn run_case(case_dir: &Path) -> CaseOutcome {
    let name = case_dir.file_name().unwrap().to_string_lossy().to_string();
    let input = case_dir.join("input.md");
    let filter = case_dir.join("filter.lua");
    let oracle_path = case_dir.join("oracle.json");

    let oracle_src = match std::fs::read_to_string(&oracle_path) {
        Ok(s) => s,
        Err(e) => {
            return CaseOutcome {
                name,
                passed: false,
                message: Some(format!(
                    "missing oracle.json ({e}) — run differential/regen-oracles.sh"
                )),
            };
        }
    };
    let oracle: Json = serde_json::from_str(&oracle_src).expect("oracle.json is not valid JSON");

    let output = Command::new(env!("CARGO_BIN_EXE_pampa"))
        .arg(&input)
        .arg("-F")
        .arg(&filter)
        .arg("-t")
        .arg("json")
        .output()
        .expect("failed to spawn pampa binary");
    if !output.status.success() {
        return CaseOutcome {
            name,
            passed: false,
            message: Some(format!(
                "pampa exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )),
        };
    }
    let pampa_ast: Json = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => {
            return CaseOutcome {
                name,
                passed: false,
                message: Some(format!("pampa produced invalid JSON: {e}")),
            };
        }
    };

    let oracle = normalize_doc(oracle);
    let pampa_ast = normalize_doc(pampa_ast);
    if oracle == pampa_ast {
        CaseOutcome {
            name,
            passed: true,
            message: None,
        }
    } else {
        let mut diffs = Vec::new();
        diff_paths(&oracle, &pampa_ast, "", &mut diffs, 5);
        CaseOutcome {
            name,
            passed: false,
            message: Some(format!("AST mismatch:\n    {}", diffs.join("\n    "))),
        }
    }
}

#[test]
fn lua_differential_cases() {
    let cases_dir = differential_dir().join("cases");
    let mut case_dirs: Vec<PathBuf> = std::fs::read_dir(&cases_dir)
        .expect("failed to read differential cases dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    case_dirs.sort();
    assert!(
        !case_dirs.is_empty(),
        "no differential cases found in {}",
        cases_dir.display()
    );

    let outcomes: Vec<CaseOutcome> = case_dirs.iter().map(|d| run_case(d)).collect();

    if std::env::var("LUA_CONFORMANCE_DUMP").is_ok() {
        for o in &outcomes {
            if !o.passed {
                let msg = o.message.as_deref().unwrap_or("").replace('\n', " ");
                println!("{} # {}", o.name, msg.trim());
            }
        }
        let failed = outcomes.iter().filter(|o| !o.passed).count();
        println!(
            "-- differential: {} passed, {} failed, {} total",
            outcomes.len() - failed,
            failed,
            outcomes.len()
        );
        return;
    }

    let xfail = load_xfail_file(&differential_dir().join("xfail.txt"));
    let mut report = String::new();
    for o in &outcomes {
        match (o.passed, xfail.contains(&o.name)) {
            (false, false) => report.push_str(&format!(
                "\nunexpected FAILURE (not in differential/xfail.txt): {}\n  {}\n",
                o.name,
                o.message.as_deref().unwrap_or("").replace('\n', "\n  ")
            )),
            (true, true) => report.push_str(&format!(
                "\nunexpected PASS (progress! remove from differential/xfail.txt): {}\n",
                o.name
            )),
            _ => {}
        }
    }
    assert!(
        report.is_empty(),
        "lua differential ratchet violated:{report}\n\
         (xfail list: crates/pampa/tests/lua-conformance/differential/xfail.txt)"
    );
}
