## MODIFIED Requirements

### Requirement: Infer an exact block transformation
The generated array SHALL contain one block rule whose matcher uses the source registry name and exact source metadata and constrains the associated block entity by its source identity when present. Its transformation template SHALL produce the target registry name, metadata, and complete optional block entity as one coordinated typed result. The rule SHALL use the supplied rule graph as validation and registry context without copying its manifests into the output.

#### Scenario: Registry identity changes
- **WHEN** the paired observations have different block registry names
- **THEN** the inferred template produces the target observation's registry name

#### Scenario: Metadata changes
- **WHEN** the paired observations have different metadata values
- **THEN** the inferred template produces the target observation's metadata

#### Scenario: Metadata is unchanged
- **WHEN** the paired observations have equal metadata values
- **THEN** the inferred template still produces the target observation's metadata as part of the complete result

#### Scenario: Block values are unchanged
- **WHEN** the paired observations have equal block registry names and metadata
- **THEN** the inferred template still produces a complete valid coordinated block result

### Requirement: Infer conservative block-entity transformation
The system SHALL represent the target observation's complete block entity as lossless typed template output, SHALL use the source entity's valid string `id` in the associated matcher when a source block entity exists, and SHALL preserve target `x`, `y`, and `z` fields by sourcing their values from the original block entity when both observations contain one. The system SHALL NOT infer generalized conditions, nested-item paths, or value-map calls from one pair. Block-entity creation and deletion SHALL be expressible by the coordinated template.

#### Scenario: Complete target block entity is emitted
- **WHEN** the target observation contains a block entity
- **THEN** the inferred template produces its complete typed structure while treating coordinate fields according to the coordinate-preservation policy

#### Scenario: Set a changed or added value
- **WHEN** a non-coordinate target value differs from the source value or exists only in the target
- **THEN** the inferred template contains that exact typed target value in its complete block-entity result

#### Scenario: Remove a source-only value
- **WHEN** a non-coordinate value exists only in the source
- **THEN** the inferred template omits that value from its complete target block-entity result

#### Scenario: Block-entity identity changes
- **WHEN** both block entities contain valid string `id` fields with different values
- **THEN** the associated matcher uses the source identity and the inferred template produces the target identity

#### Scenario: Block-entity presence differs
- **WHEN** exactly one observation contains a block entity
- **THEN** the inferred coordinated template represents its creation or deletion

#### Scenario: Block-entity identity is unavailable
- **WHEN** a source block entity exists without a valid string `id`
- **THEN** the command reports that its associated matcher cannot be formed, exits nonzero, and leaves standard output empty

#### Scenario: Block entity is created
- **WHEN** only the target observation contains a block entity
- **THEN** the inferred coordinated template explicitly produces that block entity

#### Scenario: Block entity is deleted
- **WHEN** only the source observation contains a block entity
- **THEN** the inferred coordinated template explicitly produces no block entity

#### Scenario: Source block-entity identity is unavailable
- **WHEN** a source block entity exists without a valid string `id`
- **THEN** the command reports that its associated matcher cannot be formed, exits nonzero, and leaves standard output empty

### Requirement: Emit a deterministic rule sequence
On success, the system SHALL emit a deterministic YAML sequence containing exactly one schema-compatible coordinated block template rule and exactly one trailing newline. The inferred template SHALL be emitted as a literal block scalar. The rule SHALL be valid in the supplied rule context, SHALL use a user-supplied `--rule-id <id>` when provided, and otherwise SHALL derive a stable rule identifier from the observed registry identities.

#### Scenario: Emit a rule snippet
- **WHEN** inference succeeds
- **THEN** standard output contains a YAML sequence with one block matcher and transformation template, without document-level schema, profile, manifest, or import fields

#### Scenario: Repeat inference
- **WHEN** the same world contents, coordinates, dimensions, and options are supplied more than once
- **THEN** each successful invocation produces byte-for-byte identical standard output

#### Scenario: Override generated identifiers
- **WHEN** the user supplies a valid `--rule-id <id>`
- **THEN** the generated rule uses that identifier

### Requirement: Diagnose unsupported evidence without partial output
The system SHALL exclude coordinate addressing, section, index, and lighting data from matching and SHALL use block-entity coordinates only to preserve valid placement in the inferred result. It SHALL report unsupported or ambiguous evidence on standard error and SHALL fully load the rule context, resolve both world-coordinate observations, generate, compile, serialize, and validate the candidate template rule before writing standard output. A failure SHALL produce no partial rule array and SHALL not modify either world or the supplied rule file.

#### Scenario: Location-specific data differs
- **WHEN** dimension, chunk, region, local chunk, lighting, section, or section-index data differs between the observations
- **THEN** those differences do not constrain the inferred matcher or otherwise affect the inferred transformation

#### Scenario: Candidate validation fails
- **WHEN** an inferred candidate violates the rule schema, fails template compilation, or conflicts with the supplied rule context
- **THEN** the command reports the validation failure, exits nonzero, and leaves standard output empty

#### Scenario: Inputs remain unchanged
- **WHEN** inference succeeds or fails
- **THEN** neither world nor the supplied rule file is created, overwritten, or modified
