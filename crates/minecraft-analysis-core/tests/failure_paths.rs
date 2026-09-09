use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use minecraft_analysis_core::convert::{self, TransformObject};
use minecraft_analysis_core::nbt::Value;
use minecraft_analysis_core::region::RegionReader;
use minecraft_analysis_core::registry::{
    Provenance, RegistryCatalog, RegistryEntry, RegistryKind, RegistryName,
};
use minecraft_analysis_core::rules::{
    self, Decision, NbtPatch, NbtPath, NestedLimits, NumericType, ObjectAction, PathElement,
};
use minecraft_analysis_core::{staging, world};

#[test]
fn prepublication_failures_are_explicit_and_non_destructive() {
    assert!(matches!(
        convert::apply_decision(
            &Decision {
                actions: vec![],
                trace: vec![]
            },
            TransformObject {
                identity: "mod:missing".into(),
                numeric: 0,
                nbt: None
            },
            |_| false,
        ),
        Err(convert::Error::Unresolved { .. })
    ));
    let entry = |name: &str| RegistryEntry {
        kind: RegistryKind::Block,
        name: RegistryName::parse(name).unwrap(),
        numeric_id: 20,
        provenance: Provenance::world("test", "fixture"),
    };
    let mut catalog = RegistryCatalog::default();
    catalog.insert(entry("mod:first")).unwrap();
    assert!(catalog.insert(entry("mod:second")).is_err());

    let mut nbt = Value::Compound(BTreeMap::from([("value".into(), Value::Int(128))]));
    assert!(rules::apply_patches(
        &mut nbt,
        &[NbtPatch::ConvertNumber {
            path: NbtPath(vec![PathElement::Field("value".into())]),
            to: NumericType::Byte,
        }],
    )
    .is_err());
    assert_eq!(
        nbt,
        Value::Compound(BTreeMap::from([("value".into(), Value::Int(128))]))
    );

    let recursive = Decision {
        actions: vec![(
            "recursive".into(),
            ObjectAction::Transform {
                target: None,
                numeric: None,
                patches: vec![],
                nested_items: vec![NbtPath(vec![PathElement::Field("Item".into())])],
            },
        )],
        trace: vec![],
    };
    let mut nested = Value::Compound(BTreeMap::from([(
        "Item".into(),
        Value::Compound(BTreeMap::from([(
            "Item".into(),
            Value::Compound(BTreeMap::new()),
        )])),
    )]));
    assert!(matches!(
        rules::process_declared_nested_items(
            &mut nested,
            &recursive,
            NestedLimits {
                max_depth: 8,
                max_objects: 8,
            },
            |_| recursive.clone(),
        ),
        Err(rules::Error::NestedCycle { .. })
    ));

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
fn conflicting_terminal_rules_are_rejected_during_loading() {
    let root = tempfile::tempdir().unwrap();
    let rules = root.path().join("rules.json");
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
        matches!(error, rules::Error::AmbiguousRules { .. }),
        "unexpected loader error: {error:?}"
    );
}
