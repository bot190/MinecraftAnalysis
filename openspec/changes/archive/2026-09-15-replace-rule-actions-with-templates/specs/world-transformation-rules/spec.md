## MODIFIED Requirements

### Requirement: Rule-selected source profile
Rule documents SHALL declare or import exactly one distinct supported source profile before source validation, conversion, or coverage analysis; imported profile-neutral documents MAY omit the declaration. The CLI SHALL NOT provide a source-profile override.

#### Scenario: Entry document selects a profile
- **WHEN** one loaded rule document declares `forge-1.2.5` and its imported reusable documents omit a profile
- **THEN** the complete loaded rule graph selects the Forge 1.2.5 source profile

#### Scenario: Loaded documents disagree
- **WHEN** loaded rule documents declare more than one distinct source profile
- **THEN** rule preparation fails with diagnostics identifying the conflicting documents and declarations

#### Scenario: No source profile is available
- **WHEN** conversion or coverage preparation cannot derive a source profile from the loaded rule documents
- **THEN** preparation fails before traversing source content

#### Scenario: CLI cannot replace the declaration
- **WHEN** a caller invokes conversion or coverage commands
- **THEN** source-profile selection is available only through the loaded rule graph

### Requirement: Versioned rule documents
The converter SHALL accept human-editable, machine-validatable YAML rule documents using the new template-rule schema version 1 and a stable rule-set identifier. JSON rule documents and documents using the removed action-and-patch schemas SHALL be rejected without compatibility conversion. The new schema SHALL reject removed `nested_items` and `standalone_inventories` declarations. Rule and value-map identifiers SHALL be unique across all explicitly supplied and imported documents; duplicate exact map inputs and numerically ambiguous coercible inputs SHALL be rejected during preparation.

#### Scenario: Supported document loads
- **WHEN** a YAML document declares template-rule schema version 1 and contains valid uniquely identified rules, maps, and templates
- **THEN** the converter compiles its templates and includes it in the loaded graph

#### Scenario: JSON document is supplied
- **WHEN** a rule document is encoded as JSON, even if its values otherwise match template-rule schema version 1
- **THEN** preparation rejects the document and identifies YAML as the required rule format

#### Scenario: Schema-3 document declares a reusable value map
- **WHEN** a removed schema-3 action-and-patch document declares a reusable value map
- **THEN** preparation rejects the document and requires conversion to template-rule schema version 1

#### Scenario: Existing rule document loads compatibly
- **WHEN** a document uses any removed action-and-patch schema, including schema 1 or 2
- **THEN** preparation rejects it without compatibility conversion

#### Scenario: Older schema uses value-map syntax
- **WHEN** a removed schema-1 or schema-2 document declares value-map syntax
- **THEN** preparation rejects the document and identifies template-rule schema version 1 as required

#### Scenario: Removed schema is supplied
- **WHEN** a document uses an action-and-patch rule schema or removed rule field
- **THEN** preparation fails with a diagnostic that identifies the unsupported format

#### Scenario: Imported identifiers conflict
- **WHEN** imported documents declare duplicate rule or value-map identifiers
- **THEN** preparation fails and identifies both conflicting declarations

#### Scenario: Imported maps have duplicate identifiers
- **WHEN** two documents in the loaded import graph declare the same value-map identifier
- **THEN** preparation fails and identifies both conflicting declarations

#### Scenario: Numeric coercion makes entries ambiguous
- **WHEN** a value map enables numeric coercion and declares numerically equal source entries using different numeric NBT tag types
- **THEN** preparation rejects the map as ambiguous

#### Scenario: Unknown schema version
- **WHEN** a rule document declares a version other than template-rule schema version 1
- **THEN** preparation fails and identifies version 1 as the supported version

### Requirement: Block matching and transformation
Rules SHALL match blocks by namespaced identity or explicitly qualified legacy numeric ID, with optional exact, wildcard, masked, or ranged metadata constraints and optional predicates over the block and associated block entity. A selected block template SHALL transform block identity, metadata, and the optional colocated block entity as one coordinated result.

#### Scenario: Metadata-specific mod block conversion
- **WHEN** a block's registry identity, metadata, and associated predicates satisfy a rule
- **THEN** the selected template receives the complete original block and block entity and its coordinated result is resolved against the target block registry

#### Scenario: Legacy numeric matcher is ambiguous
- **WHEN** a numeric matcher does not identify its source registry context and could match more than one registry meaning
- **THEN** rule validation rejects the matcher

### Requirement: Item matching and transformation
Rules SHALL match item stacks by namespaced identity or explicitly qualified legacy numeric ID and SHALL support predicates over count, damage, and typed NBT. A selected item template SHALL receive the complete original item stack and produce a complete typed target item or an explicit loss result. An item rule MAY declare a target registry-name projection for identity-only references. Item rules SHALL apply independently of block rules even when a block and item share a registry name.

#### Scenario: Damage value is preserved
- **WHEN** an item template copies the original damage into its target result
- **THEN** the emitted target stack retains that damage with a valid NBT numeric type

