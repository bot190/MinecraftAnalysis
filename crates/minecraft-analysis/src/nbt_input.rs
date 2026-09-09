use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use clap::Args;
use miette::{miette, IntoDiagnostic};
use minecraft_analysis_core::nbt::{self, Document};
use minecraft_analysis_core::region::{ChunkCompression, RegionReader, DEFAULT_MAX_CHUNK_BYTES};
use minecraft_analysis_core::world::{DimensionId, WorldCoordinateAddress};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Coordinates {
    pub x: i32,
    pub z: i32,
}

impl FromStr for Coordinates {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (x, z) = value
            .split_once(',')
            .ok_or_else(|| "expected coordinates in x,z form".to_owned())?;
        if z.contains(',') || x.is_empty() || z.is_empty() {
            return Err("expected exactly two coordinates in x,z form".into());
        }
        Ok(Self {
            x: x.parse()
                .map_err(|_| format!("invalid x coordinate {x:?}"))?,
            z: z.parse()
                .map_err(|_| format!("invalid z coordinate {z:?}"))?,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Args)]
pub struct ChunkSelectorArgs {
    /// Global chunk coordinates in x,z form.
    #[arg(
        long,
        value_name = "X,Z",
        conflicts_with = "local_chunk",
        allow_hyphen_values = true
    )]
    pub chunk: Option<Coordinates>,
    /// Region-local chunk coordinates in x,z form (each 0 through 31).
    #[arg(
        long,
        value_name = "X,Z",
        conflicts_with = "chunk",
        allow_hyphen_values = true
    )]
    pub local_chunk: Option<Coordinates>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChunkSelection {
    pub region: Coordinates,
    pub global: Coordinates,
    pub local: Coordinates,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Source {
    Standalone {
        path: PathBuf,
        compression: nbt::Compression,
    },
    RegionChunk {
        path: PathBuf,
        compression: ChunkCompression,
        selection: ChunkSelection,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoadedDocument {
    pub document: Document,
    pub source: Source,
}

pub fn load_world_chunk(
    world: &Path,
    dimension: &DimensionId,
    coordinate: [i32; 3],
) -> miette::Result<LoadedDocument> {
    let address = WorldCoordinateAddress::new(coordinate);
    let relative = address.relative_region_path(dimension);
    let path = world.join(&relative);
    let bytes = read_regular_file(&path).map_err(|error| {
        miette!(
            "cannot read world {} dimension {} region {} for coordinate ({},{},{}): {error}",
            world.display(),
            dimension_label(dimension),
            relative.display(),
            coordinate[0],
            coordinate[1],
            coordinate[2]
        )
    })?;
    let selection = ChunkSelection {
        region: Coordinates {
            x: address.region[0],
            z: address.region[1],
        },
        global: Coordinates {
            x: address.chunk[0],
            z: address.chunk[1],
        },
        local: Coordinates {
            x: i32::try_from(address.local_chunk[0]).expect("local chunk x fits i32"),
            z: i32::try_from(address.local_chunk[1]).expect("local chunk z fits i32"),
        },
    };
    load_region(&path, &bytes, Some(selection.global), None).map_err(|error| {
        miette!(
            "cannot load world {} dimension {} region ({},{}) chunk ({},{}) local ({},{}) for coordinate ({},{},{}): {error}",
            world.display(), dimension_label(dimension), selection.region.x, selection.region.z,
            selection.global.x, selection.global.z, selection.local.x, selection.local.z,
            coordinate[0], coordinate[1], coordinate[2]
        )
    })
}

pub fn dimension_label(dimension: &DimensionId) -> &str {
    match dimension {
        DimensionId::Overworld => "overworld",
        DimensionId::Nether => "nether",
        DimensionId::End => "end",
        DimensionId::Modded(value) => value,
    }
}

pub fn load(path: &Path, selector: ChunkSelectorArgs) -> miette::Result<LoadedDocument> {
    let bytes = read_regular_file(path)?;
    match (selector.chunk, selector.local_chunk) {
        (None, None) => {
            let (document, compression) = nbt::decode(&bytes)
                .map_err(|error| miette!("cannot decode NBT file {}: {error}", path.display()))?;
            Ok(LoadedDocument {
                document,
                source: Source::Standalone {
                    path: path.into(),
                    compression,
                },
            })
        }
        (global, local) => load_region(path, &bytes, global, local),
    }
}

fn read_regular_file(path: &Path) -> miette::Result<Vec<u8>> {
    let metadata = fs::metadata(path)
        .map_err(|error| miette!("cannot read NBT file {}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(miette!(
            "cannot read NBT file {}: not a regular file",
            path.display()
        ));
    }
    fs::read(path).map_err(|error| miette!("cannot read NBT file {}: {error}", path.display()))
}

fn load_region(
    path: &Path,
    bytes: &[u8],
    global: Option<Coordinates>,
    local: Option<Coordinates>,
) -> miette::Result<LoadedDocument> {
    let region = parse_region_filename(path)?;
    let selection = match (global, local) {
        (Some(global), None) => selection_from_global(region, global)?,
        (None, Some(local)) => selection_from_local(region, local)?,
        _ => return Err(miette!("exactly one chunk selector is required")),
    };
    let context = selection_context(path, selection);
    let reader = RegionReader::new(bytes, DEFAULT_MAX_CHUNK_BYTES)
        .map_err(|error| miette!("cannot read {context}: {error}"))?;
    let local_x = usize::try_from(selection.local.x).expect("canonical local x is nonnegative");
    let local_z = usize::try_from(selection.local.z).expect("canonical local z is nonnegative");
    let chunk = reader
        .read_chunk_with_metadata(local_x, local_z)
        .map_err(|error| miette!("cannot read {context}: {error}"))?
        .ok_or_else(|| miette!("selected {context} is absent"))?;
    let document = nbt::decode_uncompressed(&chunk.bytes)
        .map_err(|error| miette!("cannot decode {context}: {error}"))?;
    Ok(LoadedDocument {
        document,
        source: Source::RegionChunk {
            path: path.into(),
            compression: chunk.compression,
            selection,
        },
    })
}

fn parse_region_filename(path: &Path) -> miette::Result<Coordinates> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            miette!(
                "cannot select a chunk from {}: invalid region filename",
                path.display()
            )
        })?;
    let parts = name.split('.').collect::<Vec<_>>();
    if parts.len() != 4 || parts[0] != "r" || parts[3] != "mca" {
        return Err(miette!(
            "cannot select a chunk from {}: expected region filename r.<x>.<z>.mca",
            path.display()
        ));
    }
    Ok(Coordinates {
        x: parts[1]
            .parse()
            .into_diagnostic()
            .map_err(|_| miette!("invalid region x coordinate in {name:?}"))?,
        z: parts[2]
            .parse()
            .into_diagnostic()
            .map_err(|_| miette!("invalid region z coordinate in {name:?}"))?,
    })
}

