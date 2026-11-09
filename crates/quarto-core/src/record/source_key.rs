/*
 * source_key.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The `execute: record-sources` key and the one early reader of it.
 */

//! The record source list as a metadata key (OPR-5, decision D7).
//!
//! `execute: record-sources` is a **comma-separated scalar** (`positron,engine`),
//! read **before any pipeline runs**, because the `q2` source is read from disk
//! and the engine decision comes first. [`resolve_record_sources`] reads it from
//! the layers [`collect_metadata_layers`] collects (project, directory,
//! document, runtime), parsing only front matter, so it is cheap enough per
//! keystroke and compiles for wasm.
//!
//! Known limitation (a better solution is wanted later): the value is a scalar
//! and not a YAML list because the runtime layer (`-M`, the provider overlay) is
//! JSON, cannot carry `!prefer`, and a list from it would append to a document
//! list. The fix would be the provider reading the runtime layer directly, or
//! `json_to_config_value` learning a way to say "prefer".
//!
//! Each command declares what it accepts in a [`CommandSources`]: allowed
//! values and a default. A valid entry the command cannot use is ignored with
//! an info note; an unknown or malformed entry is an error. A directory entry
//! is inside the project only: it resolves against the `base_dir` of the layer
//! that set the value (a leading `/` is the project root), is normalized
//! lexically, and must exist. A Positron cache outside the project has to be
//! copied or linked in; the `positron` keyword still finds the default
//! location itself.
//!
//! An extension's `contributes.metadata` that sets the key is read by the late
//! merge but **not** by this early reader (the same early-versus-late gap the
//! format resolver has); it is not honored.
//!
//! Untrusted authors: with [`Trust::Ceiling`] the runtime layer is the host's
//! and defines the allowed set; the guest's value, read from the layers below
//! it, may only reorder or narrow that set.

use std::path::{Component, Path, PathBuf};

use quarto_system_runtime::SystemRuntime;

use super::sources::{RecordSource, RecordSources};
use crate::metadata_layers::{
    DocumentLayer, LayerInputs, LayerKind, collect_metadata_layers, layered_value,
};
use crate::project::ProjectContext;

/// The key's name as every message spells it.
pub const RECORD_SOURCES_KEY: &str = "execute: record-sources";

const KEY_PATH: [&str; 2] = ["execute", "record-sources"];

/// What one command accepts for the key.
#[derive(Debug, Clone, Copy)]
pub struct CommandSources {
    /// The command as a message names it (`q2 preview`).
    pub command: &'static str,
    /// The keywords the command can use (`cache`, `q2`, `positron`, `engine`).
    pub allowed: &'static [&'static str],
    /// The list used when no layer sets the key.
    pub default: &'static [&'static str],
    /// Whether an in-project directory entry is usable.
    pub directories: bool,
}

impl CommandSources {
    /// `q2 preview`: all five kinds, default `cache,q2,engine`.
    pub const PREVIEW: CommandSources = CommandSources {
        command: "q2 preview",
        allowed: &["cache", "q2", "positron", "engine"],
        default: &["cache", "q2", "engine"],
        directories: true,
    };
    /// `q2 convert`: `q2` and `engine`, default `q2,engine`.
    pub const CONVERT: CommandSources = CommandSources {
        command: "q2 convert",
        allowed: &["q2", "engine"],
        default: &["q2", "engine"],
        directories: false,
    };
    /// `q2 provide-hub`: the host list. No directories, no `cache`; default `engine`.
    pub const PROVIDE_HUB: CommandSources = CommandSources {
        command: "q2 provide-hub",
        allowed: &["q2", "positron", "engine"],
        default: &["engine"],
        directories: false,
    };
    /// `q2 render` (OPR-6): refuses `cache`, default `engine`.
    pub const RENDER: CommandSources = CommandSources {
        command: "q2 render",
        allowed: &["q2", "positron", "engine"],
        default: &["engine"],
        directories: true,
    };

    /// The command's default list.
    pub fn default_list(&self) -> RecordSources {
        RecordSources::new(
            self.default
                .iter()
                .filter_map(|k| k.parse::<RecordSource>().ok())
                .collect(),
        )
    }

