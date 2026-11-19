//! The sidecar document schema over `automerge::AutoCommit`, as the contract's "Sidecar doc" section says:
//! `version` (int 1), `entries` and `retired` (flat maps, key to int generation), created once, in the
//! change that creates the doc. A reader accepts both string forms (scalar and Text) for the index fields.

use super::{
    Entry, RegistryOp, RegistryView, parse_key, plan_move_forward, plan_register, plan_repair,
    plan_retire,
};
use automerge::transaction::Transactable;
use automerge::{AutoCommit, ObjId, ObjType, ROOT, ReadDoc, ScalarValue, Value};

/// A new sidecar; both maps are created in this one change, by this one writer.
pub fn create_sidecar() -> AutoCommit {
    let mut d = AutoCommit::new();
    d.put(ROOT, "version", 1_i64).expect("put version");
    d.put_object(ROOT, "entries", ObjType::Map)
        .expect("create entries");
    d.put_object(ROOT, "retired", ObjType::Map)
        .expect("create retired");
    d.commit();
    d
}

fn map_id<D: ReadDoc>(d: &D, name: &str) -> Option<ObjId> {
    match d.get(ROOT, name).ok().flatten() {
        Some((Value::Object(ObjType::Map), id)) => Some(id),
        _ => None,
    }
}

fn generation_of(v: &Value<'_>) -> i64 {
    match v {
        Value::Scalar(s) => match s.as_ref() {
            ScalarValue::Int(n) => *n,
            ScalarValue::Uint(n) => *n as i64,
            ScalarValue::F64(f) => *f as i64,
            ScalarValue::Counter(c) => i64::from(c),
            _ => 0,
        },
        _ => 0,
    }
}

fn read_map<D: ReadDoc>(d: &D, name: &str) -> Vec<Entry> {
    let Some(m) = map_id(d, name) else {
        return vec![];
    };
    d.keys(&m)
        .filter_map(|k| {
            let parts = parse_key(&k)?;
            let (v, _) = d.get(&m, &k).ok().flatten()?;
            Some(Entry {
                path: parts.path,
                blob: parts.blob,
                doc: parts.doc,
                generation: generation_of(&v),
            })
        })
        .collect()
}

pub fn read_entries<D: ReadDoc>(d: &D) -> Vec<Entry> {
    read_map(d, "entries")
}

pub fn read_retired<D: ReadDoc>(d: &D) -> Vec<Entry> {
    read_map(d, "retired")
}

pub fn view_of<D: ReadDoc>(d: &D) -> RegistryView {
    RegistryView {
        entries: read_entries(d),
        retired: read_retired(d),
    }
}

/// Applies the ops in one Automerge change. Returns whether anything was written.
pub fn apply(d: &mut AutoCommit, ops: &[RegistryOp]) -> bool {
    if ops.is_empty() {
        return false;
    }
    let entries = map_id(d, "entries").expect("sidecar has an entries map");
    let retired = map_id(d, "retired").expect("sidecar has a retired map");
    for op in ops {
        match op {
            RegistryOp::Put { key, generation } => {
                d.put(&entries, key.as_str(), *generation)
                    .expect("put entry");
            }
            RegistryOp::Delete { key } => {
                d.delete(&entries, key.as_str()).expect("delete entry");
            }
            RegistryOp::Retire { key, generation } => {
                d.put(&retired, key.as_str(), *generation)
                    .expect("put retired");
            }
        }
    }
    d.commit();
    true
}

/// Put-if-absent. Returns whether the entry was written.
pub fn register(d: &mut AutoCommit, e: &Entry) -> bool {
    let ops = plan_register(&view_of(d), e);
    apply(d, &ops)
}

pub fn move_forward(
    d: &mut AutoCommit,
    path: &str,
    old_blob: &str,
    new_blob: &str,
    doc: &str,
) -> bool {
    let ops = plan_move_forward(&view_of(d), path, old_blob, new_blob, doc);
    apply(d, &ops)
}

pub fn retire(d: &mut AutoCommit, path: &str, blob: &str, doc: &str) -> bool {
    let ops = plan_retire(&view_of(d), path, blob, doc);
    apply(d, &ops)
}

pub fn repair(d: &mut AutoCommit, path: &str, doc: &str, generation: i64, head_blob: &str) -> bool {
    let ops = plan_repair(&view_of(d), path, doc, generation, head_blob);
    apply(d, &ops)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFields {
    pub roots: Vec<String>,
    pub prefix: String,
}

/// The git-epochs keys of a version 4 index doc. Absent keys come back `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexFields {
    pub epochs: Option<String>,
    pub owner: Option<String>,
    pub repo: Option<RepoFields>,
}

/// A string in either form: a scalar string, or a Text object.
fn string_in<D: ReadDoc, P: Into<automerge::Prop>>(d: &D, obj: &ObjId, prop: P) -> Option<String> {
    let (v, id) = d.get(obj, prop).ok().flatten()?;
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    if matches!(v, Value::Object(ObjType::Text)) {
        return d.text(&id).ok();
    }
    None
}

pub fn read_index_fields<D: ReadDoc>(d: &D) -> IndexFields {
    let mut out = IndexFields {
        epochs: string_in(d, &ROOT, "epochs"),
        owner: string_in(d, &ROOT, "owner"),
        repo: None,
    };
    if let Some(repo) = map_id(d, "repo") {
        let roots = match d.get(&repo, "roots").ok().flatten() {
            Some((Value::Object(ObjType::List), list)) => (0..d.length(&list))
                .filter_map(|i| string_in(d, &list, i))
                .collect(),
            _ => vec![],
        };
        out.repo = Some(RepoFields {
            roots,
            prefix: string_in(d, &repo, "prefix").unwrap_or_default(),
        });
    }
    out
}
