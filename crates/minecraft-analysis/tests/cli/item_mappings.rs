use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use minecraft_analysis_core::nbt::{self, List, Tag, Value as NbtValue};
use minecraft_analysis_core::profile::WorldProfile;
use minecraft_analysis_core::registry::{RegistryCatalog, RegistryKind, VanillaVersion};
use minecraft_analysis_core::rules;
use serde_json::{json, Value};

fn command(source: &Path, target: &Path, rules: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"));
    command
        .args(["rules", "item-mappings", "--source-world"])
        .arg(source)
        .arg("--target-world")
        .arg(target)
        .arg("--rules")
        .arg(rules);
    command
}

fn fixture(root: &Path, legacy: bool) -> (PathBuf, PathBuf, PathBuf) {
    let source = root.join("source");
    let target = root.join("target");
    super::empty_profile_world(&source, false);
    super::empty_profile_world(&target, true);
    if legacy {
        super::declare_forge_1_2_5_level(&source);
    } else {
        write_source_stock_evidence(&source);
    }
    write_target_stock_alias(&target);
    // Valid profile evidence with deliberately unreadable region contents: this
    // command must only prepare catalogs, never inventory chunks or stacks.
    fs::write(source.join("region/r.0.0.mca"), b"not a region").unwrap();
    fs::write(target.join("region/r.0.0.mca"), b"not a region").unwrap();
    let path = root.join("rules.yaml");
    fs::write(&path, json!({
        "schema_version":1,"rule_set":"worksheet",
        "source_profile":if legacy {"forge-1.2.5"} else {"forge-1.7.10"},
        "source_manifest":[
            {"kind":"item","name":"source:explicit","numeric_id":14005},
            {"kind":"item","name":"source:also_explicit","numeric_id":14000},
            {"kind":"item","name":"source:invalid","numeric_id":14001},
            {"kind":"item","name":"legacy:item.WoodGear","numeric_id":14002},
            {"kind":"item","name":"source:wood_gear","numeric_id":14003},
            {"kind":"item","name":"source:orphan","numeric_id":14004},
            {"kind":"item","name":"source:stock_alias","numeric_id":14006},
            {"kind":"item","name":"source:stock_duplicate","numeric_id":14007},
            {"kind":"item","name":"legacy:item.Stone","numeric_id":14008}
        ],
        "target_manifest":[
            {"kind":"item","name":"target:wood_gear","numeric_id":15000},
            {"kind":"item","name":"target:plug_pulsar","numeric_id":15001},
            {"kind":"block","name":"target:ignored_block","numeric_id":3000}
        ],
        "rules":[
            {"id":"explicit","object":"item","matcher":{"name":"source:explicit"},"target_name":"target:wood_gear","template":"{{ must_not_execute() }}"},
            {"id":"also-explicit","object":"item","matcher":{"name":"source:also_explicit"},"target_name":"minecraft:stone","template":"{}"},
            {"id":"stock-alias","object":"item","matcher":{"name":"source:stock_alias"},"target_name":"minecraft:rock","template":"{}"},
            {"id":"stock-duplicate","object":"item","matcher":{"name":"source:stock_duplicate"},"target_name":"minecraft:stone","template":"{}"},
            {"id":"invalid","object":"item","matcher":{"name":"source:invalid"},"target_name":"minecraft:elytra","template":"{}"},
            {"id":"ignored-stock-source","object":"item","matcher":{"name":"minecraft:apple"},"target_name":"target:plug_pulsar","template":"{}"}
        ]
    }).to_string()).unwrap();
    (source, target, path)
}

fn write_source_stock_evidence(source: &Path) {
    let (mut level, _) = nbt::decode(&fs::read(source.join("level.dat")).unwrap()).unwrap();
    let NbtValue::Compound(fml) = level.root.get_mut("FML").unwrap() else {
        panic!("fixture FML must be a compound");
    };
    fml.insert(
        "ItemData".into(),
        NbtValue::List(List {
            element_tag: Tag::Compound,
            values: vec![NbtValue::Compound(BTreeMap::from([
                ("K".into(), NbtValue::String("\u{1}minecraft:apple".into())),
                ("V".into(), NbtValue::Int(260)),
            ]))],
        }),
    );
    fs::write(
        source.join("level.dat"),
        nbt::encode(&level, nbt::Compression::Gzip).unwrap(),
    )
    .unwrap();
}

