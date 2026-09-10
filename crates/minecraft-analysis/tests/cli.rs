use std::collections::BTreeMap;
use std::fs;
use std::process::{Command, Output};

use minecraft_analysis_core::nbt::{self, Compression, Document, List, Tag, Value};
use minecraft_analysis_core::region::RegionWriter;

fn update_manifest(rule: &std::path::Path, id_map: &std::path::Path, side: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .args(["rules", "update-manifest", "--rules"])
        .arg(rule)
        .arg("--id-map")
        .arg(id_map)
        .args(["--manifest", side])
        .output()
        .unwrap()
}

#[test]
fn rules_update_manifest_imports_legacy_map_deterministically() {
    let root = tempfile::tempdir().unwrap();
    let profile = root.path().join("profile.yaml");
    let rules = root.path().join("rules.yaml");
    let id_map = root.path().join("idmap.txt");
    fs::write(
        &profile,
        r#"{"schema_version":1,"rule_set":"profile","source_profile":"forge-1.2.5"}"#,
    )
    .unwrap();
    fs::write(
        &rules,
        r#"{
  "schema_version": 1,
  "rule_set": "root",
  "imports": ["profile.yaml"],
  "rules": [],
  "source_manifest": [
    {"kind":"item","name":"manual:kept","numeric_id":30001}
  ],
  "target_manifest": [
    {"kind":"block","name":"target:kept","numeric_id":200}
  ]
}"#,
    )
    .unwrap();
    fs::write(
        &id_map,
        "Block. Name: tile.stone. ID: 1\n\
         Block. Name: tile.machineBlock. ID: 153\n\
         Block. Name: tile.machineBlock. ID: 154\n\
         Item. Name: tile.machineBlock. ID: 153\n",
    )
    .unwrap();

    let first = update_manifest(&rules, &id_map, "source");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let summary: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(summary["added"], 2);
    assert_eq!(summary["updated"], 0);
    assert_eq!(summary["unchanged"], 0);
    assert_eq!(summary["vanilla_skipped"], 1);
    assert_eq!(summary["duplicate_skipped"], 1);

    let document: serde_json::Value = serde_yaml::from_slice(&fs::read(&rules).unwrap()).unwrap();
    assert_eq!(document["imports"], serde_json::json!(["profile.yaml"]));
    assert_eq!(document["target_manifest"][0]["name"], "target:kept");
    assert_eq!(document["source_manifest"].as_array().unwrap().len(), 3);
    assert!(document["source_manifest"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["kind"] == "block"
            && entry["name"] == "legacy:tile.machineBlock"
            && entry["numeric_id"] == 153));
    assert!(document["source_manifest"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["kind"] == "item"
            && entry["name"] == "legacy:tile.machineBlock"
            && entry["numeric_id"] == 153));

    let after_first = fs::read(&rules).unwrap();
    let second = update_manifest(&rules, &id_map, "source");
    assert!(second.status.success());
    let repeated: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(repeated["added"], 0);
    assert_eq!(repeated["unchanged"], 2);
    assert_eq!(fs::read(&rules).unwrap(), after_first);
}

#[test]
fn rules_update_manifest_preserves_file_on_input_and_version_failures() {
    let root = tempfile::tempdir().unwrap();
    let rules = root.path().join("rules.yaml");
    let id_map = root.path().join("idmap.txt");
    fs::write(
        &rules,
        r#"{"schema_version":1,"rule_set":"root","source_profile":"forge-1.2.5"}"#,
    )
    .unwrap();
    let original = fs::read(&rules).unwrap();
    fs::write(&id_map, "bad input\n").unwrap();
    let malformed = update_manifest(&rules, &id_map, "source");
    assert!(!malformed.status.success());
    assert!(String::from_utf8_lossy(&malformed.stderr).contains("line 1"));
    assert_eq!(fs::read(&rules).unwrap(), original);

    fs::write(&id_map, "Block. Name: mod:block. ID: 200\n").unwrap();
    let unsupported = update_manifest(&rules, &id_map, "target");
    assert!(!unsupported.status.success());
    assert!(String::from_utf8_lossy(&unsupported.stderr).contains("forge-1.12.2"));
    assert_eq!(fs::read(&rules).unwrap(), original);
}

#[test]
fn removed_dry_run_is_rejected_without_creating_world_output() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("output-world");
    let result = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .args([
            "dry-run",
            "--source",
            root.path().join("missing-source").to_str().unwrap(),
            "--template",
            root.path().join("missing-template").to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--rules",
            root.path().join("rules.yaml").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("unrecognized subcommand 'dry-run'"));
    assert!(!output.exists());
}

fn nbt_dump(file: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .args(["nbt", "dump"])
        .arg(file)
        .output()
        .unwrap()
}

fn nbt_dump_world(
    world: &std::path::Path,
    location: &str,
    rule: Option<&std::path::Path>,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"));
    command
        .args(["nbt", "dump", "--world"])
        .arg(world)
        .args(["--location", location]);
    if let Some(rule) = rule {
        command.arg("--rules").arg(rule);
    }
    command.output().unwrap()
}

fn write_block_world(root: &std::path::Path) {
    fs::create_dir_all(root.join("region")).unwrap();
    let level = Document {
        root_name: String::new(),
        root: BTreeMap::from([(
            "FML".into(),
            Value::Compound(BTreeMap::from([(
                "ItemData".into(),
                Value::List(List {
                    element_tag: Tag::Compound,
                    values: vec![],
                }),
            )])),
        )]),
    };
    fs::write(
        root.join("level.dat"),
        nbt::encode(&level, Compression::Gzip).unwrap(),
    )
    .unwrap();
    let mut blocks = vec![0_i8; 4096];
    blocks[4095] = 44;
    let mut add = vec![0_i8; 2048];
    add[2047] = 0x10;
    let mut data = vec![0_i8; 2048];
    data[2047] = 0x20;
    let mut sky = vec![0_i8; 2048];
    sky[2047] = -16;
    let entity = Value::Compound(BTreeMap::from([
        ("Energy".into(), Value::Long(42)),
        ("id".into(), Value::String("mod:tile".into())),
        (
            "nested".into(),
            Value::Compound(BTreeMap::from([("value".into(), Value::Int(7))])),
        ),
        ("x".into(), Value::Int(-1)),
        ("y".into(), Value::Int(15)),
        ("z".into(), Value::Int(-1)),
    ]));
    let section = Value::Compound(BTreeMap::from([
        ("Y".into(), Value::Byte(0)),
        ("Blocks".into(), Value::ByteArray(blocks)),
        ("Add".into(), Value::ByteArray(add)),
        ("Data".into(), Value::ByteArray(data)),
        ("SkyLight".into(), Value::ByteArray(sky)),
    ]));
    let chunk = Document {
        root_name: String::new(),
        root: BTreeMap::from([(
            "Level".into(),
            Value::Compound(BTreeMap::from([
                (
                    "Sections".into(),
                    Value::List(List {
                        element_tag: Tag::Compound,
                        values: vec![section],
                    }),
                ),
                (
                    "TileEntities".into(),
                    Value::List(List {
                        element_tag: Tag::Compound,
                        values: vec![entity],
                    }),
                ),
            ])),
        )]),
    };
    let mut writer = RegionWriter::new().unwrap();
    writer
        .write_chunk(31, 31, &nbt::encode_uncompressed(&chunk).unwrap(), 1)
        .unwrap();
    fs::write(root.join("region/r.-1.-1.mca"), writer.finish().unwrap()).unwrap();
}

