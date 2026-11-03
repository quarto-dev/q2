//! Lint rule: `resources/vendored.toml` agrees with the tree (bd-xm7l).
//!
//! The registry is only useful if it cannot drift the way the old prose
//! inventory did, so this rule checks it offline on every `cargo xtask lint`:
//!
//! - **Consistency.** Ids are unique; `group` / `follows` references resolve
//!   (no `follows` cycles); every `paths` entry exists; every
//!   `version_sources` entry agrees with `version`; every file of a
//!   `copy_of` copy is byte-identical to its source; `procedure` files exist.
//! - **Policy shape.** `track` needs an `upstream`; `follow` needs `follows`.
//! - **Coverage.** Every committed file under a `[coverage] roots` entry is
//!   claimed by some asset's `paths` (`native` entries included), so a new
//!   vendored asset cannot land unregistered.
//! - **Subtrees.** Every row of `pull_extension_subtree::subtrees()` has a
//!   registry asset at the same prefix with the same remote and branch.
//!
//! `subtree-split` version sources need full git history (CI clones may be
//! shallow), so they are verified by `cargo xtask vendored outdated` instead.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::Violation;
use crate::pull_extension_subtree::{SubtreeConfig, subtrees};
use crate::vendored::registry::{
    Asset, GitTrack, Policy, REGISTRY_REL, Registry, Upstream, VersionSource, under_root,
};

/// The name of this lint rule.
const RULE_NAME: &str = "vendored-registry";

/// Run the repo-level vendored-registry check against the real tree.
pub fn check(workspace_root: &Path) -> Result<Vec<Violation>> {
    let registry_path = workspace_root.join(REGISTRY_REL);
    let text = std::fs::read_to_string(&registry_path)
        .with_context(|| format!("reading {}", registry_path.display()))?;
    let files = committed_files(workspace_root)?;
    check_with(workspace_root, &text, &files, &subtrees())
}

/// All committed and untracked-but-not-ignored files, repo-relative with
/// `/` separators (so a new asset fails the lint before it is committed).
fn committed_files(workspace_root: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(workspace_root)
        .output()
        .context("running git ls-files")?;
    if !output.status.success() {
        bail!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8(output.stdout)?
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect())
}

