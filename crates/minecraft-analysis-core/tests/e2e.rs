use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use minecraft_analysis_core::nbt::{self, Compression, Document, List, Tag, Value};
use minecraft_analysis_core::pipeline;
use minecraft_analysis_core::region::{
    BlockStorage, RegionReader, RegionWriter, DEFAULT_MAX_CHUNK_BYTES,
};
use minecraft_analysis_core::registry::{
    Provenance, RegistryCatalog, RegistryEntry, RegistryKind, RegistryName,
};
use minecraft_analysis_core::rules::{LoadedRules, SourceProfile};
use minecraft_analysis_core::world::SafePaths;
use sha2::{Digest, Sha256};

fn list(values: Vec<Value>) -> Value {
    Value::List(List {
        element_tag: Tag::Compound,
        values,
    })
}

#[test]
fn action_schema_is_rejected_without_compatibility_conversion() {
    let directory = tempfile::tempdir().unwrap();
    let maps = directory.path().join("maps.yaml");
    let rules = directory.path().join("rules.yaml");
    fs::write(
        &maps,
        r#"{
      "schema_version":3,"rule_set":"buildcraft-maps",
      "value_maps":[{"id":"buildcraft:legacy-orientation","coerce_numeric":true,"entries":[
        {"from":{"type":"int","value":0},"to":{"type":"string","value":"DOWN"}},
        {"from":{"type":"int","value":1},"to":{"type":"string","value":"UP"}},
        {"from":{"type":"int","value":2},"to":{"type":"string","value":"NORTH"}},
        {"from":{"type":"int","value":3},"to":{"type":"string","value":"SOUTH"}},
        {"from":{"type":"int","value":4},"to":{"type":"string","value":"WEST"}},
        {"from":{"type":"int","value":5},"to":{"type":"string","value":"EAST"}}
      ]}]
    }"#,
    )
    .unwrap();
    fs::write(
        &rules,
        r#"{
      "schema_version":3,"rule_set":"buildcraft-engines","source_profile":"forge-1.7.10",
      "imports":["maps.yaml"],"rules":[{
        "id":"buildcraft:engine-orientation","object":"block_entity",
        "matcher":{"name":"BuildCraft|Energy:Engine"},
        "action":{"action":"transform","patches":[{
          "patch":"map_value","from":["orientation"],"to":["currentDirection"],
          "using":"buildcraft:legacy-orientation","remove_source":true
        }]}
      }]
    }"#,
    )
    .unwrap();
    assert!(minecraft_analysis_core::rules::load(&rules).is_err());
}

fn compound(values: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Compound(
        values
            .into_iter()
            .map(|(key, value)| (key.into(), value))
            .collect(),
    )
}

fn catalog(entries: &[(RegistryKind, &str, i32)]) -> RegistryCatalog {
    let mut catalog = RegistryCatalog::default();
    for (kind, name, id) in entries {
        catalog
            .insert(RegistryEntry {
                kind: kind.clone(),
                name: RegistryName::parse(name).unwrap(),
                numeric_id: *id,
                provenance: Provenance::world("synthetic", "generated e2e fixture"),
            })
            .unwrap();
    }
    catalog
}

fn empty_rules() -> LoadedRules {
    LoadedRules::empty(SourceProfile::Forge1_7_10)
}

fn write_level(path: &Path, target: bool) {
    let fml = if target {
        compound([("Registries", Value::Compound(BTreeMap::new()))])
    } else {
        compound([("ItemData", list(vec![]))])
    };
    let version = if target { 1343 } else { 0 };
    let document = Document {
        root_name: String::new(),
        root: BTreeMap::from([
            ("FML".into(), fml),
            (
                "Data".into(),
                Value::Compound(BTreeMap::from([
                    ("DataVersion".into(), Value::Int(version)),
                    (
                        "RandomSeed".into(),
                        Value::Long(if target { 999 } else { 123 }),
                    ),
                    ("UnknownGameplay".into(), Value::ByteArray(vec![4, 5, 6])),
                ])),
            ),
        ]),
    };
    fs::write(
        path.join("level.dat"),
        nbt::encode(&document, Compression::Gzip).unwrap(),
    )
    .unwrap();
}

