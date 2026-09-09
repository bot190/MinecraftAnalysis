use std::fs;
use std::path::{Path, PathBuf};

use miette::miette;
use minecraft_analysis_core::nbt::{self, Document, Value};
use minecraft_analysis_core::registry::{self, RegistryCatalog, VanillaVersion};
use minecraft_analysis_core::rules::{self, SourceProfile};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ViewProfile {
    Forge1_2_5,
    Forge1_7_10,
    Forge1_12_2,
}

impl ViewProfile {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Forge1_2_5 => "forge-1.2.5",
            Self::Forge1_7_10 => "forge-1.7.10",
            Self::Forge1_12_2 => "forge-1.12.2",
        }
    }

    const fn vanilla(self) -> VanillaVersion {
        match self {
            Self::Forge1_2_5 => VanillaVersion::Minecraft1_2_5,
            Self::Forge1_7_10 => VanillaVersion::Minecraft1_7_10,
            Self::Forge1_12_2 => VanillaVersion::Minecraft1_12_2,
        }
    }
}

impl From<SourceProfile> for ViewProfile {
    fn from(value: SourceProfile) -> Self {
        match value {
            SourceProfile::Forge1_2_5 => Self::Forge1_2_5,
            SourceProfile::Forge1_7_10 => Self::Forge1_7_10,
        }
    }
}

#[derive(Clone, Debug)]
pub struct IdentityContext {
    pub catalog: RegistryCatalog,
    pub profile: ViewProfile,
    pub assumed: bool,
    pub sources: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn load(region: &Path, rule_paths: &[PathBuf]) -> miette::Result<IdentityContext> {
    let loaded_rules = if rule_paths.is_empty() {
        None
    } else {
        Some(rules::load_many(rule_paths).map_err(|error| miette!(error.to_string()))?)
    };
    let (world_path, world_document, mut warnings) = load_nearest_level(region);
    let world = world_document
        .as_ref()
        .and_then(|document| extract_world(document, world_path.as_deref(), &mut warnings));
    let rule_profile = loaded_rules
        .as_ref()
        .map(|rules| ViewProfile::from(rules.source_profile));
    if let (Some(rule), Some((world_profile, _))) = (rule_profile, world.as_ref()) {
        if rule != *world_profile {
            return Err(miette!(
                "rule profile {} conflicts with world registry profile {} in {}",
                rule.label(),
                world_profile.label(),
                world_path.as_deref().unwrap_or(region).display()
            ));
        }
    }
    let (profile, assumed) = rule_profile
        .map(|profile| (profile, false))
        .or_else(|| world.as_ref().map(|(profile, _)| (*profile, false)))
        .unwrap_or((ViewProfile::Forge1_7_10, true));
    let has_world_registry = world.is_some();
    let mut catalog = world.map_or_else(RegistryCatalog::default, |(_, catalog)| catalog);
    let mut sources = Vec::new();
    if let (true, Some(path)) = (has_world_registry, world_path) {
        sources.push(path.display().to_string());
    }
    if let Some(loaded) = &loaded_rules {
        for (path, document) in &loaded.documents {
            rules::apply_manifest(&mut catalog, &document.source_manifest, "source rules")
                .map_err(|error| {
                    miette!(
                        "cannot apply source manifest from {}: {error}",
                        path.display()
                    )
                })?;
            sources.push(path.display().to_string());
        }
    }
    catalog.add_vanilla_fallbacks(profile.vanilla());
    sources.push(format!("vanilla {}", profile.label()));
    Ok(IdentityContext {
        catalog,
        profile,
        assumed,
        sources,
        warnings,
    })
}

fn load_nearest_level(region: &Path) -> (Option<PathBuf>, Option<Document>, Vec<String>) {
    let mut warnings = Vec::new();
    for ancestor in region.parent().into_iter().flat_map(Path::ancestors) {
        let candidate = ancestor.join("level.dat");
        let metadata = match fs::metadata(&candidate) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                warnings.push(format!("cannot inspect {}: {error}", candidate.display()));
                return (Some(candidate), None, warnings);
            }
        };
        if !metadata.is_file() {
            warnings.push(format!("{} is not a regular file", candidate.display()));
            return (Some(candidate), None, warnings);
        }
        return match fs::read(&candidate)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                nbt::decode(&bytes)
                    .map(|(document, _)| document)
                    .map_err(|error| error.to_string())
            }) {
            Ok(document) => (Some(candidate), Some(document), warnings),
            Err(error) => {
                warnings.push(format!("cannot use {}: {error}", candidate.display()));
                (Some(candidate), None, warnings)
            }
        };
    }
    warnings.push("no ancestor level.dat found; block registry profile is assumed".into());
    (None, None, warnings)
}