    fn allows(&self, source: &RecordSource) -> bool {
        match source {
            RecordSource::Dir(_) => self.directories,
            other => self.allowed.contains(&other.label().as_str()),
        }
    }
}

/// Who wrote the value the reader is allowed to believe.
#[derive(Debug, Clone, Copy)]
pub enum Trust<'a> {
    /// The author is the person running the command: `-M` is a plain override.
    Override,
    /// The runtime layer is the host's. `allowed` is the host list; a guest
    /// document may reorder or narrow it and may not add to it.
    Ceiling { allowed: &'a RecordSources },
}

/// A line the reader wants the caller to report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceNote {
    /// `true`: a warning (a guest named something outside the ceiling).
    /// `false`: an info line (an entry this command cannot use).
    pub warning: bool,
    pub text: String,
}

/// What [`resolve_record_sources`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSources {
    /// The list, in priority order. Empty means nothing usable is left.
    pub sources: RecordSources,
    /// The layer that set the key; `None` when the default applies.
    pub origin: Option<LayerKind>,
    pub notes: Vec<SourceNote>,
}

/// An unknown or malformed entry, or a directory the command cannot read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceKeyError(pub String);

impl std::fmt::Display for SourceKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SourceKeyError {}

/// Read `execute: record-sources` for one document, before any pipeline.
///
/// `content_override` is the text to read the document layer from (an editor
/// buffer); `None` reads the file through `runtime`.
pub fn resolve_record_sources(
    project: &ProjectContext,
    doc_path: &Path,
    content_override: Option<&str>,
    runtime: &dyn SystemRuntime,
    command: &CommandSources,
    trust: Trust<'_>,
) -> Result<ResolvedSources, SourceKeyError> {
    let read;
    let content = match content_override {
        Some(c) => c,
        None => {
            read = runtime.file_read_string(doc_path).unwrap_or_default();
            &read
        }
    };
    let layers = collect_metadata_layers(&LayerInputs {
        project,
        document_path: doc_path,
        document_dir: doc_path.parent().map(Path::to_path_buf).unwrap_or_default(),
        document: DocumentLayer::Content(content),
        extensions: None,
        target_format: None,
        is_book: false,
        runtime,
    });
    let include_runtime = matches!(trust, Trust::Override);
    let found = layered_value(&layers, &KEY_PATH, include_runtime).filter(|(v, _)| !v.is_null());
    let Some((value, layer)) = found else {
        let sources = match trust {
            Trust::Override => command.default_list(),
            Trust::Ceiling { allowed } => allowed.clone(),
        };
        return Ok(ResolvedSources {
            sources,
            origin: None,
            notes: Vec::new(),
        });
    };
    let Some(text) = value.as_plain_text() else {
        return Err(SourceKeyError(format!(
            "`{RECORD_SOURCES_KEY}` must be a comma-separated string such as `q2,engine`, not a \
             list or a map"
        )));
    };

    let (sources, notes) = resolve_entries(
        &text,
        &layer.base_dir,
        &project.dir,
        runtime,
        command,
        trust,
    )?;
    Ok(ResolvedSources {
        sources,
        origin: Some(layer.kind),
        notes,
    })
}

/// The host's own list, read from the runtime metadata alone (`-M`) at command start:
/// the command default when the key is absent. Directories resolve against `base_dir`
/// and must stay inside `project_dir`. This is the allowed set a [`Trust::Ceiling`]
/// reader enforces.
pub fn host_record_sources(
    runtime_metadata: Option<&serde_json::Value>,
    base_dir: &Path,
    project_dir: &Path,
    runtime: &dyn SystemRuntime,
    command: &CommandSources,
) -> Result<ResolvedSources, SourceKeyError> {
    let value = runtime_metadata
        .and_then(|m| m.get(KEY_PATH[0]))
        .and_then(|e| e.get(KEY_PATH[1]))
        .filter(|v| !v.is_null());
    let Some(value) = value else {
        return Ok(ResolvedSources {
            sources: command.default_list(),
            origin: None,
            notes: Vec::new(),
        });
    };
    let Some(text) = value.as_str() else {
        return Err(SourceKeyError(format!(
            "`{RECORD_SOURCES_KEY}` must be a comma-separated string such as `q2,engine`, not a \
             list or a map"
        )));
    };
    let (sources, notes) = resolve_entries(
        text,
        base_dir,
        project_dir,
        runtime,
        command,
        Trust::Override,
    )?;
    Ok(ResolvedSources {
        sources,
        origin: Some(LayerKind::Runtime),
        notes,
    })
}