fn write_region(path: &Path, block_id: u16, include_tile: bool) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let low = i8::from_be_bytes([block_id.to_be_bytes()[1]]);
    let high = i8::from_be_bytes([block_id.to_be_bytes()[0] & 0x0f]);
    let mut blocks = vec![0_i8; 4096];
    blocks[0] = low;
    let mut add = vec![0_i8; 2048];
    add[0] = high;
    let section = compound([
        ("Y", Value::Byte(0)),
        ("Blocks", Value::ByteArray(blocks)),
        ("Data", Value::ByteArray(vec![0; 2048])),
        ("Add", Value::ByteArray(add)),
        ("BlockLight", Value::ByteArray(vec![7; 2048])),
        ("UnknownSection", Value::Long(77)),
    ]);
    let tile = compound([
        ("id", Value::String("mod:machine_tile".into())),
        ("x", Value::Int(0)),
        ("y", Value::Int(0)),
        ("z", Value::Int(0)),
        (
            "Capability",
            compound([("opaque", Value::IntArray(vec![1, 2, 3]))]),
        ),
        (
            "Items",
            list(vec![compound([
                ("id", Value::Short(500)),
                ("Count", Value::Byte(1)),
                ("Damage", Value::Short(0)),
            ])]),
        ),
    ]);
    let mut level = BTreeMap::from([
        ("xPos".into(), Value::Int(0)),
        ("zPos".into(), Value::Int(0)),
        ("Sections".into(), list(vec![section])),
        ("Entities".into(), list(vec![])),
        ("UnknownChunk".into(), Value::String("preserve".into())),
    ]);
    level.insert(
        "TileEntities".into(),
        list(if include_tile { vec![tile] } else { vec![] }),
    );
    let document = Document {
        root_name: String::new(),
        root: BTreeMap::from([("Level".into(), Value::Compound(level))]),
    };
    let mut region = RegionWriter::new().unwrap();
    region
        .write_chunk(0, 0, &nbt::encode_uncompressed(&document).unwrap(), 1234)
        .unwrap();
    fs::write(path, region.finish().unwrap()).unwrap();
}

fn write_player(world: &Path) {
    fs::create_dir_all(world.join("playerdata")).unwrap();
    let nested = compound([
        ("id", Value::Short(501)),
        ("Count", Value::Byte(2)),
        ("Damage", Value::Short(0)),
    ]);
    let backpack = compound([
        ("id", Value::Short(500)),
        ("Count", Value::Byte(1)),
        ("Damage", Value::Short(0)),
        (
            "ForgeCaps",
            compound([("opaque", Value::ByteArray(vec![9, 8, 7]))]),
        ),
        ("tag", compound([("CustomSlots", list(vec![nested]))])),
    ]);
    let document = Document {
        root_name: String::new(),
        root: BTreeMap::from([("Inventory".into(), list(vec![backpack]))]),
    };
    fs::write(
        world.join("playerdata/player.dat"),
        nbt::encode(&document, Compression::Gzip).unwrap(),
    )
    .unwrap();
}

fn write_opaque_cache(world: &Path) {
    fs::create_dir_all(world.join("AE2/compass")).unwrap();
    fs::write(world.join("AE2/compass/-1858.dat"), vec![0_u8; 1024]).unwrap();
}

fn fingerprint(root: &Path) -> String {
    let mut paths = walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .collect::<Vec<_>>();
    paths.sort();
    let mut hash = Sha256::new();
    for path in paths {
        hash.update(
            path.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .as_bytes(),
        );
        hash.update(fs::read(path).unwrap());
    }
    hex::encode(hash.finalize())
}

fn semantic_report(json: &str) -> serde_json::Value {
    let mut report: serde_json::Value = serde_json::from_str(json).unwrap();
    let object = report.as_object_mut().unwrap();
    for field in ["objects", "files", "validation_findings", "uncovered"] {
        if let Some(records) = object
            .get_mut(field)
            .and_then(serde_json::Value::as_array_mut)
        {
            records.sort_by_key(|record| serde_json::to_string(record).unwrap());
        }
    }
    report
}