fn make_forge_112_level(root: &std::path::Path) {
    let level = Document {
        root_name: String::new(),
        root: BTreeMap::from([
            (
                "FML".into(),
                Value::Compound(BTreeMap::from([(
                    "Registries".into(),
                    Value::Compound(BTreeMap::new()),
                )])),
            ),
            (
                "Data".into(),
                Value::Compound(BTreeMap::from([("DataVersion".into(), Value::Int(1343))])),
            ),
        ]),
    };
    fs::write(
        root.join("level.dat"),
        nbt::encode(&level, Compression::Gzip).unwrap(),
    )
    .unwrap();
}

fn rules_infer(
    source: &std::path::Path,
    target: &std::path::Path,
    rules: &std::path::Path,
    extra: &[&str],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"));
    command
        .args(["rules", "infer", "--rules"])
        .arg(rules)
        .arg("--source-world")
        .arg(source)
        .arg("--target-world")
        .arg(target)
        .args(["--coordinate", "-1,15,-1:-1,15,-1"])
        .args(extra)
        .output()
        .unwrap()
}

#[test]
fn rules_infer_reads_world_pair_deterministically_and_atomically() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("target");
    write_block_world(&source);
    write_block_world(&target);
    make_forge_112_level(&target);
    let rules = root.path().join("rules.yaml");
    fs::write(
        &rules,
        r#"{
  "schema_version": 1,
  "rule_set": "inference-context",
  "source_profile": "forge-1.7.10",
  "source_manifest": [{"kind":"block","name":"source:machine","numeric_id":300}],
  "target_manifest": [{"kind":"block","name":"target:machine","numeric_id":300}]
}"#,
    )
    .unwrap();
    let source_level = fs::read(source.join("level.dat")).unwrap();
    let target_level = fs::read(target.join("level.dat")).unwrap();
    let original_rules = fs::read(&rules).unwrap();

    let first = rules_infer(&source, &target, &rules, &[]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(first.stdout.last(), Some(&b'\n'));
    assert!(!first.stdout.ends_with(b"\n\n"));
    let output: serde_json::Value = serde_yaml::from_slice(&first.stdout).unwrap();
    let entries = output.as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["object"], "block");
    assert_eq!(entries[0]["matcher"]["name"], "source:machine");
    let template = entries[0]["template"].as_str().unwrap();
    assert!(template.lines().count() > 1);
    assert!(template.contains("target:machine"));

    let inferred_path = root.path().join("inferred.yaml");
    let indented_rules = String::from_utf8(first.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        &inferred_path,
        format!(
            "schema_version: 1\nrule_set: inferred\nsource_profile: forge-1.7.10\nsource_manifest:\n  - {{ kind: block, name: source:machine, numeric_id: 300 }}\ntarget_manifest:\n  - {{ kind: block, name: target:machine, numeric_id: 300 }}\nrules:\n{indented_rules}\n"
        ),
    )
    .unwrap();
    let inferred = minecraft_analysis_core::rules::load(&inferred_path).unwrap();
    assert_eq!(inferred.ordered_rules.len(), 1);

    let repeated = rules_infer(&source, &target, &rules, &[]);
    assert!(repeated.status.success());
    assert_eq!(repeated.stdout, first.stdout);
    assert_eq!(fs::read(source.join("level.dat")).unwrap(), source_level);
    assert_eq!(fs::read(target.join("level.dat")).unwrap(), target_level);
    assert_eq!(fs::read(&rules).unwrap(), original_rules);

    fs::create_dir_all(source.join("DIM-1/region")).unwrap();
    fs::create_dir_all(target.join("DIM1/region")).unwrap();
    fs::copy(
        source.join("region/r.-1.-1.mca"),
        source.join("DIM-1/region/r.-1.-1.mca"),
    )
    .unwrap();
    fs::copy(
        target.join("region/r.-1.-1.mca"),
        target.join("DIM1/region/r.-1.-1.mca"),
    )
    .unwrap();
    let dimensions = rules_infer(
        &source,
        &target,
        &rules,
        &["--source-dimension", "nether", "--target-dimension", "end"],
    );
    assert!(dimensions.status.success());
    assert_eq!(dimensions.stdout, first.stdout);
}

#[test]
fn rules_infer_profile_and_identity_failures_leave_stdout_empty() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("target");
    write_block_world(&source);
    write_block_world(&target);
    let rules = root.path().join("rules.yaml");
    fs::write(
        &rules,
        r#"{"schema_version":1,"rule_set":"context","source_profile":"forge-1.7.10","source_manifest":[{"kind":"block","name":"source:machine","numeric_id":300}]}"#,
    )
    .unwrap();

    let incompatible = rules_infer(&source, &target, &rules, &[]);
    assert!(!incompatible.status.success());
    assert!(incompatible.stdout.is_empty());
    assert!(String::from_utf8_lossy(&incompatible.stderr).contains("Forge1_12_2"));

    make_forge_112_level(&target);
    let unresolved = rules_infer(&source, &target, &rules, &[]);
    assert!(!unresolved.status.success());
    assert!(unresolved.stdout.is_empty());
    let diagnostic = String::from_utf8_lossy(&unresolved.stderr);
    assert!(diagnostic.contains("target registry"), "{diagnostic}");
    assert!(diagnostic.contains("numeric block ID 300"), "{diagnostic}");
}

