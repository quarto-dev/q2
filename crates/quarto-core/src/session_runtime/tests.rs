//! Unit tests for [`SessionRuntime`]: layer precedence, the two project
//! sources, read-only project files, on-demand binaries, and read recording.
//! Pass 1 over a session runtime is exercised end to end in
//! `tests/integration/session_runtime_pass1.rs`.

use super::*;
use quarto_system_runtime::NativeRuntime;
use std::path::Path;

#[test]
fn every_trait_method_is_written_out() {
    use crate::overlay_runtime::tests::{method_names, trait_methods};
    let in_trait = trait_methods(include_str!("../../../quarto-system-runtime/src/traits.rs"));
    assert!(in_trait.len() > 40, "parsed {} methods", in_trait.len());
    let src = include_str!("mod.rs");
    let start = src.find("impl SystemRuntime for SessionRuntime").unwrap();
    let end = src.find("#[cfg(test)]").unwrap();
    let in_impl = method_names(&src[start..end]);
    let missing: Vec<_> = in_trait.difference(&in_impl).collect();
    assert!(
        missing.is_empty(),
        "SessionRuntime does not write out {missing:?}; a trait default would replace the \
         inner runtime's override"
    );
    let extra: Vec<_> = in_impl.difference(&in_trait).collect();
    assert!(extra.is_empty(), "not trait methods: {extra:?}");
}

/// A canonical temp project directory.
fn project_dir() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    (tmp, root)
}

fn runtime(
    root: &Path,
    source: ProjectSource,
    editor: EditorLayer,
    base: VfsBaseLayer,
) -> SessionRuntime {
    SessionRuntime::new(
        root,
        source,
        Arc::new(editor),
        Arc::new(base),
        Arc::new(NativeRuntime::new()),
    )
}

#[test]
fn buffer_wins_over_base_which_is_a_snapshot_of_disk() {
    let (_tmp, root) = project_dir();
    std::fs::write(root.join("a.qmd"), "from disk").unwrap();
    let base = VfsBaseLayer::load_from_disk(&root).unwrap();
    // The disk moves on after the feeder reported it; the base does not.
    std::fs::write(root.join("a.qmd"), "changed on disk later").unwrap();
    let rt = runtime(&root, ProjectSource::Disk, EditorLayer::new(), base);
    assert_eq!(
        rt.file_read_string(&root.join("a.qmd")).unwrap(),
        "from disk"
    );

    let mut editor = EditorLayer::new();
    editor.set("a.qmd", b"unsaved".to_vec(), 7);
    let base = VfsBaseLayer::load_from_disk(&root).unwrap();
    let rt = runtime(&root, ProjectSource::Disk, editor, base);
    let a = root.join("a.qmd");
    assert_eq!(rt.file_read_string(&a).unwrap(), "unsaved");
    assert_eq!(rt.path_metadata(&a).unwrap().size, 7);
    // An un-normalised spelling of the same file.
    let odd = root.join("sub").join("..").join(".").join("a.qmd");
    assert_eq!(rt.file_read(&odd).unwrap(), b"unsaved");
    assert_eq!(
        rt.reads().get(Path::new("a.qmd")),
        Some(&VersionKey::Buffer(7))
    );
}

#[test]
fn disk_mode_reads_unclassified_project_files_through_to_disk() {
    let (_tmp, root) = project_dir();
    std::fs::write(root.join("doc.qmd"), "x").unwrap();
    std::fs::write(root.join("data.csv"), "a,b\n1,2\n").unwrap();
    let base = VfsBaseLayer::load_from_disk(&root).unwrap();
    assert!(
        base.entry(Path::new("data.csv")).is_none(),
        "the hub classification leaves .csv out of the base"
    );
    let rt = runtime(&root, ProjectSource::Disk, EditorLayer::new(), base);
    let csv = root.join("data.csv");
    assert_eq!(rt.file_read_string(&csv).unwrap(), "a,b\n1,2\n");
    assert_eq!(
        rt.reads().get(Path::new("data.csv")),
        Some(&VersionKey::content_of(b"a,b\n1,2\n"))
    );
    // Missing project files are recorded as absent: one appearing later
    // must invalidate whatever looked for it.
    assert!(!rt.is_file(&root.join("missing.yml")).unwrap());
    assert_eq!(
        rt.reads().get(Path::new("missing.yml")),
        Some(&VersionKey::Absent)
    );
    let listing = rt.dir_list(&root).unwrap();
    assert!(listing.contains(&root.join("data.csv")), "{listing:?}");
    assert!(listing.contains(&root.join("doc.qmd")), "{listing:?}");
}