#[test]
#[allow(clippy::too_many_lines)]
fn synthetic_multi_dimension_conversion_is_safe_semantic_and_deterministic() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let template = root.path().join("template");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&template).unwrap();
    write_level(&source, false);
    write_level(&template, true);
    write_region(&source.join("region/r.0.0.mca"), 300, true);
    write_region(&source.join("DIM7/region/r.0.0.mca"), 300, true);
    write_region(&template.join("region/r.0.0.mca"), 400, false);
    write_player(&source);
    write_opaque_cache(&source);
    let source_hash = fingerprint(&source);
    let template_hash = fingerprint(&template);
    let source_catalog = catalog(&[
        (RegistryKind::Block, "mod:machine", 300),
        (RegistryKind::Item, "mod:backpack", 500),
        (RegistryKind::Item, "mod:component", 501),
    ]);
    let target_catalog = catalog(&[
        (RegistryKind::Block, "mod:machine", 400),
        (RegistryKind::Item, "mod:backpack", 600),
        (RegistryKind::Item, "mod:component", 601),
    ]);
    let rules = empty_rules();
    let vanilla_catalog = RegistryCatalog::default();
    let coverage = |jobs| {
        minecraft_analysis_core::coverage::analyze_source_with_progress_config(
            &source,
            &source_catalog,
            &vanilla_catalog,
            &rules,
            2,
            &minecraft_analysis_core::progress::NoProgress,
            minecraft_analysis_core::work::ExecutionConfig::new(jobs).unwrap(),
        )
        .unwrap()
        .to_json_pretty()
        .unwrap()
    };
    let sequential_coverage = semantic_report(&coverage(1));
    let parallel_coverage = semantic_report(&coverage(4));
    assert_eq!(sequential_coverage, parallel_coverage);
    let uncovered = sequential_coverage["uncovered"].as_array().unwrap();
    assert!(uncovered.iter().all(|record| {
        record["locations"].as_array().unwrap().len() <= 5
            && record["occurrence_count"].as_u64().unwrap()
                >= u64::try_from(record["locations"].as_array().unwrap().len()).unwrap()
    }));
    let associated = uncovered
        .iter()
        .find(|record| record.get("associated_block_entity").is_some())
        .unwrap();
    assert_eq!(associated["occurrence_count"], 2);
    let associated_snbt = associated["associated_block_entity"]["snbt"]
        .as_str()
        .unwrap();
    for coordinate in ["x", "y", "z"] {
        assert!(!associated_snbt.contains(&format!("\n  {coordinate}:")));
    }
    assert_eq!(
        sequential_coverage["counts"]["uncovered_occurrences"]
            .as_u64()
            .unwrap(),
        uncovered
            .iter()
            .map(|record| record["occurrence_count"].as_u64().unwrap())
            .sum::<u64>()
    );
    let run = |output: PathBuf, jobs| {
        let execution = minecraft_analysis_core::work::ExecutionConfig::new(jobs).unwrap();
        pipeline::convert_with_progress_config(
            &SafePaths {
                source: fs::canonicalize(&source).unwrap(),
                template: fs::canonicalize(&template).unwrap(),
                output: output.clone(),
            },
            &source_catalog,
            &target_catalog,
            &rules,
            2,
            &minecraft_analysis_core::progress::NoProgress,
            execution,
        )
        .unwrap();
        output
    };
    let first = run(root.path().join("output-one"), 1);
    let second = run(root.path().join("output-two"), 4);
    assert_eq!(fingerprint(&first), fingerprint(&second));
    assert_eq!(fingerprint(&source), source_hash);
    assert_eq!(fingerprint(&template), template_hash);
    assert_eq!(
        fs::read(first.join("AE2/compass/-1858.dat")).unwrap(),
        vec![0; 1024]
    );

    for region_path in [
        first.join("region/r.0.0.mca"),
        first.join("DIM7/region/r.0.0.mca"),
    ] {
        let bytes = fs::read(region_path).unwrap();
        let reader = RegionReader::new(&bytes, DEFAULT_MAX_CHUNK_BYTES).unwrap();
        let chunk = nbt::decode_uncompressed(&reader.read_chunk(0, 0).unwrap().unwrap()).unwrap();
        let Value::Compound(level) = chunk.root.get("Level").unwrap() else {
            panic!()
        };
        let Value::List(sections) = level.get("Sections").unwrap() else {
            panic!()
        };
        let Value::Compound(section) = &sections.values[0] else {
            panic!()
        };
        assert_eq!(BlockStorage::from_section(section).unwrap().ids[0], 400);
        assert_eq!(
            level.get("UnknownChunk"),
            Some(&Value::String("preserve".into()))
        );
    }
    let (player, _) = nbt::decode(&fs::read(first.join("playerdata/player.dat")).unwrap()).unwrap();
    let Value::List(inventory) = player.root.get("Inventory").unwrap() else {
        panic!()
    };
    let Value::Compound(backpack) = &inventory.values[0] else {
        panic!()
    };
    assert_eq!(backpack.get("id"), Some(&Value::Short(600)));
    assert!(backpack.contains_key("ForgeCaps"));
    let Value::Compound(tag) = backpack.get("tag").unwrap() else {
        panic!()
    };
    let Value::List(nested) = tag.get("CustomSlots").unwrap() else {
        panic!()
    };
    let Value::Compound(component) = &nested.values[0] else {
        panic!()
    };
    assert_eq!(component.get("id"), Some(&Value::Short(501)));
    let (level, _) = nbt::decode(&fs::read(first.join("level.dat")).unwrap()).unwrap();
    let Value::Compound(data) = level.root.get("Data").unwrap() else {
        panic!()
    };
    assert_eq!(data.get("RandomSeed"), Some(&Value::Long(123)));
    assert_eq!(data.get("DataVersion"), Some(&Value::Int(1343)));
}

