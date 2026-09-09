//! World-local Forge registry catalogs and identity resolution.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::nbt::{Document, List, Tag, Value};

/// The semantic registry containing an entry.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum RegistryKind {
    Block,
    Item,
    Other(String),
}

impl RegistryKind {
    fn from_112_name(name: &str) -> Self {
        match name {
            "minecraft:blocks" => Self::Block,
            "minecraft:items" => Self::Item,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// A namespaced registry identity. Mod namespaces are deliberately unrestricted.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RegistryName(String);

impl RegistryName {
    /// Parse a persisted `namespace:path` identity.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidName`] for a missing/empty namespace or path.
    pub fn parse(value: &str) -> Result<Self, Error> {
        let Some((namespace, path)) = value.split_once(':') else {
            return Err(Error::InvalidName(value.to_owned()));
        };
        if namespace.is_empty() || path.is_empty() {
            return Err(Error::InvalidName(value.to_owned()));
        }
        Ok(Self(value.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.0[..self.0.find(':').unwrap_or(self.0.len())]
    }
}

impl fmt::Display for RegistryName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Where a catalog fact came from.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Provenance {
    pub source: String,
    pub detail: String,
}

impl Provenance {
    #[must_use]
    pub fn world(profile: &str, detail: impl Into<String>) -> Self {
        Self {
            source: profile.to_owned(),
            detail: detail.into(),
        }
    }

    #[must_use]
    pub fn built_in(profile: &str) -> Self {
        Self {
            source: profile.to_owned(),
            detail: "verified vanilla registry table".to_owned(),
        }
    }
}

/// The built-in vanilla registry table to use when a Forge snapshot omitted an
/// otherwise well-known vanilla assignment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VanillaVersion {
    Minecraft1_2_5,
    Minecraft1_7_10,
    Minecraft1_12_2,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryEntry {
    pub kind: RegistryKind,
    pub name: RegistryName,
    pub numeric_id: i32,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Alias {
    pub kind: RegistryKind,
    pub from: RegistryName,
    pub to: RegistryName,
    pub provenance: Provenance,
}

/// A deterministic registry snapshot, indexed in both directions.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RegistryCatalog {
    entries: BTreeMap<(RegistryKind, RegistryName), RegistryEntry>,
    numeric: BTreeMap<(RegistryKind, i32), RegistryName>,
    aliases: BTreeMap<(RegistryKind, RegistryName), Alias>,
    pub blocked: BTreeMap<RegistryKind, BTreeSet<i32>>,
    pub dummied: BTreeMap<RegistryKind, BTreeSet<RegistryName>>,
    pub overrides: BTreeMap<(RegistryKind, RegistryName), String>,
}

impl RegistryCatalog {
    /// Add an identity while rejecting contradictory name or numeric assignments.
    ///
    /// # Errors
    ///
    /// Returns a contradiction error if either side of the bidirectional mapping conflicts.
    pub fn insert(&mut self, entry: RegistryEntry) -> Result<(), Error> {
        let identity_key = (entry.kind.clone(), entry.name.clone());
        if let Some(existing) = self.entries.get(&identity_key) {
            if existing.numeric_id == entry.numeric_id {
                return Ok(());
            }
            return Err(Error::ContradictoryIdentity {
                kind: entry.kind,
                name: entry.name,
                first: existing.numeric_id,
                second: entry.numeric_id,
            });
        }
        let numeric_key = (entry.kind.clone(), entry.numeric_id);
        if let Some(existing) = self.numeric.get(&numeric_key) {
            if existing != &entry.name {
                return Err(Error::DuplicateNumericId {
                    kind: entry.kind,
                    id: entry.numeric_id,
                    first: existing.clone(),
                    second: entry.name,
                });
            }
        }
        self.numeric.insert(numeric_key, entry.name.clone());
        self.entries.insert(identity_key, entry);
        Ok(())
    }

    /// Add an alias while rejecting contradictory targets.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ContradictoryAlias`] when the source already has another target.
    pub fn insert_alias(&mut self, alias: Alias) -> Result<(), Error> {
        let key = (alias.kind.clone(), alias.from.clone());
        if let Some(existing) = self.aliases.get(&key) {
            if existing.to == alias.to {
                return Ok(());
            }
            return Err(Error::ContradictoryAlias {
                kind: alias.kind,
                from: alias.from,
                first: existing.to.clone(),
                second: alias.to,
            });
        }
        self.aliases.insert(key, alias);
        Ok(())
    }

    /// Deterministically merge another catalog.
    ///
    /// # Errors
    ///
    /// Returns an error for any contradictory mapping or override.
    pub fn merge(&mut self, other: Self) -> Result<(), Error> {
        for entry in other.entries.into_values() {
            self.insert(entry)?;
        }
        for alias in other.aliases.into_values() {
            self.insert_alias(alias)?;
        }
        for (kind, ids) in other.blocked {
            self.blocked.entry(kind).or_default().extend(ids);
        }
        for (kind, names) in other.dummied {
            self.dummied.entry(kind).or_default().extend(names);
        }
        for (key, owner) in other.overrides {
            if let Some(first) = self.overrides.insert(key.clone(), owner.clone()) {
                if first != owner {
                    return Err(Error::ContradictoryOverride {
                        key,
                        first,
                        second: owner,
                    });
                }
            }
        }
        Ok(())
    }

    /// Replace both sides of an assignment after an external policy has explicitly
    /// selected it over contradictory evidence.
    pub fn replace_explicit(&mut self, entry: RegistryEntry) {
        let identity_key = (entry.kind.clone(), entry.name.clone());
        if let Some(previous) = self.entries.remove(&identity_key) {
            self.numeric.remove(&(previous.kind, previous.numeric_id));
        }
        let numeric_key = (entry.kind.clone(), entry.numeric_id);
        if let Some(previous_name) = self.numeric.remove(&numeric_key) {
            self.entries.remove(&(entry.kind.clone(), previous_name));
        }
        self.numeric.insert(numeric_key, entry.name.clone());
        self.entries.insert(identity_key, entry);
    }

    #[must_use]
    pub fn by_numeric(&self, kind: &RegistryKind, id: i32) -> Option<&RegistryEntry> {
        let name = self.numeric.get(&(kind.clone(), id))?;
        self.entries.get(&(kind.clone(), name.clone()))
    }

    #[must_use]
    pub fn by_name(&self, kind: &RegistryKind, name: &RegistryName) -> Option<&RegistryEntry> {
        let mut current = name;
        let mut visited = BTreeSet::new();
        while let Some(alias) = self.aliases.get(&(kind.clone(), current.clone())) {
            if !visited.insert(current.clone()) {
                return None;
            }
            current = &alias.to;
        }
        self.entries.get(&(kind.clone(), current.clone()))
    }

    pub fn entries(&self) -> impl Iterator<Item = &RegistryEntry> {
        self.entries.values()
    }

    pub fn aliases(&self) -> impl Iterator<Item = &Alias> {
        self.aliases.values()
    }

    /// Fill absent vanilla entries from a version-specific verified table.
    ///
    /// Existing evidence always wins. In particular, a numeric slot occupied by
    /// a mod entry is never reinterpreted as vanilla, and an existing vanilla
    /// name is never reassigned to its built-in numeric ID.
    pub fn add_vanilla_fallbacks(&mut self, version: VanillaVersion) {
        let profile = match version {
            VanillaVersion::Minecraft1_2_5 => "minecraft-1.2.5",
            VanillaVersion::Minecraft1_7_10 => "minecraft-1.7.10",
            VanillaVersion::Minecraft1_12_2 => "minecraft-1.12.2",
        };
        let entries = match version {
            VanillaVersion::Minecraft1_2_5 => VANILLA_1_2_5.to_vec(),
            VanillaVersion::Minecraft1_7_10 => legacy_base_blocks()
                .chain(
                    COMMON_VANILLA
                        .iter()
                        .copied()
                        .filter(|(kind, _, _)| matches!(kind, VanillaKind::Item)),
                )
                .chain(VANILLA_1_7_10.iter().copied())
                .collect(),
            VanillaVersion::Minecraft1_12_2 => legacy_base_blocks()
                .chain(
                    COMMON_VANILLA
                        .iter()
                        .copied()
                        .filter(|(kind, _, _)| matches!(kind, VanillaKind::Item)),
                )
                .chain(VANILLA_1_7_10.iter().copied())
                .chain(VANILLA_1_12_2.iter().copied())
                .collect(),
        };
        for (kind, name, numeric_id) in entries {
            let kind = match kind {
                VanillaKind::Block => RegistryKind::Block,
                VanillaKind::Item => RegistryKind::Item,
            };
            let name = RegistryName((*name).to_owned());
            if self.by_numeric(&kind, numeric_id).is_some() || self.by_name(&kind, &name).is_some()
            {
                continue;
            }
            let result = self.insert(RegistryEntry {
                kind,
                name,
                numeric_id,
                provenance: Provenance::built_in(profile),
            });
            debug_assert!(result.is_ok(), "absence checks prevent contradictions");
        }
    }

    /// Construct the authoritative built-in Forge 1.2.5 source catalog.
    #[must_use]
    pub fn forge_1_2_5() -> Self {
        let mut catalog = Self::default();
        catalog.add_vanilla_fallbacks(VanillaVersion::Minecraft1_2_5);
        let block_items: Vec<_> = catalog
            .entries()
            .filter(|entry| entry.kind == RegistryKind::Block && entry.numeric_id > 0)
            .cloned()
            .collect();
        for entry in block_items {
            if catalog.by_name(&RegistryKind::Item, &entry.name).is_some() {
                continue;
            }
            let result = catalog.insert(RegistryEntry {
                kind: RegistryKind::Item,
                name: entry.name,
                numeric_id: entry.numeric_id,
                provenance: entry.provenance,
            });
            debug_assert!(result.is_ok(), "verified item assignments do not conflict");
        }
        catalog
    }
}

fn legacy_base_blocks() -> impl Iterator<Item = (VanillaKind, &'static str, i32)> {
    VANILLA_1_2_5
        .iter()
        .copied()
        .filter(|(kind, _, id)| matches!(kind, VanillaKind::Block) && *id != 95)
}

#[derive(Clone, Copy)]
enum VanillaKind {
    Block,
    Item,
}

// Verified against the corresponding Mojang/Forge bootstrap registries. This
// deliberately small recovery table contains stable baseline identities; world
// snapshots remain authoritative and can be expanded without changing policy.
const COMMON_VANILLA: &[(VanillaKind, &str, i32)] = &[
    (VanillaKind::Block, "minecraft:air", 0),
    (VanillaKind::Block, "minecraft:stone", 1),
    (VanillaKind::Block, "minecraft:grass", 2),
    (VanillaKind::Block, "minecraft:dirt", 3),
    (VanillaKind::Block, "minecraft:cobblestone", 4),
    (VanillaKind::Block, "minecraft:planks", 5),
    (VanillaKind::Item, "minecraft:stone", 1),
    (VanillaKind::Item, "minecraft:grass", 2),
    (VanillaKind::Item, "minecraft:dirt", 3),
    (VanillaKind::Item, "minecraft:cobblestone", 4),
    (VanillaKind::Item, "minecraft:planks", 5),
    (VanillaKind::Item, "minecraft:iron_shovel", 256),
    (VanillaKind::Item, "minecraft:iron_pickaxe", 257),
    (VanillaKind::Item, "minecraft:iron_axe", 258),
];

const VANILLA_1_7_10: &[(VanillaKind, &str, i32)] = &[
    (VanillaKind::Block, "minecraft:stained_glass", 95),
    (VanillaKind::Block, "minecraft:double_wooden_slab", 125),
    (VanillaKind::Block, "minecraft:wooden_slab", 126),
    (VanillaKind::Block, "minecraft:cocoa", 127),
    (VanillaKind::Block, "minecraft:sandstone_stairs", 128),
    (VanillaKind::Block, "minecraft:emerald_ore", 129),
    (VanillaKind::Block, "minecraft:ender_chest", 130),
    (VanillaKind::Block, "minecraft:tripwire_hook", 131),
    (VanillaKind::Block, "minecraft:tripwire", 132),
    (VanillaKind::Block, "minecraft:emerald_block", 133),
    (VanillaKind::Block, "minecraft:spruce_stairs", 134),
    (VanillaKind::Block, "minecraft:birch_stairs", 135),
    (VanillaKind::Block, "minecraft:jungle_stairs", 136),
    (VanillaKind::Block, "minecraft:command_block", 137),
    (VanillaKind::Block, "minecraft:beacon", 138),
    (VanillaKind::Block, "minecraft:cobblestone_wall", 139),
    (VanillaKind::Block, "minecraft:flower_pot", 140),
    (VanillaKind::Block, "minecraft:carrots", 141),
    (VanillaKind::Block, "minecraft:potatoes", 142),
    (VanillaKind::Block, "minecraft:wooden_button", 143),
    (VanillaKind::Block, "minecraft:skull", 144),
    (VanillaKind::Block, "minecraft:anvil", 145),
    (VanillaKind::Block, "minecraft:trapped_chest", 146),
    (
        VanillaKind::Block,
        "minecraft:light_weighted_pressure_plate",
        147,
    ),
    (
        VanillaKind::Block,
        "minecraft:heavy_weighted_pressure_plate",
        148,
    ),
    (VanillaKind::Block, "minecraft:unpowered_comparator", 149),
    (VanillaKind::Block, "minecraft:powered_comparator", 150),
    (VanillaKind::Block, "minecraft:daylight_detector", 151),
    (VanillaKind::Block, "minecraft:redstone_block", 152),
    (VanillaKind::Block, "minecraft:quartz_ore", 153),
    (VanillaKind::Block, "minecraft:hopper", 154),
    (VanillaKind::Block, "minecraft:quartz_block", 155),
    (VanillaKind::Block, "minecraft:quartz_stairs", 156),
    (VanillaKind::Block, "minecraft:activator_rail", 157),
    (VanillaKind::Block, "minecraft:dropper", 158),
    (VanillaKind::Block, "minecraft:stained_hardened_clay", 159),
    (VanillaKind::Block, "minecraft:stained_glass_pane", 160),
    (VanillaKind::Block, "minecraft:leaves2", 161),
    (VanillaKind::Block, "minecraft:log2", 162),
    (VanillaKind::Block, "minecraft:acacia_stairs", 163),
    (VanillaKind::Block, "minecraft:dark_oak_stairs", 164),
    (VanillaKind::Block, "minecraft:hay_block", 170),
    (VanillaKind::Block, "minecraft:carpet", 171),
    (VanillaKind::Block, "minecraft:hardened_clay", 172),
    (VanillaKind::Block, "minecraft:coal_block", 173),
    (VanillaKind::Block, "minecraft:packed_ice", 174),
    (VanillaKind::Block, "minecraft:double_plant", 175),
];

const VANILLA_1_12_2: &[(VanillaKind, &str, i32)] = &[
    (VanillaKind::Block, "minecraft:slime", 165),
    (VanillaKind::Block, "minecraft:barrier", 166),
    (VanillaKind::Block, "minecraft:iron_trapdoor", 167),
    (VanillaKind::Block, "minecraft:prismarine", 168),
    (VanillaKind::Block, "minecraft:sea_lantern", 169),
    (VanillaKind::Block, "minecraft:standing_banner", 176),
    (VanillaKind::Block, "minecraft:wall_banner", 177),
    (
        VanillaKind::Block,
        "minecraft:daylight_detector_inverted",
        178,
    ),
    (VanillaKind::Block, "minecraft:red_sandstone", 179),
    (VanillaKind::Block, "minecraft:red_sandstone_stairs", 180),
    (VanillaKind::Block, "minecraft:double_stone_slab2", 181),
    (VanillaKind::Block, "minecraft:stone_slab2", 182),
    (VanillaKind::Block, "minecraft:spruce_fence_gate", 183),
    (VanillaKind::Block, "minecraft:birch_fence_gate", 184),
    (VanillaKind::Block, "minecraft:jungle_fence_gate", 185),
    (VanillaKind::Block, "minecraft:dark_oak_fence_gate", 186),
    (VanillaKind::Block, "minecraft:acacia_fence_gate", 187),
    (VanillaKind::Block, "minecraft:spruce_fence", 188),
    (VanillaKind::Block, "minecraft:birch_fence", 189),
    (VanillaKind::Block, "minecraft:jungle_fence", 190),
    (VanillaKind::Block, "minecraft:dark_oak_fence", 191),
    (VanillaKind::Block, "minecraft:acacia_fence", 192),
    (VanillaKind::Block, "minecraft:spruce_door", 193),
    (VanillaKind::Block, "minecraft:birch_door", 194),
    (VanillaKind::Block, "minecraft:jungle_door", 195),
    (VanillaKind::Block, "minecraft:acacia_door", 196),
    (VanillaKind::Block, "minecraft:dark_oak_door", 197),
    (VanillaKind::Block, "minecraft:end_rod", 198),
    (VanillaKind::Block, "minecraft:chorus_plant", 199),
    (VanillaKind::Block, "minecraft:chorus_flower", 200),
    (VanillaKind::Block, "minecraft:purpur_block", 201),
    (VanillaKind::Block, "minecraft:purpur_pillar", 202),
    (VanillaKind::Block, "minecraft:purpur_stairs", 203),
    (VanillaKind::Block, "minecraft:purpur_double_slab", 204),
    (VanillaKind::Block, "minecraft:purpur_slab", 205),
    (VanillaKind::Block, "minecraft:end_bricks", 206),
    (VanillaKind::Block, "minecraft:beetroots", 207),
    (VanillaKind::Block, "minecraft:grass_path", 208),
    (VanillaKind::Block, "minecraft:end_gateway", 209),
    (VanillaKind::Block, "minecraft:repeating_command_block", 210),
    (VanillaKind::Block, "minecraft:chain_command_block", 211),
    (VanillaKind::Block, "minecraft:frosted_ice", 212),
    (VanillaKind::Block, "minecraft:magma", 213),
    (VanillaKind::Block, "minecraft:nether_wart_block", 214),
    (VanillaKind::Block, "minecraft:red_nether_brick", 215),
    (VanillaKind::Block, "minecraft:bone_block", 216),
    (VanillaKind::Block, "minecraft:structure_void", 217),
    (VanillaKind::Block, "minecraft:observer", 218),
    (VanillaKind::Block, "minecraft:white_shulker_box", 219),
    (VanillaKind::Block, "minecraft:orange_shulker_box", 220),
    (VanillaKind::Block, "minecraft:magenta_shulker_box", 221),
    (VanillaKind::Block, "minecraft:light_blue_shulker_box", 222),
    (VanillaKind::Block, "minecraft:yellow_shulker_box", 223),
    (VanillaKind::Block, "minecraft:lime_shulker_box", 224),
    (VanillaKind::Block, "minecraft:pink_shulker_box", 225),
    (VanillaKind::Block, "minecraft:gray_shulker_box", 226),
    (VanillaKind::Block, "minecraft:silver_shulker_box", 227),
    (VanillaKind::Block, "minecraft:cyan_shulker_box", 228),
    (VanillaKind::Block, "minecraft:purple_shulker_box", 229),
    (VanillaKind::Block, "minecraft:blue_shulker_box", 230),
    (VanillaKind::Block, "minecraft:brown_shulker_box", 231),
    (VanillaKind::Block, "minecraft:green_shulker_box", 232),
    (VanillaKind::Block, "minecraft:red_shulker_box", 233),
    (VanillaKind::Block, "minecraft:black_shulker_box", 234),
    (VanillaKind::Block, "minecraft:white_glazed_terracotta", 235),
    (
        VanillaKind::Block,
        "minecraft:orange_glazed_terracotta",
        236,
    ),
    (
        VanillaKind::Block,
        "minecraft:magenta_glazed_terracotta",
        237,
    ),
    (
        VanillaKind::Block,
        "minecraft:light_blue_glazed_terracotta",
        238,
    ),
    (
        VanillaKind::Block,
        "minecraft:yellow_glazed_terracotta",
        239,
    ),
    (VanillaKind::Block, "minecraft:lime_glazed_terracotta", 240),
    (VanillaKind::Block, "minecraft:pink_glazed_terracotta", 241),
    (VanillaKind::Block, "minecraft:gray_glazed_terracotta", 242),
    (
        VanillaKind::Block,
        "minecraft:silver_glazed_terracotta",
        243,
    ),
    (VanillaKind::Block, "minecraft:cyan_glazed_terracotta", 244),
    (
        VanillaKind::Block,
        "minecraft:purple_glazed_terracotta",
        245,
    ),
    (VanillaKind::Block, "minecraft:blue_glazed_terracotta", 246),
    (VanillaKind::Block, "minecraft:brown_glazed_terracotta", 247),
    (VanillaKind::Block, "minecraft:green_glazed_terracotta", 248),
    (VanillaKind::Block, "minecraft:red_glazed_terracotta", 249),
    (VanillaKind::Block, "minecraft:black_glazed_terracotta", 250),
    (VanillaKind::Block, "minecraft:concrete", 251),
    (VanillaKind::Block, "minecraft:concrete_powder", 252),
    (VanillaKind::Block, "minecraft:structure_block", 255),
];

// Minecraft 1.2.5 numeric registries, transcribed from the vanilla 1.2.5
// Block/Item bootstrap assignments. Block and item namespaces are deliberately
// separate: placeable block-items share numbers with their block identities.
const VANILLA_1_2_5: &[(VanillaKind, &str, i32)] = &[
    (VanillaKind::Block, "minecraft:air", 0),
    (VanillaKind::Block, "minecraft:stone", 1),
    (VanillaKind::Block, "minecraft:grass", 2),
    (VanillaKind::Block, "minecraft:dirt", 3),
    (VanillaKind::Block, "minecraft:cobblestone", 4),
    (VanillaKind::Block, "minecraft:planks", 5),
    (VanillaKind::Block, "minecraft:sapling", 6),
    (VanillaKind::Block, "minecraft:bedrock", 7),
    (VanillaKind::Block, "minecraft:flowing_water", 8),
    (VanillaKind::Block, "minecraft:water", 9),
    (VanillaKind::Block, "minecraft:flowing_lava", 10),
    (VanillaKind::Block, "minecraft:lava", 11),
    (VanillaKind::Block, "minecraft:sand", 12),
    (VanillaKind::Block, "minecraft:gravel", 13),
    (VanillaKind::Block, "minecraft:gold_ore", 14),
    (VanillaKind::Block, "minecraft:iron_ore", 15),
    (VanillaKind::Block, "minecraft:coal_ore", 16),
    (VanillaKind::Block, "minecraft:log", 17),
    (VanillaKind::Block, "minecraft:leaves", 18),
    (VanillaKind::Block, "minecraft:sponge", 19),
    (VanillaKind::Block, "minecraft:glass", 20),
    (VanillaKind::Block, "minecraft:lapis_ore", 21),
    (VanillaKind::Block, "minecraft:lapis_block", 22),
    (VanillaKind::Block, "minecraft:dispenser", 23),
    (VanillaKind::Block, "minecraft:sandstone", 24),
    (VanillaKind::Block, "minecraft:noteblock", 25),
    (VanillaKind::Block, "minecraft:bed", 26),
    (VanillaKind::Block, "minecraft:golden_rail", 27),
    (VanillaKind::Block, "minecraft:detector_rail", 28),
    (VanillaKind::Block, "minecraft:sticky_piston", 29),
    (VanillaKind::Block, "minecraft:web", 30),
    (VanillaKind::Block, "minecraft:tallgrass", 31),
    (VanillaKind::Block, "minecraft:deadbush", 32),
    (VanillaKind::Block, "minecraft:piston", 33),
    (VanillaKind::Block, "minecraft:piston_head", 34),
    (VanillaKind::Block, "minecraft:wool", 35),
    (VanillaKind::Block, "minecraft:piston_extension", 36),
    (VanillaKind::Block, "minecraft:yellow_flower", 37),
    (VanillaKind::Block, "minecraft:red_flower", 38),
    (VanillaKind::Block, "minecraft:brown_mushroom", 39),
    (VanillaKind::Block, "minecraft:red_mushroom", 40),
    (VanillaKind::Block, "minecraft:gold_block", 41),
    (VanillaKind::Block, "minecraft:iron_block", 42),
    (VanillaKind::Block, "minecraft:double_stone_slab", 43),
    (VanillaKind::Block, "minecraft:stone_slab", 44),
    (VanillaKind::Block, "minecraft:brick_block", 45),
    (VanillaKind::Block, "minecraft:tnt", 46),
    (VanillaKind::Block, "minecraft:bookshelf", 47),
    (VanillaKind::Block, "minecraft:mossy_cobblestone", 48),
    (VanillaKind::Block, "minecraft:obsidian", 49),
    (VanillaKind::Block, "minecraft:torch", 50),
    (VanillaKind::Block, "minecraft:fire", 51),
    (VanillaKind::Block, "minecraft:mob_spawner", 52),
    (VanillaKind::Block, "minecraft:oak_stairs", 53),
    (VanillaKind::Block, "minecraft:chest", 54),
    (VanillaKind::Block, "minecraft:redstone_wire", 55),
    (VanillaKind::Block, "minecraft:diamond_ore", 56),
    (VanillaKind::Block, "minecraft:diamond_block", 57),
    (VanillaKind::Block, "minecraft:crafting_table", 58),
    (VanillaKind::Block, "minecraft:wheat", 59),
    (VanillaKind::Block, "minecraft:farmland", 60),
    (VanillaKind::Block, "minecraft:furnace", 61),
    (VanillaKind::Block, "minecraft:lit_furnace", 62),
    (VanillaKind::Block, "minecraft:standing_sign", 63),
    (VanillaKind::Block, "minecraft:wooden_door", 64),
    (VanillaKind::Block, "minecraft:ladder", 65),
    (VanillaKind::Block, "minecraft:rail", 66),
    (VanillaKind::Block, "minecraft:stone_stairs", 67),
    (VanillaKind::Block, "minecraft:wall_sign", 68),
    (VanillaKind::Block, "minecraft:lever", 69),
    (VanillaKind::Block, "minecraft:stone_pressure_plate", 70),
    (VanillaKind::Block, "minecraft:iron_door", 71),
    (VanillaKind::Block, "minecraft:wooden_pressure_plate", 72),
    (VanillaKind::Block, "minecraft:redstone_ore", 73),
    (VanillaKind::Block, "minecraft:lit_redstone_ore", 74),
    (VanillaKind::Block, "minecraft:unlit_redstone_torch", 75),
    (VanillaKind::Block, "minecraft:redstone_torch", 76),
    (VanillaKind::Block, "minecraft:stone_button", 77),
    (VanillaKind::Block, "minecraft:snow_layer", 78),
    (VanillaKind::Block, "minecraft:ice", 79),
    (VanillaKind::Block, "minecraft:snow", 80),
    (VanillaKind::Block, "minecraft:cactus", 81),
    (VanillaKind::Block, "minecraft:clay", 82),
    (VanillaKind::Block, "minecraft:reeds", 83),
    (VanillaKind::Block, "minecraft:jukebox", 84),
    (VanillaKind::Block, "minecraft:fence", 85),
    (VanillaKind::Block, "minecraft:pumpkin", 86),
    (VanillaKind::Block, "minecraft:netherrack", 87),
    (VanillaKind::Block, "minecraft:soul_sand", 88),
    (VanillaKind::Block, "minecraft:glowstone", 89),
    (VanillaKind::Block, "minecraft:portal", 90),
    (VanillaKind::Block, "minecraft:lit_pumpkin", 91),
    (VanillaKind::Block, "minecraft:cake", 92),
    (VanillaKind::Block, "minecraft:unpowered_repeater", 93),
    (VanillaKind::Block, "minecraft:powered_repeater", 94),
    (VanillaKind::Block, "minecraft:locked_chest", 95),
    (VanillaKind::Block, "minecraft:trapdoor", 96),
    (VanillaKind::Block, "minecraft:monster_egg", 97),
    (VanillaKind::Block, "minecraft:stonebrick", 98),
    (VanillaKind::Block, "minecraft:brown_mushroom_block", 99),
    (VanillaKind::Block, "minecraft:red_mushroom_block", 100),
    (VanillaKind::Block, "minecraft:iron_bars", 101),
    (VanillaKind::Block, "minecraft:glass_pane", 102),
    (VanillaKind::Block, "minecraft:melon_block", 103),
    (VanillaKind::Block, "minecraft:pumpkin_stem", 104),
    (VanillaKind::Block, "minecraft:melon_stem", 105),
    (VanillaKind::Block, "minecraft:vine", 106),
    (VanillaKind::Block, "minecraft:fence_gate", 107),
    (VanillaKind::Block, "minecraft:brick_stairs", 108),
    (VanillaKind::Block, "minecraft:stone_brick_stairs", 109),
    (VanillaKind::Block, "minecraft:mycelium", 110),
    (VanillaKind::Block, "minecraft:waterlily", 111),
    (VanillaKind::Block, "minecraft:nether_brick", 112),
    (VanillaKind::Block, "minecraft:nether_brick_fence", 113),
    (VanillaKind::Block, "minecraft:nether_brick_stairs", 114),
    (VanillaKind::Block, "minecraft:nether_wart", 115),
    (VanillaKind::Block, "minecraft:enchanting_table", 116),
    (VanillaKind::Block, "minecraft:brewing_stand", 117),
    (VanillaKind::Block, "minecraft:cauldron", 118),
    (VanillaKind::Block, "minecraft:end_portal", 119),
    (VanillaKind::Block, "minecraft:end_portal_frame", 120),
    (VanillaKind::Block, "minecraft:end_stone", 121),
    (VanillaKind::Block, "minecraft:dragon_egg", 122),
    (VanillaKind::Block, "minecraft:redstone_lamp", 123),
    (VanillaKind::Block, "minecraft:lit_redstone_lamp", 124),
    // Standalone items. Placeable block-item overlaps are added from the block
    // table by `forge_1_2_5` so they remain a distinct registry kind.
    (VanillaKind::Item, "minecraft:iron_shovel", 256),
    (VanillaKind::Item, "minecraft:iron_pickaxe", 257),
    (VanillaKind::Item, "minecraft:iron_axe", 258),
    (VanillaKind::Item, "minecraft:flint_and_steel", 259),
    (VanillaKind::Item, "minecraft:apple", 260),
    (VanillaKind::Item, "minecraft:bow", 261),
    (VanillaKind::Item, "minecraft:arrow", 262),
    (VanillaKind::Item, "minecraft:coal", 263),
    (VanillaKind::Item, "minecraft:diamond", 264),
    (VanillaKind::Item, "minecraft:iron_ingot", 265),
    (VanillaKind::Item, "minecraft:gold_ingot", 266),
    (VanillaKind::Item, "minecraft:iron_sword", 267),
    (VanillaKind::Item, "minecraft:wooden_sword", 268),
    (VanillaKind::Item, "minecraft:wooden_shovel", 269),
    (VanillaKind::Item, "minecraft:wooden_pickaxe", 270),
    (VanillaKind::Item, "minecraft:wooden_axe", 271),
    (VanillaKind::Item, "minecraft:stone_sword", 272),
    (VanillaKind::Item, "minecraft:stone_shovel", 273),
    (VanillaKind::Item, "minecraft:stone_pickaxe", 274),
    (VanillaKind::Item, "minecraft:stone_axe", 275),
    (VanillaKind::Item, "minecraft:diamond_sword", 276),
    (VanillaKind::Item, "minecraft:diamond_shovel", 277),
    (VanillaKind::Item, "minecraft:diamond_pickaxe", 278),
    (VanillaKind::Item, "minecraft:diamond_axe", 279),
    (VanillaKind::Item, "minecraft:stick", 280),
    (VanillaKind::Item, "minecraft:bowl", 281),
    (VanillaKind::Item, "minecraft:mushroom_stew", 282),
    (VanillaKind::Item, "minecraft:golden_sword", 283),
    (VanillaKind::Item, "minecraft:golden_shovel", 284),
    (VanillaKind::Item, "minecraft:golden_pickaxe", 285),
    (VanillaKind::Item, "minecraft:golden_axe", 286),
    (VanillaKind::Item, "minecraft:string", 287),
    (VanillaKind::Item, "minecraft:feather", 288),
    (VanillaKind::Item, "minecraft:gunpowder", 289),
    (VanillaKind::Item, "minecraft:wooden_hoe", 290),
    (VanillaKind::Item, "minecraft:stone_hoe", 291),
    (VanillaKind::Item, "minecraft:iron_hoe", 292),
    (VanillaKind::Item, "minecraft:diamond_hoe", 293),
    (VanillaKind::Item, "minecraft:golden_hoe", 294),
    (VanillaKind::Item, "minecraft:wheat_seeds", 295),
    (VanillaKind::Item, "minecraft:wheat", 296),
    (VanillaKind::Item, "minecraft:bread", 297),
    (VanillaKind::Item, "minecraft:leather_helmet", 298),
    (VanillaKind::Item, "minecraft:leather_chestplate", 299),
    (VanillaKind::Item, "minecraft:leather_leggings", 300),
    (VanillaKind::Item, "minecraft:leather_boots", 301),
    (VanillaKind::Item, "minecraft:chainmail_helmet", 302),
    (VanillaKind::Item, "minecraft:chainmail_chestplate", 303),
    (VanillaKind::Item, "minecraft:chainmail_leggings", 304),
    (VanillaKind::Item, "minecraft:chainmail_boots", 305),
    (VanillaKind::Item, "minecraft:iron_helmet", 306),
    (VanillaKind::Item, "minecraft:iron_chestplate", 307),
    (VanillaKind::Item, "minecraft:iron_leggings", 308),
    (VanillaKind::Item, "minecraft:iron_boots", 309),
    (VanillaKind::Item, "minecraft:diamond_helmet", 310),
    (VanillaKind::Item, "minecraft:diamond_chestplate", 311),
    (VanillaKind::Item, "minecraft:diamond_leggings", 312),
    (VanillaKind::Item, "minecraft:diamond_boots", 313),
    (VanillaKind::Item, "minecraft:golden_helmet", 314),
    (VanillaKind::Item, "minecraft:golden_chestplate", 315),
    (VanillaKind::Item, "minecraft:golden_leggings", 316),
    (VanillaKind::Item, "minecraft:golden_boots", 317),
    (VanillaKind::Item, "minecraft:flint", 318),
    (VanillaKind::Item, "minecraft:porkchop", 319),
    (VanillaKind::Item, "minecraft:cooked_porkchop", 320),
    (VanillaKind::Item, "minecraft:painting", 321),
    (VanillaKind::Item, "minecraft:golden_apple", 322),
    (VanillaKind::Item, "minecraft:sign", 323),
    (VanillaKind::Item, "minecraft:wooden_door", 324),
    (VanillaKind::Item, "minecraft:bucket", 325),
    (VanillaKind::Item, "minecraft:water_bucket", 326),
    (VanillaKind::Item, "minecraft:lava_bucket", 327),
    (VanillaKind::Item, "minecraft:minecart", 328),
    (VanillaKind::Item, "minecraft:saddle", 329),
    (VanillaKind::Item, "minecraft:iron_door", 330),
    (VanillaKind::Item, "minecraft:redstone", 331),
    (VanillaKind::Item, "minecraft:snowball", 332),
    (VanillaKind::Item, "minecraft:boat", 333),
    (VanillaKind::Item, "minecraft:leather", 334),
    (VanillaKind::Item, "minecraft:milk_bucket", 335),
    (VanillaKind::Item, "minecraft:brick", 336),
    (VanillaKind::Item, "minecraft:clay_ball", 337),
    (VanillaKind::Item, "minecraft:reeds", 338),
    (VanillaKind::Item, "minecraft:paper", 339),
    (VanillaKind::Item, "minecraft:book", 340),
    (VanillaKind::Item, "minecraft:slime_ball", 341),
    (VanillaKind::Item, "minecraft:chest_minecart", 342),
    (VanillaKind::Item, "minecraft:furnace_minecart", 343),
    (VanillaKind::Item, "minecraft:egg", 344),
    (VanillaKind::Item, "minecraft:compass", 345),
    (VanillaKind::Item, "minecraft:fishing_rod", 346),
    (VanillaKind::Item, "minecraft:clock", 347),
    (VanillaKind::Item, "minecraft:glowstone_dust", 348),
    (VanillaKind::Item, "minecraft:fish", 349),
    (VanillaKind::Item, "minecraft:cooked_fish", 350),
    (VanillaKind::Item, "minecraft:dye", 351),
    (VanillaKind::Item, "minecraft:bone", 352),
    (VanillaKind::Item, "minecraft:sugar", 353),
    (VanillaKind::Item, "minecraft:cake", 354),
    (VanillaKind::Item, "minecraft:bed", 355),
    (VanillaKind::Item, "minecraft:repeater", 356),
    (VanillaKind::Item, "minecraft:cookie", 357),
    (VanillaKind::Item, "minecraft:filled_map", 358),
    (VanillaKind::Item, "minecraft:shears", 359),
    (VanillaKind::Item, "minecraft:melon", 360),
    (VanillaKind::Item, "minecraft:pumpkin_seeds", 361),
    (VanillaKind::Item, "minecraft:melon_seeds", 362),
    (VanillaKind::Item, "minecraft:beef", 363),
    (VanillaKind::Item, "minecraft:cooked_beef", 364),
    (VanillaKind::Item, "minecraft:chicken", 365),
    (VanillaKind::Item, "minecraft:cooked_chicken", 366),
    (VanillaKind::Item, "minecraft:rotten_flesh", 367),
    (VanillaKind::Item, "minecraft:ender_pearl", 368),
    (VanillaKind::Item, "minecraft:blaze_rod", 369),
    (VanillaKind::Item, "minecraft:ghast_tear", 370),
    (VanillaKind::Item, "minecraft:gold_nugget", 371),
    (VanillaKind::Item, "minecraft:nether_wart", 372),
    (VanillaKind::Item, "minecraft:potion", 373),
    (VanillaKind::Item, "minecraft:glass_bottle", 374),
    (VanillaKind::Item, "minecraft:spider_eye", 375),
    (VanillaKind::Item, "minecraft:fermented_spider_eye", 376),
    (VanillaKind::Item, "minecraft:blaze_powder", 377),
    (VanillaKind::Item, "minecraft:magma_cream", 378),
    (VanillaKind::Item, "minecraft:brewing_stand", 379),
    (VanillaKind::Item, "minecraft:cauldron", 380),
    (VanillaKind::Item, "minecraft:ender_eye", 381),
    (VanillaKind::Item, "minecraft:speckled_melon", 382),
    (VanillaKind::Item, "minecraft:spawn_egg", 383),
    (VanillaKind::Item, "minecraft:experience_bottle", 384),
    (VanillaKind::Item, "minecraft:fire_charge", 385),
    (VanillaKind::Item, "minecraft:record_13", 2256),
    (VanillaKind::Item, "minecraft:record_cat", 2257),
    (VanillaKind::Item, "minecraft:record_blocks", 2258),
    (VanillaKind::Item, "minecraft:record_chirp", 2259),
    (VanillaKind::Item, "minecraft:record_far", 2260),
    (VanillaKind::Item, "minecraft:record_mall", 2261),
    (VanillaKind::Item, "minecraft:record_mellohi", 2262),
    (VanillaKind::Item, "minecraft:record_stal", 2263),
    (VanillaKind::Item, "minecraft:record_strad", 2264),
    (VanillaKind::Item, "minecraft:record_ward", 2265),
    (VanillaKind::Item, "minecraft:record_11", 2266),
];

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("missing Forge registry field {0}")]
    MissingField(String),
    #[error("Forge registry field {path} has the wrong NBT type (expected {expected:?})")]
    WrongType { path: String, expected: Tag },
    #[error("invalid namespaced registry name {0:?}")]
    InvalidName(String),
    #[error("unknown 1.7.10 ItemData prefix {0:#04x}")]
    UnknownLegacyPrefix(u32),
    #[error("{kind:?} identity {name} has contradictory IDs {first} and {second}")]
    ContradictoryIdentity {
        kind: RegistryKind,
        name: RegistryName,
        first: i32,
        second: i32,
    },
    #[error("{kind:?} numeric ID {id} names both {first} and {second}")]
    DuplicateNumericId {
        kind: RegistryKind,
        id: i32,
        first: RegistryName,
        second: RegistryName,
    },
    #[error("{kind:?} alias {from} points to both {first} and {second}")]
    ContradictoryAlias {
        kind: RegistryKind,
        from: RegistryName,
        first: RegistryName,
        second: RegistryName,
    },
    #[error("registry override {key:?} names both owners {first} and {second}")]
    ContradictoryOverride {
        key: (RegistryKind, RegistryName),
        first: String,
        second: String,
    },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Extract the block/item catalog persisted by Forge 1.7.10.
///
/// # Errors
///
/// Returns an error for missing, mistyped, invalid, or contradictory registry data.
pub fn extract_forge_1_7_10(document: &Document) -> Result<RegistryCatalog> {
    let fml = fml_compound(document)?;
    let items = compound_list(required(fml, "ItemData", "FML.ItemData")?, "FML.ItemData")?;
    let provenance = Provenance::world("forge-1.7.10", "FML.ItemData");
    let mut catalog = RegistryCatalog::default();
    for entry in items {
        let raw = string(
            required(entry, "K", "ItemData entry K")?,
            "ItemData entry K",
        )?;
        let id = int(
            required(entry, "V", "ItemData entry V")?,
            "ItemData entry V",
        )?;
        let mut chars = raw.chars();
        let prefix = chars
            .next()
            .ok_or_else(|| Error::InvalidName(raw.to_owned()))?;
        let kind = match prefix {
            '\u{1}' => RegistryKind::Block,
            '\u{2}' => RegistryKind::Item,
            other => return Err(Error::UnknownLegacyPrefix(other as u32)),
        };
        catalog.insert(RegistryEntry {
            kind,
            name: RegistryName::parse(chars.as_str())?,
            numeric_id: id,
            provenance: provenance.clone(),
        })?;
    }
    extract_legacy_aliases(fml, "BlockAliases", &RegistryKind::Block, &mut catalog)?;
    extract_legacy_aliases(fml, "ItemAliases", &RegistryKind::Item, &mut catalog)?;
    if let Some(Value::IntArray(ids)) = fml.get("BlockedItemIds") {
        catalog
            .blocked
            .entry(RegistryKind::Item)
            .or_default()
            .extend(ids.iter().copied());
    }
    Ok(catalog)
}

/// Extract all persisted Forge 1.12.2 registries, retaining custom registry kinds.
///
/// # Errors
///
/// Returns an error for missing, mistyped, invalid, or contradictory registry data.
pub fn extract_forge_1_12_2(document: &Document) -> Result<RegistryCatalog> {
    let fml = fml_compound(document)?;
    let registries = compound(
        required(fml, "Registries", "FML.Registries")?,
        "FML.Registries",
    )?;
    let mut catalog = RegistryCatalog::default();
    for (registry_name, value) in registries {
        let snapshot = compound(value, &format!("FML.Registries.{registry_name}"))?;
        let kind = RegistryKind::from_112_name(registry_name);
        let provenance =
            Provenance::world("forge-1.12.2", format!("FML.Registries.{registry_name}"));
        for pair in optional_compound_list(snapshot.get("ids"), "ids")? {
            let name = RegistryName::parse(string(required(pair, "K", "ids.K")?, "ids.K")?)?;
            let numeric_id = int(required(pair, "V", "ids.V")?, "ids.V")?;
            catalog.insert(RegistryEntry {
                kind: kind.clone(),
                name,
                numeric_id,
                provenance: provenance.clone(),
            })?;
        }
        for pair in optional_compound_list(snapshot.get("aliases"), "aliases")? {
            let from =
                RegistryName::parse(string(required(pair, "K", "aliases.K")?, "aliases.K")?)?;
            let raw_to = string(required(pair, "V", "aliases.V")?, "aliases.V")?;
            // Forge 1.12.2 bug #4894 wrote override owners into aliases without a colon.
            if raw_to.contains(':') {
                let to = RegistryName::parse(raw_to)?;
                if from != to {
                    catalog.insert_alias(Alias {
                        kind: kind.clone(),
                        from,
                        to,
                        provenance: provenance.clone(),
                    })?;
                }
            } else {
                catalog
                    .overrides
                    .insert((kind.clone(), from), raw_to.to_owned());
            }
        }
        for pair in optional_compound_list(snapshot.get("overrides"), "overrides")? {
            let name =
                RegistryName::parse(string(required(pair, "K", "overrides.K")?, "overrides.K")?)?;
            let owner = string(required(pair, "V", "overrides.V")?, "overrides.V")?.to_owned();
            catalog.overrides.insert((kind.clone(), name), owner);
        }
        if let Some(Value::IntArray(ids)) = snapshot.get("blocked") {
            catalog
                .blocked
                .entry(kind.clone())
                .or_default()
                .extend(ids.iter().copied());
        }
        if let Some(Value::List(List { values, .. })) = snapshot.get("dummied") {
            let set = catalog.dummied.entry(kind).or_default();
            for value in values {
                match value {
                    Value::String(name) => {
                        set.insert(RegistryName::parse(name)?);
                    }
                    Value::Compound(pair) => {
                        set.insert(RegistryName::parse(string(
                            required(pair, "K", "dummied.K")?,
                            "dummied.K",
                        )?)?);
                    }
                    _ => {
                        return Err(Error::WrongType {
                            path: "dummied[]".to_owned(),
                            expected: Tag::String,
                        })
                    }
                }
            }
        }
    }
    Ok(catalog)
}

fn fml_compound(document: &Document) -> Result<&BTreeMap<String, Value>> {
    if let Some(value) = document.root.get("FML") {
        return compound(value, "FML");
    }
    if let Some(Value::Compound(data)) = document.root.get("Data") {
        if let Some(value) = data.get("FML") {
            return compound(value, "Data.FML");
        }
    }
    Err(Error::MissingField("FML".to_owned()))
}

fn extract_legacy_aliases(
    fml: &BTreeMap<String, Value>,
    field: &str,
    kind: &RegistryKind,
    catalog: &mut RegistryCatalog,
) -> Result<()> {
    let provenance = Provenance::world("forge-1.7.10", format!("FML.{field}"));
    for pair in optional_compound_list(fml.get(field), field)? {
        catalog.insert_alias(Alias {
            kind: kind.clone(),
            from: RegistryName::parse(string(required(pair, "K", "alias.K")?, "alias.K")?)?,
            to: RegistryName::parse(string(required(pair, "V", "alias.V")?, "alias.V")?)?,
            provenance: provenance.clone(),
        })?;
    }
    Ok(())
}

fn required<'a>(map: &'a BTreeMap<String, Value>, key: &str, path: &str) -> Result<&'a Value> {
    map.get(key)
        .ok_or_else(|| Error::MissingField(path.to_owned()))
}
fn compound<'a>(value: &'a Value, path: &str) -> Result<&'a BTreeMap<String, Value>> {
    if let Value::Compound(value) = value {
        Ok(value)
    } else {
        Err(Error::WrongType {
            path: path.to_owned(),
            expected: Tag::Compound,
        })
    }
}
fn compound_list<'a>(value: &'a Value, path: &str) -> Result<Vec<&'a BTreeMap<String, Value>>> {
    optional_compound_list(Some(value), path)
}
fn optional_compound_list<'a>(
    value: Option<&'a Value>,
    path: &str,
) -> Result<Vec<&'a BTreeMap<String, Value>>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Value::List(list) = value else {
        return Err(Error::WrongType {
            path: path.to_owned(),
            expected: Tag::List,
        });
    };
    list.values
        .iter()
        .map(|value| compound(value, &format!("{path}[]")))
        .collect()
}
fn string<'a>(value: &'a Value, path: &str) -> Result<&'a str> {
    if let Value::String(value) = value {
        Ok(value)
    } else {
        Err(Error::WrongType {
            path: path.to_owned(),
            expected: Tag::String,
        })
    }
}
fn int(value: &Value, path: &str) -> Result<i32> {
    if let Value::Int(value) = value {
        Ok(*value)
    } else {
        Err(Error::WrongType {
            path: path.to_owned(),
            expected: Tag::Int,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(key: &str, value: Value) -> Value {
        Value::Compound(BTreeMap::from([
            ("K".into(), Value::String(key.into())),
            ("V".into(), value),
        ]))
    }
    fn list(values: Vec<Value>) -> Value {
        Value::List(List {
            element_tag: Tag::Compound,
            values,
        })
    }
    fn document(fml: BTreeMap<String, Value>) -> Document {
        Document {
            root_name: String::new(),
            root: BTreeMap::from([("FML".into(), Value::Compound(fml))]),
        }
    }

    #[test]
    fn extracts_minimized_1710_fixture_with_mod_entries() {
        let fixture = document(BTreeMap::from([
            (
                "ItemData".into(),
                list(vec![
                    pair("\u{1}minecraft:stone", Value::Int(1)),
                    pair("\u{1}examplemod:machine", Value::Int(3000)),
                    pair("\u{2}examplemod:wrench", Value::Int(12000)),
                ]),
            ),
            (
                "BlockAliases".into(),
                list(vec![pair(
                    "examplemod:old_machine",
                    Value::String("examplemod:machine".into()),
                )]),
            ),
            ("BlockedItemIds".into(), Value::IntArray(vec![12001])),
        ]));
        let catalog = extract_forge_1_7_10(&fixture).unwrap();
        let machine = RegistryName::parse("examplemod:machine").unwrap();
        assert_eq!(
            catalog
                .by_name(&RegistryKind::Block, &machine)
                .unwrap()
                .numeric_id,
            3000
        );
        assert_eq!(
            catalog
                .by_numeric(&RegistryKind::Item, 12000)
                .unwrap()
                .name
                .as_str(),
            "examplemod:wrench"
        );
        assert_eq!(catalog.aliases().count(), 1);
    }

    #[test]
    fn extracts_legacy_1710_registry_names_with_spaces() {
        let fixture = document(BTreeMap::from([(
            "ItemData".into(),
            list(vec![
                pair("\u{1}BiblioCraft:Typesetting Machine", Value::Int(184)),
                pair("\u{2}BiblioCraft:Typesetting Machine", Value::Int(184)),
            ]),
        )]));

        let catalog = extract_forge_1_7_10(&fixture).unwrap();
        let name = RegistryName::parse("BiblioCraft:Typesetting Machine").unwrap();

        assert_eq!(
            catalog
                .by_name(&RegistryKind::Block, &name)
                .unwrap()
                .numeric_id,
            184
        );
        assert_eq!(
            catalog
                .by_name(&RegistryKind::Item, &name)
                .unwrap()
                .numeric_id,
            184
        );
    }

    #[test]
    fn extracts_minimized_1122_fixture_and_reverse_lookup() {
        let blocks = Value::Compound(BTreeMap::from([
            (
                "ids".into(),
                list(vec![
                    pair("minecraft:stone", Value::Int(1)),
                    pair("examplemod:machine", Value::Int(1500)),
                ]),
            ),
            (
                "aliases".into(),
                list(vec![pair(
                    "examplemod:old_machine",
                    Value::String("examplemod:machine".into()),
                )]),
            ),
            ("blocked".into(), Value::IntArray(vec![4090])),
            (
                "dummied".into(),
                Value::List(List {
                    element_tag: Tag::String,
                    values: vec![Value::String("gone:machine".into())],
                }),
            ),
        ]));
        let items = Value::Compound(BTreeMap::from([(
            "ids".into(),
            list(vec![pair("examplemod:wrench", Value::Int(6200))]),
        )]));
        let fixture = document(BTreeMap::from([(
            "Registries".into(),
            Value::Compound(BTreeMap::from([
                ("minecraft:blocks".into(), blocks),
                ("minecraft:items".into(), items),
            ])),
        )]));
        let catalog = extract_forge_1_12_2(&fixture).unwrap();
        let name = RegistryName::parse("examplemod:wrench").unwrap();
        assert_eq!(
            catalog
                .by_name(&RegistryKind::Item, &name)
                .unwrap()
                .numeric_id,
            6200
        );
        assert_eq!(
            catalog
                .by_numeric(&RegistryKind::Block, 1500)
                .unwrap()
                .name
                .as_str(),
            "examplemod:machine"
        );
    }

    #[test]
    fn deterministic_merge_rejects_contradictions() {
        let entry = |id| RegistryEntry {
            kind: RegistryKind::Block,
            name: RegistryName::parse("mod:block").unwrap(),
            numeric_id: id,
            provenance: Provenance::world("test", "fixture"),
        };
        let mut first = RegistryCatalog::default();
        first.insert(entry(20)).unwrap();
        let mut second = RegistryCatalog::default();
        second.insert(entry(21)).unwrap();
        assert!(matches!(
            first.merge(second),
            Err(Error::ContradictoryIdentity { .. })
        ));
    }

    #[test]
    fn vanilla_fallback_fills_only_missing_assignments() {
        let mut catalog = RegistryCatalog::default();
        catalog.add_vanilla_fallbacks(VanillaVersion::Minecraft1_7_10);
        let stone = RegistryName::parse("minecraft:stone").unwrap();
        let entry = catalog.by_name(&RegistryKind::Block, &stone).unwrap();
        assert_eq!(entry.numeric_id, 1);
        assert_eq!(entry.provenance.source, "minecraft-1.7.10");
    }

    #[test]
    fn vanilla_fallback_never_reinterprets_modded_numeric_evidence() {
        let mut catalog = RegistryCatalog::default();
        catalog
            .insert(RegistryEntry {
                kind: RegistryKind::Block,
                name: RegistryName::parse("examplemod:machine").unwrap(),
                numeric_id: 1,
                provenance: Provenance::world("forge-1.7.10", "FML.ItemData"),
            })
            .unwrap();

        catalog.add_vanilla_fallbacks(VanillaVersion::Minecraft1_7_10);

        assert_eq!(
            catalog
                .by_numeric(&RegistryKind::Block, 1)
                .unwrap()
                .name
                .as_str(),
            "examplemod:machine"
        );
        assert!(catalog
            .by_name(
                &RegistryKind::Block,
                &RegistryName::parse("minecraft:stone").unwrap()
            )
            .is_none());
    }

    #[test]
    fn later_vanilla_catalogs_cover_version_specific_blocks() {
        let mut one_seven = RegistryCatalog::default();
        one_seven.add_vanilla_fallbacks(VanillaVersion::Minecraft1_7_10);
        assert_eq!(
            one_seven
                .by_numeric(&RegistryKind::Block, 95)
                .unwrap()
                .name
                .as_str(),
            "minecraft:stained_glass"
        );
        assert_eq!(
            one_seven
                .by_numeric(&RegistryKind::Block, 175)
                .unwrap()
                .name
                .as_str(),
            "minecraft:double_plant"
        );
        assert!(one_seven.by_numeric(&RegistryKind::Block, 176).is_none());

        let mut one_twelve = RegistryCatalog::default();
        one_twelve.add_vanilla_fallbacks(VanillaVersion::Minecraft1_12_2);
        assert_eq!(
            one_twelve
                .by_numeric(&RegistryKind::Block, 219)
                .unwrap()
                .name
                .as_str(),
            "minecraft:white_shulker_box"
        );
        assert_eq!(
            one_twelve
                .by_numeric(&RegistryKind::Block, 252)
                .unwrap()
                .name
                .as_str(),
            "minecraft:concrete_powder"
        );
        assert_eq!(
            one_twelve
                .by_numeric(&RegistryKind::Block, 255)
                .unwrap()
                .name
                .as_str(),
            "minecraft:structure_block"
        );
    }

    #[test]
    fn forge_1_2_5_catalog_is_complete_separate_and_authoritative() {
        let catalog = RegistryCatalog::forge_1_2_5();
        let blocks: Vec<_> = catalog
            .entries()
            .filter(|entry| entry.kind == RegistryKind::Block)
            .collect();
        assert_eq!(blocks.len(), 125);
        assert_eq!(blocks.iter().map(|entry| entry.numeric_id).min(), Some(0));
        assert_eq!(blocks.iter().map(|entry| entry.numeric_id).max(), Some(124));
        let mut ids = blocks
            .iter()
            .map(|entry| entry.numeric_id)
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), blocks.len());
        let stone = RegistryName::parse("minecraft:stone").unwrap();
        assert_eq!(
            catalog
                .by_name(&RegistryKind::Block, &stone)
                .unwrap()
                .numeric_id,
            1
        );
        assert_eq!(
            catalog
                .by_name(&RegistryKind::Item, &stone)
                .unwrap()
                .numeric_id,
            1
        );
        assert_eq!(
            catalog
                .by_numeric(&RegistryKind::Item, 385)
                .unwrap()
                .name
                .as_str(),
            "minecraft:fire_charge"
        );
        assert_eq!(
            catalog
                .by_numeric(&RegistryKind::Item, 2266)
                .unwrap()
                .name
                .as_str(),
            "minecraft:record_11"
        );
        assert!(catalog.entries().all(|entry| {
            entry.provenance.source == "minecraft-1.2.5"
                && entry.provenance.detail == "verified vanilla registry table"
        }));
    }
}