#[test]
fn nbt_dump_world_coordinate_uses_shared_context_and_is_atomic() {
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("world");
    write_block_world(&world);
    let source_rule = root.path().join("source.yaml");
    fs::write(
        &source_rule,
        r#"{
      "schema_version":1,"rule_set":"source","source_profile":"forge-1.7.10",
      "source_manifest":[{"kind":"block","name":"source:machine","numeric_id":300}]
    }"#,
    )
    .unwrap();
    let source = nbt_dump_world(&world, "-1,15,-1", Some(&source_rule));
    assert!(
        source.status.success(),
        "{}",
        String::from_utf8_lossy(&source.stderr)
    );
    let text = String::from_utf8(source.stdout).unwrap();
    assert_eq!(text, "coordinate: -1,15,-1\ndimension: overworld\nglobal chunk: -1,-1\nregion: -1,-1\nlocal chunk: 31,31\nnumeric ID: 300\nregistry name: source:machine\nmetadata: 2\nblock light: unavailable\nsky light: 15\nsection Y: 0\nsection index: 4095\nblock entity:\n{\n  Energy: 42L,\n  id: \"mod:tile\",\n  nested: {\n    value: 7\n  },\n  x: -1,\n  y: 15,\n  z: -1\n}\n");
    let repeated = nbt_dump_world(&world, "-1,15,-1", Some(&source_rule));
    assert_eq!(repeated.stdout, text.as_bytes());

    let without_entity = nbt_dump_world(&world, "-16,0,-16", Some(&source_rule));
    assert!(without_entity.status.success());
    assert!(String::from_utf8_lossy(&without_entity.stdout).ends_with("block entity:\nnone\n"));

    fs::create_dir_all(world.join("DIM-1/region")).unwrap();
    fs::copy(
        world.join("region/r.-1.-1.mca"),
        world.join("DIM-1/region/r.-1.-1.mca"),
    )
    .unwrap();
    let nether = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .args(["nbt", "dump", "--world"])
        .arg(&world)
        .args(["--location", "-1,15,-1", "--dimension", "nether", "--rules"])
        .arg(&source_rule)
        .output()
        .unwrap();
    assert!(nether.status.success());
    assert!(String::from_utf8_lossy(&nether.stdout).contains("dimension: nether\n"));

    let unresolved = nbt_dump_world(&world, "-1,15,-1", None);
    assert!(unresolved.status.success());
    assert!(String::from_utf8_lossy(&unresolved.stdout)
        .contains("numeric ID: 300\nregistry name: unresolved\n"));
}

fn nbt_view(file: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .args(["nbt", "view"])
        .arg(file)
        .output()
        .unwrap()
}

fn nbt_view_world(world: &std::path::Path, location: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .args(["nbt", "view", "--world"])
        .arg(world)
        .args(["--location", location])
        .output()
        .unwrap()
}

fn assert_diagnostic_words(stderr: &str, expected: &str) {
    assert!(
        expected
            .split_whitespace()
            .all(|word| stderr.contains(word)),
        "missing diagnostic phrase {expected:?}: {stderr}"
    );
}

