# Standalone Inventory Discovery Specification

## Purpose

Discover item inventories in vanilla and mod-defined standalone NBT layouts without allowing an incompatible inventory shape to abort processing of otherwise valid world data.

## Requirements

### Requirement: Built-in standalone inventories remain discoverable
The system SHALL inspect the built-in standalone inventory paths `Inventory` and `EnderItems` and SHALL process a present homogeneous list of item compounds through the normal item analysis and conversion pipeline.

#### Scenario: Standard player inventory
- **WHEN** a standalone NBT document contains a compound-item list at `Inventory`
- **THEN** the system analyzes and converts each item in that list through the normal item pipeline

#### Scenario: Built-in inventory path is absent
- **WHEN** a standalone NBT document does not contain a built-in inventory path
- **THEN** the system continues without reporting that absence as a validation finding

### Requirement: Incompatible inventory shapes are non-fatal validation findings
When a built-in or rule-declared inventory path is present but cannot be parsed as a homogeneous list of item compounds, the system SHALL record a structured validation finding for that path, SHALL skip item processing at that path, and SHALL continue processing other paths and files. The finding SHALL identify the file, NBT path, expected inventory shape, and observed incompatibility.

#### Scenario: Built-in inventory is a mod container
- **WHEN** a standalone NBT document contains a compound at `Inventory` instead of an item list
- **THEN** the system records a validation finding for `Inventory`, skips that built-in path, and continues processing the document

#### Scenario: Inventory list contains an incompatible element
- **WHEN** a configured inventory path resolves to a list containing a value that is not an item compound
- **THEN** the system records a validation finding, does not partially process that inventory path, and continues processing other paths and files

#### Scenario: Other paths remain usable after a validation finding
- **WHEN** one configured inventory path has an incompatible shape and another configured path in the same document is a valid item list
- **THEN** the system skips the incompatible path and processes the valid path

### Requirement: Rule sets declare standalone inventory paths
The rule-set format SHALL allow an inventory discovery declaration to identify an NBT path relative to the root compound of a standalone NBT document. A declared path that resolves to a homogeneous list of item compounds SHALL feed those items through the same analysis and conversion pipeline as built-in inventories.

#### Scenario: Discover a mod-wrapped inventory
- **WHEN** a rule set declares the path `["Inventory", "Items"]` and a standalone NBT document contains an item list at that path
- **THEN** the system analyzes and converts the list's items through the normal item pipeline

#### Scenario: Declared path is absent
- **WHEN** a rule-declared inventory path is absent from a standalone NBT document
- **THEN** the system continues without reporting the absence as a validation finding

#### Scenario: Imported rule set declares an inventory path
- **WHEN** a loaded rule set imports another rule set containing a standalone inventory declaration
- **THEN** the imported declaration participates in inventory discovery

### Requirement: Inventory discovery is deterministic and bounded
The system SHALL canonicalize built-in and rule-declared inventory paths before traversal, SHALL process an inventory list at most once when declarations repeat or overlap at the same resolved list, and SHALL apply configured nested-object resource limits to discovered items.

#### Scenario: Duplicate path declarations
- **WHEN** the effective rule set declares the same inventory path more than once
- **THEN** each item at that path is analyzed and converted exactly once

#### Scenario: Discovery exceeds an object limit
- **WHEN** valid declared inventories would exceed the configured nested-object limit
- **THEN** the operation fails with the existing resource-limit diagnostic rather than continuing without a bound

### Requirement: Analysis and conversion use the same effective inventory paths
The system SHALL use the same canonical built-in and rule-declared path set and the same inventory-shape acceptance rules during analysis and conversion.

#### Scenario: Analyzed mod inventory is converted
- **WHEN** analysis discovers items through a rule-declared standalone inventory path and conversion proceeds with the same inputs
- **THEN** conversion visits those same items at that path

#### Scenario: Invalid path is preserved
- **WHEN** an inventory path has an incompatible shape and is reported as a validation finding
- **THEN** conversion preserves the NBT value at that path without applying item transformations to it