/// The check proper, with the tree, the committed-file list and the subtree
/// table injected so tests can use a synthetic workspace.
fn check_with(
    root: &Path,
    text: &str,
    files: &[String],
    subtree_rows: &[SubtreeConfig],
) -> Result<Vec<Violation>> {
    let registry_path = root.join(REGISTRY_REL);
    let registry = match Registry::parse(text) {
        Ok(r) => r,
        Err(e) => {
            return Ok(vec![Violation {
                file: registry_path,
                line: 1,
                column: 1,
                rule: RULE_NAME,
                message: format!("{e:#}"),
                suggestion: None,
            }]);
        }
    };

    let mut out = Vec::new();
    let mut push = |id: Option<&str>, message: String, suggestion: Option<&str>| {
        out.push(Violation {
            file: registry_path.clone(),
            line: id.map_or(1, |id| line_of_id(text, id)),
            column: 1,
            rule: RULE_NAME,
            message,
            suggestion: suggestion.map(str::to_string),
        });
    };

    // Unique ids.
    let mut seen = HashSet::new();
    for id in registry
        .groups
        .iter()
        .map(|g| &g.id)
        .chain(registry.assets.iter().map(|a| &a.id))
    {
        if !seen.insert(id.as_str()) {
            push(Some(id), format!("duplicate id `{id}`"), None);
        }
    }

    let group_ids: HashSet<&str> = registry.groups.iter().map(|g| g.id.as_str()).collect();

    for asset in &registry.assets {
        let id = Some(asset.id.as_str());

        if let Some(g) = &asset.group
            && !group_ids.contains(g.as_str())
        {
            push(id, format!("`{}`: unknown group `{g}`", asset.id), None);
        }

        match asset.policy {
            Policy::Track if asset.upstream.is_none() => push(
                id,
                format!("`{}`: policy `track` needs an `upstream`", asset.id),
                Some("add an upstream, or use policy `follow`/`frozen`/`native`/`external`"),
            ),
            Policy::Follow if asset.follows.is_none() => push(
                id,
                format!("`{}`: policy `follow` needs `follows`", asset.id),
                None,
            ),
            _ => {}
        }
        if let Some(target) = &asset.follows {
            if asset.policy != Policy::Follow {
                push(
                    id,
                    format!(
                        "`{}`: `follows` is only meaningful with policy `follow`",
                        asset.id
                    ),
                    None,
                );
            }
            if registry.asset(target).is_none() || target == &asset.id {
                push(
                    id,
                    format!(
                        "`{}`: `follows` target `{target}` is not another asset",
                        asset.id
                    ),
                    None,
                );
            }
        }

        if asset.paths.is_empty() {
            push(id, format!("`{}`: `paths` is empty", asset.id), None);
        }
        for p in &asset.paths {
            if !root.join(p).exists() {
                push(
                    id,
                    format!("`{}`: path `{p}` does not exist", asset.id),
                    None,
                );
            }
        }

        if let Some(procedure) = &asset.procedure {
            let file = procedure.split('#').next().unwrap_or(procedure);
            if !root.join(file).exists() {
                push(
                    id,
                    format!("`{}`: procedure file `{file}` does not exist", asset.id),
                    None,
                );
            }
        }

        if let Some(date) = &asset.last_reviewed
            && !is_iso_date(date)
        {
            push(
                id,
                format!("`{}`: last_reviewed `{date}` is not YYYY-MM-DD", asset.id),
                None,
            );
        }

        for source in &asset.version_sources {
            if let Some(message) = check_version_source(root, asset, source) {
                push(id, format!("`{}`: {message}", asset.id), None);
            }
        }

        if let Some(other) = &asset.copy_of
            && let Some(first) = asset.paths.first()
            && let Some(message) = compare_trees(root, first, other)
        {
            push(
                id,
                format!(
                    "`{}`: `{first}` is not a byte-identical subset of `{other}`: {message}",
                    asset.id
                ),
                Some(
                    "re-copy from the `copy_of` source, or drop `copy_of` if the copy diverged on purpose",
                ),
            );
        }
    }

    // `follows` cycles.
    for asset in &registry.assets {
        let mut cursor = asset;
        let mut visited = HashSet::new();
        while let Some(next) = cursor.follows.as_deref().and_then(|t| registry.asset(t)) {
            if !visited.insert(next.id.as_str()) || next.id == asset.id {
                push(
                    Some(&asset.id),
                    format!("`{}`: `follows` chain contains a cycle", asset.id),
                    None,
                );
                break;
            }
            cursor = next;
        }
    }

    // Coverage.
    let mut unclaimed_by_dir: HashMap<String, Vec<&str>> = HashMap::new();
    for file in files {
        if !registry.coverage.roots.iter().any(|r| under_root(file, r)) {
            continue;
        }
        if !registry.assets.iter().any(|a| a.claims(file)) {
            let dir = file.rsplit_once('/').map_or("", |(d, _)| d).to_string();
            unclaimed_by_dir.entry(dir).or_default().push(file);
        }
    }
    let mut dirs: Vec<_> = unclaimed_by_dir.into_iter().collect();
    dirs.sort();
    for (dir, files) in dirs {
        push(
            None,
            format!(
                "{} file(s) under `{dir}/` are not claimed by any registry asset (e.g. `{}`)",
                files.len(),
                files[0]
            ),
            Some(
                "add an `[[asset]]` whose `paths` cover them (policy `native` if they are repo-owned)",
            ),
        );
    }

    // Subtree table cross-check.
    for row in subtree_rows {
        let matching = registry.assets.iter().find(|a| {
            a.paths
                .iter()
                .any(|p| p.trim_end_matches('/') == row.prefix)
        });
        match matching {
            None => push(
                None,
                format!(
                    "extension subtree `{}` (`{}`) has no registry asset",
                    row.name, row.prefix
                ),
                Some(
                    "add an `[[asset]]` with policy `track` and a `git` upstream on the subtree's branch",
                ),
            ),
            Some(asset) => match &asset.upstream {
                Some(Upstream::Git {
                    repo,
                    track: GitTrack::Branch(branch),
                    ..
                }) if same_repo(repo, &row.remote_url) && branch == &row.remote_branch => {}
                _ => push(
                    Some(&asset.id),
                    format!(
                        "`{}`: upstream must be `git` on `{}` branch `{}` to match pull-extension-subtree's table",
                        asset.id, row.remote_url, row.remote_branch
                    ),
                    None,
                ),
            },
        }
    }

    Ok(out)
}