#[test]
fn direct_conversion_classifies_the_captured_opaque_source() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let template = root.path().join("template");
    fs::create_dir_all(source.join("AE2/compass")).unwrap();
    fs::create_dir_all(&template).unwrap();
    write_level(&source, false);
    write_level(&template, true);
    fs::write(source.join("AE2/compass/cache.dat"), b"opaque").unwrap();
    let source_catalog = RegistryCatalog::default();
    let target_catalog = RegistryCatalog::default();
    let rules = empty_rules();
    fs::write(source.join("AE2/compass/cache.dat"), b"changed").unwrap();
    pipeline::convert(
        &SafePaths {
            source: fs::canonicalize(&source).unwrap(),
            template: fs::canonicalize(&template).unwrap(),
            output: root.path().join("output"),
        },
        &source_catalog,
        &target_catalog,
        &rules,
    )
    .unwrap();
    assert_eq!(
        fs::read(root.path().join("output/AE2/compass/cache.dat")).unwrap(),
        b"changed"
    );
}

#[test]
fn concurrent_region_failure_does_not_publish_or_leave_temporaries() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let template = root.path().join("template");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&template).unwrap();
    write_level(&source, false);
    write_level(&template, true);
    write_region(&source.join("region/r.0.0.mca"), 300, false);
    write_region(&source.join("region/r.1.0.mca"), 300, false);
    let source_catalog = catalog(&[(RegistryKind::Block, "mod:machine", 300)]);
    let target_catalog = catalog(&[(RegistryKind::Block, "mod:machine", 400)]);
    let rules = empty_rules();
    fs::write(source.join("region/r.0.0.mca"), b"corrupt after preflight").unwrap();
    let output = root.path().join("output");
    let error = pipeline::convert_with_progress_config(
        &SafePaths {
            source: fs::canonicalize(&source).unwrap(),
            template: fs::canonicalize(&template).unwrap(),
            output: output.clone(),
        },
        &source_catalog,
        &target_catalog,
        &rules,
        2,
        &minecraft_analysis_core::progress::NoProgress,
        minecraft_analysis_core::work::ExecutionConfig::new(2).unwrap(),
    )
    .unwrap_err();
    assert!(matches!(error, pipeline::Error::Region(_)));
    assert!(!output.exists());
    let staging = root.path().join(".output.minecraft-analysis-staging");
    let temporaries = walkdir::WalkDir::new(staging)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .contains("minecraft-analysis-tmp")
        })
        .count();
    assert_eq!(temporaries, 0);
}