fn write_target_stock_alias(target: &Path) {
    let (mut level, _) = nbt::decode(&fs::read(target.join("level.dat")).unwrap()).unwrap();
    let NbtValue::Compound(fml) = level.root.get_mut("FML").unwrap() else {
        panic!("fixture FML must be a compound");
    };
    let NbtValue::Compound(registries) = fml.get_mut("Registries").unwrap() else {
        panic!("fixture registries must be a compound");
    };
    registries.insert(
        "minecraft:items".into(),
        NbtValue::Compound(BTreeMap::from([(
            "aliases".into(),
            NbtValue::List(List {
                element_tag: Tag::Compound,
                values: vec![NbtValue::Compound(BTreeMap::from([
                    ("K".into(), NbtValue::String("minecraft:rock".into())),
                    ("V".into(), NbtValue::String("minecraft:stone".into())),
                ]))],
            }),
        )])),
    );
    fs::write(
        target.join("level.dat"),
        nbt::encode(&level, nbt::Compression::Gzip).unwrap(),
    )
    .unwrap();
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            result.extend(snapshot(&entry.path()));
        } else {
            result.insert(entry.path(), fs::read(entry.path()).unwrap());
        }
    }
    result
}

fn identity(entry: &Value) -> (String, i32) {
    (
        entry["name"].as_str().unwrap().to_owned(),
        i32::try_from(entry["numeric_id"].as_i64().unwrap()).unwrap(),
    )
}

fn expected_catalogs(path: &Path, legacy: bool) -> (RegistryCatalog, RegistryCatalog) {
    let mut source = if legacy {
        RegistryCatalog::forge_1_2_5()
    } else {
        RegistryCatalog::default()
    };
    if !legacy {
        source.add_vanilla_fallbacks(VanillaVersion::Minecraft1_7_10);
    }
    let mut target = RegistryCatalog::default();
    target.add_vanilla_fallbacks(VanillaVersion::Minecraft1_12_2);
    for (_, document) in rules::load(path).unwrap().documents {
        rules::apply_manifest(&mut source, &document.source_manifest, "test").unwrap();
        rules::apply_manifest(&mut target, &document.target_manifest, "test").unwrap();
    }
    (source, target)
}

fn catalog_identities(catalog: &RegistryCatalog) -> BTreeSet<(String, i32)> {
    catalog
        .entries()
        .filter(|entry| entry.kind == RegistryKind::Item)
        .map(|entry| (entry.name.to_string(), entry.numeric_id))
        .collect()
}