/// Parse, filter and resolve the entries of one value.
fn resolve_entries(
    text: &str,
    base_dir: &Path,
    project_dir: &Path,
    runtime: &dyn SystemRuntime,
    command: &CommandSources,
    trust: Trust<'_>,
) -> Result<(RecordSources, Vec<SourceNote>), SourceKeyError> {
    let mut notes = Vec::new();
    let mut out: Vec<RecordSource> = Vec::new();
    for raw in text.split(',') {
        let entry = raw.trim();
        let parsed = entry
            .parse::<RecordSource>()
            .map_err(|e| SourceKeyError(format!("`{RECORD_SOURCES_KEY}`: {e}")))?;
        if !command.allows(&parsed) {
            notes.push(SourceNote {
                warning: false,
                text: format!(
                    "`{RECORD_SOURCES_KEY}`: `{entry}` is ignored; `{}` cannot use it",
                    command.command
                ),
            });
            continue;
        }
        let source = match (parsed, trust) {
            (RecordSource::Dir(_), Trust::Ceiling { .. }) => {
                notes.push(SourceNote {
                    warning: true,
                    text: format!(
                        "`{RECORD_SOURCES_KEY}`: the directory `{entry}` in the document is ignored; \
                         a document may not add a source the host did not allow"
                    ),
                });
                continue;
            }
            (RecordSource::Dir(p), Trust::Override) => RecordSource::Dir(resolve_directory(
                entry,
                &p,
                base_dir,
                project_dir,
                runtime,
            )?),
            (other, Trust::Ceiling { allowed }) => {
                if !allowed.as_slice().contains(&other) {
                    notes.push(SourceNote {
                        warning: true,
                        text: format!(
                            "`{RECORD_SOURCES_KEY}`: `{entry}` in the document is ignored; the \
                             host did not allow it (allowed: {})",
                            describe(allowed)
                        ),
                    });
                    continue;
                }
                other
            }
            (other, Trust::Override) => other,
        };
        if !out.contains(&source) {
            out.push(source);
        }
    }
    Ok((RecordSources::new(out), notes))
}

/// A list as a message prints it (`cache, q2, engine`).
pub fn describe(list: &RecordSources) -> String {
    let labels: Vec<String> = list.iter().map(RecordSource::label).collect();
    if labels.is_empty() {
        "nothing".to_string()
    } else {
        labels.join(", ")
    }
}

/// A directory entry, inside the project only.
fn resolve_directory(
    entry: &str,
    path: &Path,
    layer_base: &Path,
    project_dir: &Path,
    runtime: &dyn SystemRuntime,
) -> Result<PathBuf, SourceKeyError> {
    let outside = || {
        SourceKeyError(format!(
            "`{RECORD_SOURCES_KEY}`: the directory `{entry}` is outside the project; only \
             directories inside the project are accepted (a leading `/` means the project \
             root). Copy or link a cache from elsewhere into the project"
        ))
    };
    if entry.starts_with('~') {
        return Err(outside());
    }
    let joined = if let Some(rest) = entry.strip_prefix('/') {
        project_dir.join(rest)
    } else if path.is_absolute() {
        return Err(outside());
    } else {
        layer_base.join(path)
    };
    let normal = normalize(&joined).ok_or_else(outside)?;
    if !normal.starts_with(project_dir) && !same_place(&normal, project_dir, runtime) {
        return Err(outside());
    }
    match runtime.is_dir(&normal) {
        Ok(true) => Ok(normal),
        _ if runtime.is_file(&normal).unwrap_or(false) => Err(SourceKeyError(format!(
            "`{RECORD_SOURCES_KEY}`: the entry `{entry}` ({}) is a file, not a directory",
            normal.display()
        ))),
        _ => Err(SourceKeyError(format!(
            "`{RECORD_SOURCES_KEY}`: the directory `{entry}` ({}) does not exist",
            normal.display()
        ))),
    }
}

