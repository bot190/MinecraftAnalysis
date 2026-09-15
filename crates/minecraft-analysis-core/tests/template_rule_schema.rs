use std::{collections::BTreeMap, fs};

use minecraft_analysis_core::rules::{self, RuleBody, RuleDocument};
use minecraft_analysis_core::{
    nbt::{List, Tag, Value},
    registry::{Provenance, RegistryCatalog, RegistryEntry, RegistryKind, RegistryName},
    template::{EntityContext, EntityOriginal, EntityResult},
};

fn document(rule: &str) -> String {
    format!(
        r#"{{
  "schema_version": 1,
  "rule_set": "test",
  "source_profile": "forge-1.7.10",
  "rules": [{rule}]
}}"#
    )
}

#[test]
fn checked_in_example_rule_graph_loads() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/rules/example.yaml");
    rules::load(&path).unwrap();
}

#[test]
fn template_schema_accepts_each_rule_kind_and_item_projection() {
    let yaml = r#"{
      "schema_version": 1,
      "rule_set": "new-schema",
      "source_profile": "forge-1.7.10",
      "value_maps": [{"id":"map","entries":[{"from":{"type":"byte","value":1},"to":{"type":"int","value":2}}]}],
      "rules": [
        {"id":"b","object":"block","matcher":{"name":"mod:block"},"template":"{\"disposition\":\"unchanged\"}"},
        {"id":"i","object":"item","matcher":{"name":"mod:item"},"target_name":"mod:new_item","template":"{\"disposition\":\"unchanged\"}"},
        {"id":"e","object":"entity","matcher":{"name":"mod:entity"},"template":"{\"disposition\":\"unchanged\"}"}
      ]
    }"#;
    let parsed: RuleDocument = serde_yaml::from_str(yaml).unwrap();
    assert!(matches!(
        parsed.rules[1].body,
        RuleBody::Item {
            target_name: Some(_),
            ..
        }
    ));
}

#[test]
fn yaml_literal_template_preserves_lines_and_serializes_readably() {
    let yaml = r#"schema_version: 1
rule_set: multiline
source_profile: forge-1.7.10
rules:
  - id: e
    object: entity
    matcher: { name: mod:entity }
    template: |
      {
        "disposition": "unchanged"
      }
"#;
    let parsed: RuleDocument = serde_yaml::from_str(yaml).unwrap();
    let RuleBody::Entity { template, .. } = &parsed.rules[0].body else {
        panic!()
    };
    assert_eq!(template, "{\n  \"disposition\": \"unchanged\"\n}\n");

    let serialized = serde_yaml::to_string(&parsed).unwrap();
    assert!(serialized.contains("template: |"), "{serialized}");
}

#[test]
fn json_rule_documents_and_non_string_templates_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let json_path = directory.path().join("rules.json");
    fs::write(
        &json_path,
        r#"{"schema_version":1,"rule_set":"json","source_profile":"forge-1.7.10"}"#,
    )
    .unwrap();
    let error = rules::load(&json_path).unwrap_err().to_string();
    assert!(error.contains("unsupported"), "{error}");
    assert!(error.contains("YAML"), "{error}");

    let yaml_path = directory.path().join("rules.yaml");
    fs::write(
        &yaml_path,
        "schema_version: 1\nrule_set: invalid-template\nsource_profile: forge-1.7.10\nrules:\n  - id: e\n    object: entity\n    matcher: { name: mod:entity }\n    template: [not, a, string]\n",
    )
    .unwrap();
    assert!(matches!(
        rules::load(&yaml_path),
        Err(rules::Error::Yaml { .. })
    ));
}

#[test]
fn removed_action_schema_and_fields_are_rejected() {
    let base = r#"{"id":"r","object":"block","matcher":{"name":"mod:block"},"template":"{\"disposition\":\"unchanged\"}"}"#;
    for removed in [
        r#"{"id":"r","object":"block","matcher":{"name":"mod:block"},"action":{"action":"delete"}}"#,
        r#"{"id":"r","object":"block_entity","matcher":{"name":"mod:tile"},"template":"{}"}"#,
        r#"{"id":"r","terminal":true,"object":"block","matcher":{"name":"mod:block"},"template":"{}"}"#,
    ] {
        assert!(serde_yaml::from_str::<RuleDocument>(&document(removed)).is_err());
    }
    for field in ["nested_items", "standalone_inventories", "patches"] {
        let injected =
            document(base).replace("\"rules\":", &format!("\"{field}\": [], \"rules\":"));
        assert!(
            serde_yaml::from_str::<RuleDocument>(&injected).is_err(),
            "{field}"
        );
    }
    let old_version = document(base).replace("\"schema_version\": 1", "\"schema_version\": 3");
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rules.yaml");
    fs::write(&path, old_version).unwrap();
    assert!(matches!(
        rules::load(&path),
        Err(rules::Error::Schema { actual: 3 })
    ));
}