fn selection_from_global(
    region: Coordinates,
    global: Coordinates,
) -> miette::Result<ChunkSelection> {
    let actual = Coordinates {
        x: global.x.div_euclid(32),
        z: global.z.div_euclid(32),
    };
    if actual != region {
        return Err(miette!(
            "global chunk ({},{}) belongs to region r.{}.{}.mca, not r.{}.{}.mca",
            global.x,
            global.z,
            actual.x,
            actual.z,
            region.x,
            region.z
        ));
    }
    Ok(ChunkSelection {
        region,
        global,
        local: Coordinates {
            x: global.x.rem_euclid(32),
            z: global.z.rem_euclid(32),
        },
    })
}

fn selection_from_local(region: Coordinates, local: Coordinates) -> miette::Result<ChunkSelection> {
    if !(0..32).contains(&local.x) || !(0..32).contains(&local.z) {
        return Err(miette!(
            "local chunk coordinates ({},{}) must each be in the range 0 through 31",
            local.x,
            local.z
        ));
    }
    let global_x = region
        .x
        .checked_mul(32)
        .and_then(|base| base.checked_add(local.x));
    let global_z = region
        .z
        .checked_mul(32)
        .and_then(|base| base.checked_add(local.z));
    let global = Coordinates {
        x: global_x
            .ok_or_else(|| miette!("global x coordinate derived from region is out of range"))?,
        z: global_z
            .ok_or_else(|| miette!("global z coordinate derived from region is out of range"))?,
    };
    Ok(ChunkSelection {
        region,
        global,
        local,
    })
}

pub fn selection_context(path: &Path, selection: ChunkSelection) -> String {
    format!(
        "region chunk in {} at global ({},{}) / local ({},{})",
        path.display(),
        selection.global.x,
        selection.global.z,
        selection.local.x,
        selection.local.z
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_conversion_uses_euclidean_coordinates() {
        let selection =
            selection_from_global(Coordinates { x: -2, z: -1 }, Coordinates { x: -33, z: -1 })
                .unwrap();
        assert_eq!(selection.local, Coordinates { x: 31, z: 31 });
        assert!(
            selection_from_global(Coordinates { x: -1, z: 0 }, Coordinates { x: -33, z: 0 })
                .is_err()
        );
    }

    #[test]
    fn local_conversion_checks_range_and_overflow() {
        let selection =
            selection_from_local(Coordinates { x: -1, z: 1 }, Coordinates { x: 31, z: 0 }).unwrap();
        assert_eq!(selection.global, Coordinates { x: -1, z: 32 });
        assert!(
            selection_from_local(Coordinates { x: 0, z: 0 }, Coordinates { x: 32, z: 0 }).is_err()
        );
    }
}