fn extract_world(
    document: &Document,
    path: Option<&Path>,
    warnings: &mut Vec<String>,
) -> Option<(ViewProfile, RegistryCatalog)> {
    let fml = document
        .root
        .get("FML")
        .or_else(|| match document.root.get("Data") {
            Some(Value::Compound(data)) => data.get("FML"),
            _ => None,
        });
    let Some(Value::Compound(fml)) = fml else {
        warnings.push(format!(
            "{} has no supported Forge registry",
            path.unwrap_or(Path::new("level.dat")).display()
        ));
        return None;
    };
    let result = if fml.contains_key("Registries") {
        registry::extract_forge_1_12_2(document).map(|catalog| (ViewProfile::Forge1_12_2, catalog))
    } else if fml.contains_key("ItemData") {
        registry::extract_forge_1_7_10(document).map(|catalog| (ViewProfile::Forge1_7_10, catalog))
    } else {
        warnings.push(format!(
            "{} has an unsupported Forge registry shape",
            path.unwrap_or(Path::new("level.dat")).display()
        ));
        return None;
    };
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            warnings.push(format!(
                "cannot use registry in {}: {error}",
                path.unwrap_or(Path::new("level.dat")).display()
            ));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use minecraft_analysis_core::nbt::{List, Tag};
    use tempfile::tempdir;

    fn level_document() -> Document {
        Document {
            root_name: String::new(),
            root: std::collections::BTreeMap::from([(
                "FML".into(),
                Value::Compound(std::collections::BTreeMap::from([(
                    "ItemData".into(),
                    Value::List(List {
                        element_tag: Tag::Compound,
                        values: vec![],
                    }),
                )])),
            )]),
        }
    }

    #[test]
    fn nearest_ancestor_level_is_selected_for_nested_dimension() {
        let temp = tempdir().unwrap();
        let world = temp.path().join("world");
        let region = world.join("dimensions/mod/name/region/r.0.0.mca");
        fs::create_dir_all(region.parent().unwrap()).unwrap();
        fs::write(
            world.join("level.dat"),
            nbt::encode(&level_document(), nbt::Compression::Gzip).unwrap(),
        )
        .unwrap();
        let context = load(&region, &[]).unwrap();
        assert_eq!(context.profile, ViewProfile::Forge1_7_10);
        assert!(!context.assumed);
        assert!(context.sources[0].ends_with("world/level.dat"));
    }

    #[test]
    fn missing_level_uses_labeled_default() {
        let temp = tempdir().unwrap();
        let context = load(&temp.path().join("region/r.0.0.mca"), &[]).unwrap();
        assert!(context.assumed);
        assert!(!context.warnings.is_empty());
    }

    #[test]
    fn rule_manifest_supplements_world_and_selects_profile() {
        let temp = tempdir().unwrap();
        let world = temp.path().join("world");
        let region = world.join("region/r.0.0.mca");
        fs::create_dir_all(region.parent().unwrap()).unwrap();
        fs::write(
            world.join("level.dat"),
            nbt::encode(&level_document(), nbt::Compression::Gzip).unwrap(),
        )
        .unwrap();
        let rules = temp.path().join("rules.json");
        fs::write(&rules, r#"{"schema_version":2,"rule_set":"viewer","source_profile":"forge-1.7.10","source_manifest":[{"kind":"block","name":"mod:machine","numeric_id":300}]}"#).unwrap();
        let context = load(&region, &[rules]).unwrap();
        assert_eq!(
            context
                .catalog
                .by_numeric(&registry::RegistryKind::Block, 300)
                .unwrap()
                .name
                .as_str(),
            "mod:machine"
        );
        assert!(!context.assumed);
    }

    #[test]
    fn invalid_explicit_rules_fail_before_viewer_context_is_ready() {
        let temp = tempdir().unwrap();
        let rules = temp.path().join("bad.json");
        fs::write(&rules, "not json").unwrap();
        assert!(load(&temp.path().join("region/r.0.0.mca"), &[rules]).is_err());
    }

    #[test]
    fn nearer_unusable_level_is_not_bypassed() {
        let temp = tempdir().unwrap();
        let outer = temp.path().join("level.dat");
        fs::write(
            &outer,
            nbt::encode(&level_document(), nbt::Compression::Gzip).unwrap(),
        )
        .unwrap();
        let inner = temp.path().join("world/DIM-1");
        fs::create_dir_all(inner.join("region")).unwrap();
        fs::write(inner.join("level.dat"), b"broken").unwrap();
        let context = load(&inner.join("region/r.0.0.mca"), &[]).unwrap();
        assert!(context.assumed);
        assert!(context
            .warnings
            .iter()
            .any(|warning| warning.contains("DIM-1/level.dat")));
    }

    #[test]
    fn explicit_rules_cannot_contradict_world_registry_profile() {
        let temp = tempdir().unwrap();
        let world = temp.path().join("world");
        let region = world.join("region/r.0.0.mca");
        fs::create_dir_all(region.parent().unwrap()).unwrap();
        let level = Document {
            root_name: String::new(),
            root: std::collections::BTreeMap::from([(
                "FML".into(),
                Value::Compound(std::collections::BTreeMap::from([(
                    "Registries".into(),
                    Value::Compound(std::collections::BTreeMap::new()),
                )])),
            )]),
        };
        fs::write(
            world.join("level.dat"),
            nbt::encode(&level, nbt::Compression::Gzip).unwrap(),
        )
        .unwrap();
        let rules = temp.path().join("rules.json");
        fs::write(
            &rules,
            r#"{"schema_version":2,"rule_set":"viewer","source_profile":"forge-1.7.10"}"#,
        )
        .unwrap();
        let error = load(&region, &[rules]).unwrap_err().to_string();
        assert!(error.contains("forge-1.7.10"));
        assert!(error.contains("forge-1.12.2"));
    }
}