#[test]
fn templates_compile_during_graph_loading_with_document_and_rule_context() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rules.yaml");
    fs::write(
        &path,
        document(
            r#"{"id":"broken","object":"entity","matcher":{"name":"mod:e"},"template":"{% if"}"#,
        ),
    )
    .unwrap();
    let error = rules::load(&path).unwrap_err().to_string();
    assert!(error.contains(&path.to_string_lossy().to_string()));
    assert!(error.contains("broken"));
}

#[test]
fn identity_indices_limit_candidates_and_first_match_uses_stable_precedence() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rules.yaml");
    fs::write(&path, r#"{
      "schema_version":1,"rule_set":"indexed","source_profile":"forge-1.7.10",
      "rules":[
        {"id":"unrelated","priority":100,"object":"block","matcher":{"name":"other:block","nbt":[{"predicate":"exists","path":["impossible"]}]},"template":"{\"disposition\":\"unchanged\"}"},
        {"id":"first","priority":10,"object":"block","matcher":{"name":"mod:block"},"template":"{\"disposition\":\"unchanged\"}"},
        {"id":"later","priority":10,"object":"block","matcher":{"name":"mod:block"},"template":"{\"disposition\":\"replace_with_air\"}"}
      ]
    }"#).unwrap();
    let loaded = rules::load(&path).unwrap();
    assert_eq!(loaded.indices.block_names["mod:block"].len(), 2);
    let decision = rules::evaluate_block(
        &loaded,
        &RegistryKind::Block,
        &RegistryName::parse("mod:block").unwrap(),
        200,
        0,
        Some(&Value::Compound(BTreeMap::default())),
    );
    assert_eq!(decision.selected_rule.as_deref(), Some("first"));
    assert_eq!(decision.candidates.len(), 1);
    assert!(decision
        .candidates
        .iter()
        .all(|entry| entry.rule_id != "unrelated"));
}

fn catalog(entries: &[(&str, i32)]) -> RegistryCatalog {
    let mut catalog = RegistryCatalog::default();
    for (name, id) in entries {
        catalog
            .insert(RegistryEntry {
                kind: RegistryKind::Item,
                name: RegistryName::parse(name).unwrap(),
                numeric_id: *id,
                provenance: Provenance::world("test", "fixture"),
            })
            .unwrap();
    }
    catalog
}

