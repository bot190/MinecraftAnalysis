# Block Rule Inference Specification

## Purpose

Enable users to derive a deterministic, reviewable transformation-rule snippet from one paired source and Forge 1.12.2 target block observation selected directly from world coordinates.

## Requirements

### Requirement: Infer from one ordered pair of world coordinates
The system SHALL provide `rules infer --rules <file> --source-world <world> --target-world <world> --coordinate <source-x,source-y,source-z:target-x,target-y,target-z>`, SHALL load the supplied rule file as read-only inference context, and SHALL require exactly one `--coordinate` occurrence. The coordinate value SHALL contain exactly one colon separating exactly two coordinate triples, each containing exactly three signed 32-bit integers. The triple before the colon SHALL select the source observation and the triple after the colon SHALL select the target observation. The command SHALL accept independent `--source-dimension <dimension>` and `--target-dimension <dimension>` arguments, each defaulting to `overworld`.

#### Scenario: Accept a complete pair and establish direction from arguments
- **WHEN** readable, compatible worlds and one valid paired coordinate are supplied
- **THEN** the command treats the coordinate before the colon as source evidence and the coordinate after the colon as target evidence, loads both observations, and attempts to produce one rule document

#### Scenario: Reject malformed paired coordinate
- **WHEN** the coordinate value lacks exactly one colon, either side is not an exact three-component coordinate, or any component is not a signed 32-bit integer
- **THEN** the command identifies the malformed coordinate pair, exits nonzero before loading either observation, and leaves standard output empty

#### Scenario: Reject repeated paired coordinates
- **WHEN** `--coordinate` is supplied more than once
- **THEN** the command reports that exactly one coordinate pair is supported, exits nonzero before loading any observation, and leaves standard output empty

#### Scenario: Select independent dimensions
- **WHEN** source and target dimensions are supplied
- **THEN** each coordinate is loaded from its independently selected dimension

#### Scenario: Default dimensions
- **WHEN** neither dimension argument is supplied
- **THEN** both observations are loaded from the overworld

### Requirement: Resolve observations using compatible world registries
The system SHALL interpret the source world using the supplied rule graph's source profile and SHALL require the target world to be compatible with Forge 1.12.2. It SHALL derive the source and target registry catalogs from their respective worlds, supplement them with the corresponding rule manifests, and use those catalogs to resolve each observed numeric block ID to a registry name.

#### Scenario: Resolve both observations
- **WHEN** both worlds match their required profiles and each observed numeric block ID resolves through its world-derived and manifest-supplemented registry catalog
- **THEN** the command obtains a typed observation containing the registry name, metadata, and optional associated block entity for each coordinate

#### Scenario: Reject incompatible source world
- **WHEN** the source world does not match the rule graph's source profile
- **THEN** the command identifies the source-profile incompatibility, exits nonzero, and leaves standard output empty

#### Scenario: Reject incompatible target world
- **WHEN** the target world is not compatible with Forge 1.12.2
- **THEN** the command identifies the target-profile incompatibility, exits nonzero, and leaves standard output empty

#### Scenario: Reject unavailable coordinate evidence
- **WHEN** either world is unreadable, a selected dimension or region is unavailable, a selected chunk is absent or malformed, or no stored block is indexed at a coordinate
- **THEN** the command identifies the invalid side and coordinate context, exits nonzero, and leaves standard output empty

#### Scenario: Reject unresolved registry identity
- **WHEN** either observed numeric block ID does not resolve through the applicable registry catalog
- **THEN** the command identifies the unresolved side and numeric ID, exits nonzero, and leaves standard output empty

#### Scenario: Reject invalid rule context
- **WHEN** the supplied rule file cannot be read or loaded as a valid rule graph
- **THEN** the command identifies the rule-context failure, exits nonzero, and leaves standard output empty

### Requirement: Infer an exact block transformation
The generated array SHALL contain a block rule whose matcher uses the source registry name and exact source metadata. Its transform action SHALL target the target registry name and SHALL set metadata to the target value when the source and target metadata differ. The rule SHALL use the supplied rule graph as validation and registry context without copying its manifests into the output.

