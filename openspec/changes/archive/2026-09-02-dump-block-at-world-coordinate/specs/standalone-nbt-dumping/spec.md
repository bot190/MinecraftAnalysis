## ADDED Requirements

### Requirement: Dump a block at a world-global coordinate
The system SHALL provide an `nbt dump --world <world> --location <x,y,z>` mode that resolves one block from a Java Edition world, SHALL require exactly one of `--source-rule <file>` or `--target-rule <file>` to select the registry context, SHALL default to the overworld unless `--dimension <dimension>` is supplied, and SHALL leave the existing `nbt dump <file>` document and region-chunk modes unchanged.

#### Scenario: Dump an overworld block
- **WHEN** a user supplies a readable world root and a valid world-global coordinate whose region, chunk, and stored section are present
- **THEN** the command reports the exact block at that coordinate and exits successfully

#### Scenario: Dump a block from another dimension
- **WHEN** a user supplies a valid named vanilla or safe existing modded dimension
- **THEN** the command resolves the coordinate beneath that dimension's region directory rather than the overworld region directory

#### Scenario: Resolve negative coordinate boundaries
- **WHEN** either horizontal coordinate lies on or adjacent to a negative chunk or region boundary
- **THEN** the command selects the region, global chunk, region-local chunk, and in-chunk position defined by Euclidean coordinate division

#### Scenario: Keep input modes mutually exclusive
- **WHEN** a user combines `--world` or `--location` with a positional NBT file, `--chunk`, or `--local-chunk`, supplies only one of `--world` and `--location`, supplies neither rule-side flag, or supplies both `--source-rule` and `--target-rule`
- **THEN** the command fails with a usage diagnostic and leaves standard output empty

### Requirement: Resolve the block registry name through the selected rule side
The world-coordinate mode SHALL load the supplied rule file, SHALL construct block and item registry catalogs using the selected source or target side's manifest and established profile fallbacks, and SHALL resolve the stored numeric block ID through that side's block registry before emitting output. The command SHALL NOT emit a numeric-only fallback when registry resolution fails.

#### Scenario: Resolve through the source rule side
- **WHEN** the user supplies `--source-rule <file>` and the source catalog assigns the stored numeric block ID
- **THEN** the command reports the assigned source block registry name

#### Scenario: Resolve through the target rule side
- **WHEN** the user supplies `--target-rule <file>` and the target catalog assigns the stored numeric block ID
- **THEN** the command reports the assigned target block registry name

#### Scenario: Numeric block ID is unresolved
- **WHEN** the selected rule side's block registry does not assign the stored numeric block ID
- **THEN** the command identifies the selected side and numeric ID, exits nonzero, and leaves standard output empty

### Requirement: Report stored block information deterministically
The world-coordinate mode SHALL emit a deterministic human-readable record containing, in stable order, the requested coordinate, dimension, global chunk, region, region-local chunk, full numeric block ID, resolved block registry name, metadata, available block light and sky light, section Y coordinate, section storage index, and block-entity result, followed by exactly one trailing newline.

#### Scenario: Report a stored block
- **WHEN** the selected block is stored in a supported pre-flattening Anvil section
- **THEN** the output contains the decoded full block ID including optional extended ID bits, its resolved registry name, metadata, each available lighting value, and the block's addressing and section fields

#### Scenario: Report unavailable lighting
- **WHEN** the selected section omits an optional block-light or sky-light array
- **THEN** the corresponding output field explicitly identifies the value as unavailable

#### Scenario: Repeat a coordinate dump
- **WHEN** the same unchanged world coordinate is dumped more than once
- **THEN** each successful invocation produces byte-for-byte identical standard output

### Requirement: Include associated block-entity SNBT
The world-coordinate mode SHALL associate a block entity only when its top-level lowercase integer `x`, `y`, and `z` coordinates exactly equal the requested block coordinate, and SHALL emit the complete associated compound as canonical typed SNBT without removing or normalizing any fields.

#### Scenario: Dump an associated block entity
- **WHEN** the selected chunk contains a block entity at the exact requested coordinate
- **THEN** the block record includes its complete canonical SNBT, including its top-level coordinate fields and all nested values

#### Scenario: Report no associated block entity
- **WHEN** no block entity has all three coordinates equal to the requested coordinate
- **THEN** the block record explicitly reports that no associated block entity exists

### Requirement: Diagnose unavailable coordinate data without partial output
The world-coordinate mode SHALL fail contextually with a nonzero exit status and empty standard output when the world path, dimension, derived region, selected chunk, chunk NBT, supported block storage, requested section, or block-entity SNBT cannot produce one complete block record.

#### Scenario: World or dimension is unavailable
- **WHEN** the world root is unreadable or the requested dimension is invalid or absent
- **THEN** the command identifies the world or dimension failure and leaves standard output empty

#### Scenario: Derived region or chunk is unavailable
- **WHEN** the coordinate's derived region file is unreadable, its selected chunk slot is absent, or its region or chunk payload is invalid
- **THEN** the command identifies the coordinate and derived region and chunk context and leaves standard output empty

#### Scenario: Chunk block storage is unsupported
- **WHEN** the selected chunk does not contain supported pre-flattening Anvil section storage
- **THEN** the command reports why block lookup is unavailable and leaves standard output empty

#### Scenario: Coordinate has no stored section
- **WHEN** the requested horizontal position belongs to the selected chunk but its Y coordinate is not represented by a stored section
- **THEN** the command reports that no stored block is indexed at the coordinate and leaves standard output empty

#### Scenario: Associated SNBT cannot be rendered
- **WHEN** the matching block entity contains a value that cannot be rendered as portable SNBT
- **THEN** the command reports the rendering failure with coordinate context and leaves standard output empty
