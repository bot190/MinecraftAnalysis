//! Version-aware import of runtime ID maps into rule manifests.

#![allow(clippy::result_large_err)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::registry::{RegistryCatalog, RegistryKind};
use crate::rules::{self, ManifestEntry, ManifestKind, RuleDocument, SourceProfile};

/// The rule manifest selected for update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManifestSide {
    Source,
    Target,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IdMapProfile {
    Forge1_2_5,
    Forge1_7_10,
    Forge1_12_2,
}

impl IdMapProfile {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Forge1_2_5 => "forge-1.2.5",
            Self::Forge1_7_10 => "forge-1.7.10",
            Self::Forge1_12_2 => "forge-1.12.2",
        }
    }
}

/// Counts reported after a successful manifest update.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct UpdateSummary {
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub vanilla_skipped: usize,
    pub duplicate_skipped: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot load rule graph rooted at {path}: {source}")]
    LoadRules { path: PathBuf, source: rules::Error },
    #[error("cannot read {kind} {path}: {source}")]
    Read {
        kind: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid JSON rule document {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("ID-map parsing is not supported for {0}")]
    UnsupportedProfile(&'static str),
    #[error("invalid Forge 1.2.5 ID-map record at line {line}: {detail}")]
    InvalidRecord { line: usize, detail: String },
    #[error("manifest assignment {kind:?} {name}={numeric_id} conflicts with {existing_name}")]
    ManifestConflict {
        kind: ManifestKind,
        name: String,
        numeric_id: i32,
        existing_name: String,
    },
    #[error("cannot create temporary rule document beside {path}: {source}")]
    CreateTemp {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot write temporary rule document beside {path}: {source}")]
    WriteTemp {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot serialize updated rule document: {0}")]
    Serialize(serde_json::Error),
    #[error("updated rule graph is invalid: {0}")]
    Validate(rules::Error),
    #[error("cannot atomically replace rule document {path}: {source}")]
    Replace {
        path: PathBuf,
        source: tempfile::PersistError,
    },
}

#[derive(Clone, Debug)]
struct ParsedMap {
    entries: Vec<ManifestEntry>,
    duplicate_skipped: usize,
}

/// Update one manifest in a rule document and atomically replace it.
///
/// # Errors
///
/// Returns an error without replacing `rule_path` when loading, parsing,
/// merging, validating, serializing, or installing the candidate fails.
pub fn update_manifest(
    rule_path: &Path,
    id_map_path: &Path,
    side: ManifestSide,
) -> Result<UpdateSummary, Error> {
    let loaded = rules::load(rule_path).map_err(|source| Error::LoadRules {
        path: rule_path.to_owned(),
        source,
    })?;
    let profile = selected_profile(side, loaded.source_profile);
    let bytes = fs::read(rule_path).map_err(|source| Error::Read {
        kind: "rule document",
        path: rule_path.to_owned(),
        source,
    })?;
    let mut document: RuleDocument =
        serde_json::from_slice(&bytes).map_err(|source| Error::Json {
            path: rule_path.to_owned(),
            source,
        })?;
    let id_map = fs::read_to_string(id_map_path).map_err(|source| Error::Read {
        kind: "ID map",
        path: id_map_path.to_owned(),
        source,
    })?;
    let parsed = parse(profile, &id_map)?;
    let mut summary = UpdateSummary {
        duplicate_skipped: parsed.duplicate_skipped,
        ..UpdateSummary::default()
    };
    let entries = filter_vanilla(profile, parsed.entries, &mut summary);
    let manifest = match side {
        ManifestSide::Source => &mut document.source_manifest,
        ManifestSide::Target => &mut document.target_manifest,
    };
    merge_manifest(manifest, entries, &mut summary)?;
    install_candidate(rule_path, &document)?;
    Ok(summary)
}

fn selected_profile(side: ManifestSide, source: SourceProfile) -> IdMapProfile {
    match side {
        ManifestSide::Target => IdMapProfile::Forge1_12_2,
        ManifestSide::Source => match source {
            SourceProfile::Forge1_2_5 => IdMapProfile::Forge1_2_5,
            SourceProfile::Forge1_7_10 => IdMapProfile::Forge1_7_10,
        },
    }
}

fn parse(profile: IdMapProfile, input: &str) -> Result<ParsedMap, Error> {
    match profile {
        IdMapProfile::Forge1_2_5 => parse_forge_1_2_5(input),
        unsupported => Err(Error::UnsupportedProfile(unsupported.as_str())),
    }
}

fn parse_forge_1_2_5(input: &str) -> Result<ParsedMap, Error> {
    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    let mut duplicate_skipped = 0;
    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        if line.is_empty() {
            continue;
        }
        let (kind, remainder) = if let Some(remainder) = line.strip_prefix("Block. Name: ") {
            (ManifestKind::Block, remainder)
        } else if let Some(remainder) = line.strip_prefix("Item. Name: ") {
            (ManifestKind::Item, remainder)
        } else {
            return Err(invalid_record(line_number, "unrecognized record kind"));
        };
        let Some((name, numeric)) = remainder.rsplit_once(". ID: ") else {
            return Err(invalid_record(line_number, "missing `. ID: ` delimiter"));
        };
        if name.is_empty() {
            return Err(invalid_record(line_number, "name is empty"));
        }
        let numeric_id = numeric.parse::<i32>().map_err(|_| {
            invalid_record(
                line_number,
                format!("invalid 32-bit numeric ID {numeric:?}"),
            )
        })?;
        let key = (kind_key(&kind), name.to_owned());
        if !seen.insert(key) {
            duplicate_skipped += 1;
            continue;
        }
        entries.push(ManifestEntry {
            kind,
            name: format!("legacy:{name}"),
            numeric_id,
            conflict: None,
        });
    }
    Ok(ParsedMap {
        entries,
        duplicate_skipped,
    })
}

fn invalid_record(line: usize, detail: impl Into<String>) -> Error {
    Error::InvalidRecord {
        line,
        detail: detail.into(),
    }
}

fn filter_vanilla(
    profile: IdMapProfile,
    entries: Vec<ManifestEntry>,
    summary: &mut UpdateSummary,
) -> Vec<ManifestEntry> {
    let catalog = match profile {
        IdMapProfile::Forge1_2_5 => RegistryCatalog::forge_1_2_5(),
        IdMapProfile::Forge1_7_10 | IdMapProfile::Forge1_12_2 => RegistryCatalog::default(),
    };
    entries
        .into_iter()
        .filter(|entry| {
            let vanilla = catalog
                .by_numeric(&registry_kind(&entry.kind), entry.numeric_id)
                .is_some();
            if vanilla {
                summary.vanilla_skipped += 1;
            }
            !vanilla
        })
        .collect()
}

fn merge_manifest(
    manifest: &mut Vec<ManifestEntry>,
    incoming: Vec<ManifestEntry>,
    summary: &mut UpdateSummary,
) -> Result<(), Error> {
    ensure_unique(manifest)?;
    for entry in incoming {
        if let Some(existing_index) = manifest
            .iter_mut()
            .position(|candidate| candidate.kind == entry.kind && candidate.name == entry.name)
        {
            if manifest[existing_index].numeric_id == entry.numeric_id {
                summary.unchanged += 1;
                continue;
            }
            if let Some(conflict) = manifest.iter().find(|candidate| {
                candidate.kind == entry.kind
                    && candidate.numeric_id == entry.numeric_id
                    && candidate.name != entry.name
            }) {
                return Err(conflict_error(&entry, &conflict.name));
            }
            let existing = &mut manifest[existing_index];
            existing.numeric_id = entry.numeric_id;
            existing.conflict = None;
            summary.updated += 1;
        } else {
            if let Some(conflict) = manifest.iter().find(|candidate| {
                candidate.kind == entry.kind && candidate.numeric_id == entry.numeric_id
            }) {
                return Err(conflict_error(&entry, &conflict.name));
            }
            manifest.push(entry);
            summary.added += 1;
        }
    }
    manifest.sort_by(|left, right| {
        kind_key(&left.kind)
            .cmp(&kind_key(&right.kind))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.numeric_id.cmp(&right.numeric_id))
    });
    Ok(())
}

fn ensure_unique(manifest: &[ManifestEntry]) -> Result<(), Error> {
    let mut names = BTreeMap::new();
    let mut numeric = BTreeMap::new();
    for entry in manifest {
        let kind = kind_key(&entry.kind);
        if let Some(existing_id) =
            names.insert((kind.clone(), entry.name.clone()), entry.numeric_id)
        {
            if existing_id != entry.numeric_id {
                return Err(conflict_error(entry, &entry.name));
            }
        }
        if let Some(existing_name) = numeric.insert((kind, entry.numeric_id), entry.name.clone()) {
            if existing_name != entry.name {
                return Err(conflict_error(entry, &existing_name));
            }
        }
    }
    Ok(())
}

fn conflict_error(entry: &ManifestEntry, existing_name: &str) -> Error {
    Error::ManifestConflict {
        kind: entry.kind.clone(),
        name: entry.name.clone(),
        numeric_id: entry.numeric_id,
        existing_name: existing_name.to_owned(),
    }
}

fn kind_key(kind: &ManifestKind) -> String {
    match kind {
        ManifestKind::Block => "block".to_owned(),
        ManifestKind::Item => "item".to_owned(),
        ManifestKind::Other(name) => format!("other:{name}"),
    }
}

fn registry_kind(kind: &ManifestKind) -> RegistryKind {
    match kind {
        ManifestKind::Block => RegistryKind::Block,
        ManifestKind::Item => RegistryKind::Item,
        ManifestKind::Other(name) => RegistryKind::Other(name.clone()),
    }
}

fn install_candidate(rule_path: &Path, document: &RuleDocument) -> Result<(), Error> {
    let parent = rule_path.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::Builder::new()
        .prefix(".minecraft-analysis-rules-")
        .suffix(".json")
        .tempfile_in(parent)
        .map_err(|source| Error::CreateTemp {
            path: rule_path.to_owned(),
            source,
        })?;
    serde_json::to_writer_pretty(&mut temporary, document).map_err(Error::Serialize)?;
    writeln!(temporary).map_err(|source| Error::WriteTemp {
        path: rule_path.to_owned(),
        source,
    })?;
    temporary.flush().map_err(|source| Error::WriteTemp {
        path: rule_path.to_owned(),
        source,
    })?;
    rules::load(temporary.path()).map_err(Error::Validate)?;
    temporary
        .persist(rule_path)
        .map_err(|source| Error::Replace {
            path: rule_path.to_owned(),
            source,
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ConflictSelection;

    #[test]
    fn parses_normalizes_deduplicates_and_preserves_kind_boundaries() {
        let parsed = parse_forge_1_2_5(
            "Block. Name: tile.machine.Block. ID: 153\n\
             Block. Name: tile.machine.Block. ID: 154\n\
             Item. Name: tile.machine.Block. ID: 30000\n\
             Block. Name: tile.Teleport Tether. ID: 189\n",
        )
        .unwrap();
        assert_eq!(parsed.duplicate_skipped, 1);
        assert_eq!(parsed.entries.len(), 3);
        assert_eq!(parsed.entries[0].name, "legacy:tile.machine.Block");
        assert_eq!(parsed.entries[0].numeric_id, 153);
        assert_eq!(parsed.entries[1].kind, ManifestKind::Item);
        assert_eq!(parsed.entries[2].name, "legacy:tile.Teleport Tether");
    }

    #[test]
    fn reports_strict_line_errors() {
        for (input, detail) in [
            ("Thing. Name: x. ID: 1", "line 1"),
            ("Block. Name: . ID: 1", "name is empty"),
            ("Item. Name: x. ID: 999999999999", "32-bit"),
        ] {
            assert!(parse_forge_1_2_5(input)
                .unwrap_err()
                .to_string()
                .contains(detail));
        }
    }

    #[test]
    fn filters_vanilla_by_kind_and_numeric_id() {
        let parsed =
            parse_forge_1_2_5("Block. Name: tile.stone. ID: 1\nItem. Name: mod:item. ID: 180\n")
                .unwrap();
        let mut summary = UpdateSummary::default();
        let filtered = filter_vanilla(IdMapProfile::Forge1_2_5, parsed.entries, &mut summary);
        assert_eq!(summary.vanilla_skipped, 1);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].kind, ManifestKind::Item);
    }

    #[test]
    fn merges_updates_and_sorts_while_clearing_conflict() {
        let mut manifest = vec![
            ManifestEntry {
                kind: ManifestKind::Item,
                name: "manual:kept".into(),
                numeric_id: 9,
                conflict: None,
            },
            ManifestEntry {
                kind: ManifestKind::Block,
                name: "legacy:updated".into(),
                numeric_id: 200,
                conflict: Some(ConflictSelection::Manifest),
            },
        ];
        let incoming = vec![
            ManifestEntry {
                kind: ManifestKind::Block,
                name: "legacy:updated".into(),
                numeric_id: 201,
                conflict: None,
            },
            ManifestEntry {
                kind: ManifestKind::Block,
                name: "legacy:added".into(),
                numeric_id: 202,
                conflict: None,
            },
        ];
        let mut summary = UpdateSummary::default();
        merge_manifest(&mut manifest, incoming, &mut summary).unwrap();
        assert_eq!(summary.updated, 1);
        assert_eq!(summary.added, 1);
        assert_eq!(manifest[0].name, "legacy:added");
        assert_eq!(manifest[1].name, "legacy:updated");
        assert_eq!(manifest[1].conflict, None);
        assert_eq!(manifest[2].name, "manual:kept");
    }

    #[test]
    fn rejects_numeric_conflicts() {
        let mut manifest = vec![ManifestEntry {
            kind: ManifestKind::Block,
            name: "manual:block".into(),
            numeric_id: 200,
            conflict: None,
        }];
        let incoming = vec![ManifestEntry {
            kind: ManifestKind::Block,
            name: "legacy:block".into(),
            numeric_id: 200,
            conflict: None,
        }];
        assert!(matches!(
            merge_manifest(&mut manifest, incoming, &mut UpdateSummary::default()),
            Err(Error::ManifestConflict { .. })
        ));
    }

    #[test]
    fn dispatches_only_to_available_parser() {
        assert!(parse(IdMapProfile::Forge1_2_5, "").is_ok());
        assert!(matches!(
            parse(IdMapProfile::Forge1_7_10, ""),
            Err(Error::UnsupportedProfile("forge-1.7.10"))
        ));
        assert!(matches!(
            parse(IdMapProfile::Forge1_12_2, ""),
            Err(Error::UnsupportedProfile("forge-1.12.2"))
        ));
    }

    #[test]
    fn failed_update_preserves_original_rule_bytes() {
        let root = tempfile::tempdir().unwrap();
        let rules = root.path().join("rules.json");
        let id_map = root.path().join("idmap.txt");
        let original = br#"{
  "schema_version": 2,
  "rule_set": "test",
  "source_profile": "forge-1.2.5",
  "rules": []
}
"#;
        fs::write(&rules, original).unwrap();
        fs::write(&id_map, "malformed\n").unwrap();
        assert!(matches!(
            update_manifest(&rules, &id_map, ManifestSide::Source),
            Err(Error::InvalidRecord { .. })
        ));
        assert_eq!(fs::read(&rules).unwrap(), original);
    }

    #[test]
    fn validation_and_installation_failures_do_not_replace_destination() {
        let root = tempfile::tempdir().unwrap();
        let rules = root.path().join("rules.json");
        let original = br#"{"schema_version":2,"rule_set":"valid","source_profile":"forge-1.2.5"}"#;
        fs::write(&rules, original).unwrap();
        let invalid: RuleDocument = serde_json::from_str(
            r#"{"schema_version":99,"rule_set":"invalid","source_profile":"forge-1.2.5"}"#,
        )
        .unwrap();
        assert!(matches!(
            install_candidate(&rules, &invalid),
            Err(Error::Validate(_))
        ));
        assert_eq!(fs::read(&rules).unwrap(), original);

        let destination_directory = root.path().join("destination-directory");
        fs::create_dir(&destination_directory).unwrap();
        fs::write(destination_directory.join("marker"), b"kept").unwrap();
        let valid: RuleDocument = serde_json::from_slice(original).unwrap();
        assert!(matches!(
            install_candidate(&destination_directory, &valid),
            Err(Error::Replace { .. })
        ));
        assert_eq!(
            fs::read(destination_directory.join("marker")).unwrap(),
            b"kept"
        );
    }
}