/// Whether `dir` is inside `project_dir` once symlinks are resolved (the
/// lexical check fails for a document reached through a link).
fn same_place(dir: &Path, project_dir: &Path, runtime: &dyn SystemRuntime) -> bool {
    match (runtime.canonicalize(dir), runtime.canonicalize(project_dir)) {
        (Ok(d), Ok(p)) => d.starts_with(p),
        _ => false,
    }
}

/// Drop `.` and fold `..` without touching the filesystem; `None` when a
/// `..` would climb above the root.
fn normalize(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::metadata_layers::test_runtime::MetaRuntime;
    use serde_json::json;
    use tempfile::TempDir;

    struct Fx {
        _tmp: TempDir,
        root: PathBuf,
        doc: PathBuf,
        project: ProjectContext,
    }

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    /// `_quarto.yml` (with `project_extra` lines), `a/_metadata.yml`
    /// (`dir_extra`), `a/doc.qmd` (front matter `doc_extra`), and a `cache/`
    /// directory at the project root and one under `a/`.
    fn fx(project_extra: &str, dir_extra: &str, doc_extra: &str) -> Fx {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        write(
            &root.join("_quarto.yml"),
            &format!("project:\n  type: default\n{project_extra}"),
        );
        write(&root.join("a/_metadata.yml"), dir_extra);
        let doc = root.join("a/doc.qmd");
        write(&doc, &format!("---\n{doc_extra}---\n\nbody\n"));
        std::fs::create_dir_all(root.join("recs")).unwrap();
        std::fs::create_dir_all(root.join("a/local")).unwrap();
        let project =
            ProjectContext::discover_with_profile(&root, &MetaRuntime::new(None), None).unwrap();
        Fx {
            _tmp: tmp,
            root,
            doc,
            project,
        }
    }

    fn read(
        f: &Fx,
        runtime: serde_json::Value,
        cmd: &CommandSources,
        trust: Trust<'_>,
    ) -> Result<ResolvedSources, SourceKeyError> {
        let rt = MetaRuntime::new(Some(runtime));
        resolve_record_sources(&f.project, &f.doc, None, &rt, cmd, trust)
    }

    fn labels(r: &ResolvedSources) -> Vec<String> {
        r.sources.iter().map(RecordSource::label).collect()
    }

    fn key(v: &str) -> serde_json::Value {
        json!({"execute": {"record-sources": v}})
    }

    #[test]
    fn default_when_no_layer_sets_the_key() {
        let f = fx("", "", "");
        let r = read(&f, json!({}), &CommandSources::PREVIEW, Trust::Override).unwrap();
        assert_eq!(labels(&r), ["cache", "q2", "engine"]);
        assert_eq!(r.origin, None);
        let r = read(&f, json!({}), &CommandSources::PROVIDE_HUB, Trust::Override).unwrap();
        assert_eq!(labels(&r), ["engine"]);
    }

    #[test]
    fn precedence_project_directory_document_runtime() {
        let project = "execute:\n  record-sources: positron\n";
        let f = fx(project, "", "");
        let r = read(&f, json!({}), &CommandSources::PREVIEW, Trust::Override).unwrap();
        assert_eq!(
            (labels(&r), r.origin),
            (vec!["positron".into()], Some(LayerKind::Project))
        );

        let f = fx(project, "execute:\n  record-sources: q2\n", "");
        let r = read(&f, json!({}), &CommandSources::PREVIEW, Trust::Override).unwrap();
        assert_eq!(
            (labels(&r), r.origin),
            (vec!["q2".into()], Some(LayerKind::Directory))
        );

        let f = fx(
            project,
            "execute:\n  record-sources: q2\n",
            "execute:\n  record-sources: cache\n",
        );
        let r = read(&f, json!({}), &CommandSources::PREVIEW, Trust::Override).unwrap();
        assert_eq!(
            (labels(&r), r.origin),
            (vec!["cache".into()], Some(LayerKind::Document))
        );

        let r = read(
            &f,
            key("engine,q2"),
            &CommandSources::PREVIEW,
            Trust::Override,
        )
        .unwrap();
        assert_eq!(labels(&r), ["engine", "q2"]);
        assert_eq!(r.origin, Some(LayerKind::Runtime));
    }

    #[test]
    fn early_reader_agrees_with_the_merged_metadata() {
        use crate::metadata_layers::{MetadataLayer, collect_metadata_layers};
        use quarto_config::MergedConfig;
        let f = fx(
            "execute:\n  record-sources: positron\n",
            "execute:\n  record-sources: q2\n",
            "execute:\n  record-sources: cache,engine\n",
        );
        let rt = MetaRuntime::new(Some(key("engine")));
        let layers: Vec<MetadataLayer> = collect_metadata_layers(&LayerInputs {
            project: &f.project,
            document_path: &f.doc,
            document_dir: f.doc.parent().unwrap().to_path_buf(),
            document: DocumentLayer::Content(&std::fs::read_to_string(&f.doc).unwrap()),
            extensions: None,
            target_format: None,
            is_book: false,
            runtime: &rt,
        });
        let merged = MergedConfig::new(layers.iter().map(|l| &l.config).collect())
            .materialize()
            .unwrap();
        let merged_text = merged
            .get("execute")
            .and_then(|e| e.get("record-sources"))
            .and_then(|v| v.as_plain_text());
        let r = resolve_record_sources(
            &f.project,
            &f.doc,
            None,
            &rt,
            &CommandSources::PREVIEW,
            Trust::Override,
        )
        .unwrap();
        assert_eq!(merged_text.as_deref(), Some("engine"));
        assert_eq!(labels(&r), ["engine"]);
    }

    #[test]
    fn content_override_beats_the_file() {
        let f = fx("", "", "execute:\n  record-sources: q2\n");
        let rt = MetaRuntime::new(None);
        let r = resolve_record_sources(
            &f.project,
            &f.doc,
            Some("---\nexecute:\n  record-sources: engine\n---\n"),
            &rt,
            &CommandSources::PREVIEW,
            Trust::Override,
        )
        .unwrap();
        assert_eq!(labels(&r), ["engine"]);
    }

    #[test]
    fn an_entry_the_command_cannot_use_is_ignored_with_an_info_note() {
        let f = fx("", "", "");
        let r = read(
            &f,
            key("cache,q2,positron,engine"),
            &CommandSources::CONVERT,
            Trust::Override,
        )
        .unwrap();
        assert_eq!(labels(&r), ["q2", "engine"]);
        assert_eq!(r.notes.len(), 2);
        assert!(r.notes.iter().all(|n| !n.warning));
        assert!(r.notes[0].text.contains("`cache`") && r.notes[0].text.contains("q2 convert"));
        // A directory is not usable by convert either.
        let r = read(
            &f,
            key("./recs,q2"),
            &CommandSources::CONVERT,
            Trust::Override,
        )
        .unwrap();
        assert_eq!(labels(&r), ["q2"]);
        assert_eq!(r.notes.len(), 1);
    }

    #[test]
    fn unknown_empty_and_non_scalar_values_are_errors() {
        let f = fx("", "", "");
        let e = read(
            &f,
            key("q2,bogus"),
            &CommandSources::PREVIEW,
            Trust::Override,
        )
        .unwrap_err();
        assert!(
            e.0.contains("`bogus`") && e.0.contains(RECORD_SOURCES_KEY),
            "{e}"
        );
        assert!(
            read(
                &f,
                key("q2,,engine"),
                &CommandSources::PREVIEW,
                Trust::Override
            )
            .is_err()
        );
        let e = read(
            &f,
            json!({"execute": {"record-sources": ["q2", "engine"]}}),
            &CommandSources::PREVIEW,
            Trust::Override,
        )
        .unwrap_err();
        assert!(e.0.contains("comma-separated string"), "{e}");
    }

    #[test]
    fn a_directory_resolves_against_the_declaring_layer() {
        // Set in the directory layer (`a/_metadata.yml`): relative to `a/`.
        let f = fx("", "execute:\n  record-sources: ./local,engine\n", "");
        let r = read(&f, json!({}), &CommandSources::PREVIEW, Trust::Override).unwrap();
        assert_eq!(
            r.sources.as_slice()[0],
            RecordSource::Dir(f.root.join("a/local"))
        );
        // Set in the project layer: relative to the project root.
        let f = fx("execute:\n  record-sources: ./recs\n", "", "");
        let r = read(&f, json!({}), &CommandSources::PREVIEW, Trust::Override).unwrap();
        assert_eq!(
            r.sources.as_slice()[0],
            RecordSource::Dir(f.root.join("recs"))
        );
        // A leading `/` is the project root, from a nested layer.
        let f = fx("", "execute:\n  record-sources: /recs\n", "");
        let r = read(&f, json!({}), &CommandSources::PREVIEW, Trust::Override).unwrap();
        assert_eq!(
            r.sources.as_slice()[0],
            RecordSource::Dir(f.root.join("recs"))
        );
        // `..` that stays inside the project is fine; the runtime layer is
        // relative to the document's directory.
        let r = read(
            &f,
            key("../recs"),
            &CommandSources::PREVIEW,
            Trust::Override,
        )
        .unwrap();
        assert_eq!(
            r.sources.as_slice()[0],
            RecordSource::Dir(f.root.join("recs"))
        );
    }

    #[test]
    fn directories_outside_the_project_or_missing_are_errors() {
        let f = fx("", "", "");
        for entry in ["../../elsewhere", "~/cache", "../.."] {
            let e = read(&f, key(entry), &CommandSources::PREVIEW, Trust::Override).unwrap_err();
            assert!(
                e.0.contains(entry) && e.0.contains("outside the project"),
                "{entry}: {e}"
            );
        }
        let e = read(&f, key("./nope"), &CommandSources::PREVIEW, Trust::Override).unwrap_err();
        assert!(
            e.0.contains("./nope") && e.0.contains("does not exist"),
            "{e}"
        );
        write(&f.root.join("a/afile"), "x");
        let e = read(
            &f,
            key("./afile"),
            &CommandSources::PREVIEW,
            Trust::Override,
        )
        .unwrap_err();
        assert!(e.0.contains("is a file"), "{e}");
    }

    #[test]
    fn ceiling_lets_a_document_reorder_and_narrow() {
        let host = RecordSources::new(vec![
            RecordSource::Positron,
            RecordSource::Q2,
            RecordSource::Engine,
        ]);
        let f = fx(
            "",
            "",
            "execute:\n  record-sources: engine,positron,engine\n",
        );
        // The host's own value (the runtime layer) is not read as the guest's.
        let r = read(
            &f,
            key("q2"),
            &CommandSources::PROVIDE_HUB,
            Trust::Ceiling { allowed: &host },
        )
        .unwrap();
        assert_eq!(labels(&r), ["engine", "positron"]);
        assert!(r.notes.is_empty());
        // No guest key: the whole host list.
        let f = fx("", "", "");
        let r = read(
            &f,
            json!({}),
            &CommandSources::PROVIDE_HUB,
            Trust::Ceiling { allowed: &host },
        )
        .unwrap();
        assert_eq!(labels(&r), ["positron", "q2", "engine"]);
    }

    #[test]
    fn ceiling_ignores_what_the_host_did_not_allow_with_a_warning() {
        let host = RecordSources::provider_default();
        let f = fx("", "", "execute:\n  record-sources: positron,engine\n");
        let r = read(
            &f,
            json!({}),
            &CommandSources::PROVIDE_HUB,
            Trust::Ceiling { allowed: &host },
        )
        .unwrap();
        assert_eq!(labels(&r), ["engine"]);
        assert_eq!(r.notes.len(), 1);
        assert!(r.notes[0].warning && r.notes[0].text.contains("`positron`"));
        // Narrowed to nothing: an empty list, with the warning.
        let f = fx("", "", "execute:\n  record-sources: positron\n");
        let r = read(
            &f,
            json!({}),
            &CommandSources::PROVIDE_HUB,
            Trust::Ceiling { allowed: &host },
        )
        .unwrap();
        assert!(r.sources.as_slice().is_empty());
        assert_eq!(r.notes.len(), 1);
        // A guest directory is ignored with a warning, even when it exists.
        let f = fx("", "", "execute:\n  record-sources: ./local,engine\n");
        let r = read(
            &f,
            json!({}),
            &CommandSources::PREVIEW,
            Trust::Ceiling { allowed: &host },
        )
        .unwrap();
        assert_eq!(labels(&r), ["engine"]);
        assert!(r.notes[0].warning && r.notes[0].text.contains("./local"));
    }
}
