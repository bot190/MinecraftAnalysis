## MODIFIED Requirements

### Requirement: Report uncovered signatures and every occurrence
The command SHALL emit a deterministic machine-readable report that groups uncovered objects by distinct kind, source identity, numeric representation, metadata or damage, and SNBT object data. For an uncovered block with associated block-entity data, the signature and reported associated SNBT SHALL omit only the block entity's top-level lowercase `x`, `y`, and `z` fields; rule evaluation SHALL continue to use the complete unmodified block-entity NBT. Each group SHALL report the exact number of raw observations classified under that signature and SHALL include at most the first five distinct locations in canonical order. Multiple observations of the same location SHALL contribute separately to the occurrence count.

#### Scenario: Repeated uncovered block
- **WHEN** equivalent uncovered blocks are observed at multiple positions or the same position is observed more than once
- **THEN** one stable signature group reports the total raw observation count and at most the first five distinct canonical locations with their dimensions, files, chunks, and block coordinates

#### Scenario: More than five locations share a signature
- **WHEN** an uncovered block or item signature is observed at more than five distinct locations
- **THEN** its occurrence count includes every raw observation while its locations contain exactly the first five distinct locations in canonical order

#### Scenario: Uncovered item in NBT
- **WHEN** an uncovered item is found in a supported inventory or container path
- **THEN** its group includes its identity, numeric ID, damage, count, complete SNBT, raw observation count, and up to five canonical example locations containing the file, world location, and NBT path

#### Scenario: Block has associated block-entity data
- **WHEN** an uncovered block has a block entity at the same coordinates
- **THEN** the report includes the block entity identity and its canonical SNBT without top-level lowercase `x`, `y`, and `z` fields with the block signature

#### Scenario: Coordinate-only block-entity differences
- **WHEN** otherwise equivalent uncovered blocks have associated block entities that differ only in their top-level lowercase `x`, `y`, or `z` fields
- **THEN** the command groups them under one signature, counts every observation, and retains their distinct block coordinates in the canonical location sample

#### Scenario: Significant coordinate-like block-entity data differs
- **WHEN** associated block entities differ in a nested `x`, `y`, or `z` field, in a differently cased field, or in any other NBT data
- **THEN** those differences remain present in reported SNBT and participate in signature grouping

#### Scenario: Block-entity rule uses coordinates
- **WHEN** a coordinated block rule evaluates a predicate that reads a top-level `x`, `y`, or `z` field from its associated block entity
- **THEN** rule evaluation receives the complete unmodified block-entity NBT

#### Scenario: Analysis execution order varies
- **WHEN** the same source and rules are analyzed with different worker counts or spill boundaries
- **THEN** each signature has the same raw observation count and the same ordered location sample
