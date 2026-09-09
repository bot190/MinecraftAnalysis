# Forge World Conversion Specification

## Purpose

Define a safe and auditable conversion of vanilla or modded Forge 1.7.10 Anvil worlds into worlds compatible with the registry assignments and storage conventions of a supplied Forge 1.12.2 template world.

## Requirements

### Requirement: Explicit conversion inputs
The converter SHALL require a source world, a distinct Forge 1.12.2 template world, a distinct output path, and one or more transformation rule documents. It SHALL reject path configurations in which the output is the source, the template, or an ancestor containing either input.

#### Scenario: Valid paths are accepted
- **WHEN** the caller supplies separate readable source and template worlds and a non-conflicting output path
- **THEN** the converter accepts the paths for preflight validation

#### Scenario: In-place conversion is rejected
- **WHEN** the requested output path would overwrite or contain either input world
- **THEN** the converter fails before creating or modifying output data

### Requirement: Version and format validation
Before conversion, the converter SHALL load the rules, resolve their selected source profile, validate that terrain uses the profile's supported Anvil structures, and validate that the template represents a supported Forge 1.12.2 world. It SHALL support existing Forge 1.7.10 sources and rule-selected Forge 1.2.5 sources. Validation failures SHALL identify the incompatible or missing structure or declaration. Content-specific failures encountered after staging begins SHALL abort conversion and prevent publication.

#### Scenario: Supported rule-selected source
- **WHEN** the loaded rules select Forge 1.2.5 and the source satisfies that profile's structural requirements
- **THEN** the converter accepts the source without requiring Forge 1.7.10 registry metadata

#### Scenario: Unsupported source version
- **WHEN** the loaded rules select a source profile the converter does not support or the source is structurally incompatible with the selected profile
- **THEN** the converter rejects the source before creating staging with a profile-specific diagnostic

#### Scenario: Missing Forge registry data
- **WHEN** conversion encounters a numeric identity that the selected source profile's built-in catalog, world registry evidence, and rule manifests cannot resolve
- **THEN** the region or standalone-file work unit returns the unresolved registry error and conversion does not publish an output world

### Requirement: World-local registry resolution
The converter SHALL resolve each numeric block and item identity through the selected source profile's authoritative catalog, transform the resulting registry name, and resolve that name through the template world's target registry mappings. A Forge 1.7.10 catalog SHALL use persisted world mappings with verified vanilla fallbacks; a Forge 1.2.5 catalog SHALL use complete built-in vanilla mappings plus rule-manifest modpack mappings. The converter MUST NOT treat modded numeric IDs as globally stable or infer identity from numeric equality.

#### Scenario: Numeric IDs differ between worlds
- **WHEN** a source block ID resolves to `example:machine` and the template assigns `example:machine` a different numeric ID
- **THEN** the output block uses the template's numeric ID while preserving or transforming its metadata and associated NBT as specified

#### Scenario: Target identity is absent
- **WHEN** an encountered source identity has no applicable transformation and no matching target registry entry
- **THEN** the converter records an unresolved identity and does not publish an output world under the default policy

#### Scenario: Rule manifest supplies historical identity
- **WHEN** a Forge 1.2.5 source numeric ID resolves through the loaded source manifest
- **THEN** all occurrences use that declared semantic identity before rule matching and target resolution

### Requirement: Pre-flattening block storage conversion
The converter SHALL read and write pre-flattening Anvil chunk sections, including `Blocks`, `Data`, optional `Add`, block light, sky light, and unknown chunk fields. It SHALL preserve block position and correctly encode target block IDs across the full 0 through 4095 representable range.

#### Scenario: Extended numeric block ID
- **WHEN** a transformed block receives a target numeric ID greater than 255
- **THEN** the converter encodes the high bits in `Add` and the low bits in `Blocks` without changing its position or metadata

#### Scenario: Unchanged chunk fields
- **WHEN** a chunk contains fields outside the converter's recognized transformation targets
- **THEN** those fields retain their typed NBT values in the output chunk

### Requirement: Complete supported world traversal
The converter SHALL process terrain blocks, block entities, entities, player data, and item stacks in the standard locations declared by the selected source profile, including inventories, ender storage, equipment, dropped items, and nested item containers identified by transformation rules. It SHALL apply conversion across every dimension present in the source.