#[test]
fn nbt_view_rejects_invalid_inputs_before_terminal_initialization() {
    let root = tempfile::tempdir().unwrap();
    let missing_path = root.path().join("missing.nbt");
    let missing = nbt_view(&missing_path);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("cannot read NBT file"));
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains(missing_path.to_string_lossy().as_ref())
    );

    let directory = root.path().join("directory.nbt");
    fs::create_dir(&directory).unwrap();
    let not_file = nbt_view(&directory);
    assert!(!not_file.status.success());
    let not_file_stderr = String::from_utf8_lossy(&not_file.stderr);
    assert!(not_file_stderr.contains("regular"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let unreadable_path = root.path().join("unreadable.nbt");
        fs::write(&unreadable_path, [10, 0, 0, 0]).unwrap();
        fs::set_permissions(&unreadable_path, fs::Permissions::from_mode(0o000)).unwrap();
        let unreadable = nbt_view(&unreadable_path);
        fs::set_permissions(&unreadable_path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(!unreadable.status.success());
        assert!(String::from_utf8_lossy(&unreadable.stderr).contains("cannot read NBT file"));
    }

    for (name, bytes, expected) in [
        (
            "malformed.nbt",
            b"not nbt".to_vec(),
            "cannot decode NBT file",
        ),
        ("truncated.nbt", vec![10, 0], "cannot decode NBT file"),
        (
            "non-compound.nbt",
            vec![1, 0, 0, 0],
            "NBT root must be a compound",
        ),
        ("trailing.nbt", vec![10, 0, 0, 0, 99], "trailing bytes"),
    ] {
        let path = root.path().join(name);
        fs::write(&path, bytes).unwrap();
        let output = nbt_view(&path);
        assert!(!output.status.success(), "{name} unexpectedly succeeded");
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_diagnostic_words(&stderr, expected);
        assert!(stderr.contains(path.to_string_lossy().as_ref()));
        assert!(
            !stderr.contains("terminal"),
            "viewer initialized for {name}"
        );
    }
}

#[test]
fn nbt_view_resolves_requested_world_block_before_terminal_initialization() {
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("world");
    write_block_world(&world);

    let valid = nbt_view_world(&world, "-1,15,-1");
    let valid_stderr = String::from_utf8_lossy(&valid.stderr);
    assert!(!valid.status.success());
    assert!(valid_stderr.contains("terminal"), "{valid_stderr}");

    let absent = nbt_view_world(&world, "-1,32,-1");
    let absent_stderr = String::from_utf8_lossy(&absent.stderr);
    assert!(!absent.status.success());
    assert!(
        absent_stderr.contains("no stored block is indexed"),
        "{absent_stderr}"
    );
    assert!(!absent_stderr.contains("initialize NBT viewer terminal"));

    let missing = nbt_view_world(&root.path().join("missing"), "0,0,0");
    let missing_stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(!missing.status.success());
    assert!(
        missing_stderr.contains("cannot resolve NBT world"),
        "{missing_stderr}"
    );
    assert!(!missing_stderr.contains("initialize NBT viewer terminal"));
}

#[test]
fn nbt_dump_handles_supported_compression_and_arbitrary_profiles_deterministically() {
    let root = tempfile::tempdir().unwrap();
    let document = Document {
        root_name: "ignored binary root name".into(),
        root: BTreeMap::from([(
            "UnknownProfile".into(),
            Value::Compound(BTreeMap::from([
                ("DataVersion".into(), Value::Int(99_999)),
                (
                    "Values".into(),
                    Value::List(List {
                        element_tag: Tag::Long,
                        values: vec![Value::Long(1), Value::Long(2)],
                    }),
                ),
            ])),
        )]),
    };
    for compression in [
        Compression::Uncompressed,
        Compression::Gzip,
        Compression::Zlib,
    ] {
        let path = root.path().join(format!("{compression:?}.nbt"));
        fs::write(&path, nbt::encode(&document, compression).unwrap()).unwrap();
        let first = nbt_dump(&path);
        let second = nbt_dump(&path);
        assert!(
            first.status.success(),
            "{}",
            String::from_utf8_lossy(&first.stderr)
        );
        assert_eq!(first.stdout, second.stdout);
        assert_eq!(
            String::from_utf8(first.stdout).unwrap(),
            "{\n  UnknownProfile: {\n    DataVersion: 99999,\n    Values: [\n      1L,\n      2L\n    ]\n  }\n}\n"
        );
    }
}

#[test]
fn nbt_dump_preserves_typed_data_and_escaping() {
    let root = tempfile::tempdir().unwrap();
    let pair = Value::Compound(BTreeMap::from([
        ("K".into(), Value::String("\u{1}examplemod:machine".into())),
        ("V".into(), Value::Int(3000)),
    ]));
    let document = Document {
        root_name: "not emitted".into(),
        root: BTreeMap::from([
            ("bytes".into(), Value::ByteArray(vec![-1, 2])),
            ("double".into(), Value::Double(2.5)),
            ("float".into(), Value::Float(1.5)),
            ("ints".into(), Value::IntArray(vec![3, 4])),
            ("longs".into(), Value::LongArray(vec![5, 6])),
            ("short".into(), Value::Short(7)),
            (
                "quoted key".into(),
                Value::String("quote \" slash \\ newline\n".into()),
            ),
            (
                "FML".into(),
                Value::Compound(BTreeMap::from([(
                    "ItemData".into(),
                    Value::List(List {
                        element_tag: Tag::Compound,
                        values: vec![pair],
                    }),
                )])),
            ),
        ]),
    };
    let path = root.path().join("typed.nbt");
    fs::write(&path, nbt::encode(&document, Compression::Gzip).unwrap()).unwrap();

    let output = nbt_dump(&path);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("FML: {"));
    assert!(stdout.contains("ItemData: ["));
    assert!(stdout.contains(r#"K: "\u0001examplemod:machine""#));
    assert!(stdout.contains("V: 3000"));
    assert!(stdout.contains("bytes: [B;\n"));
    assert!(stdout.contains("    -1b,\n    2b\n"));
    assert!(stdout.contains("ints: [I;\n"));
    assert!(stdout.contains("    3,\n    4\n"));
    assert!(stdout.contains("longs: [L;\n"));
    assert!(stdout.contains("    5L,\n    6L\n"));
    assert!(stdout.contains("float: 1.5f"));
    assert!(stdout.contains("double: 2.5d"));
    assert!(stdout.contains("short: 7s"));
    assert!(stdout.contains(r#""quoted key": "quote \" slash \\ newline\n""#));
    assert!(!stdout.contains("not emitted"));
}

#[test]
fn nbt_dump_failures_have_context_and_no_partial_stdout() {
    let root = tempfile::tempdir().unwrap();
    let missing_path = root.path().join("missing.nbt");
    let missing = nbt_dump(&missing_path);
    assert!(!missing.status.success());
    assert!(missing.stdout.is_empty());
    let missing_stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(missing_stderr.contains("cannot read NBT file"));
    assert!(missing_stderr.contains(missing_path.to_string_lossy().as_ref()));

    let directory = root.path().join("directory.nbt");
    fs::create_dir(&directory).unwrap();
    let not_file = nbt_dump(&directory);
    assert!(!not_file.status.success());
    assert!(not_file.stdout.is_empty());
    assert!(String::from_utf8_lossy(&not_file.stderr).contains("regular file"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let unreadable_path = root.path().join("unreadable.nbt");
        fs::write(&unreadable_path, [10, 0, 0, 0]).unwrap();
        fs::set_permissions(&unreadable_path, fs::Permissions::from_mode(0o000)).unwrap();
        let unreadable = nbt_dump(&unreadable_path);
        fs::set_permissions(&unreadable_path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(!unreadable.status.success());
        assert!(unreadable.stdout.is_empty());
        assert!(String::from_utf8_lossy(&unreadable.stderr).contains("cannot read NBT file"));
    }

    for (name, bytes, expected) in [
        (
            "malformed.nbt",
            b"not nbt".to_vec(),
            "cannot decode NBT file",
        ),
        ("truncated.nbt", vec![10, 0], "cannot decode NBT file"),
        (
            "non-compound.nbt",
            vec![1, 0, 0, 0],
            "NBT root must be a compound",
        ),
        ("trailing.nbt", vec![10, 0, 0, 0, 99], "trailing bytes"),
    ] {
        let path = root.path().join(name);
        fs::write(&path, bytes).unwrap();
        let output = nbt_dump(&path);
        assert!(!output.status.success(), "{name} unexpectedly succeeded");
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_diagnostic_words(&stderr, expected);
        assert!(stderr.contains(path.to_string_lossy().as_ref()));
    }

    let non_finite_path = root.path().join("non-finite.nbt");
    let document = Document {
        root_name: String::new(),
        root: BTreeMap::from([("bad".into(), Value::Double(f64::NAN))]),
    };
    fs::write(
        &non_finite_path,
        nbt::encode_uncompressed(&document).unwrap(),
    )
    .unwrap();
    let non_finite = nbt_dump(&non_finite_path);
    assert!(!non_finite.status.success());
    assert!(non_finite.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&non_finite.stderr);
    assert!(stderr.contains("cannot render NBT file"));
    assert!(stderr.contains(non_finite_path.to_string_lossy().as_ref()));
}

#[test]
fn nbt_help_exposes_dump_and_rejects_level() {
    let help = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .args(["nbt", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let stdout = String::from_utf8_lossy(&help.stdout);
    assert!(stdout.contains("dump"));
    assert!(stdout.contains("view"));
    assert!(!stdout.contains("level"));

    let dump_help = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .args(["nbt", "dump", "--help"])
        .output()
        .unwrap();
    assert!(dump_help.status.success());
    assert!(String::from_utf8_lossy(&dump_help.stdout).contains("<FILE>"));
    let dump_help = String::from_utf8_lossy(&dump_help.stdout);
    assert!(dump_help.contains("--world <WORLD>"));
    assert!(dump_help.contains("--location <X,Y,Z>"));
    assert!(dump_help.contains("--dimension <DIMENSION>"));
    assert!(dump_help.contains("--rules <FILE>"));
    assert!(!dump_help.contains("--chunk"));
    assert!(!dump_help.contains("--local-chunk"));
    assert!(!dump_help.contains("--source-rule"));
    assert!(!dump_help.contains("--target-rule"));

    let view_help = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .args(["nbt", "view", "--help"])
        .output()
        .unwrap();
    let view_help = String::from_utf8_lossy(&view_help.stdout);
    assert!(view_help.contains("--world <WORLD>"));
    assert!(view_help.contains("--location <X,Y,Z>"));
    assert!(view_help.contains("--rules <FILE>"));
    assert!(!view_help.contains("--chunk"));
    assert!(!view_help.contains("--local-chunk"));

    let level = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .args(["nbt", "level", "--world", "."])
        .output()
        .unwrap();
    assert!(!level.status.success());
}

#[test]
fn jobs_is_global_positive_and_available_on_region_commands() {
    let executable = env!("CARGO_BIN_EXE_minecraft-analysis");
    let help = Command::new(executable).arg("--help").output().unwrap();
    assert!(help.status.success());
    let stdout = String::from_utf8_lossy(&help.stdout);
    assert!(stdout.contains("--jobs <N>"), "{stdout}");
    assert!(stdout.contains("available CPUs"), "{stdout}");

    let zero = Command::new(executable)
        .args([
            "--jobs",
            "0",
            "convert",
            "--source",
            ".",
            "--template",
            ".",
            "--output",
            "out",
            "--rules",
            "rules.yaml",
        ])
        .output()
        .unwrap();
    assert!(!zero.status.success());
    let stderr = String::from_utf8_lossy(&zero.stderr);
    assert!(stderr.contains("--jobs"), "{stderr}");
    assert!(stderr.contains('0'), "{stderr}");

    let one = Command::new(executable)
        .args(["convert", "--jobs", "1", "--help"])
        .output()
        .unwrap();
    assert!(one.status.success());
}

fn coverage_world(root: &std::path::Path, include_tiles: bool) {
    fs::create_dir_all(root.join("region")).unwrap();
    let registry = Value::Compound(BTreeMap::from([
        ("K".into(), Value::String("\u{1}mod:machine".into())),
        ("V".into(), Value::Int(300)),
    ]));
    let level = Document {
        root_name: String::new(),
        root: BTreeMap::from([(
            "FML".into(),
            Value::Compound(BTreeMap::from([(
                "ItemData".into(),
                Value::List(List {
                    element_tag: Tag::Compound,
                    values: vec![registry],
                }),
            )])),
        )]),
    };
    fs::write(
        root.join("level.dat"),
        nbt::encode(&level, Compression::Gzip).unwrap(),
    )
    .unwrap();

    let mut blocks = vec![0_i8; 4096];
    blocks[..7].fill(44_i8);
    let mut add = vec![0_i8; 2048];
    add[..3].fill(0x11);
    add[3] = 0x01;
    let section = Value::Compound(BTreeMap::from([
        ("Y".into(), Value::Byte(0)),
        ("Blocks".into(), Value::ByteArray(blocks)),
        ("Data".into(), Value::ByteArray(vec![0; 2048])),
        ("Add".into(), Value::ByteArray(add)),
    ]));
    let tile = |x| {
        Value::Compound(BTreeMap::from([
            ("id".into(), Value::String("mod:machine_tile".into())),
            ("x".into(), Value::Int(x)),
            ("y".into(), Value::Int(0)),
            ("z".into(), Value::Int(0)),
            ("Energy".into(), Value::Long(42)),
        ]))
    };
    let chunk = Document {
        root_name: String::new(),
        root: BTreeMap::from([(
            "Level".into(),
            Value::Compound(BTreeMap::from([
                (
                    "Sections".into(),
                    Value::List(List {
                        element_tag: Tag::Compound,
                        values: vec![section],
                    }),
                ),
                (
                    "TileEntities".into(),
                    Value::List(List {
                        element_tag: Tag::Compound,
                        values: if include_tiles {
                            vec![tile(0), tile(1)]
                        } else {
                            vec![]
                        },
                    }),
                ),
                (
                    "Entities".into(),
                    Value::List(List {
                        element_tag: Tag::Compound,
                        values: vec![],
                    }),
                ),
            ])),
        )]),
    };
    let mut region = RegionWriter::new().unwrap();
    region
        .write_chunk(0, 0, &nbt::encode_uncompressed(&chunk).unwrap(), 1)
        .unwrap();
    fs::write(root.join("region/r.0.0.mca"), region.finish().unwrap()).unwrap();
}

fn empty_profile_world(root: &std::path::Path, target: bool) {
    fs::create_dir_all(root.join("region")).unwrap();
    let fml = if target {
        BTreeMap::from([("Registries".into(), Value::Compound(BTreeMap::new()))])
    } else {
        BTreeMap::from([(
            "ItemData".into(),
            Value::List(List {
                element_tag: Tag::Compound,
                values: vec![],
            }),
        )])
    };
    let level = Document {
        root_name: String::new(),
        root: BTreeMap::from([
            ("FML".into(), Value::Compound(fml)),
            (
                "Data".into(),
                Value::Compound(BTreeMap::from([(
                    "DataVersion".into(),
                    Value::Int(if target { 1343 } else { 0 }),
                )])),
            ),
        ]),
    };
    fs::write(
        root.join("level.dat"),
        nbt::encode(&level, Compression::Gzip).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("region/r.0.0.mca"),
        RegionWriter::new().unwrap().finish().unwrap(),
    )
    .unwrap();
}

fn declare_forge_1_2_5_level(root: &std::path::Path) {
    let level = Document {
        root_name: String::new(),
        root: BTreeMap::from([(
            "Data".into(),
            Value::Compound(BTreeMap::from([(
                "CallerAssertionFixture".into(),
                Value::Byte(1),
            )])),
        )]),
    };
    fs::write(
        root.join("level.dat"),
        nbt::encode(&level, Compression::Gzip).unwrap(),
    )
    .unwrap();
}

fn coverage_command(world: &std::path::Path, rules: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .args(["rules", "coverage", "--world"])
        .arg(world)
        .arg("--rules")
        .arg(rules)
        .output()
        .unwrap()
}

fn explain_command(
    source: &std::path::Path,
    template: &std::path::Path,
    rules: &std::path::Path,
    location: &str,
    dimension: Option<&str>,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"));
    command
        .arg("--no-progress")
        .arg("explain")
        .arg("--source")
        .arg(source)
        .arg("--template")
        .arg(template)
        .arg("--rules")
        .arg(rules)
        .arg("--location")
        .arg(location);
    if let Some(dimension) = dimension {
        command.arg("--dimension").arg(dimension);
    }
    command.output().unwrap()
}

#[test]
fn explain_targets_overworld_and_explicit_modded_dimension() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let template = root.path().join("template");
    let rules = root.path().join("empty.yaml");
    coverage_world(&source, true);
    empty_profile_world(&template, true);
    fs::write(
        &rules,
        r#"{"schema_version":1,"rule_set":"explain","source_profile":"forge-1.7.10","rules":[{"id":"explain:machine","object":"block","matcher":{"name":"mod:machine"},"template":"{\"disposition\":\"unchanged\"}"}]}"#,
    )
    .unwrap();
    fs::create_dir_all(source.join("DIM7/region")).unwrap();
    fs::copy(
        source.join("region/r.0.0.mca"),
        source.join("DIM7/region/r.0.0.mca"),
    )
    .unwrap();

    let overworld = explain_command(&source, &template, &rules, "0,0,0", None);
    assert!(
        overworld.status.success(),
        "{}",
        String::from_utf8_lossy(&overworld.stderr)
    );
    let overworld: serde_json::Value = serde_json::from_slice(&overworld.stdout).unwrap();
    assert_eq!(overworld["objects"].as_array().unwrap().len(), 2);
    let block = &overworld["objects"].as_array().unwrap()[0];
    assert_eq!(block["candidates"][0]["rule_id"], "explain:machine");
    assert_eq!(
        block["template_diagnostics"][0]["phase"],
        "selected_template"
    );
    assert!(overworld["objects"]
        .as_array()
        .unwrap()
        .iter()
        .all(|record| record["location"]["file"] == "region/r.0.0.mca"));

    let modded = explain_command(&source, &template, &rules, "0,0,0", Some("DIM7"));
    assert!(
        modded.status.success(),
        "{}",
        String::from_utf8_lossy(&modded.stderr)
    );
    let modded: serde_json::Value = serde_json::from_slice(&modded.stdout).unwrap();
    assert!(modded["objects"]
        .as_array()
        .unwrap()
        .iter()
        .all(|record| record["location"]["file"] == "DIM7/region/r.0.0.mca"));
}

#[test]
fn explain_failures_are_actionable_for_coordinates_dimensions_and_containers() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let template = root.path().join("template");
    let rules = root.path().join("empty.yaml");
    coverage_world(&source, false);
    empty_profile_world(&template, true);
    fs::write(
        &rules,
        r#"{"schema_version":1,"rule_set":"empty","source_profile":"forge-1.7.10","rules":[]}"#,
    )
    .unwrap();
    let no_object = explain_command(&source, &template, &rules, "0,100,0", None);
    assert!(!no_object.status.success());
    assert_diagnostic_words(
        &String::from_utf8_lossy(&no_object.stderr),
        "no explainable coordinate-owned object",
    );

    let missing_region = explain_command(&source, &template, &rules, "512,0,0", None);
    assert!(!missing_region.status.success());
    assert_diagnostic_words(
        &String::from_utf8_lossy(&missing_region.stderr),
        "selected region does not exist",
    );

    let missing_chunk = explain_command(&source, &template, &rules, "16,0,0", None);
    assert!(!missing_chunk.status.success());
    assert_diagnostic_words(
        &String::from_utf8_lossy(&missing_chunk.stderr),
        "selected chunk is absent",
    );

    for (location, dimension, phrase) in [
        (
            "region/r.0.0.mca:0,0,0",
            None,
            "old file:x,y,z syntax is no longer supported",
        ),
        ("0,0,0", Some("DIM../escape"), "invalid dimension"),
        ("0,0,0", Some("DIM404"), "selected dimension does not exist"),
    ] {
        let failure = explain_command(&source, &template, &rules, location, dimension);
        assert!(!failure.status.success());
        assert_diagnostic_words(&String::from_utf8_lossy(&failure.stderr), phrase);
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn rule_coverage_is_read_only_deterministic_and_uses_three_exit_classes() {
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("world");
    fs::create_dir(&world).unwrap();
    coverage_world(&world, true);
    let before_level = fs::read(world.join("level.dat")).unwrap();
    let before_region = fs::read(world.join("region/r.0.0.mca")).unwrap();

    let empty_rules = root.path().join("empty.yaml");
    fs::write(
        &empty_rules,
        r#"{"schema_version":1,"rule_set":"empty","source_profile":"forge-1.7.10","rules":[]}"#,
    )
    .unwrap();
    let first = coverage_command(&world, &empty_rules);
    let second = coverage_command(&world, &empty_rules);
    assert_eq!(first.status.code(), Some(1));
    assert!(first.stderr.is_empty());
    assert!(second.stderr.is_empty());
    assert_eq!(first.stdout, second.stdout);
    let report: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(report["report_schema"], 1);
    assert_eq!(report["complete"], false);
    assert_eq!(report["uncovered"][0]["source_identity"], "mod:machine");
    let uncovered = report["uncovered"].as_array().unwrap();
    let associated = uncovered
        .iter()
        .find(|group| group.get("associated_block_entity").is_some())
        .unwrap();
    assert_eq!(associated["occurrence_count"], 2);
    assert_eq!(
        associated["associated_block_entity"]["snbt"],
        "{\n  Energy: 42L,\n  id: \"mod:machine_tile\"\n}"
    );
    assert_eq!(
        associated["locations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|location| location["block"][0].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    let unassociated = uncovered
        .iter()
        .find(|group| group.get("associated_block_entity").is_none())
        .unwrap();
    assert_eq!(unassociated["occurrence_count"], 5);
    assert_eq!(unassociated["locations"].as_array().unwrap().len(), 5);
    assert_eq!(report["counts"]["uncovered_occurrences"], 7);
    assert_eq!(fs::read(world.join("level.dat")).unwrap(), before_level);
    assert_eq!(
        fs::read(world.join("region/r.0.0.mca")).unwrap(),
        before_region
    );

    let matching_rules = root.path().join("matching.yaml");
    fs::write(
        &matching_rules,
        r#"{
          "schema_version": 1,
          "rule_set": "matching",
          "source_profile": "forge-1.7.10",
          "rules": [{
            "id": "test:machine",
            "object": "block",
            "matcher": {"name": "mod:machine"},
            "template": "{\"disposition\":\"unchanged\"}"
          }]
        }"#,
    )
    .unwrap();
    let report_path = root.path().join("coverage.json");
    let complete = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .args(["rules", "coverage", "--world"])
        .arg(&world)
        .arg("--rules")
        .arg(&matching_rules)
        .arg("--report")
        .arg(&report_path)
        .output()
        .unwrap();
    assert_eq!(complete.status.code(), Some(0));
    assert!(complete.stdout.is_empty());
    assert!(complete.stderr.is_empty());
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(report_path).unwrap()).unwrap();
    assert_eq!(report["complete"], true);
    for removed in [
        "input_fingerprint",
        "input_fingerprints",
        "files",
        "registry_mappings",
        "outcome",
        "objects",
        "staging_estimate",
    ] {
        assert!(
            report.get(removed).is_none(),
            "unexpected coverage field {removed}"
        );
    }

    let error = coverage_command(&root.path().join("missing"), &empty_rules);
    assert_eq!(error.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&error.stderr).contains("cannot resolve coverage world"));
}

#[test]
#[allow(clippy::too_many_lines)]
fn forge_1_2_5_profile_drives_coverage_conversion_and_unmapped_failures() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source-125");
    let template = root.path().join("template");
    coverage_world(&source, false);
    declare_forge_1_2_5_level(&source);
    empty_profile_world(&template, true);
    let before_level = fs::read(source.join("level.dat")).unwrap();
    let before_region = fs::read(source.join("region/r.0.0.mca")).unwrap();

    let incomplete = root.path().join("incomplete.yaml");
    fs::write(
        &incomplete,
        r#"{"schema_version":1,"rule_set":"incomplete","source_profile":"forge-1.2.5","rules":[]}"#,
    )
    .unwrap();
    let incomplete_coverage = coverage_command(&source, &incomplete);
    assert_eq!(incomplete_coverage.status.code(), Some(1));
    assert!(incomplete_coverage.stderr.is_empty());
    let incomplete_report: serde_json::Value =
        serde_json::from_slice(&incomplete_coverage.stdout).unwrap();
    assert_eq!(incomplete_report["report_schema"], 1);
    assert_eq!(incomplete_report["complete"], false);
    assert_eq!(incomplete_report["uncovered"][0]["kind"], "block");
    assert_eq!(
        incomplete_report["uncovered"][0]["source_identity"],
        "numeric:300"
    );
    assert_eq!(incomplete_report["uncovered"][0]["numeric_id"], 300);
    assert_eq!(
        incomplete_report["uncovered"][0]["diagnostic"],
        "missing source registry mapping"
    );
    assert!(incomplete_report["uncovered"][0]["locations"][0]["file"]
        .as_str()
        .unwrap()
        .ends_with("region/r.0.0.mca"));
    assert_eq!(
        incomplete_report["uncovered"][0]["locations"][0]["dimension"],
        "Overworld"
    );
    assert_eq!(
        incomplete_report["uncovered"][0]["locations"][0]["chunk"],
        serde_json::json!([0, 0])
    );
    assert_eq!(
        incomplete_report["uncovered"][0]["locations"][0]["block"],
        serde_json::json!([0, 0, 0])
    );
    let failed_output = root.path().join("failed-output");
    let failed = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .args(["convert", "--source"])
        .arg(&source)
        .arg("--template")
        .arg(&template)
        .arg("--output")
        .arg(&failed_output)
        .arg("--rules")
        .arg(&incomplete)
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(2));
    assert!(!failed_output.exists());
    let failed_diagnostic = String::from_utf8_lossy(&failed.stderr);
    assert!(
        failed_diagnostic.contains("block numeric ID 300"),
        "{failed_diagnostic}"
    );
    assert!(failed_diagnostic.contains("block"), "{failed_diagnostic}");
    assert!(
        failed_diagnostic.contains("r.0.0.mca"),
        "{failed_diagnostic}"
    );
    assert!(
        failed_diagnostic.contains("Overworld"),
        "{failed_diagnostic}"
    );
    assert!(
        failed_diagnostic.contains("chunk [0, 0]"),
        "{failed_diagnostic}"
    );
    assert!(
        failed_diagnostic.contains("block [0, 0, 0]"),
        "{failed_diagnostic}"
    );
    assert!(failed.stdout.is_empty());
    assert_eq!(fs::read(source.join("level.dat")).unwrap(), before_level);
    assert_eq!(
        fs::read(source.join("region/r.0.0.mca")).unwrap(),
        before_region
    );

    let rules = root.path().join("complete.yaml");
    fs::write(
        &rules,
        r#"{
          "schema_version":1,
          "rule_set":"pack-125",
          "source_profile":"forge-1.2.5",
          "source_manifest":[{"kind":"block","name":"mod:machine","numeric_id":300}],
          "target_manifest":[{"kind":"block","name":"mod:machine","numeric_id":500}],
          "rules":[{
            "id":"machine","object":"block",
            "matcher":{"name":"mod:machine"},
            "template":"{\"disposition\":\"unchanged\"}"
          }]
        }"#,
    )
    .unwrap();
    let coverage = coverage_command(&source, &rules);
    assert_eq!(
        coverage.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&coverage.stderr)
    );
    let coverage_report: serde_json::Value = serde_json::from_slice(&coverage.stdout).unwrap();
    assert_eq!(coverage_report["source_profile"], "forge-1.2.5");
    assert_eq!(coverage_report["complete"], true);

    let output = root.path().join("converted");
    let conversion = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .args(["convert", "--source"])
        .arg(&source)
        .arg("--template")
        .arg(&template)
        .arg("--output")
        .arg(&output)
        .arg("--rules")
        .arg(&rules)
        .output()
        .unwrap();
    assert!(
        conversion.status.success(),
        "{}",
        String::from_utf8_lossy(&conversion.stderr)
    );
    assert!(output.is_dir());
    assert!(conversion.stdout.is_empty());
    assert_eq!(fs::read(source.join("level.dat")).unwrap(), before_level);
    assert_eq!(
        fs::read(source.join("region/r.0.0.mca")).unwrap(),
        before_region
    );
}

#[test]
fn no_progress_is_a_global_option_and_preserves_machine_readable_stdout() {
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("world");
    fs::create_dir(&world).unwrap();
    coverage_world(&world, false);
    let rules = root.path().join("empty.yaml");
    fs::write(
        &rules,
        r#"{"schema_version":1,"rule_set":"empty","source_profile":"forge-1.7.10","rules":[]}"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .arg("--no-progress")
        .args(["rules", "coverage", "--world"])
        .arg(&world)
        .arg("--rules")
        .arg(&rules)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap();
}

#[test]
fn convert_failure_emits_context_without_a_report_and_refuses_publication() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let template = root.path().join("template");
    let output_world = root.path().join("output");
    let rules = root.path().join("empty.yaml");
    empty_profile_world(&source, false);
    empty_profile_world(&template, true);
    fs::write(source.join("region/r.0.0.mca"), b"malformed region").unwrap();
    fs::write(
        &rules,
        r#"{"schema_version":1,"rule_set":"empty","source_profile":"forge-1.7.10","rules":[]}"#,
    )
    .unwrap();

    let removed_report = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .args(["convert", "--source"])
        .arg(&source)
        .arg("--template")
        .arg(&template)
        .arg("--output")
        .arg(&output_world)
        .arg("--rules")
        .arg(&rules)
        .args(["--report", "removed.json"])
        .output()
        .unwrap();
    assert_eq!(removed_report.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&removed_report.stderr).contains("unexpected argument '--report'")
    );

    let result = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .args(["convert", "--source"])
        .arg(&source)
        .arg("--template")
        .arg(&template)
        .arg("--output")
        .arg(&output_world)
        .arg("--rules")
        .arg(&rules)
        .output()
        .unwrap();

    assert_eq!(result.status.code(), Some(2));
    assert!(!output_world.exists());
    assert!(root
        .path()
        .join(".output.minecraft-analysis-staging")
        .is_dir());
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("cannot convert region"));
}

#[test]
fn template_limit_failure_is_contextual_and_leaves_output_unpublished() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let template = root.path().join("template");
    let output = root.path().join("output");
    coverage_world(&source, false);
    empty_profile_world(&template, true);
    let rules = root.path().join("rules.yaml");
    fs::write(&rules, serde_json::json!({
        "schema_version": 1,
        "rule_set": "limit",
        "source_profile": "forge-1.7.10",
        "source_manifest": [{"kind":"block","name":"mod:machine","numeric_id":300}],
        "target_manifest": [{"kind":"block","name":"mod:machine","numeric_id":300}],
        "rules": [{
            "id":"limit:machine", "object":"block", "matcher":{"name":"mod:machine"},
            "template":"{% for value in range(200000) %} {% endfor %}{\"disposition\":\"unchanged\"}"
        }]
    }).to_string()).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_minecraft-analysis"))
        .args(["convert", "--source"])
        .arg(&source)
        .arg("--template")
        .arg(&template)
        .arg("--output")
        .arg(&output)
        .arg("--rules")
        .arg(&rules)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(!output.exists());
    let diagnostic = String::from_utf8_lossy(&result.stderr);
    assert!(diagnostic.contains("limit:machine"), "{diagnostic}");
    assert!(diagnostic.contains("region/r.0.0.mca"), "{diagnostic}");
    assert!(
        diagnostic.contains("fuel") || diagnostic.contains("limit"),
        "{diagnostic}"
    );
}

