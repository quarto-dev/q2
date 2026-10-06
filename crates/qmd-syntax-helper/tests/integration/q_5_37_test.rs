use qmd_syntax_helper::rule::RuleRegistry;
use qmd_syntax_helper::utils::resources::ResourceManager;
use std::fs;
use std::path::PathBuf;

const CONFIG: &str = "project:\n  type: website\n";

/// A project with `_quarto.yml` = `config`, the given brand files, and a
/// `doc.qmd` in a subdirectory (rules are handed .qmd files).
fn project(config: &str, brand_files: &[&str]) -> (ResourceManager, PathBuf, PathBuf) {
    let rm = ResourceManager::new().unwrap();
    let root = rm.temp_dir().to_path_buf();
    fs::write(root.join("_quarto.yml"), config).unwrap();
    for rel in brand_files {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "color:\n  primary: red\n").unwrap();
    }
    fs::create_dir_all(root.join("sub")).unwrap();
    let doc = root.join("sub/doc.qmd");
    fs::write(&doc, "# Hi\n").unwrap();
    (rm, root, doc)
}

fn rule() -> std::sync::Arc<dyn qmd_syntax_helper::rule::Rule + Send + Sync> {
    RuleRegistry::new().unwrap().get("q-5-37").unwrap()
}

#[test]
fn detects_unreferenced_brand_file_from_a_nested_document() {
    let (_rm, _root, doc) = project(CONFIG, &["_brand.yml"]);
    let results = rule().check(&doc, false).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].error_code.as_deref(), Some("Q-5-37"));
}

#[test]
fn no_issue_without_a_brand_file_or_when_declared() {
    let (_a, _r, doc) = project(CONFIG, &[]);
    assert!(rule().check(&doc, false).unwrap().is_empty());

    let (_b, _r, doc) = project(&format!("{CONFIG}brand: _brand.yml\n"), &["_brand.yml"]);
    assert!(rule().check(&doc, false).unwrap().is_empty());

    let (_c, _r, doc) = project(
        &format!("{CONFIG}format:\n  html:\n    brand: _brand.yml\n"),
        &["_brand.yml"],
    );
    assert!(rule().check(&doc, false).unwrap().is_empty());
}

#[test]
fn no_issue_when_metadata_yml_or_front_matter_declares_brand() {
    let (_a, root, doc) = project(CONFIG, &["_brand.yml"]);
    fs::write(root.join("sub/_metadata.yml"), "brand: _brand.yml\n").unwrap();
    assert!(rule().check(&doc, false).unwrap().is_empty());

    let (_b, _r, doc) = project(CONFIG, &["_brand.yml"]);
    fs::write(&doc, "---\ntitle: T\nbrand: _brand.yml\n---\n\nHi\n").unwrap();
    assert!(rule().check(&doc, false).unwrap().is_empty());
}

#[test]
fn no_issue_outside_a_project() {
    let rm = ResourceManager::new().unwrap();
    let doc = rm.temp_dir().join("doc.qmd");
    fs::write(&doc, "# Hi\n").unwrap();
    fs::write(rm.temp_dir().join("_brand.yml"), "color:\n").unwrap();
    assert!(rule().check(&doc, false).unwrap().is_empty());
}

#[test]
fn convert_appends_the_key_and_is_idempotent() {
    let (_rm, root, doc) = project(CONFIG, &["_brand.yml"]);
    let result = rule().convert(&doc, true, false, false).unwrap();
    assert_eq!(result.fixes_applied, 1);
    assert_eq!(
        fs::read_to_string(root.join("_quarto.yml")).unwrap(),
        format!("{CONFIG}\n# Added by qmd-syntax-helper (Q-5-37)\nbrand: _brand.yml\n")
    );
    assert!(rule().check(&doc, false).unwrap().is_empty());
    assert_eq!(
        rule()
            .convert(&doc, true, false, false)
            .unwrap()
            .fixes_applied,
        0
    );
}

#[test]
fn convert_adds_a_missing_trailing_newline() {
    let (_rm, root, doc) = project("project:\n  type: website", &["_brand.yml"]);
    rule().convert(&doc, true, false, false).unwrap();
    let text = fs::read_to_string(root.join("_quarto.yml")).unwrap();
    assert!(
        text.starts_with("project:\n  type: website\n\n# Added"),
        "{text}"
    );
}

#[test]
fn picks_the_last_existing_candidate_like_quarto_1() {
    let (_rm, root, doc) = project(CONFIG, &["_brand.yml", "_brand/_brand.yml"]);
    rule().convert(&doc, true, false, false).unwrap();
    let text = fs::read_to_string(root.join("_quarto.yml")).unwrap();
    assert!(text.ends_with("brand: _brand/_brand.yml\n"), "{text}");
}

#[test]
fn check_mode_and_dry_run_do_not_write() {
    let (_rm, root, doc) = project(CONFIG, &["_brand.yml"]);
    let checked = rule().convert(&doc, false, true, false).unwrap();
    assert_eq!(checked.fixes_applied, 1);
    let dry = rule().convert(&doc, false, false, false).unwrap();
    assert!(dry.message.unwrap().ends_with("brand: _brand.yml\n"));
    assert_eq!(
        fs::read_to_string(root.join("_quarto.yml")).unwrap(),
        CONFIG
    );
}

#[test]
fn is_opt_in_only() {
    let registry = RuleRegistry::new().unwrap();
    assert!(
        registry
            .all_auto_convertible()
            .iter()
            .all(|r| r.name() != "q-5-37")
    );
    assert!(registry.all().iter().any(|r| r.name() == "q-5-37"));
}
