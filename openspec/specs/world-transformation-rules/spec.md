# World Transformation Rules Specification

## Purpose

Define deterministic, declarative rules that can express vanilla and mod-specific identity, metadata, and typed-NBT changes required when migrating world objects from Forge 1.7.10 to Forge 1.12.2.

## Requirements

### Requirement: Rule-selected source profile
Rule documents SHALL be able to declare a supported source profile. Across all explicitly supplied documents and their imports, the system SHALL resolve exactly one distinct source-profile declaration before source validation, conversion, or coverage analysis; imported profile-neutral documents MAY omit the declaration. The CLI SHALL NOT provide a source-profile override.

#### Scenario: Entry document selects a profile
- **WHEN** one loaded rule document declares `forge-1.2.5` and its imported reusable documents omit a profile
- **THEN** the complete loaded rule graph selects the Forge 1.2.5 source profile

#### Scenario: Loaded documents disagree
- **WHEN** loaded rule documents declare more than one distinct source profile
- **THEN** rule preparation fails with diagnostics identifying the conflicting documents and declarations

#### Scenario: No source profile is available
- **WHEN** conversion or coverage preparation cannot derive a source profile from the loaded rule documents or a backwards-compatible legacy rule schema
- **THEN** preparation fails before traversing source content

#### Scenario: CLI cannot replace the declaration
- **WHEN** a caller invokes conversion or coverage commands
- **THEN** source-profile selection is available only through the loaded rule graph

### Requirement: Versioned rule documents
The converter SHALL accept human-editable, machine-validatable rule documents carrying a rule-schema version and a stable rule-set identifier. The supported schema SHALL represent source-profile declarations while preserving documented compatibility for existing Forge 1.7.10 rule documents. Unsupported schema versions and duplicate rule identifiers SHALL be rejected during preparation.

#### Scenario: Supported document loads
- **WHEN** a document declares a supported schema version and contains valid uniquely identified rules
- **THEN** the converter includes it in the ordered rule set

#### Scenario: Existing rule document loads compatibly
- **WHEN** an existing supported rule document predates explicit source-profile declarations
- **THEN** the system preserves its Forge 1.7.10 source behavior without requiring a CLI option

#### Scenario: Unknown schema version
- **WHEN** a rule document declares an unsupported schema version
- **THEN** preparation fails with the supported version range

### Requirement: Source and target registry manifests
Rule documents SHALL be able to supplement source and target mappings without silently replacing contradictory registry evidence. For Forge 1.2.5 sources, source manifests SHALL define modpack-specific numeric block and item assignments while the profile's complete built-in vanilla assignments remain authoritative. Any other contradiction SHALL require an explicit conflict-resolution declaration where the selected profile permits replacement.

#### Scenario: Missing historical registry mapping is supplied
- **WHEN** an external manifest assigns an unresolved source numeric ID to a namespaced identity without contradicting authoritative profile or world evidence
- **THEN** rules may match that resolved identity

#### Scenario: Manifest contradicts world registry
- **WHEN** a manifest and replaceable source-world evidence assign different names to the same numeric registry entry
- **THEN** preparation fails unless the document explicitly selects the intended source

#### Scenario: Manifest contradicts built-in vanilla registry
- **WHEN** a manifest conflicts with an authoritative built-in vanilla assignment for the selected source profile
- **THEN** preparation fails and conflict selection cannot replace the vanilla assignment

### Requirement: Block matching and transformation
Rules SHALL match blocks by namespaced identity or explicitly qualified legacy numeric ID, with optional exact, wildcard, masked, or ranged metadata constraints. A block transformation SHALL be able to change identity and metadata while independently transforming its block entity.

#### Scenario: Metadata-specific mod block conversion
- **WHEN** a block's registry identity and metadata satisfy a rule
- **THEN** the rule's target identity and metadata transformation are applied before target numeric ID resolution

#### Scenario: Legacy numeric matcher is ambiguous
- **WHEN** a numeric matcher does not identify its source registry context and could match more than one registry meaning
- **THEN** rule validation rejects the matcher