#### Scenario: Block and item use different mappings
- **WHEN** a block and item share a source registry name but their applicable rules select different templates
- **THEN** each is transformed according to its own object kind

### Requirement: Entity and block-entity transformation
Rules SHALL match standalone entity identities and typed NBT and SHALL transform them through entity templates. Block entities SHALL NOT have independently selected transformation rules; they SHALL be matched and transformed only as part of the block at the same coordinates.

#### Scenario: Entity template changes schema
- **WHEN** an entity matches a rule
- **THEN** its selected template can produce a renamed identity and complete typed target NBT

#### Scenario: Block entity schema changes
- **WHEN** a matched machine block has an associated block entity
- **THEN** the selected block template reads and produces the block and block entity as one atomic conversion

### Requirement: Deterministic rule selection
Rule evaluation SHALL index candidates by object kind and resolved source identity and SHALL order each candidate set by declared priority followed by deterministic document, import, and rule order. The first candidate whose matcher succeeds SHALL be selected and rendered; no later candidate SHALL be evaluated or rendered for that object. The schema SHALL NOT support terminal behavior or template composition.

#### Scenario: Unrelated identities are not evaluated
- **WHEN** rules exist for identities other than the object being transformed
- **THEN** those rules do not proceed to predicate evaluation or template rendering

#### Scenario: Explicitly ordered rules
- **WHEN** multiple rules could match and have distinct priorities
- **THEN** the highest-precedence matching rule is rendered and every later candidate is ignored

#### Scenario: First match stops selection
- **WHEN** an ordered candidate matcher succeeds
- **THEN** no lower-precedence candidate is evaluated or rendered

#### Scenario: Conflicting terminal rules
- **WHEN** multiple equally prioritized rules can match the same object and produce different results
- **THEN** deterministic document, import, and rule order selects the first match without an ambiguity error

### Requirement: Explicit loss policies
Deletion, replacement with air, dropping an item, discarding NBT, clamping values, and substituting an unrelated target SHALL occur only through an explicit typed template result or caller-selected policy. The default policy for unrepresentable or unresolved data SHALL be failure. A direct conversion failure SHALL identify the affected object and available location context without relying on a migration report.

#### Scenario: No mapping exists
- **WHEN** an object cannot be represented in the target and no selected template or loss policy applies
- **THEN** conversion fails with the object's identity and available location context

#### Scenario: Explicit deletion rule
- **WHEN** the selected template produces the deletion result permitted for its object kind
- **THEN** the converter deletes it and attributes the disposition to that rule

### Requirement: Rule traceability
For every changed or deliberately discarded object, diagnostics SHALL identify the responsible rule and template. Coordinate explanation SHALL list indexed candidates evaluated through the first match, matcher outcomes, the selected template, successful value-map and item-transformation calls, and the final typed representation. Coordinate-owned objects SHALL include the terrain block, its associated block entity, and embedded items explicitly passed to an item-transformation function by the selected template. Coverage SHALL treat a covered containing object as sufficient without statically discovering its embedded items.

#### Scenario: Explain a transformed block
- **WHEN** the caller requests diagnostics for a coordinate whose block template changes its block entity
- **THEN** the result lists matcher outcomes through the first match, the selected template, value-map calls, and final block and block-entity representations

#### Scenario: Default to the overworld
- **WHEN** the caller requests diagnostics for a world-global block coordinate without selecting a dimension
- **THEN** the command explains the coordinate in the overworld

#### Scenario: Explain a transformed block entity
- **WHEN** the selected coordinate owns a block entity transformed by a coordinated block template
- **THEN** the result lists matcher outcomes through the first match, the selected template, successful value-map calls, and the final coordinated representation

#### Scenario: Explain coordinate-owned inventories
- **WHEN** the selected block template calls an item-transformation function for embedded inventory items
- **THEN** the result contains deterministic explanation records for the coordinated block and each invoked item transformation, with template context distinguishing the nested records

#### Scenario: Select a non-overworld dimension
- **WHEN** the caller selects a supported vanilla or modded dimension and supplies a world-global block coordinate
- **THEN** the command explains only coordinate-owned objects in that dimension

#### Scenario: Convert a transformed object
- **WHEN** direct conversion applies templates successfully
- **THEN** conversion mutates the object without retaining or emitting a diagnostic trace for that object

## REMOVED Requirements

### Requirement: Nested item discovery rules
**Reason**: Templates can traverse arbitrary embedded inventory layouts and explicitly invoke normal item transformation, so declarative NBT paths are both less flexible and redundant.

**Migration**: Remove every `nested_items` declaration and call `transform_item` or `transform_items` while constructing the containing template's complete target NBT.

### Requirement: Typed NBT predicates and patches
**Reason**: Typed NBT predicates remain part of matchers, but the fixed patch language is replaced by lossless transformation templates and the `value_map` template function.

**Migration**: Rewrite every action and patch sequence as a complete typed template result; no automatic or runtime compatibility conversion is provided.
