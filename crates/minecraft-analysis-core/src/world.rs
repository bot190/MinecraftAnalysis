//! Safe world path validation and deterministic file discovery.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use walkdir::WalkDir;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SafePaths {
    pub source: PathBuf,
    pub template: PathBuf,
    pub output: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorldInventory {
    pub dimensions: Vec<Dimension>,
    pub player_data: Vec<PathBuf>,
    pub standalone_nbt: Vec<PathBuf>,
    pub auxiliary_files: Vec<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dimension {
    pub id: DimensionId,
    pub directory: PathBuf,
    pub regions: Vec<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum DimensionId {
    Overworld,
    Nether,
    End,
    Modded(String),
}

impl DimensionId {
    #[must_use]
    pub(crate) fn directory(&self) -> Option<&str> {
        match self {
            Self::Overworld => None,
            Self::Nether => Some("DIM-1"),
            Self::End => Some("DIM1"),
            Self::Modded(value) => Some(value),
        }
    }
}

impl FromStr for DimensionId {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "overworld" => Ok(Self::Overworld),
            "nether" => Ok(Self::Nether),
            "end" => Ok(Self::End),
            value
                if value.starts_with("DIM")
                    && value.len() > 3
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')) =>
            {
                Ok(Self::Modded(value.to_owned()))
            }
            _ => Err(format!(
                "invalid dimension {value:?}; expected overworld, nether, end, or a safe DIM... directory name"
            )),
        }
    }
}

/// A world-global block coordinate and its directly derived Anvil address.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorldCoordinateAddress {
    pub chunk: [i32; 2],
    pub region: [i32; 2],
    pub local_chunk: [usize; 2],
}

impl WorldCoordinateAddress {
    #[must_use]
    pub fn new(block: [i32; 3]) -> Self {
        let chunk = [block[0].div_euclid(16), block[2].div_euclid(16)];
        Self {
            chunk,
            region: [chunk[0].div_euclid(32), chunk[1].div_euclid(32)],
            local_chunk: [
                chunk[0].rem_euclid(32) as usize,
                chunk[1].rem_euclid(32) as usize,
            ],
        }
    }

    #[must_use]
    pub fn relative_region_path(&self, dimension: &DimensionId) -> PathBuf {
        let file = format!("r.{}.{}.mca", self.region[0], self.region[1]);
        dimension.directory().map_or_else(
            || PathBuf::from("region").join(file.clone()),
            |directory| PathBuf::from(directory).join("region").join(&file),
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot resolve {role} path {path}: {source}")]
    Resolve {
        role: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("source and template resolve to the same directory {0}")]
    SameInputs(PathBuf),
    #[error("unsafe output {output}: it equals or contains {input_role} {input}")]
    OutputContainsInput {
        output: PathBuf,
        input_role: &'static str,
        input: PathBuf,
    },
    #[error("unsafe output {output}: it is inside {input_role} {input}")]
    OutputInsideInput {
        output: PathBuf,
        input_role: &'static str,
        input: PathBuf,
    },
    #[error("output already exists: {0}")]
    OutputExists(PathBuf),
    #[error("cannot inspect world path {path}: {source}")]
    Inspect {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// Resolve and validate all caller paths before preflight or staging.
///
/// # Errors
///
/// Returns an error for missing inputs, aliases, overlapping trees, an existing
/// output, or an output whose nearest existing parent cannot be canonicalized.
pub fn validate_paths(source: &Path, template: &Path, output: &Path) -> Result<SafePaths> {
    let source = fs::canonicalize(source).map_err(|source_error| Error::Resolve {
        role: "source",
        path: source.to_owned(),
        source: source_error,
    })?;
    let template = fs::canonicalize(template).map_err(|source_error| Error::Resolve {
        role: "template",
        path: template.to_owned(),
        source: source_error,
    })?;
    if source == template {
        return Err(Error::SameInputs(source));
    }
    if output.exists() {
        return Err(Error::OutputExists(output.to_owned()));
    }
    let output = canonicalize_missing(output).map_err(|source_error| Error::Resolve {
        role: "output",
        path: output.to_owned(),
        source: source_error,
    })?;
    check_overlap(&output, &source, "source")?;
    check_overlap(&output, &template, "template")?;
    Ok(SafePaths {
        source,
        template,
        output,
    })
}

fn check_overlap(output: &Path, input: &Path, input_role: &'static str) -> Result<()> {
    if input.starts_with(output) {
        return Err(Error::OutputContainsInput {
            output: output.to_owned(),
            input_role,
            input: input.to_owned(),
        });
    }
    if output.starts_with(input) {
        return Err(Error::OutputInsideInput {
            output: output.to_owned(),
            input_role,
            input: input.to_owned(),
        });
    }
    Ok(())
}

fn canonicalize_missing(path: &Path) -> std::io::Result<PathBuf> {
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "unresolved parent component",
        ));
    }
    let mut missing = Vec::new();
    let mut existing = path;
    while !existing.exists() {
        let name = existing.file_name().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "output has no existing ancestor",
            )
        })?;
        missing.push(name.to_owned());
        existing = existing.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "output has no existing ancestor",
            )
        })?;
    }
    let mut resolved = fs::canonicalize(existing)?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

