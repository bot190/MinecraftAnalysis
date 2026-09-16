## MODIFIED Requirements

### Requirement: Incompatible inventory shapes are non-fatal validation findings
When a built-in standalone inventory path is present but cannot be parsed as a homogeneous list of item compounds, the system SHALL record a structured validation finding for that path, SHALL skip item processing at that path, and SHALL continue processing other built-in paths and files. The finding SHALL identify the file, NBT path, expected inventory shape, and observed incompatibility.

#### Scenario: Built-in inventory is a mod container
- **WHEN** a standalone NBT document contains a compound at `Inventory` instead of an item list
- **THEN** the system records a validation finding for `Inventory`, skips that built-in path, and continues processing the document

#### Scenario: Inventory list contains an incompatible element
- **WHEN** a built-in inventory path resolves to a list containing a value that is not an item compound
- **THEN** the system records a validation finding, does not partially process that inventory path, and continues processing the other built-in path and files

#### Scenario: Other paths remain usable after a validation finding
- **WHEN** one built-in inventory path has an incompatible shape and the other built-in path in the same document is a valid item list
- **THEN** the system skips the incompatible path and processes the valid path

### Requirement: Inventory discovery is deterministic and bounded
The system SHALL process the built-in `Inventory` and `EnderItems` paths in deterministic canonical order, SHALL process each resolved inventory list at most once, and SHALL apply configured nested-object resource limits to discovered items. Rule documents SHALL NOT add standalone inventory paths.

#### Scenario: Duplicate path declarations
- **WHEN** a new-schema rule document declares a configurable standalone inventory path, whether once or repeatedly
- **THEN** rule preparation rejects the removed declaration rather than adding it to the effective paths

#### Scenario: Discovery exceeds an object limit
- **WHEN** valid built-in inventories would exceed the configured nested-object limit
- **THEN** the operation fails with the existing resource-limit diagnostic rather than continuing without a bound

### Requirement: Analysis and conversion use the same effective inventory paths
The system SHALL use the same canonical built-in `Inventory` and `EnderItems` path set and the same inventory-shape acceptance rules during analysis and conversion. No configurable standalone inventory path SHALL participate in either operation.

#### Scenario: Analyzed mod inventory is converted
- **WHEN** a mod-specific inventory exists outside the built-in `Inventory` and `EnderItems` paths
- **THEN** standalone analysis does not discover it and standalone conversion does not treat it as an item inventory

#### Scenario: Invalid path is preserved
- **WHEN** a built-in inventory path has an incompatible shape and is reported as a validation finding
- **THEN** conversion preserves the NBT value at that path without applying item transformations to it

## REMOVED Requirements

### Requirement: Rule sets declare standalone inventory paths
**Reason**: Configurable standalone inventory paths require tool-owned knowledge of custom NBT layouts. This change reserves custom layout handling for explicit templates and defers standalone-document templates to a future change.

**Migration**: Remove every `standalone_inventories` declaration. Built-in `Inventory` and `EnderItems` paths continue to work; custom standalone layouts are not converted until standalone-document templates are introduced.