#[cfg(unix)]
#[test]
fn interactive_stderr_shows_analysis_progress_and_honors_disable() {
    if Command::new("script").arg("--version").output().is_err() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("world");
    fs::create_dir(&world).unwrap();
    coverage_world(&world, false);
    fs::create_dir_all(world.join("DIM7/region")).unwrap();
    fs::write(
        world.join("DIM7/region/r.1.0.mca"),
        RegionWriter::new().unwrap().finish().unwrap(),
    )
    .unwrap();
    let rules = root.path().join("empty.yaml");
    fs::write(
        &rules,
        r#"{"schema_version":1,"rule_set":"empty","source_profile":"forge-1.7.10","rules":[]}"#,
    )
    .unwrap();
    let executable = env!("CARGO_BIN_EXE_minecraft-analysis");
    let command = format!(
        "{executable} rules coverage --world {} --rules {}",
        world.display(),
        rules.display()
    );
    let output = Command::new("script")
        .args(["-qec", &command, "/dev/null"])
        .output()
        .unwrap();
    let terminal = String::from_utf8_lossy(&output.stdout);
    assert!(terminal.contains("Analysis"), "{terminal}");
    assert!(terminal.contains("2/2"), "{terminal}");
    assert!(terminal.contains("region files complete"), "{terminal}");
    assert!(!terminal.contains("Analysis finalization"), "{terminal}");
    assert!(!terminal.contains("Report generation"), "{terminal}");
    for unperformed in ["Conversion", "Verification", "Publication"] {
        assert!(!terminal.contains(unperformed), "{terminal}");
    }

    let quiet_command = format!(
        "{executable} --no-progress rules coverage --world {} --rules {}",
        world.display(),
        rules.display()
    );
    let quiet = Command::new("script")
        .args(["-qec", &quiet_command, "/dev/null"])
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&quiet.stdout).contains("Analysis ["));
}