#### Scenario: Registry identity changes
- **WHEN** the paired observations have different block registry names
- **THEN** the block transform targets the target observation's registry name

#### Scenario: Metadata changes
- **WHEN** the paired observations have different metadata values
- **THEN** the block action contains a numeric `set` transform with the target metadata value

#### Scenario: Metadata is unchanged
- **WHEN** the paired observations have equal metadata values
- **THEN** the block action omits a numeric transform

### Requirement: Infer conservative block-entity transformation
When both observations contain block entities, the system SHALL infer a block-entity rule matched by the source entity's string `id`, SHALL target the target entity's string `id` when it differs, and SHALL emit typed `set` and `remove` patches only for unambiguous field differences. Comparison SHALL ignore the top-level `x`, `y`, and `z` fields. The system SHALL preserve NBT types and SHALL NOT infer renames, moves, copies, conditions, nested-item paths, or value maps from one pair.

#### Scenario: Set a changed or added value
- **WHEN** a non-coordinate path has a target value different from the source value, or exists only in the target, and its containing structure can be represented unambiguously
- **THEN** the inferred block-entity action sets that path to the exact typed target value

#### Scenario: Remove a source-only value
- **WHEN** a non-coordinate path exists only in the source and its removal is unambiguous
- **THEN** the inferred block-entity action removes that path

#### Scenario: Block-entity identity changes
- **WHEN** both compounds contain valid string `id` fields with different values
- **THEN** the block-entity transform targets the target `id` and does not patch the `id` field

#### Scenario: Block-entity presence differs
- **WHEN** exactly one observation contains a block entity
- **THEN** the command reports that creation or deletion cannot be inferred safely, exits nonzero, and leaves standard output empty

#### Scenario: Block-entity identity is unavailable
- **WHEN** an observed block entity lacks a valid string `id`
- **THEN** the command reports that a rule matcher cannot be formed, exits nonzero, and leaves standard output empty

### Requirement: Emit a deterministic rule array
On success, the system SHALL emit a deterministic pretty-printed JSON array containing the inferred block rule followed by an optional companion block-entity rule and exactly one trailing newline. Each entry SHALL use the existing rule schema, SHALL be valid in the supplied rule context, SHALL use a user-supplied `--rule-id <id>` when provided, and otherwise SHALL derive stable rule identifiers from the observed registry identities.

#### Scenario: Emit a rule snippet
- **WHEN** inference succeeds
- **THEN** standard output contains a JSON array with the block rule first and any companion block-entity rule second, without document-level schema, profile, manifest, or import fields

#### Scenario: Repeat inference
- **WHEN** the same world contents, coordinates, dimensions, and options are supplied more than once
- **THEN** each successful invocation produces byte-for-byte identical standard output

#### Scenario: Override generated identifiers
- **WHEN** the user supplies a valid `--rule-id <id>`
- **THEN** the generated block rule uses that identifier and any generated companion block-entity rule uses a deterministic identifier derived from it

### Requirement: Diagnose unsupported evidence without partial output
The system SHALL exclude coordinate addressing, section, index, and lighting data from inference. It SHALL report unsupported or ambiguous evidence on standard error and SHALL fully load the rule context, resolve both world-coordinate observations, infer, serialize, and validate the candidate rules before writing standard output. A failure SHALL produce no partial rule array and SHALL not modify either world or the supplied rule file.

#### Scenario: Location-specific data differs
- **WHEN** coordinate, dimension, chunk, region, local chunk, lighting, section, or section-index data differs between the observations
- **THEN** those differences do not themselves affect the inferred rules

#### Scenario: Candidate validation fails
- **WHEN** an inferred candidate violates the existing rule schema or conflicts with the supplied rule context
- **THEN** the command reports the validation failure, exits nonzero, and leaves standard output empty

#### Scenario: Inputs remain unchanged
- **WHEN** inference succeeds or fails
- **THEN** neither world nor the supplied rule file is created, overwritten, or modified