#### Scenario: Item appears in multiple storage locations
- **WHEN** the same source item identity appears in standard player, block-entity, and dropped-item locations for the selected profile
- **THEN** all occurrences are resolved and transformed using the same applicable item rules

#### Scenario: Custom dimension is present
- **WHEN** the source contains a mod-defined dimension directory with supported Anvil regions
- **THEN** the converter processes its regions under the same profile validation and transformation rules as other dimensions

#### Scenario: Profile-specific player layout is present
- **WHEN** a selected source profile stores player NBT under its standard legacy or modern player directory
- **THEN** the converter discovers and processes each supported player file exactly once

### Requirement: Typed NBT preservation
The converter SHALL preserve the distinction among all NBT scalar, list, compound, byte-array, int-array, and long-array types. Unknown NBT fields SHALL survive conversion unless an explicit transformation removes or replaces them.

#### Scenario: Equal-valued numeric tags have different types
- **WHEN** preserved data contains byte, short, int, and long tags whose numeric values are equal
- **THEN** each output tag retains its original NBT type

#### Scenario: Mod capability is not understood
- **WHEN** an item or entity contains an unrecognized Forge capability compound
- **THEN** the complete compound is copied unchanged unless a rule targets it

### Requirement: Non-destructive and transactional output
The converter SHALL never modify the source or template world. It SHALL build output in a staging location and publish it only after every staged work unit has converted and locally verified its content; failed runs SHALL not leave a path that appears to be a completed output world.

#### Scenario: Failure during region conversion
- **WHEN** conversion fails after some staged regions have been written
- **THEN** both inputs remain byte-for-byte unchanged, diagnostic staging remains available, and the final output path is not published as successful

#### Scenario: Successful publication
- **WHEN** every source entry has been converted or copied and every transformed file has passed local verification
- **THEN** the completed staged world is made available at the requested output path

### Requirement: Preserve unrelated world content
The converter SHALL copy source files that are not transformed, except transient lock files and documented version-specific files that must be regenerated or omitted. The report SHALL state the disposition of every skipped or specially handled file.

#### Scenario: Mod-specific data file
- **WHEN** the source contains an unrecognized file under its world directory
- **THEN** the file is copied byte-for-byte to the corresponding output location

### Requirement: Deterministic migration report
Every successful conversion SHALL produce a machine-readable report containing the selected source profile, input fingerprints, rule-set identity, registry mappings used, counts by object disposition, warnings, object locations, file dispositions, and a success outcome. A failed conversion report SHALL identify the fatal error and contain the report contributions from work successfully reduced before the deterministic failure boundary; it is not required to describe unprocessed source content. Repeated successful conversions of the same inputs and rules SHALL produce semantically identical reports apart from explicitly documented runtime metadata.

#### Scenario: Unresolved block is reported
- **WHEN** a source block cannot be mapped to a target registry entry
- **THEN** the fatal diagnostic and partial report identify the selected source profile, source registry name or numeric identity, metadata, dimension, chunk, and block coordinates

#### Scenario: Unresolved block stops conversion
- **WHEN** a source block cannot be mapped to a target registry entry
- **THEN** the error and partial report identify the block's source identity and location, conversion stops admitting new work, and no output world is published

#### Scenario: Dry run
- **WHEN** the caller requests validation without conversion
- **THEN** the converter performs complete profile-aware registry and rule analysis, writes a complete report, and does not create a world output

### Requirement: Output structural verification
Before committing each transformed NBT or region file to its staged destination, the converter SHALL reopen and validate the temporary emitted content and SHALL verify that emitted block and item IDs exist in the target registry mappings. The system SHALL NOT require a separate world-wide verification phase before publication.

#### Scenario: Written region cannot be reopened
- **WHEN** a temporary staged region fails structural validation
- **THEN** its work unit returns an error, the file is not committed, and the output world is not published

#### Scenario: Locally verified conversion completes
- **WHEN** every transformed file is reopened and verified successfully within its conversion work unit
- **THEN** publication may proceed without traversing all staged regions in a separate verification phase
