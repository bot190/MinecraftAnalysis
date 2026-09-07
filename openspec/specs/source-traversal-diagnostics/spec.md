# source-traversal-diagnostics Specification

## Purpose

Provide actionable, deterministic diagnostics that identify the exact region-chunk NBT location and type conflict responsible for a source traversal failure.

## Requirements

### Requirement: Identify the failing chunk
When traversal of a decoded region chunk fails, the system SHALL identify the region path, dimension, global chunk coordinates, and region-local chunk coordinates in the resulting diagnostic.

#### Scenario: Traversal fails inside a region chunk
- **WHEN** analysis decodes a chunk but cannot traverse a recognized NBT structure within it
- **THEN** the diagnostic identifies the region path, dimension, global chunk coordinates, and local coordinates in the range 0 through 31

#### Scenario: Failure propagates through parallel analysis
- **WHEN** a region worker returns a chunk traversal failure
- **THEN** the command-level diagnostic preserves the location of the selected failing work item regardless of worker completion order

### Requirement: Identify the complete NBT location
The system SHALL report the complete canonical NBT path of an incompatible recognized value and SHALL include available owning-object context.

#### Scenario: Nested item has an incompatible type
- **WHEN** an entity or block entity contains a recognized item location whose value has an incompatible type
- **THEN** the diagnostic includes the complete path from the chunk root through the owning list index and item field

#### Scenario: Owning object has identifying context
- **WHEN** the owning entity or block entity provides an identity or block coordinates
- **THEN** the diagnostic includes the available object kind, identity, and coordinates

#### Scenario: Singular item field fails
- **WHEN** a singular `Item` field has an incompatible value
- **THEN** its path ends in `.Item` and does not append a synthetic list index

### Requirement: Explain NBT type conflicts
An incompatible-type diagnostic SHALL identify the expected NBT tag and the actual NBT tag and SHALL provide a bounded value preview when that value can be rendered safely.

#### Scenario: Scalar appears where a compound is required
- **WHEN** traversal expects a compound and encounters a scalar NBT value
- **THEN** the diagnostic names `Compound` as expected, names the scalar's exact tag type as actual, and includes its value in a bounded preview

#### Scenario: Large or deeply nested value conflicts
- **WHEN** the incompatible value cannot be included within the diagnostic preview bound
- **THEN** the diagnostic still reports expected and actual tags and truncates or omits the preview without traversing the value without a bound

### Requirement: Treat structural and inferred locations according to confidence
The system SHALL fail traversal when required recognized chunk structure is incompatible. When a field is considered an item location only by a generic field-name inference and has an incompatible type, the system SHALL record a validation finding with the same location and type context and continue analyzing other source observations.

#### Scenario: Required section structure is malformed
- **WHEN** a recognized chunk section or required entity-list container has an incompatible type
- **THEN** source traversal fails with a location-aware diagnostic

#### Scenario: Mod-specific field reuses an inferred item name
- **WHEN** an otherwise traversable entity or block entity uses a generically inferred item field name for a non-item value
- **THEN** analysis records a validation finding and continues without interpreting that value as an item stack

#### Scenario: Declared inventory path is incompatible
- **WHEN** an explicit rule or profile declares a value as an inventory location and the value violates that declaration
- **THEN** traversal applies the existing strict declared-path failure behavior with the improved location and type context

### Requirement: Keep diagnostics deterministic
Equivalent source data and configuration SHALL produce the same traversal diagnostic independent of configured region-worker concurrency.

#### Scenario: Multiple workers encounter failures
- **WHEN** multiple admitted region work items fail in different completion orders
- **THEN** repeated analysis selects and renders the same canonical failure
