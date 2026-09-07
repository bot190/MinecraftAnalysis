# Rule Coverage Analysis Specification

## Purpose

Help transformation-rule authors identify and investigate source-world blocks and items that are not covered by built-in vanilla migration or an explicit rule.

## Requirements

### Requirement: Analyze rule coverage from source inputs
The system SHALL provide a read-only rule-coverage command that accepts a source world and one or more transformation rule documents, selects the source profile from the loaded rule graph, and supports Forge 1.7.10 and Forge 1.2.5 sources. The command SHALL NOT require a target template or output-world path and SHALL NOT modify the world or rule documents.

#### Scenario: Analyze a valid source world
- **WHEN** the caller supplies a world compatible with the source profile selected by valid rule documents
- **THEN** the command inventories the profile's supported block and item locations and evaluates their rule coverage without creating or changing world data

#### Scenario: Reject invalid analysis inputs
- **WHEN** the selected source profile is unsupported, the world is structurally incompatible with it, or a rule document is invalid
- **THEN** the command fails with a contextual diagnostic distinct from an incomplete-coverage result

### Requirement: Distinguish built-in vanilla coverage from explicit rule coverage
The system SHALL consider blocks and items in the selected source profile's authoritative built-in vanilla catalog covered by built-in migration behavior without requiring explicit rules. Every encountered non-vanilla block or item SHALL be considered covered only when its identity resolves through authoritative source evidence and an applicable explicit transformation rule is selected. During rules coverage, an encountered numeric block or item that cannot resolve through the selected profile's built-in, world, or manifest catalog SHALL be classified as uncovered rather than preventing the remaining source inventory from being analyzed.

#### Scenario: Vanilla object has no explicit rule
- **WHEN** an encountered block or item belongs to the selected profile's built-in vanilla catalog and has no selected explicit rule
- **THEN** the object is classified as covered by built-in vanilla migration

#### Scenario: Modded object has a selected rule
- **WHEN** an encountered Forge 1.2.5 mod object resolves through the source manifest and selects an applicable explicit rule
- **THEN** the object is classified as covered and the selected rule identifier is recorded

#### Scenario: Modded object has no selected rule
- **WHEN** an encountered non-vanilla block or item resolves to an identity but selects no explicit transformation rule
- **THEN** the object is classified as uncovered even if its registry identity could otherwise remain unchanged

#### Scenario: Numeric object has no source mapping
- **WHEN** rules coverage encounters a numeric block or item that cannot resolve through the selected profile's built-in, world, or manifest catalog
- **THEN** the object is classified as uncovered with a stable numeric fallback identity and analysis continues through the remaining source inventory

### Requirement: Report unresolved numeric coverage signatures
Rules coverage SHALL group unresolved numeric blocks and items using the same deterministic signature, occurrence-count, and location-sampling behavior as other uncovered objects. Each unresolved group SHALL identify the object kind and numeric ID, use a stable numeric fallback identity, state that the source registry mapping is missing, and include at most the first five distinct locations in canonical order. The completed report SHALL be incomplete and the command SHALL exit with status 1 when one or more unresolved groups are present. Unresolved handling by conversion and failures that prevent a complete source inventory SHALL remain errors.

#### Scenario: Multiple unresolved identities occur in one world
- **WHEN** rules coverage encounters multiple numeric block or item identities that have no source mapping
- **THEN** it analyzes the complete source inventory and emits a distinct uncovered signature group for each unresolved kind, numeric ID, metadata or damage, and object data signature

#### Scenario: Unresolved signature occurs repeatedly
- **WHEN** equivalent unresolved objects occur at multiple positions or the same position is observed more than once
- **THEN** their group reports every raw occurrence and at most the first five distinct canonical locations

#### Scenario: Coverage contains unresolved signatures
- **WHEN** analysis completes with one or more unresolved numeric signature groups
- **THEN** the command emits the complete coverage report and exits with status 1 rather than an execution-error status

#### Scenario: Conversion encounters an unresolved numeric object
- **WHEN** conversion encounters a numeric block or item that cannot resolve through authoritative source evidence
- **THEN** conversion remains failed and does not treat the object as ordinary incomplete rule coverage

#### Scenario: Coverage inventory cannot complete
- **WHEN** invalid inputs, decoding failures, traversal failures, or other execution errors prevent rules coverage from completing the source inventory
- **THEN** the command fails with an execution-error status rather than emitting an incomplete-coverage result as though the inventory were complete