#[test]
fn attached_mode_never_falls_through_for_project_paths() {
    let (_tmp, root) = project_dir();
    // On disk, but not in the base: invisible to an attached session.
    std::fs::write(root.join("only-on-disk.qmd"), "disk").unwrap();
    let mut base = VfsBaseLayer::new();
    base.insert_file("index.qmd", b"# hub".to_vec());
    base.insert_file("sub/page.qmd", b"# page".to_vec());
    let rt = runtime(&root, ProjectSource::Attached, EditorLayer::new(), base);

    let hidden = root.join("only-on-disk.qmd");
    assert!(!rt.is_file(&hidden).unwrap());
    assert!(!rt.path_exists(&hidden, None).unwrap());
    assert!(rt.file_read(&hidden).is_err());
    assert!(rt.canonicalize(&hidden).is_err());

    assert_eq!(
        rt.file_read_string(&root.join("index.qmd")).unwrap(),
        "# hub"
    );
    assert!(rt.is_dir(&root.join("sub")).unwrap());
    assert_eq!(
        rt.canonicalize(&root.join("sub").join("..").join("index.qmd"))
            .unwrap(),
        root.join("index.qmd")
    );
    assert_eq!(
        rt.dir_list(&root).unwrap(),
        vec![root.join("index.qmd"), root.join("sub")]
    );
    assert_eq!(rt.cwd().unwrap(), root);

    // Outside the root, the inner runtime still answers.
    let outside = tempfile::tempdir().unwrap();
    let file = outside.path().join("x.txt");
    std::fs::write(&file, "outside").unwrap();
    assert_eq!(rt.file_read_string(&file).unwrap(), "outside");
}

#[test]
fn project_files_are_read_only_outside_paths_are_not() {
    let (_tmp, root) = project_dir();
    std::fs::write(root.join("a.qmd"), "x").unwrap();
    let base = VfsBaseLayer::load_from_disk(&root).unwrap();
    let rt = runtime(&root, ProjectSource::Disk, EditorLayer::new(), base);

    let a = root.join("a.qmd");
    let refused = |r: RuntimeResult<()>| {
        assert!(
            matches!(r, Err(RuntimeError::PermissionDenied(_))),
            "expected a read-only refusal, got {r:?}"
        )
    };
    refused(rt.file_write(&a, b"y"));
    refused(rt.file_remove(&a));
    refused(rt.dir_create(&root.join("new"), true));
    refused(rt.path_rename(&a, &root.join("b.qmd")));
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "x");

    let outside = tempfile::tempdir().unwrap();
    let out = outside.path().join("copy.qmd");
    rt.file_copy(&a, &out).unwrap();
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "x");
    refused(rt.file_copy(&out, &root.join("back.qmd")));
}

#[test]
fn binaries_are_present_by_metadata_and_read_on_demand() {
    let (_tmp, root) = project_dir();
    std::fs::create_dir_all(root.join("img")).unwrap();
    std::fs::write(root.join("img").join("logo.png"), [0x89, b'P', b'N', b'G']).unwrap();
    let base = VfsBaseLayer::load_from_disk(&root).unwrap();
    match base.read(Path::new("img/logo.png")) {
        Some(BaseRead::OnDemand(source)) => assert_eq!(source, root.join("img/logo.png")),
        other => panic!("expected an on-demand entry, got {other:?}"),
    }
    let rt = runtime(&root, ProjectSource::Disk, EditorLayer::new(), base);
    let png = root.join("img").join("logo.png");
    assert!(rt.is_file(&png).unwrap());
    assert_eq!(rt.path_metadata(&png).unwrap().size, 4);
    assert_eq!(rt.dir_list(&root.join("img")).unwrap(), vec![png.clone()]);
    assert_eq!(rt.file_read(&png).unwrap(), vec![0x89, b'P', b'N', b'G']);
    assert!(matches!(
        rt.reads().get(Path::new("img/logo.png")),
        Some(VersionKey::Stat { size: 4, .. })
    ));
}

