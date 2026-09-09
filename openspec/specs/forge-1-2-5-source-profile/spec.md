# Forge 1.2.5 Source Profile Specification

## Purpose

Define the safe interpretation of Forge 1.2.5 Anvil worlds whose vanilla identities are known by the converter and whose modpack-specific numeric assignments are declared by transformation rules.

## Requirements

### Requirement: Declared Forge 1.2.5 source profile
The system SHALL accept `forge-1.2.5` as a rule-selected source profile and SHALL validate the supplied world for the supported Anvil container, chunk-section, NBT, and numeric storage structures before treating it as that profile. The declaration is a caller assertion of the historical version; the system SHALL NOT claim to identify the exact modpack automatically.

#### Scenario: Structurally compatible world is accepted
- **WHEN** the complete rule graph selects `forge-1.2.5` and the source contains compatible `level.dat` metadata and Anvil terrain
- **THEN** the system validates it using the Forge 1.2.5 source profile

#### Scenario: Incompatible structure is rejected
- **WHEN** a rule graph selects `forge-1.2.5` but a required region, chunk section, or recognized NBT structure is incompatible
- **THEN** the system fails with a path-specific structural diagnostic and does not publish output

### Requirement: Complete built-in vanilla catalog
The Forge 1.2.5 source profile SHALL provide verified built-in mappings for every supported vanilla 1.2.5 block and item numeric identity. Built-in vanilla assignments SHALL be authoritative and MUST NOT be replaced by a contradictory rule manifest.

#### Scenario: Vanilla identity needs no manifest entry
- **WHEN** a vanilla 1.2.5 block or item is encountered and no source-manifest entry repeats its assignment
- **THEN** the object resolves through the built-in vanilla catalog

#### Scenario: Manifest contradicts vanilla assignment
- **WHEN** a source manifest assigns a different identity to a numeric block or item slot owned by the built-in vanilla catalog
- **THEN** rule preparation fails even if the manifest requests conflict selection

### Requirement: Rule-defined modpack catalog
The Forge 1.2.5 source profile SHALL construct the non-vanilla portion of its source registry from `source_manifest` block and item entries in the loaded rule graph. Block and item registries SHALL remain distinct, and the system SHALL reject contradictory assignments before traversal.

#### Scenario: Mod block resolves from manifest
- **WHEN** a source manifest assigns a modpack block identity to an encountered numeric block ID
- **THEN** conversion and coverage analysis resolve that block to the declared identity before evaluating transformation rules

#### Scenario: Block and item share a numeric ID
- **WHEN** source manifests assign block and item identities at the same numeric value
- **THEN** each identity resolves independently in its declared registry

#### Scenario: Encountered mod ID is undeclared
- **WHEN** traversal encounters a non-vanilla block or item numeric ID absent from the complete loaded source manifest
- **THEN** the operation fails with the kind, numeric ID, and object location instead of inferring an identity

### Requirement: Standard Forge 1.2.5 world traversal
The Forge 1.2.5 source profile SHALL enumerate and process standard terrain blocks, block entities, entities, legacy player files, item stacks, and dimensions represented by supported 1.2.5 world layouts. Unknown typed NBT fields SHALL remain preserved unless a rule explicitly changes them.

#### Scenario: Legacy player inventory is processed
- **WHEN** a numeric item stack appears in a standard `players/` inventory for a Forge 1.2.5 source
- **THEN** it is resolved and transformed using the same catalog and item rules as stacks in terrain containers

#### Scenario: Mod-specific nested storage is undeclared
- **WHEN** a mod stores items outside standard profile locations and no applicable rule declares that nested path
- **THEN** the system preserves the data without guessing that it is an inventory and discloses the traversal boundary in analysis