#[cfg(unix)]
#[test]
fn interactive_convert_reports_every_performed_phase_in_order() {
    if Command::new("script").arg("--version").output().is_err() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let template = root.path().join("template");
    let output_world = root.path().join("output");
    let rules = root.path().join("empty.yaml");
    empty_profile_world(&source, false);
    empty_profile_world(&template, true);
    fs::write(
        &rules,
        r#"{"schema_version":1,"rule_set":"empty","source_profile":"forge-1.7.10","rules":[]}"#,
    )
    .unwrap();

    let executable = env!("CARGO_BIN_EXE_minecraft-analysis");
    let command = format!(
        "{executable} convert --source {} --template {} --output {} --rules {}",
        source.display(),
        template.display(),
        output_world.display(),
        rules.display()
    );
    let result = Command::new("script")
        .args(["-qec", &command, "/dev/null"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    let terminal = String::from_utf8_lossy(&result.stdout);
    let labels = ["Conversion", "Publication"];
    let mut previous = 0;
    for label in labels {
        let position = terminal[previous..].find(label).map_or_else(
            || panic!("missing {label}: {terminal}"),
            |position| previous + position,
        );
        previous = position;
    }
    for removed_phase in ["Analysis", "Analysis finalization", "Verification"] {
        assert!(!terminal.contains(removed_phase), "{terminal}");
    }
    assert!(terminal.contains("region files complete"), "{terminal}");
    assert!(output_world.is_dir());
}

#[cfg(unix)]
#[test]
fn interactive_explain_reports_only_targeted_work() {
    if Command::new("script").arg("--version").output().is_err() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let template = root.path().join("template");
    let rules = root.path().join("empty.yaml");
    coverage_world(&source, false);
    empty_profile_world(&template, true);
    fs::write(
        &rules,
        r#"{"schema_version":1,"rule_set":"empty","source_profile":"forge-1.7.10","rules":[]}"#,
    )
    .unwrap();
    let executable = env!("CARGO_BIN_EXE_minecraft-analysis");
    let explain_common = format!(
        "--source {} --template {} --rules {}",
        source.display(),
        template.display(),
        rules.display()
    );
    let commands = [format!(
        "{executable} explain {explain_common} --location 0,0,0"
    )];
    for command in commands {
        let result = Command::new("script")
            .args(["-qec", &command, "/dev/null"])
            .output()
            .unwrap();
        let terminal = String::from_utf8_lossy(&result.stdout);
        assert!(result.status.success(), "{command}: {terminal}");
        assert!(!terminal.contains("Report generation"), "{terminal}");
        assert!(terminal.contains("Targeted analysis"), "{terminal}");
        assert!(!terminal.contains("region files complete"), "{terminal}");
        assert!(!terminal.contains("Analysis finalization"), "{terminal}");
        for unperformed in ["Conversion", "Verification", "Publication"] {
            assert!(!terminal.contains(unperformed), "{terminal}");
        }
    }
}