#[test]
fn an_unsaved_new_file_is_listed_and_readable() {
    let (_tmp, root) = project_dir();
    let mut editor = EditorLayer::new();
    editor.set("drafts/new.qmd", b"# new".to_vec(), 1);
    let rt = runtime(&root, ProjectSource::Attached, editor, VfsBaseLayer::new());
    assert!(rt.is_dir(&root.join("drafts")).unwrap());
    assert_eq!(
        rt.dir_list(&root).unwrap(),
        vec![root.join("drafts")],
        "the buffer's directory is listed at the root"
    );
    assert_eq!(
        rt.dir_list(&root.join("drafts")).unwrap(),
        vec![root.join("drafts").join("new.qmd")]
    );
    assert_eq!(
        rt.file_read_string(&root.join("drafts/new.qmd")).unwrap(),
        "# new"
    );
}

#[test]
fn still_matches_validates_every_kind_of_read() {
    let (_tmp, root) = project_dir();
    std::fs::write(root.join("a.qmd"), "base").unwrap();
    std::fs::write(root.join("data.csv"), "@a").unwrap();
    let base = Arc::new(VfsBaseLayer::load_from_disk(&root).unwrap());
    let mut editor = EditorLayer::new();
    editor.set("open.qmd", b"buffer".to_vec(), 2);
    let editor = Arc::new(editor);
    let rt = |editor: Arc<EditorLayer>| {
        SessionRuntime::new(
            &root,
            ProjectSource::Disk,
            editor,
            base.clone(),
            Arc::new(NativeRuntime::new()),
        )
    };

    let first = rt(editor.clone());
    first.file_read(&root.join("a.qmd")).unwrap();
    first.file_read(&root.join("data.csv")).unwrap();
    first.file_read(&root.join("open.qmd")).unwrap();
    assert!(!first.is_file(&root.join("later.yml")).unwrap());
    let reads = first.reads();
    assert_eq!(reads.len(), 4, "{reads:?}");

    // Nothing changed: every read still matches.
    let same = rt(editor.clone());
    assert!(reads.iter().all(|(rel, key)| same.still_matches(rel, key)));

    // Each kind of change invalidates exactly its own entry.
    let mut edited = (*editor).clone();
    edited.set("open.qmd", b"buffer v3".to_vec(), 3);
    let after_edit = rt(Arc::new(edited));
    assert!(!after_edit.still_matches(Path::new("open.qmd"), &reads[Path::new("open.qmd")]));
    assert!(after_edit.still_matches(Path::new("a.qmd"), &reads[Path::new("a.qmd")]));

    let mut shadowing = (*editor).clone();
    shadowing.set("a.qmd", b"now open".to_vec(), 1);
    let after_open = rt(Arc::new(shadowing));
    assert!(!after_open.still_matches(Path::new("a.qmd"), &reads[Path::new("a.qmd")]));

    std::fs::write(root.join("data.csv"), "@b").unwrap();
    std::fs::write(root.join("later.yml"), "x: 1").unwrap();
    let after_disk = rt(editor.clone());
    assert!(!after_disk.still_matches(Path::new("data.csv"), &reads[Path::new("data.csv")]));
    assert!(!after_disk.still_matches(Path::new("later.yml"), &reads[Path::new("later.yml")]));
    // The base is a snapshot: a disk edit to a base file is not seen until
    // the feeder reports it.
    std::fs::write(root.join("a.qmd"), "changed").unwrap();
    assert!(after_disk.still_matches(Path::new("a.qmd"), &reads[Path::new("a.qmd")]));
}