fn assert_accounting(
    report: &Value,
    source: &RegistryCatalog,
    target: &RegistryCatalog,
    source_profile: WorldProfile,
) {
    let source_items = report["source_items"].as_array().unwrap();
    let expected_source: BTreeSet<_> = source
        .entries()
        .filter(|entry| {
            entry.kind == RegistryKind::Item && !source_profile.is_stock_item(&entry.name)
        })
        .map(|entry| (entry.name.to_string(), entry.numeric_id))
        .collect();
    assert_eq!(
        source_items.iter().map(identity).collect::<BTreeSet<_>>(),
        expected_source
    );
    assert_eq!(source_items.len(), expected_source.len());
    assert!(source_items.windows(2).all(|entries| {
        let (a, aid) = identity(&entries[0]);
        let (b, bid) = identity(&entries[1]);
        (aid, a) < (bid, b)
    }));
    let mut referenced = BTreeSet::new();
    let mut counts = [0; 3];
    for source in source_items {
        let mapping = &source["mapping"];
        match mapping["kind"].as_str().unwrap() {
            "explicit" => {
                counts[0] += 1;
                if !mapping["target"]["numeric_id"].is_null() {
                    referenced.insert(identity(&mapping["target"]));
                }
            }
            "prospective" => {
                counts[1] += 1;
                let candidates = mapping["candidates"].as_array().unwrap();
                assert!(!candidates.is_empty());
                referenced.extend(candidates.iter().map(identity));
            }
            "none" => counts[2] += 1,
            other => panic!("unexpected mapping kind {other}"),
        }
    }
    let unmatched = report["unmatched_target_items"].as_array().unwrap();
    assert!(unmatched
        .windows(2)
        .all(|entries| identity(&entries[0]) < identity(&entries[1])));
    let unmatched_set: BTreeSet<_> = unmatched.iter().map(identity).collect();
    assert_eq!(unmatched.len(), unmatched_set.len());
    assert!(referenced.is_disjoint(&unmatched_set));
    let target_identities = catalog_identities(target);
    let non_stock_targets: BTreeSet<_> = target
        .entries()
        .filter(|entry| {
            entry.kind == RegistryKind::Item
                && !WorldProfile::Forge1_12_2.is_stock_item(&entry.name)
        })
        .map(|entry| (entry.name.to_string(), entry.numeric_id))
        .collect();
    let referenced_stock: BTreeSet<_> = referenced
        .iter()
        .filter(|identity| !non_stock_targets.contains(*identity))
        .cloned()
        .collect();
    let worksheet_targets: BTreeSet<_> = non_stock_targets
        .union(&referenced_stock)
        .cloned()
        .collect();
    assert!(referenced.is_subset(&target_identities));
    assert!(unmatched_set.is_subset(&non_stock_targets));
    assert_eq!(
        referenced
            .union(&unmatched_set)
            .cloned()
            .collect::<BTreeSet<_>>(),
        worksheet_targets
    );
    assert_eq!(
        report["summary"],
        json!({
            "source_items":source_items.len(),"explicitly_mapped_source_items":counts[0],
            "source_items_with_suggestions":counts[1],"source_items_without_suggestions":counts[2],
            "target_items":worksheet_targets.len(),"target_items_referenced":referenced.len(),
            "unmatched_target_items":unmatched.len()
        })
    );
}

#[test]
fn item_mappings_stock_only_catalogs_have_empty_worksheets() {
    for legacy in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let target = root.path().join("target");
        super::empty_profile_world(&source, false);
        super::empty_profile_world(&target, true);
        if legacy {
            super::declare_forge_1_2_5_level(&source);
        }
        let rules = root.path().join("rules.yaml");
        fs::write(
            &rules,
            json!({
                "schema_version":1,"rule_set":"stock-only",
                "source_profile":if legacy {"forge-1.2.5"} else {"forge-1.7.10"},
                "rules":[
                    {"id":"ignored-stock-source","object":"item","matcher":{"name":"minecraft:apple"},"target_name":"minecraft:stone","template":"{}"}
                ]
            })
            .to_string(),
        )
        .unwrap();
        let result = command(&source, &target, &rules).output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let report: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(report["source_items"], json!([]));
        assert_eq!(report["unmatched_target_items"], json!([]));
        assert_eq!(
            report["summary"],
            json!({
                "source_items":0,"explicitly_mapped_source_items":0,
                "source_items_with_suggestions":0,"source_items_without_suggestions":0,
                "target_items":0,"target_items_referenced":0,"unmatched_target_items":0
            })
        );
    }
}