/// Discover supported world files in stable lexical order.
///
/// # Errors
///
/// Returns a contextual filesystem error while traversing the world.
pub fn discover(world: &Path) -> Result<WorldInventory> {
    let mut dimensions = Vec::new();
    for (id, directory) in dimension_directories(world)? {
        let region_dir = directory.join("region");
        let regions = sorted_files_with_extension(&region_dir, "mca")?;
        if !regions.is_empty() {
            dimensions.push(Dimension {
                id,
                directory,
                regions,
            });
        }
    }
    dimensions.sort_by(|a, b| a.id.cmp(&b.id));

    let mut player_data = Vec::new();
    for directory in [world.join("playerdata"), world.join("players")] {
        player_data.extend(sorted_files_with_extension(&directory, "dat")?);
    }
    player_data.sort();

    let known = BTreeSet::from([
        PathBuf::from("level.dat"),
        PathBuf::from("level.dat_old"),
        PathBuf::from("session.lock"),
    ]);
    let mut standalone_nbt = Vec::new();
    let mut auxiliary_files = Vec::new();
    for entry in WalkDir::new(world).follow_links(false).sort_by_file_name() {
        let entry = entry.map_err(|error| Error::Inspect {
            path: error.path().unwrap_or(world).to_owned(),
            source: error.into(),
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(relative) = entry.path().strip_prefix(world) else {
            continue;
        };
        let relative = relative.to_owned();
        if known.contains(&relative)
            || relative.starts_with("region")
            || relative
                .components()
                .any(|part| part.as_os_str() == "playerdata" || part.as_os_str() == "players")
            || relative
                .extension()
                .is_some_and(|extension| extension == "mca")
        {
            continue;
        }
        if relative
            .extension()
            .is_some_and(|extension| extension == "dat")
        {
            standalone_nbt.push(relative);
        } else {
            auxiliary_files.push(relative);
        }
    }
    Ok(WorldInventory {
        dimensions,
        player_data,
        standalone_nbt,
        auxiliary_files,
    })
}

fn dimension_directories(world: &Path) -> Result<Vec<(DimensionId, PathBuf)>> {
    let mut result = vec![(DimensionId::Overworld, world.to_owned())];
    let entries = fs::read_dir(world).map_err(|source| Error::Inspect {
        path: world.to_owned(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| Error::Inspect {
            path: world.to_owned(),
            source,
        })?;
        if !entry
            .file_type()
            .map_err(|source| Error::Inspect {
                path: entry.path(),
                source,
            })?
            .is_dir()
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let id = match name.as_str() {
            "DIM-1" => Some(DimensionId::Nether),
            "DIM1" => Some(DimensionId::End),
            value if value.starts_with("DIM") => Some(DimensionId::Modded(value.to_owned())),
            _ => None,
        };
        if let Some(id) = id {
            result.push((id, entry.path()));
        }
    }
    Ok(result)
}

fn sorted_files_with_extension(directory: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Ok(Vec::new());
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| Error::Inspect {
            path: directory.to_owned(),
            source,
        })?;
        if entry
            .file_type()
            .map_err(|source| Error::Inspect {
                path: entry.path(),
                source,
            })?
            .is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|value| value == extension)
        {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_coordinate_address_uses_euclidean_boundaries() {
        let cases = [
            (1, 0, 0, 0),
            (-1, -1, -1, 31),
            (-16, -1, -1, 31),
            (-17, -2, -1, 30),
            (-512, -32, -1, 0),
            (-513, -33, -2, 31),
        ];
        for (block, chunk, region, local) in cases {
            let address = WorldCoordinateAddress::new([block, 64, block]);
            assert_eq!(address.chunk, [chunk, chunk]);
            assert_eq!(address.region, [region, region]);
            assert_eq!(address.local_chunk, [local, local]);
        }
        assert_eq!(
            WorldCoordinateAddress::new([-513, 0, 1]).relative_region_path(&DimensionId::Nether),
            PathBuf::from("DIM-1/region/r.-2.0.mca")
        );
    }

    #[test]
    fn dimensions_parse_only_named_vanilla_and_safe_modded_directories() {
        assert_eq!("overworld".parse(), Ok(DimensionId::Overworld));
        assert_eq!("nether".parse(), Ok(DimensionId::Nether));
        assert_eq!("end".parse(), Ok(DimensionId::End));
        assert_eq!(
            "DIM42_test".parse(),
            Ok(DimensionId::Modded("DIM42_test".into()))
        );
        for invalid in ["DIM", "dimensions/mod", "DIM../escape", "Overworld"] {
            assert!(invalid.parse::<DimensionId>().is_err(), "{invalid}");
        }
    }

    #[test]
    fn rejects_aliasing_and_overlapping_paths() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let template = root.path().join("template");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&template).unwrap();
        assert!(validate_paths(&source, &template, &source.join("output")).is_err());
        assert!(validate_paths(&source, &template, root.path()).is_err());
        let safe = validate_paths(&source, &template, &root.path().join("output")).unwrap();
        assert_eq!(safe.output, root.path().join("output"));
    }

    #[test]
    fn discovers_vanilla_and_mod_dimensions_deterministically() {
        let root = tempfile::tempdir().unwrap();
        for directory in [
            "region",
            "DIM-1/region",
            "DIM1/region",
            "DIM7/region",
            "playerdata",
            "data",
        ] {
            fs::create_dir_all(root.path().join(directory)).unwrap();
        }
        for file in [
            "region/r.1.0.mca",
            "region/r.0.0.mca",
            "DIM-1/region/r.0.0.mca",
            "DIM1/region/r.0.0.mca",
            "DIM7/region/r.0.0.mca",
            "playerdata/user.dat",
            "data/map_0.dat",
            "icon.png",
        ] {
            fs::write(root.path().join(file), []).unwrap();
        }
        let found = discover(root.path()).unwrap();
        assert_eq!(
            found
                .dimensions
                .iter()
                .map(|dimension| &dimension.id)
                .collect::<Vec<_>>(),
            vec![
                &DimensionId::Overworld,
                &DimensionId::Nether,
                &DimensionId::End,
                &DimensionId::Modded("DIM7".into())
            ]
        );
        assert!(found.dimensions[0].regions[0].ends_with("r.0.0.mca"));
        assert_eq!(found.player_data.len(), 1);
        assert_eq!(found.standalone_nbt, vec![PathBuf::from("data/map_0.dat")]);
    }
}
