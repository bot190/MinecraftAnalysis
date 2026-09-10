use std::fs;
use std::path::Path;

use minecraft_analysis_core::region::RegionReader;
use minecraft_analysis_core::registry::{
    Provenance, RegistryCatalog, RegistryEntry, RegistryKind, RegistryName,
};
use minecraft_analysis_core::rules;
use minecraft_analysis_core::{staging, world};

#[test]
fn prepublication_failures_are_explicit_and_non_destructive() {
    let entry = |name: &str| RegistryEntry {
        kind: RegistryKind::Block,
        name: RegistryName::parse(name).unwrap(),
        numeric_id: 20,
        provenance: Provenance::world("test", "fixture"),
    };
    let mut catalog = RegistryCatalog::default();
    catalog.insert(entry("mod:first")).unwrap();
    assert!(catalog.insert(entry("mod:second")).is_err());

    assert!(RegionReader::new(b"corrupt", 1024).is_err());

    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let template = root.path().join("template");
    fs::create_dir(&source).unwrap();
    fs::create_dir(&template).unwrap();
    assert!(world::validate_paths(&source, &template, &source.join("output")).is_err());

    let output = root.path().join("output");
    let staged = staging::staging_path(&output).unwrap();
    fs::create_dir(&staged).unwrap();
    fs::write(staged.join("unknown"), b"preserve").unwrap();
    let diagnostic = staging::diagnose_interrupted(&output).unwrap().unwrap();
    assert!(diagnostic.diagnostic.contains("explicitly"));
    assert_eq!(fs::read(staged.join("unknown")).unwrap(), b"preserve");
}

#[test]
fn removed_action_fields_are_rejected_during_loading() {
    let root = tempfile::tempdir().unwrap();
    let rules = root.path().join("rules.yaml");
    fs::write(
        &rules,
        r#"{
          "schema_version": 1,
          "rule_set": "conflict-test",
          "rules": [
            {"id":"one","priority":1,"terminal":true,"object":"block","matcher":{"name":"mod:block"},"action":{"action":"delete"}},
            {"id":"two","priority":1,"terminal":true,"object":"block","matcher":{"name":"mod:block"},"action":{"action":"replace_with_air"}}
          ]
        }"#,
    )
    .unwrap();
    let error = rules::load(Path::new(&rules)).unwrap_err();
    assert!(
        matches!(error, rules::Error::Yaml { .. }),
        "unexpected loader error: {error:?}"
    );
}