/// `None` when `source` agrees with the asset's version; otherwise why not.
fn check_version_source(root: &Path, asset: &Asset, source: &VersionSource) -> Option<String> {
    let Some(version) = asset.version.as_deref() else {
        return Some("has `version_sources` but no `version`".to_string());
    };
    let read = |file: &str| {
        std::fs::read_to_string(root.join(file)).map_err(|e| format!("cannot read `{file}`: {e}"))
    };
    match source {
        VersionSource::FileContains { file } => match read(file) {
            Err(e) => Some(e),
            Ok(text) if text.contains(version) => None,
            Ok(_) => Some(format!("`{file}` does not mention version `{version}`")),
        },
        VersionSource::FileRegex { file, regex } => {
            let re = match regex::Regex::new(regex) {
                Ok(re) => re,
                Err(e) => return Some(format!("bad regex `{regex}`: {e}")),
            };
            match read(file) {
                Err(e) => Some(e),
                Ok(text) => match re.captures(&text).and_then(|c| c.get(1)) {
                    None => Some(format!("regex `{regex}` has no match in `{file}`")),
                    Some(m) if m.as_str() == version => None,
                    Some(m) => Some(format!(
                        "`{file}` says `{}` but the registry says `{version}`",
                        m.as_str()
                    )),
                },
            }
        }
        VersionSource::JsonKey { file, key } => {
            let text = match read(file) {
                Ok(t) => t,
                Err(e) => return Some(e),
            };
            let json: serde_json::Value = match serde_json::from_str(&text) {
                Ok(j) => j,
                Err(e) => return Some(format!("`{file}` is not JSON: {e}")),
            };
            let found = key.segments().into_iter().try_fold(&json, |v, k| v.get(k));
            match found.and_then(|v| v.as_str()) {
                Some(s) if s == version => None,
                Some(s) => Some(format!(
                    "`{file}` key `{key}` is `{s}` but the registry says `{version}`"
                )),
                None => Some(format!("`{file}` has no string at key `{key}`")),
            }
        }
        VersionSource::PathSegment => {
            let first = asset.paths.first()?;
            if first.split('/').any(|c| c == version) {
                None
            } else {
                Some(format!("no component of `{first}` is `{version}`"))
            }
        }
        VersionSource::SubtreeSplit => None,
    }
}

