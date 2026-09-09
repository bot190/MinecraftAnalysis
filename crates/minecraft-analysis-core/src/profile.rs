//! Explicit Forge source and Forge 1.12.2 target profiles.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::nbt::{self, Document, Value};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorldProfile {
    Forge1_2_5,
    Forge1_7_10,
    Forge1_12_2,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Evidence {
    pub path: String,
    pub observation: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DetectedWorld {
    pub profile: WorldProfile,
    pub level_dat: Document,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read world metadata {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot decode world metadata {path}: {source}")]
    Decode { path: PathBuf, source: nbt::Error },
    #[error("world {0} does not contain level.dat")]
    MissingLevelDat(PathBuf),
    #[error("world {world} has no Anvil .mca terrain under region/ or DIM*/region/")]
    MissingAnvilTerrain { world: PathBuf },
    #[error("unsupported world format: {diagnostics}")]
    Unsupported { diagnostics: String },
    #[error("ambiguous world format: evidence matches both Forge 1.7.10 and Forge 1.12.2")]
    Ambiguous,
    #[error("expected {expected:?}, but detected {actual:?}")]
    Unexpected {
        expected: WorldProfile,
        actual: WorldProfile,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// Audit information for target-owned fields adopted during metadata merging.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelMerge {
    pub document: Document,
    pub adopted_target_paths: Vec<String>,
}

/// Construct target metadata without replacing source gameplay state.
///
/// The source root is authoritative except for the target's complete `FML`
/// structure and the version-identifying `Data.DataVersion` and `Data.Version`
/// fields. Seed, time, spawn, game rules, player data, and unknown fields therefore
/// remain byte-type-equivalent to the source tree.
#[must_use]
pub fn merge_level_dat(source: &Document, target: &Document) -> LevelMerge {
    let mut document = source.clone();
    let mut adopted = Vec::new();
    if let Some(fml) = target.root.get("FML") {
        document.root.insert("FML".into(), fml.clone());
        adopted.push("FML".into());
    }
    if let Some(Value::Compound(target_data)) = target.root.get("Data") {
        let data = match document
            .root
            .entry("Data".into())
            .or_insert_with(|| Value::Compound(BTreeMap::new()))
        {
            Value::Compound(data) => data,
            value => {
                *value = Value::Compound(BTreeMap::new());
                let Value::Compound(data) = value else {
                    unreachable!()
                };
                data
            }
        };
        for field in ["DataVersion", "Version"] {
            if let Some(value) = target_data.get(field) {
                data.insert(field.into(), value.clone());
                adopted.push(format!("Data.{field}"));
            }
        }
    }
    LevelMerge {
        document,
        adopted_target_paths: adopted,
    }
}

/// Detect a supported world from explicit, persisted format evidence.
///
/// # Errors
///
/// Returns a contextual error when metadata or Anvil terrain is absent, malformed,
/// unsupported, or ambiguous.
pub fn detect_world(world: &Path) -> Result<DetectedWorld> {
    let level_path = world.join("level.dat");
    if !level_path.is_file() {
        return Err(Error::MissingLevelDat(level_path));
    }
    if !has_anvil_regions(world).map_err(|source| Error::Read {
        path: world.to_owned(),
        source,
    })? {
        return Err(Error::MissingAnvilTerrain {
            world: world.to_owned(),
        });
    }
    let bytes = fs::read(&level_path).map_err(|source| Error::Read {
        path: level_path.clone(),
        source,
    })?;
    let (level_dat, _) = nbt::decode(&bytes).map_err(|source| Error::Decode {
        path: level_path,
        source,
    })?;
    detect_document(level_dat)
}

/// Detect and require a particular endpoint profile.
///
/// # Errors
///
/// Returns the detection error or [`Error::Unexpected`] for the other endpoint.
pub fn detect_expected(world: &Path, expected: WorldProfile) -> Result<DetectedWorld> {
    if expected == WorldProfile::Forge1_2_5 {
        return validate_declared_forge_1_2_5(world);
    }
    let detected = detect_world(world)?;
    if detected.profile != expected {
        return Err(Error::Unexpected {
            expected,
            actual: detected.profile,
        });
    }
    Ok(detected)
}

/// Validate the structures required by the caller-asserted Forge 1.2.5 profile.
/// This intentionally performs no mod-list or exact-version detection.
///
/// # Errors
///
/// Returns a contextual error for missing or malformed metadata or Anvil terrain.
pub fn validate_declared_forge_1_2_5(world: &Path) -> Result<DetectedWorld> {
    let level_path = world.join("level.dat");
    if !level_path.is_file() {
        return Err(Error::MissingLevelDat(level_path));
    }
    if !has_anvil_regions(world).map_err(|source| Error::Read {
        path: world.to_owned(),
        source,
    })? {
        return Err(Error::MissingAnvilTerrain {
            world: world.to_owned(),
        });
    }
    let bytes = fs::read(&level_path).map_err(|source| Error::Read {
        path: level_path.clone(),
        source,
    })?;
    let (level_dat, _) = nbt::decode(&bytes).map_err(|source| Error::Decode {
        path: level_path,
        source,
    })?;
    if !matches!(level_dat.root.get("Data"), Some(Value::Compound(_))) {
        return Err(Error::Unsupported {
            diagnostics: "forge-1.2.5 requires a compound Data root in level.dat".into(),
        });
    }
    Ok(DetectedWorld {
        profile: WorldProfile::Forge1_2_5,
        level_dat,
        evidence: vec![Evidence {
            path: "rules.source_profile".into(),
            observation:
                "caller asserted forge-1.2.5; compatible level.dat and Anvil terrain validated"
                    .into(),
        }],
    })
}

fn detect_document(level_dat: Document) -> Result<DetectedWorld> {
    let fml = find_fml(&level_dat);
    let mut source = Vec::new();
    let mut target = Vec::new();
    if let Some(fml) = fml {
        if matches!(fml.get("ItemData"), Some(Value::List(_))) {
            source.push(Evidence {
                path: "FML.ItemData".into(),
                observation: "legacy prefixed block/item registry snapshot".into(),
            });
        }
        if matches!(fml.get("Registries"), Some(Value::Compound(_))) {
            target.push(Evidence {
                path: "FML.Registries".into(),
                observation: "generic Forge registry snapshots".into(),
            });
        }
        for (id, version) in mod_versions(fml) {
            if id.eq_ignore_ascii_case("FML") || id.eq_ignore_ascii_case("Forge") {
                if version.contains("1.7.10") || version.starts_with("7.10.") {
                    source.push(Evidence {
                        path: "FML.ModList".into(),
                        observation: format!("{id} version {version}"),
                    });
                }
                if version.contains("1.12.2")
                    || version.starts_with("8.0.")
                    || version.starts_with("14.23.")
                {
                    target.push(Evidence {
                        path: "FML.ModList".into(),
                        observation: format!("{id} version {version}"),
                    });
                }
            }
        }
    }
    if data_version(&level_dat) == Some(1343) {
        target.push(Evidence {
            path: "Data.DataVersion".into(),
            observation: "1343 (Minecraft 1.12.2)".into(),
        });
    }

    match (!source.is_empty(), !target.is_empty()) {
        (true, false) => Ok(DetectedWorld {
            profile: WorldProfile::Forge1_7_10,
            level_dat,
            evidence: source,
        }),
        (false, true) if target.iter().any(|item| item.path == "FML.Registries") => {
            Ok(DetectedWorld {
                profile: WorldProfile::Forge1_12_2,
                level_dat,
                evidence: target,
            })
        }
        (true, true) => Err(Error::Ambiguous),
        _ => Err(Error::Unsupported {
            diagnostics:
                "expected FML.ItemData for 1.7.10 or FML.Registries for 1.12.2; neither was present"
                    .into(),
        }),
    }
}

fn find_fml(document: &Document) -> Option<&BTreeMap<String, Value>> {
    match document.root.get("FML") {
        Some(Value::Compound(value)) => Some(value),
        _ => match document.root.get("Data") {
            Some(Value::Compound(data)) => match data.get("FML") {
                Some(Value::Compound(value)) => Some(value),
                _ => None,
            },
            _ => None,
        },
    }
}

fn data_version(document: &Document) -> Option<i32> {
    match document.root.get("Data") {
        Some(Value::Compound(data)) => match data.get("DataVersion") {
            Some(Value::Int(value)) => Some(*value),
            _ => None,
        },
        _ => None,
    }
}

fn mod_versions(fml: &BTreeMap<String, Value>) -> Vec<(&str, &str)> {
    let Some(Value::List(list)) = fml.get("ModList") else {
        return Vec::new();
    };
    list.values
        .iter()
        .filter_map(|value| {
            let Value::Compound(value) = value else {
                return None;
            };
            match (value.get("ModId"), value.get("ModVersion")) {
                (Some(Value::String(id)), Some(Value::String(version))) => {
                    Some((id.as_str(), version.as_str()))
                }
                _ => None,
            }
        })
        .collect()
}

fn has_anvil_regions(world: &Path) -> std::io::Result<bool> {
    let mut directories = vec![world.join("region")];
    for entry in fs::read_dir(world)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.file_name().to_string_lossy().starts_with("DIM") {
            directories.push(entry.path().join("region"));
        }
    }
    for directory in directories {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_some_and(|extension| extension == "mca") {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::{List, Tag};

    fn document(fml: BTreeMap<String, Value>, data_version: Option<i32>) -> Document {
        let mut data = BTreeMap::new();
        if let Some(version) = data_version {
            data.insert("DataVersion".into(), Value::Int(version));
        }
        Document {
            root_name: String::new(),
            root: BTreeMap::from([
                ("FML".into(), Value::Compound(fml)),
                ("Data".into(), Value::Compound(data)),
            ]),
        }
    }

    #[test]
    fn detects_both_supported_profiles_from_registry_evidence() {
        let empty_compounds = || {
            Value::List(List {
                element_tag: Tag::Compound,
                values: Vec::new(),
            })
        };
        let source = detect_document(document(
            BTreeMap::from([("ItemData".into(), empty_compounds())]),
            None,
        ))
        .unwrap();
        assert_eq!(source.profile, WorldProfile::Forge1_7_10);
        let target = detect_document(document(
            BTreeMap::from([("Registries".into(), Value::Compound(BTreeMap::new()))]),
            Some(1343),
        ))
        .unwrap();
        assert_eq!(target.profile, WorldProfile::Forge1_12_2);
    }

    #[test]
    fn rejects_unsupported_and_ambiguous_documents() {
        assert!(matches!(
            detect_document(document(BTreeMap::new(), None)),
            Err(Error::Unsupported { .. })
        ));
        let both = document(
            BTreeMap::from([
                (
                    "ItemData".into(),
                    Value::List(List {
                        element_tag: Tag::Compound,
                        values: Vec::new(),
                    }),
                ),
                ("Registries".into(), Value::Compound(BTreeMap::new())),
            ]),
            Some(1343),
        );
        assert!(matches!(detect_document(both), Err(Error::Ambiguous)));
    }

    #[test]
    fn level_merge_adopts_only_target_registry_and_version_structures() {
        let source = Document {
            root_name: "source".into(),
            root: BTreeMap::from([
                (
                    "FML".into(),
                    Value::Compound(BTreeMap::from([("ItemData".into(), Value::Int(1))])),
                ),
                (
                    "Data".into(),
                    Value::Compound(BTreeMap::from([
                        ("RandomSeed".into(), Value::Long(42)),
                        ("Time".into(), Value::Long(9001)),
                        ("DataVersion".into(), Value::Int(0)),
                        ("UnknownModState".into(), Value::Byte(7)),
                    ])),
                ),
            ]),
        };
        let target_fml = Value::Compound(BTreeMap::from([(
            "Registries".into(),
            Value::Compound(BTreeMap::new()),
        )]));
        let target = Document {
            root_name: "target".into(),
            root: BTreeMap::from([
                ("FML".into(), target_fml.clone()),
                (
                    "Data".into(),
                    Value::Compound(BTreeMap::from([
                        ("DataVersion".into(), Value::Int(1343)),
                        (
                            "Version".into(),
                            Value::Compound(BTreeMap::from([("Id".into(), Value::Int(1343))])),
                        ),
                        ("RandomSeed".into(), Value::Long(999)),
                    ])),
                ),
            ]),
        };
        let merged = merge_level_dat(&source, &target);
        assert_eq!(merged.document.root_name, "source");
        assert_eq!(merged.document.root.get("FML"), Some(&target_fml));
        let Value::Compound(data) = merged.document.root.get("Data").unwrap() else {
            panic!()
        };
        assert_eq!(data.get("RandomSeed"), Some(&Value::Long(42)));
        assert_eq!(data.get("Time"), Some(&Value::Long(9001)));
        assert_eq!(data.get("UnknownModState"), Some(&Value::Byte(7)));
        assert_eq!(data.get("DataVersion"), Some(&Value::Int(1343)));
        assert_eq!(
            merged.adopted_target_paths,
            vec!["FML", "Data.DataVersion", "Data.Version"]
        );
    }

    #[test]
    fn declared_forge_1_2_5_validates_structure_without_version_heuristics() {
        let temporary = tempfile::tempdir().unwrap();
        let world = temporary.path();
        fs::create_dir(world.join("region")).unwrap();
        fs::write(world.join("region/r.0.0.mca"), []).unwrap();
        let level = Document {
            root_name: String::new(),
            root: BTreeMap::from([(
                "Data".into(),
                Value::Compound(BTreeMap::from([(
                    "UnknownPackMarker".into(),
                    Value::String("caller-owned".into()),
                )])),
            )]),
        };
        fs::write(
            world.join("level.dat"),
            nbt::encode(&level, nbt::Compression::Gzip).unwrap(),
        )
        .unwrap();
        let validated = validate_declared_forge_1_2_5(world).unwrap();
        assert_eq!(validated.profile, WorldProfile::Forge1_2_5);
        assert_eq!(validated.evidence[0].path, "rules.source_profile");

        fs::remove_file(world.join("region/r.0.0.mca")).unwrap();
        assert!(matches!(
            validate_declared_forge_1_2_5(world),
            Err(Error::MissingAnvilTerrain { .. })
        ));
    }
}