### Requirement: Preserve known block context in coverage locations
Rules coverage SHALL retain the block coordinates of each encountered block and SHALL propagate a block-located inventory owner's coordinates to its contained items and their recursively discovered nested items. Uncovered report locations SHALL include every available location component for the affected block or item, including file, dimension, chunk, block coordinates, and NBT path. The system MUST omit a location component when traversal cannot establish it and MUST NOT invent or approximate block coordinates.

#### Scenario: Unresolved block reports its coordinates
- **WHEN** rules coverage encounters a numeric block that cannot resolve through the selected source catalogs
- **THEN** its uncovered signature location identifies the block's file, dimension, chunk, block coordinates, and NBT path

#### Scenario: Item inherits its block-located container coordinates
- **WHEN** rules coverage discovers an item inside an inventory owner with known block coordinates
- **THEN** the item's coverage location contains those block coordinates together with its own NBT path

#### Scenario: Nested item retains containing block coordinates
- **WHEN** rules coverage recursively discovers an item beneath another item whose location contains known block coordinates
- **THEN** the nested item's coverage location retains those block coordinates and identifies the nested item's NBT path

#### Scenario: Item has no known containing block
- **WHEN** rules coverage discovers an item in a context for which traversal cannot establish block coordinates
- **THEN** its coverage report location omits block coordinates while retaining every other available location component

### Requirement: Report uncovered signatures and every occurrence
The command SHALL emit a deterministic machine-readable report containing the selected source profile, report completeness, coverage counts, rule-set identifiers, warnings, validation findings, and uncovered signature groups. It SHALL NOT include target-registry mappings, migration outcomes, target identities, staging estimates, complete covered-object records, or per-file disposition records. Each uncovered group SHALL be distinguished by kind, source identity, numeric representation, metadata or damage, and SNBT object data. For an uncovered block with associated block-entity data, the signature and reported associated SNBT SHALL omit only the block entity's top-level lowercase `x`, `y`, and `z` fields; rule evaluation SHALL continue to use the complete unmodified block-entity NBT. Each group SHALL report the exact number of raw observations classified under that signature and SHALL include at most the first five distinct locations in canonical order. Multiple observations of the same location SHALL contribute separately to the occurrence count.

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

#### Scenario: Coverage report excludes migration audit data
- **WHEN** rules coverage completes with covered or uncovered source objects
- **THEN** its JSON contains only source-focused rule-authoring data and omits migration-report and per-file audit fields

### Requirement: Explain rule rejection
Every uncovered signature SHALL include a deterministic trace of the relevant candidate rules and the reasons they did not match.

#### Scenario: Metadata predicate rejects a rule
- **WHEN** a rule's identity matches an object but its metadata or damage predicate does not
- **THEN** the report identifies that rule and the failed predicate in the rejection trace

#### Scenario: No rule targets the identity
- **WHEN** no rule is a candidate for an uncovered identity
- **THEN** the report states that no candidate rule targets the object kind and identity

### Requirement: Disclose nested-item discovery limits
The command SHALL scan standard item locations and custom nested item paths declared by applicable rules. It SHALL warn that items at undeclared mod-specific paths cannot be established as covered by the analysis.

#### Scenario: Rules declare a nested inventory path
- **WHEN** an applicable rule declares a custom nested item path
- **THEN** items discovered at that path participate in the same coverage analysis and location reporting as standard items

#### Scenario: Custom traversal cannot be proven complete
- **WHEN** the analysis cannot know whether additional mod-specific item paths exist
- **THEN** the report contains an explicit coverage-boundary warning

### Requirement: Return coverage-specific exit status
The command SHALL exit with status 0 when no uncovered objects are found and status 1 when one or more uncovered objects are found. Invalid input, decoding failures, and other execution errors SHALL use an error status other than 1.

#### Scenario: Coverage is complete
- **WHEN** every encountered object is covered by built-in vanilla behavior or a selected explicit rule
- **THEN** the command exits with status 0

#### Scenario: Coverage is incomplete
- **WHEN** at least one uncovered signature is reported
- **THEN** the command emits the complete report and exits with status 1

#### Scenario: Analysis cannot complete
- **WHEN** an input or execution error prevents a complete inventory
- **THEN** the command emits a contextual diagnostic and exits with a status other than 0 or 1