/// `None` when every file under repo-relative directory `a` (the copy) is
/// byte-identical to the same file under `b` (the source). The source may
/// hold more files: a copy can deliberately take a subset (e.g. a Typst
/// package's import working set without its `template/`).
fn compare_trees(root: &Path, a: &str, b: &str) -> Option<String> {
    let list = |dir: &str| -> Result<Vec<String>, String> {
        let base = root.join(dir);
        let mut files = Vec::new();
        for entry in walkdir::WalkDir::new(&base) {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_type().is_file() {
                let rel = entry.path().strip_prefix(&base).unwrap();
                files.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
        files.sort();
        Ok(files)
    };
    let (fa, fb) = match (list(a), list(b)) {
        (Ok(fa), Ok(fb)) => (fa, fb),
        (Err(e), _) | (_, Err(e)) => return Some(e),
    };
    if let Some(extra) = fa.iter().find(|f| !fb.contains(f)) {
        return Some(format!("`{extra}` is not in the `copy_of` source"));
    }
    for f in &fa {
        let ba = std::fs::read(root.join(a).join(f)).ok();
        let bb = std::fs::read(root.join(b).join(f)).ok();
        if ba != bb {
            return Some(format!("`{f}` differs"));
        }
    }
    None
}

fn same_repo(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.trim_end_matches('/')
            .trim_end_matches(".git")
            .to_lowercase()
    };
    norm(a) == norm(b)
}

fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// 1-indexed line of `id = "<id>"` in the registry text (1 if not found).
fn line_of_id(text: &str, id: &str) -> usize {
    let needle = format!("id = \"{id}\"");
    text.lines()
        .position(|l| l.trim() == needle)
        .map_or(1, |i| i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(root: &Path, rel: &str, content: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    /// A small tree: one tracked asset with a README-recorded version, one
    /// byte-identical copy, and one native file.
    fn fixture() -> (TempDir, Vec<String>) {
        let dir = TempDir::new().unwrap();
        let r = dir.path();
        write(r, "resources/lib/1.2.3/lib.js", "lib");
        write(r, "resources/lib/README.md", "lib 1.2.3 from npm");
        write(r, "resources/sub/pkg/1.2.3/lib.js", "lib");
        write(r, "resources/native/ours.js", "ours");
        write(r, "resources/meta.json", r#"{ "a": { "b": "1.2.3" } }"#);
        let files = [
            "resources/lib/1.2.3/lib.js",
            "resources/lib/README.md",
            "resources/sub/pkg/1.2.3/lib.js",
            "resources/native/ours.js",
            "resources/meta.json",
            "elsewhere/ignored.txt",
        ]
        .map(str::to_string)
        .to_vec();
        (dir, files)
    }

    const GOOD: &str = r#"
[coverage]
roots = ["resources"]

[[group]]
id = "g"
title = "group"

[[asset]]
id = "lib"
title = "lib"
paths = ["resources/lib", "resources/meta.json"]
policy = "track"
group = "g"
version = "1.2.3"
version_sources = [
  { kind = "file-contains", file = "resources/lib/README.md" },
  { kind = "file-regex", file = "resources/lib/README.md", regex = 'lib (\S+) from' },
  { kind = "json-key", file = "resources/meta.json", key = "a.b" },
]
upstream = { kind = "npm", package = "lib" }
procedure = "resources/lib/README.md#updating"
last_reviewed = "2026-10-21"

[[asset]]
id = "lib-copy"
title = "copy of lib"
paths = ["resources/sub/pkg/1.2.3"]
policy = "follow"
follows = "lib"
version = "1.2.3"
version_sources = [{ kind = "path-segment" }]
copy_of = "resources/lib/1.2.3"

[[asset]]
id = "ours"
title = "native"
paths = ["resources/native"]
policy = "native"
"#;

    fn run(text: &str) -> Vec<Violation> {
        let (dir, files) = fixture();
        check_with(dir.path(), text, &files, &[]).unwrap()
    }

    fn messages(v: &[Violation]) -> Vec<String> {
        v.iter().map(|v| v.message.clone()).collect()
    }

    #[test]
    fn a_consistent_registry_is_clean() {
        let v = run(GOOD);
        assert!(v.is_empty(), "{:#?}", messages(&v));
    }

    #[test]
    fn version_disagreement_is_flagged_at_the_asset() {
        let text = GOOD.replacen("version = \"1.2.3\"", "version = \"1.2.4\"", 1);
        let v = run(&text);
        let m = messages(&v);
        assert_eq!(v.len(), 3, "{m:#?}");
        assert!(
            m.iter()
                .any(|m| m.contains("does not mention version `1.2.4`"))
        );
        assert!(
            m.iter()
                .any(|m| m.contains("says `1.2.3` but the registry says `1.2.4`"))
        );
        assert!(v.iter().all(|v| v.line == line_of_id(&text, "lib")));
    }

    #[test]
    fn json_keys_accept_segment_arrays_for_dotted_names() {
        let (dir, files) = fixture();
        write(
            dir.path(),
            "resources/meta.json",
            r#"{ "a": { "b.js": "1.2.3" } }"#,
        );
        let dotted = GOOD.replace("key = \"a.b\"", "key = \"a.b.js\"");
        let m = messages(&check_with(dir.path(), &dotted, &files, &[]).unwrap());
        assert!(
            m.iter()
                .any(|m| m.contains("has no string at key `a.b.js`")),
            "{m:#?}"
        );

        let segments = GOOD.replace("key = \"a.b\"", "key = [\"a\", \"b.js\"]");
        let v = check_with(dir.path(), &segments, &files, &[]).unwrap();
        assert!(v.is_empty(), "{:#?}", messages(&v));
    }

    #[test]
    fn unclaimed_files_are_flagged_per_directory() {
        let text = GOOD.replace(
            "paths = [\"resources/native\"]",
            "paths = [\"resources/lib\"]",
        );
        let m = messages(&run(&text));
        assert_eq!(m.len(), 1, "{m:#?}");
        assert!(m[0].contains("under `resources/native/`"), "{m:#?}");
    }

    #[test]
    fn diverged_copies_are_flagged() {
        let (dir, files) = fixture();
        write(dir.path(), "resources/sub/pkg/1.2.3/lib.js", "patched");
        let m = messages(&check_with(dir.path(), GOOD, &files, &[]).unwrap());
        assert_eq!(m.len(), 1, "{m:#?}");
        assert!(m[0].contains("`lib.js` differs"), "{m:#?}");
    }

    #[test]
    fn copies_may_take_a_subset_of_the_source_but_not_add_files() {
        let (dir, files) = fixture();
        write(
            dir.path(),
            "resources/lib/1.2.3/template/main.typ",
            "only upstream",
        );
        let v = check_with(dir.path(), GOOD, &files, &[]).unwrap();
        assert!(v.is_empty(), "{:#?}", messages(&v));

        write(
            dir.path(),
            "resources/sub/pkg/1.2.3/extra.js",
            "only in copy",
        );
        let m = messages(&check_with(dir.path(), GOOD, &files, &[]).unwrap());
        assert!(
            m.iter()
                .any(|m| m.contains("`extra.js` is not in the `copy_of` source")),
            "{m:#?}"
        );
    }

    #[test]
    fn policy_shape_and_references_are_checked() {
        let text = GOOD
            .replace("upstream = { kind = \"npm\", package = \"lib\" }", "")
            .replace("follows = \"lib\"", "follows = \"nope\"")
            .replace("group = \"g\"", "group = \"h\"");
        let m = messages(&run(&text));
        assert!(
            m.iter().any(|m| m.contains("needs an `upstream`")),
            "{m:#?}"
        );
        assert!(m.iter().any(|m| m.contains("target `nope`")), "{m:#?}");
        assert!(m.iter().any(|m| m.contains("unknown group `h`")), "{m:#?}");
    }

    #[test]
    fn follows_cycles_are_flagged() {
        let text = GOOD.replace(
            "policy = \"track\"\ngroup",
            "policy = \"follow\"\nfollows = \"lib-copy\"\ngroup",
        );
        let m = messages(&run(&text));
        assert!(m.iter().any(|m| m.contains("cycle")), "{m:#?}");
    }

    #[test]
    fn missing_paths_and_procedures_are_flagged() {
        let text = GOOD
            .replace("\"resources/meta.json\"]", "\"resources/gone\"]")
            .replace("resources/lib/README.md#updating", "resources/lib/GONE.md");
        let m = messages(&run(&text));
        assert!(
            m.iter()
                .any(|m| m.contains("path `resources/gone` does not exist")),
            "{m:#?}"
        );
        assert!(
            m.iter()
                .any(|m| m.contains("procedure file `resources/lib/GONE.md`")),
            "{m:#?}"
        );
    }

    #[test]
    fn subtree_rows_must_match_a_git_branch_asset() {
        let (dir, files) = fixture();
        let row = SubtreeConfig {
            name: "lib".to_string(),
            prefix: "resources/lib".to_string(),
            remote_url: "https://example.com/lib.git".to_string(),
            remote_branch: "main".to_string(),
        };
        let m =
            messages(&check_with(dir.path(), GOOD, &files, std::slice::from_ref(&row)).unwrap());
        assert_eq!(m.len(), 1, "{m:#?}");
        assert!(
            m[0].contains("to match pull-extension-subtree's table"),
            "{m:#?}"
        );

        let text = GOOD.replace(
            "upstream = { kind = \"npm\", package = \"lib\" }",
            "upstream = { kind = \"git\", repo = \"https://example.com/lib\", track = { branch = \"main\" } }",
        );
        let v = check_with(dir.path(), &text, &files, &[row]).unwrap();
        assert!(v.is_empty(), "{:#?}", messages(&v));
    }

    #[test]
    fn parse_errors_are_reported_not_raised() {
        let m = messages(&run("[coverage]\nroots = 3\n"));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn test_real_tree_is_clean() {
        let manifest_dir = Path::new(&*std::env::var("CARGO_MANIFEST_DIR").unwrap().leak());
        let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();
        let violations = check(workspace_root).unwrap();
        assert!(
            violations.is_empty(),
            "Real tree should have no violations, but got:\n{}",
            violations
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}