fn assert_complete_report(report: &Value, rules: &Path, legacy: bool) {
    assert_eq!(report["report_schema"], 1);
    assert_eq!(
        report["source_profile"],
        if legacy {
            "forge-1.2.5"
        } else {
            "forge-1.7.10"
        }
    );
    assert_eq!(report["target_profile"], "forge-1.12.2");
    assert_eq!(report["rule_sets"], json!(["worksheet"]));
    let (expected_source, expected_target) = expected_catalogs(rules, legacy);
    assert_accounting(
        report,
        &expected_source,
        &expected_target,
        if legacy {
            WorldProfile::Forge1_2_5
        } else {
            WorldProfile::Forge1_7_10
        },
    );
    let sources = report["source_items"].as_array().unwrap();
    let mapping = |name| &sources.iter().find(|entry| entry["name"] == name).unwrap()["mapping"];
    assert!(sources
        .iter()
        .all(|entry| entry["name"] != "minecraft:apple"));
    assert_eq!(mapping("source:explicit")["kind"], "explicit");
    assert_eq!(
        mapping("source:also_explicit")["target"],
        json!({"name":"minecraft:stone","numeric_id":1})
    );
    assert_eq!(
        mapping("source:stock_alias"),
        &json!({
            "kind":"explicit","rule_id":"stock-alias","target_name":"minecraft:rock",
            "target":{"name":"minecraft:stone","numeric_id":1}
        })
    );
    assert_eq!(
        mapping("source:stock_duplicate")["target"],
        mapping("source:also_explicit")["target"]
    );
    assert_eq!(mapping("source:orphan"), &json!({"kind":"none"}));
    assert_eq!(mapping("legacy:item.Stone"), &json!({"kind":"none"}));
    assert_eq!(mapping("source:invalid")["diagnostic"], "invalid-target");
    assert_eq!(mapping("source:invalid")["target_name"], "minecraft:elytra");
    assert!(mapping("source:invalid")["target"]["numeric_id"].is_null());
    for name in ["legacy:item.WoodGear", "source:wood_gear"] {
        assert_eq!(mapping(name)["kind"], "prospective");
        assert_eq!(mapping(name)["candidates"][0]["name"], "target:wood_gear");
        assert!(mapping(name)["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|candidate| !candidate["name"]
                .as_str()
                .unwrap()
                .starts_with("minecraft:")));
    }
    assert_eq!(
        mapping("legacy:item.WoodGear")["candidates"][0]["score"],
        1320
    );
    assert!(report["unmatched_target_items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["name"] == "target:plug_pulsar"));
    assert_eq!(report["summary"]["source_items"], 9);
    assert_eq!(report["summary"]["explicitly_mapped_source_items"], 5);
    assert_eq!(report["summary"]["source_items_with_suggestions"], 2);
    assert_eq!(report["summary"]["source_items_without_suggestions"], 2);
    assert_eq!(report["summary"]["target_items"], 3);
    assert_eq!(report["summary"]["target_items_referenced"], 2);
    assert_eq!(report["summary"]["unmatched_target_items"], 1);
}

#[test]
fn item_mappings_complete_deterministic_read_only_worksheet() {
    for legacy in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (source, target, rules) = fixture(root.path(), legacy);
        let before = snapshot(root.path());
        let first = command(&source, &target, &rules).output().unwrap();
        assert!(
            first.status.success(),
            "{}",
            String::from_utf8_lossy(&first.stderr)
        );
        assert_complete_report(
            &serde_json::from_slice(&first.stdout).unwrap(),
            &rules,
            legacy,
        );
        let second = command(&source, &target, &rules).output().unwrap();
        assert!(second.status.success());
        assert_eq!(second.stdout, first.stdout);
        assert_eq!(snapshot(root.path()), before);
        let output_path = root.path().join("report.json");
        fs::write(&output_path, "old report").unwrap();
        let file = command(&source, &target, &rules)
            .arg("--report")
            .arg(&output_path)
            .output()
            .unwrap();
        assert!(
            file.status.success(),
            "{}",
            String::from_utf8_lossy(&file.stderr)
        );
        assert!(file.stdout.is_empty());
        assert_eq!(fs::read(&output_path).unwrap(), first.stdout);
        fs::remove_file(output_path).unwrap();
        assert_eq!(snapshot(root.path()), before);
    }
}

#[test]
fn item_mappings_help_required_arguments_and_unknown_options() {
    let help = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .args(["rules", "item-mappings", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for flag in [
        "--rules <FILE>",
        "--source-world <WORLD>",
        "--target-world <WORLD>",
        "--report <FILE>",
    ] {
        assert!(help.contains(flag), "{help}");
    }
    let required = [
        ("--rules", "r"),
        ("--source-world", "s"),
        ("--target-world", "t"),
    ];
    for missing in 0..required.len() {
        let mut command = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"));
        command.args(["rules", "item-mappings"]);
        for (index, &(flag, value)) in required.iter().enumerate() {
            if index != missing {
                command.args([flag, value]);
            }
        }
        let result = command.output().unwrap();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains(required[missing].0));
    }
    let result = command(Path::new("s"), Path::new("t"), Path::new("r"))
        .arg("--unknown")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("--unknown"));
}

#[test]
fn item_mappings_loads_repeated_rules_imports_and_conflict_selections() {
    let root = tempfile::tempdir().unwrap();
    let (source, target, rules) = fixture(root.path(), false);
    let extra = root.path().join("extra.yaml");
    fs::write(&extra, json!({
        "schema_version":1,"rule_set":"additional","imports":["rules.yaml"],
        "target_manifest":[{"kind":"item","name":"replacement:item","numeric_id":15001,"conflict":"manifest"}]
    }).to_string()).unwrap();
    let result = command(&source, &target, &rules)
        .arg("--rules")
        .arg(&extra)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["rule_sets"], json!(["additional", "worksheet"]));
    let target_names: Vec<_> = report["unmatched_target_items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["name"].as_str().unwrap())
        .collect();
    assert!(target_names.contains(&"replacement:item"));
    assert!(!target_names.contains(&"target:plug_pulsar"));
}

#[test]
fn item_mappings_invalid_inputs_never_publish_partial_output() {
    for failure in ["profile", "target-profile", "rules", "manifest", "registry"] {
        let root = tempfile::tempdir().unwrap();
        let (source, target, rules) = fixture(root.path(), false);
        match failure {
            "profile" => super::empty_profile_world(&source, true),
            "target-profile" => super::empty_profile_world(&target, false),
            "rules" => fs::write(&rules, "not a valid document").unwrap(),
            "manifest" => {
                let mut document: Value =
                    serde_json::from_slice(&fs::read(&rules).unwrap()).unwrap();
                document["source_manifest"][0]["numeric_id"] = json!(14000);
                fs::write(&rules, document.to_string()).unwrap();
            }
            "registry" => fs::write(target.join("level.dat"), b"invalid NBT").unwrap(),
            _ => unreachable!(),
        }
        let before = snapshot(root.path());
        let result = command(&source, &target, &rules).output().unwrap();
        assert!(!result.status.success(), "{failure}");
        assert!(result.stdout.is_empty(), "{failure}");
        assert!(!result.stderr.is_empty());
        assert_eq!(snapshot(root.path()), before);
        let output = root.path().join("report.json");
        let absent = command(&source, &target, &rules)
            .arg("--report")
            .arg(&output)
            .output()
            .unwrap();
        assert!(!absent.status.success());
        assert!(!output.exists());
        fs::write(&output, "previous report").unwrap();
        let existing = command(&source, &target, &rules)
            .arg("--report")
            .arg(&output)
            .output()
            .unwrap();
        assert!(!existing.status.success());
        assert!(existing.stdout.is_empty());
        assert_eq!(fs::read_to_string(output).unwrap(), "previous report");
    }
}

#[test]
fn item_mappings_report_cannot_replace_inputs() {
    let root = tempfile::tempdir().unwrap();
    let (source, target, rules) = fixture(root.path(), false);
    let imported = root.path().join("imported.yaml");
    fs::write(
        &imported,
        "schema_version: 1\nrule_set: imported\nimports: [rules.yaml]\n",
    )
    .unwrap();
    let before = snapshot(root.path());
    for output in [
        source.join("level.dat"),
        target.join("new.json"),
        rules.clone(),
        imported.clone(),
    ] {
        let result = command(&source, &target, &imported)
            .arg("--report")
            .arg(output)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("must be outside"));
        assert_eq!(snapshot(root.path()), before);
    }
    #[cfg(unix)]
    {
        let link = root.path().join("report-link");
        std::os::unix::fs::symlink(&rules, &link).unwrap();
        let result = command(&source, &target, &rules)
            .arg("--report")
            .arg(&link)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        assert_eq!(fs::read(&rules).unwrap(), before[&rules]);
    }
}