### Requirement: Item matching and transformation
Rules SHALL match item stacks by namespaced identity or explicitly qualified legacy numeric ID and SHALL support predicates and transformations for count, damage, and typed NBT. Item rules SHALL apply independently of block rules even when a block and item share a registry name.

#### Scenario: Damage value is preserved
- **WHEN** an item rule changes identity but does not modify damage
- **THEN** the source damage value is retained

#### Scenario: Block and item use different mappings
- **WHEN** a block and item share a source registry name but their applicable rules specify different targets
- **THEN** each is transformed according to its own object kind

### Requirement: Entity and block-entity transformation
Rules SHALL match entity and block-entity identities and SHALL support renaming their identities and patching their typed NBT. Rules SHALL be able to coordinate a block transformation with the block entity stored at the same coordinates.

#### Scenario: Block entity schema changes
- **WHEN** a matched machine block has an associated block entity targeted by the rule
- **THEN** the block and block entity identity and NBT changes are applied as one conversion decision

### Requirement: Typed NBT predicates and patches
Rules SHALL support typed predicates for existence, absence, equality, numeric range, string pattern, compound paths, and list elements. Patches SHALL support setting a typed value, removing, renaming, copying, moving, and applying checked numeric conversions without converting untouched values through JSON types.

#### Scenario: Type-sensitive equality
- **WHEN** a predicate expects a byte tag with value `1` but the source contains an int tag with value `1`
- **THEN** the predicate does not match unless it explicitly permits numeric type coercion

#### Scenario: Checked numeric conversion overflows
- **WHEN** a patch converts a numeric value to a narrower NBT type and the value is outside that type's range
- **THEN** the object is unresolved and the original value is reported without truncation

### Requirement: Nested item discovery rules
Rules SHALL be able to identify item-stack lists or compounds at mod-specific NBT paths and invoke normal item transformation recursively. Recursive traversal SHALL enforce configurable depth and object-count limits and detect cycles in rule invocation.

#### Scenario: Backpack contains nested items
- **WHEN** a matched backpack item declares its internal item list path in a rule
- **THEN** each contained stack is processed by the normal item rule set

#### Scenario: Recursion limit is exceeded
- **WHEN** nested item traversal exceeds the configured safety limit
- **THEN** conversion fails with the containing object's location and rule invocation chain

### Requirement: Deterministic rule selection
Rule evaluation SHALL have a documented deterministic precedence model. If equally applicable rules would produce different results and no explicit priority or terminal behavior resolves them, preflight SHALL report a conflict rather than depending on file-system or map iteration order.

#### Scenario: Explicitly ordered rules
- **WHEN** multiple matching rules have distinct declared priorities
- **THEN** they are evaluated in the documented priority order

#### Scenario: Conflicting terminal rules
- **WHEN** two terminal rules of equal precedence match the same object and produce different targets
- **THEN** preflight rejects the rule set as ambiguous

### Requirement: Explicit loss policies
Deletion, replacement with air, dropping an item, discarding NBT, clamping values, and substituting an unrelated target SHALL occur only through an explicit rule or caller-selected policy. The default policy for unrepresentable or unresolved data SHALL be failure.

#### Scenario: No mapping exists
- **WHEN** an object cannot be represented in the target and no loss policy applies
- **THEN** conversion fails and reports the object without silently changing it

#### Scenario: Explicit deletion rule
- **WHEN** a terminal deletion rule matches an object
- **THEN** the converter deletes it and records the rule identifier and location in the report

### Requirement: Rule traceability
For every changed or deliberately discarded object, the migration report SHALL identify the rule or policy responsible. A diagnostic mode SHALL explain rule matching and rejection for a selected object location.

#### Scenario: Explain a transformed block
- **WHEN** the caller requests diagnostics for a block coordinate
- **THEN** the result lists the evaluated rules, predicate outcomes, selected rule sequence, and final target representation