#[test]
fn recursive_item_functions_filter_drops_and_map_identity_without_rendering() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rules.yaml");
    let entity_template = r#"{"disposition":"transform","entity":{"name":"mod:entity","nbt":{"type":"compound","value":{"Items":{"type":"list","value":{"element_type":"compound","values":{{ transform_items(original.nbt.value.Items.value.values)|tojson }}}},"Mapped":{"type":"string","value":{{ map_item_id(5)|tojson }}}}}}}"#;
    let item_template = r#"{"disposition":"transform","item":{"name":"target:item","count":1,"damage":2,"nbt":{{ original.nbt|tojson }}}}"#;
    fs::write(&path, serde_json::json!({
        "schema_version": 1, "rule_set": "recursive", "source_profile": "forge-1.7.10",
        "rules": [
            {"id":"map-item","object":"item","matcher":{"name":"source:item"},"target_name":"target:item","template":item_template},
            {"id":"drop-item","priority":20,"object":"item","matcher":{"name":"source:drop"},"template":"{\"disposition\":\"drop\"}"},
            {"id":"entity","object":"entity","matcher":{"name":"mod:entity"},"template":entity_template}
        ]
    }).to_string()).unwrap();
    let loaded = rules::load(&path).unwrap();
    let source = catalog(&[("source:item", 5), ("source:drop", 6)]);
    let target = catalog(&[("target:item", 15)]);
    let stack = |id| {
        Value::Compound(std::collections::BTreeMap::from([
            ("id".into(), Value::Short(id)),
            ("Count".into(), Value::Byte(1)),
            ("Damage".into(), Value::Short(0)),
        ]))
    };
    let original = Value::Compound(std::collections::BTreeMap::from([
        ("id".into(), Value::String("mod:entity".into())),
        (
            "Items".into(),
            Value::List(List {
                element_tag: Tag::Compound,
                values: vec![stack(5), stack(6)],
            }),
        ),
    ]));
    let decision = rules::evaluate_entity_for_execution(&loaded, "mod:entity", &original);
    let session = rules::template_session(
        &loaded,
        &source,
        &target,
        rules::NestedLimits {
            max_depth: 8,
            max_objects: 8,
        },
    );
    let result = rules::render_entity(
        &loaded,
        &decision,
        EntityContext {
            original: EntityOriginal {
                name: "mod:entity".into(),
                nbt: rules::typed_nbt(&original),
            },
        },
        session.callbacks(),
    )
    .unwrap()
    .unwrap();
    let EntityResult::Transform { entity } = result else {
        panic!("expected transform")
    };
    let Value::Compound(output) = Value::from(entity.nbt.clone()) else {
        panic!()
    };
    let Value::List(items) = &output["Items"] else {
        panic!()
    };
    assert_eq!(items.values.len(), 1);
    assert_eq!(output["Mapped"], Value::String("target:item".into()));
    let (nested, maps, _) = session.outcomes();
    assert_eq!(nested.len(), 2);
    assert!(nested[1].dropped);
    assert_eq!(maps[0].target, "target:item");

    let repeat = rules::render_entity(
        &loaded,
        &decision,
        EntityContext {
            original: EntityOriginal {
                name: "mod:entity".into(),
                nbt: rules::typed_nbt(&original),
            },
        },
        rules::template_session(
            &loaded,
            &source,
            &target,
            rules::NestedLimits {
                max_depth: 8,
                max_objects: 8,
            },
        )
        .callbacks(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(repeat, EntityResult::Transform { entity });
}

#[test]
fn recursive_item_calls_report_cycles_and_shared_object_limits() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rules.yaml");
    fs::write(
        &path,
        serde_json::json!({
            "schema_version": 1, "rule_set": "limits", "source_profile": "forge-1.7.10",
            "rules": [
                {"id":"recursive","object":"item","matcher":{"name":"source:item"},
                 "template":"{{ transform_item(original.nbt)|tojson }}"},
                {"id":"entity","object":"entity","matcher":{"name":"mod:entity"},
                 "template":"{{ transform_item(original.nbt.value.Item)|tojson }}"}
            ]
        })
        .to_string(),
    )
    .unwrap();
    let loaded = rules::load(&path).unwrap();
    let catalogs = catalog(&[("source:item", 5)]);
    let stack = Value::Compound(std::collections::BTreeMap::from([
        ("id".into(), Value::Short(5)),
        ("Count".into(), Value::Byte(1)),
        ("Damage".into(), Value::Short(0)),
    ]));
    let original = Value::Compound(std::collections::BTreeMap::from([("Item".into(), stack)]));
    let decision = rules::evaluate_entity_for_execution(&loaded, "mod:entity", &original);
    let error = rules::render_entity(
        &loaded,
        &decision,
        EntityContext {
            original: EntityOriginal {
                name: "mod:entity".into(),
                nbt: rules::typed_nbt(&original),
            },
        },
        rules::template_session(
            &loaded,
            &catalogs,
            &catalogs,
            rules::NestedLimits {
                max_depth: 8,
                max_objects: 8,
            },
        )
        .callbacks(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("cycle"), "{error}");

    let error = rules::render_entity(
        &loaded,
        &decision,
        EntityContext {
            original: EntityOriginal {
                name: "mod:entity".into(),
                nbt: rules::typed_nbt(&original),
            },
        },
        rules::template_session(
            &loaded,
            &catalogs,
            &catalogs,
            rules::NestedLimits {
                max_depth: 8,
                max_objects: 0,
            },
        )
        .callbacks(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("object limit"), "{error}");
}

#[test]
fn identity_mapping_rejects_missing_and_stack_dependent_projections() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rules.yaml");
    fs::write(&path, serde_json::json!({
        "schema_version": 1, "rule_set": "identity-errors", "source_profile": "forge-1.7.10",
        "rules": [
            {"id":"dependent","priority":10,"object":"item","matcher":{"name":"source:item","damage":{"mode":"exact","value":1}},"target_name":"target:item","template":"{\"disposition\":\"drop\"}"},
            {"id":"entity","object":"entity","matcher":{"name":"mod:entity"},"template":"{\"disposition\":\"transform\",\"entity\":{\"name\":\"mod:entity\",\"nbt\":{\"type\":\"compound\",\"value\":{\"Mapped\":{\"type\":\"string\",\"value\":{{ map_item_id(5)|tojson }}}}}}}"}
        ]
    }).to_string()).unwrap();
    let loaded = rules::load(&path).unwrap();
    let source = catalog(&[("source:item", 5)]);
    let target = catalog(&[("target:item", 15)]);
    let original = Value::Compound(BTreeMap::default());
    let decision = rules::evaluate_entity_for_execution(&loaded, "mod:entity", &original);
    let error = rules::render_entity(
        &loaded,
        &decision,
        EntityContext {
            original: EntityOriginal {
                name: "mod:entity".into(),
                nbt: rules::typed_nbt(&original),
            },
        },
        rules::template_session(
            &loaded,
            &source,
            &target,
            rules::NestedLimits {
                max_depth: 8,
                max_objects: 8,
            },
        )
        .callbacks(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("complete stack evidence"), "{error}");
}
