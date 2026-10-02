//! The mount rules a request must obey (design: Contracts).
//!
//! The worker's filesystem starts empty and is built from the request alone:
//! the share tree, `files` and `resource_refs`; `dirs` are empty directories.
//! Nothing is ever overwritten, so a path claimed twice is a hard error.

use std::collections::{BTreeMap, BTreeSet};

use super::share::share_tree_entries;
use super::types::{PandocRequest, RequestFile};

/// Dedupe identical entries within each list, then check the rules.
/// `wasm_mode` (`collect_resources`) adds the reserved-prefix rules; native
/// temp roots may sit under `/tmp`, so native requests skip them.
pub fn validate_mounts(request: &mut PandocRequest, wasm_mode: bool) -> Result<(), String> {
    dedupe_files(&mut request.files, "files")?;
    dedupe_files(&mut request.resource_refs, "resource_refs")?;
    let mut seen = BTreeSet::new();
    request.dirs.retain(|d| seen.insert(d.clone()));

    let files: BTreeSet<&str> = request.files.iter().map(|f| f.path.as_str()).collect();
    let refs: BTreeSet<&str> = request
        .resource_refs
        .iter()
        .map(|f| f.path.as_str())
        .collect();
    let dirs: BTreeSet<&str> = request.dirs.iter().map(String::as_str).collect();

    for (a_name, a, b_name, b) in [
        ("files", &files, "resource_refs", &refs),
        ("files", &files, "dirs", &dirs),
        ("resource_refs", &refs, "dirs", &dirs),
    ] {
        if let Some(path) = a.intersection(b).next() {
            return Err(format!("path {path} is in both {a_name} and {b_name}"));
        }
    }

    // A path that is a file and also the parent of another file.
    let all_files: BTreeSet<&str> = files.union(&refs).copied().collect();
    for path in &all_files {
        let mut ancestor = *path;
        while let Some(i) = ancestor.rfind('/') {
            ancestor = &ancestor[..i];
            if !ancestor.is_empty() && all_files.contains(ancestor) {
                return Err(format!("path {ancestor} is both a file and a directory"));
            }
        }
    }

    let share_prefix = format!("{}/", request.share_tree_path);
    for f in &request.files {
        if let Some(rel) = f.path.strip_prefix(&share_prefix)
            && share_tree_entries().iter().any(|e| e.rel_path == rel)
        {
            return Err(format!("files path {} is a share-tree entry", f.path));
        }
    }

    if wasm_mode {
        let reserved = |p: &str, root: &str| p == root || p.starts_with(&format!("{root}/"));
        for f in &request.resource_refs {
            for root in [request.share_root.as_str(), "/tmp"] {
                if reserved(&f.path, root) {
                    return Err(format!(
                        "resource_refs path {} is under the reserved {root}",
                        f.path
                    ));
                }
            }
        }
        for f in &request.files {
            if reserved(&f.path, "/tmp") {
                return Err(format!("files path {} is under the reserved /tmp", f.path));
            }
        }
    }
    Ok(())
}

fn dedupe_files(list: &mut Vec<RequestFile>, name: &str) -> Result<(), String> {
    let mut by_path: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut out = Vec::with_capacity(list.len());
    for f in list.drain(..) {
        match by_path.get(&f.path) {
            None => {
                by_path.insert(f.path.clone(), f.bytes.clone());
                out.push(f);
            }
            Some(bytes) if *bytes == f.bytes => {}
            Some(_) => {
                return Err(format!(
                    "path {} appears twice in {name} with different bytes",
                    f.path
                ));
            }
        }
    }
    *list = out;
    Ok(())
}
