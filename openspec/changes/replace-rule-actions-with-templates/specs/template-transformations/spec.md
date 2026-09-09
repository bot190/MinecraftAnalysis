## Purpose

Define lossless, bounded template execution for transforming matched Minecraft objects while keeping applicability deterministic and inexpensive to determine.

## ADDED Requirements

### Requirement: Matcher-selected transformation templates
Each transformation rule SHALL retain a structured object matcher and SHALL contain a transformation template instead of an action or patch collection. The system SHALL index rules by object kind and resolved source identity, evaluate metadata and typed-NBT predicates only among identity-compatible candidates, and render templates only for selected rules.

#### Scenario: Identity index limits candidate evaluation
- **WHEN** an object is evaluated against a loaded graph containing rules for multiple source identities
- **THEN** only rules indexed for that object's kind and resolved source identity proceed to predicate evaluation and possible template rendering

#### Scenario: Unmatched template is not rendered
- **WHEN** a rule's structured matcher does not match an object
- **THEN** its transformation template is not rendered

### Requirement: Lossless template values
Templates SHALL receive every original NBT value and emit transformation results without losing distinctions among byte, short, int, long, float, double, string, compound, list element type, byte array, int array, and long array tags. Template output that cannot be decoded into the required typed result SHALL fail with the responsible rule and template context.

#### Scenario: Numeric tags remain distinct
- **WHEN** a template copies byte, int, and long values with equal mathematical values from its input to its result
- **THEN** the result retains each original NBT tag type

#### Scenario: Invalid typed output
- **WHEN** a selected template emits a value that is not a valid result for its object kind or contains an invalid typed-NBT representation
- **THEN** conversion fails before publishing any partially transformed object

### Requirement: Original and composed template contexts
Every selected template SHALL receive an immutable `original` value representing the object state before any selected template ran and a `current` value representing the result composed by preceding selected templates. A block template SHALL receive and produce the terrain block and its optional colocated block entity together.

#### Scenario: Later template observes both states
- **WHEN** multiple non-terminal rules match and an earlier template changes the current value
- **THEN** a later template can read the unchanged original value and the composed current value

#### Scenario: Coordinate-owned block entity is coordinated
- **WHEN** a selected block template changes both the block and its colocated block entity
- **THEN** both results are validated and applied as one transformation without exposing either partial result

#### Scenario: Block entity presence changes
- **WHEN** a block template produces a block entity where the original had none or produces no block entity where the original had one
- **THEN** the coordinated result explicitly creates or removes the block entity

### Requirement: Value-map template function
The template environment SHALL expose a `value_map(map_id, value)` function that resolves document-level typed value maps. Matching SHALL remain type-sensitive unless the selected map enables numeric coercion, the declared destination SHALL determine the exact output type, and an unknown map or unmapped input SHALL fail with the rule ID, map ID, and complete typed input. The environment SHALL NOT expose an item-ID-to-name function.

#### Scenario: Template maps a typed value
- **WHEN** a selected template calls `value_map` with a declared map and matching input
- **THEN** the function returns that entry's explicitly typed destination value

#### Scenario: Template value is unmapped
- **WHEN** a selected template calls `value_map` with no applicable entry
- **THEN** transformation fails without applying the template's partial result

### Requirement: Bounded and deterministic execution
Rule loading SHALL compile every template and reject syntax or statically detectable configuration failures before world traversal. Rendering SHALL use strict undefined-value behavior, SHALL expose no filesystem, process, network, clock, randomness, or arbitrary host-language access, and SHALL enforce bounded computation, recursion, and output size. The same loaded rules and input values SHALL produce byte-for-byte equivalent typed results and diagnostics.

#### Scenario: Invalid template fails preparation
- **WHEN** a rule contains invalid template syntax
- **THEN** preparation fails and identifies the document and rule before source traversal begins

#### Scenario: Undefined input fails rendering
- **WHEN** a selected template reads an unavailable value without explicitly handling its absence
- **THEN** rendering fails with rule and template context instead of silently producing an empty value

#### Scenario: Execution limit is exceeded
- **WHEN** rendering exceeds a configured computation, recursion, or output bound
- **THEN** transformation fails deterministically without publishing partial output

