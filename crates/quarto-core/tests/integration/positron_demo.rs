//! Spike S6 (EXR-16) harness, driven by environment variables; both tests are `#[ignore]`.
//!
//! * `writes_a_demo_record` builds a real q2 record (RecordBuilder, coalesce, write_record) for
//!   the document in `Q2_DEMO_QMD` with distinctive outputs and writes it to `Q2_DEMO_OUT`. The
//!   outputs say "q2 record" so the demo shows whose outputs Positron displays, and one cell
//!   carries an unknown mime type.
//! * `reports_a_positron_cache` reads the Positron cache file `Q2_DEMO_CACHE` through
//!   `import_positron_cache` and prints, for each unit of `Q2_DEMO_QMD`, what matched.
//!
//! Run: `Q2_DEMO_QMD=... Q2_DEMO_OUT=... cargo nextest run -p quarto-core --run-ignored only
//! --no-capture -E 'test(positron_demo::)'`.

use quarto_core::engine::units::{Unit, scan_units};
use quarto_core::record::{
    AuthorTexts, EngineEntryInput, HashDefinition, JUPYTER_ENGINE, RecordBuilder, UnitResult,
    coalesce, fill_ids, import_positron_cache, write_record,
};
use serde_json::json;

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {name}"))
}

fn staged(text: &str) -> (Vec<Unit>, AuthorTexts) {
    let mut units = scan_units(text);
    let texts = AuthorTexts::from_scanned(&units);
    fill_ids(&mut units, &texts).unwrap();
    (units, texts)
}

#[test]
#[ignore = "spike S6 harness; needs Q2_DEMO_QMD and Q2_DEMO_OUT"]
fn writes_a_demo_record() {
    let text = std::fs::read_to_string(var("Q2_DEMO_QMD")).unwrap();
    let (units, texts) = staged(&text);
    let def = HashDefinition::conservative("jupyter");
    let mut b = RecordBuilder::new("jupyter", &def, &units, &texts).unwrap();
    let mut cell_n = 0;
    for (i, u) in units.iter().enumerate() {
        let outputs = match u {
            Unit::Inline(_) => vec![json!({
                "output_type": "execute_result",
                "execution_count": 1,
                "metadata": {},
                "data": {"text/plain": "'q2-span'", "text/x-quarto-markdown": "**q2 span**"}
            })],
            Unit::Cell(_) => {
                cell_n += 1;
                match cell_n {
                    1 => vec![json!({"output_type": "stream", "name": "stdout",
                                     "text": ["hello from the q2 record\n"]})],
                    2 => vec![
                        json!({"output_type": "execute_result", "execution_count": 2,
                                     "metadata": {},
                                     "data": {"text/plain": "q2-six"}}),
                    ],
                    _ => vec![
                        json!({"output_type": "display_data",
                                     "metadata": {"quarto_widget": {"k": 1}},
                                     "data": {"text/plain": "unknown-mime fallback from q2",
                                              "application/x-q2-unknown": {"a": 1}}}),
                        json!({"output_type": "execute_result", "execution_count": 3,
                                      "metadata": {},
                                      "data": {"text/html": "<b>html from the q2 record</b>",
                                               "text/plain": "html fallback"}}),
                    ],
                }
            }
        };
        b.push(
            i,
            UnitResult {
                outputs,
                execution_count: Some(i as u64 + 1),
                kernel: Some("python3".into()),
            },
        )
        .unwrap();
    }
    let entry = b
        .finish(EngineEntryInput {
            can_freeze: true,
            owned_languages: vec!["python".into()],
            ..Default::default()
        })
        .unwrap();
    let record = coalesce(&[entry], &units).unwrap();
    let json = write_record(&record).unwrap();
    std::fs::write(var("Q2_DEMO_OUT"), &json).unwrap();
    println!("wrote {} bytes, {} cells", json.len(), record.cells.len());
}

#[test]
#[ignore = "spike S6 harness; needs Q2_DEMO_QMD and Q2_DEMO_CACHE"]
fn reports_a_positron_cache() {
    let text = std::fs::read_to_string(var("Q2_DEMO_QMD")).unwrap();
    let (units, texts) = staged(&text);
    let cache = std::fs::read(var("Q2_DEMO_CACHE")).unwrap();
    let import = import_positron_cache(&cache).expect("the cache file imports");
    println!("hashes in the cache file: {}", import.len());
    for n in &import.notices {
        println!("notice: {n}");
    }
    let mut seen: std::collections::BTreeMap<String, usize> = Default::default();
    for (u, a) in units.iter().zip(&texts.0) {
        let occ = seen.entry(a.content_hash.clone()).or_default();
        let lang = u.lang();
        let found = import.outputs_for(&a.content_hash, *occ, JUPYTER_ENGINE, lang);
        *occ += 1;
        let what = match found {
            None => "no match".to_string(),
            Some(o) => o
                .outputs()
                .iter()
                .map(|v| {
                    let mimes: Vec<&str> = v
                        .get("data")
                        .and_then(|d| d.as_object())
                        .map(|d| d.keys().map(String::as_str).collect())
                        .unwrap_or_default();
                    format!("{}{:?}", v["output_type"].as_str().unwrap_or("?"), mimes)
                })
                .collect::<Vec<_>>()
                .join(", "),
        };
        let kind = if matches!(u, Unit::Inline(_)) {
            "span"
        } else {
            "cell"
        };
        println!("{kind} {} -> {what}", a.content_hash);
    }
}
