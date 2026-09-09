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
The converter SHALL accept human-editable, machine-validatable rule documents using the new template-rule schema version 1 and a stable rule-set identifier. Documents using the removed action-and-patch schemas SHALL be rejected without compatibility conversion. Rule and value-map identifiers SHALL be unique across all explicitly supplied and imported documents; duplicate exact map inputs and numerically ambiguous coercible inputs SHALL be rejected during preparation.

#### Scenario: Supported document loads
- **WHEN** a document declares template-rule schema version 1 and contains valid uniquely identified rules, maps, and templates
- **THEN** the converter compiles its templates and includes it in the loaded graph

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
Rules SHALL match item stacks by namespaced identity or explicitly qualified legacy numeric ID and SHALL support predicates over count, damage, and typed NBT. A selected item template SHALL receive the complete original and current item stack and produce a complete typed target item or an explicit loss result. Item rules SHALL apply independently of block rules even when a block and item share a registry name.

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

### Requirement: Nested item discovery rules
Rules SHALL be able to declare item-stack lists or compounds at mod-specific NBT paths independently of transformation template output and SHALL invoke normal item transformation recursively after the containing template result is produced. Recursive traversal SHALL enforce configurable depth and object-count limits and detect cycles in rule invocation.

#### Scenario: Backpack contains nested items
- **WHEN** a matched backpack item declares its internal item list path and its template produces that path
- **THEN** each contained stack is processed by the normal item rule set after the backpack template

#### Scenario: Recursion limit is exceeded
- **WHEN** nested item traversal exceeds the configured safety limit
- **THEN** conversion fails with the containing object's location and rule invocation chain

### Requirement: Deterministic rule selection
Rule evaluation SHALL index candidates by object kind and resolved source identity and SHALL use a documented deterministic precedence model within each candidate set. Selected non-terminal templates SHALL compose in that order, and a terminal match SHALL stop further selection. If equally applicable rules would produce different results and no explicit priority or terminal behavior resolves them, preflight SHALL report a conflict rather than depending on file-system or map iteration order.

#### Scenario: Unrelated identities are not evaluated
- **WHEN** rules exist for identities other than the object being transformed
- **THEN** those rules do not proceed to predicate evaluation or template rendering

#### Scenario: Explicitly ordered rules
- **WHEN** multiple matching rules have distinct priorities and no earlier rule is terminal
- **THEN** their templates receive successively composed current results in documented priority order

#### Scenario: Terminal rule stops selection
- **WHEN** a matched rule is terminal
- **THEN** no lower-precedence candidates are selected or rendered

#### Scenario: Conflicting terminal rules
- **WHEN** two terminal rules of equal precedence can match the same object and produce different results
- **THEN** preflight rejects the rule set as ambiguous

### Requirement: Explicit loss policies
Deletion, replacement with air, dropping an item, discarding NBT, clamping values, and substituting an unrelated target SHALL occur only through an explicit typed template result or caller-selected policy. The default policy for unrepresentable or unresolved data SHALL be failure. A direct conversion failure SHALL identify the affected object and available location context without relying on a migration report.

#### Scenario: No mapping exists
- **WHEN** an object cannot be represented in the target and no selected template or loss policy applies
- **THEN** conversion fails with the object's identity and available location context

#### Scenario: Explicit deletion rule
- **WHEN** a selected terminal template produces the deletion result permitted for its object kind
- **THEN** the converter deletes it and attributes the disposition to that rule

### Requirement: Rule traceability
For every changed or deliberately discarded object, diagnostics SHALL identify the responsible rule and template. Coordinate explanation SHALL list indexed candidates, matcher outcomes, selected template sequence, successful value-map calls, and the final typed representation. Coordinate-owned objects SHALL include the terrain block, its associated block entity, directly contained inventory items, and recursively nested inventory items.

#### Scenario: Explain a transformed block
- **WHEN** the caller requests diagnostics for a coordinate whose block template changes its block entity
- **THEN** the result lists matcher outcomes, selected templates, value-map calls, and final block and block-entity representations

#### Scenario: Default to the overworld
- **WHEN** the caller requests diagnostics for a world-global block coordinate without selecting a dimension
- **THEN** the command explains the coordinate in the overworld

#### Scenario: Explain a transformed block entity
- **WHEN** the selected coordinate owns a block entity transformed by a coordinated block template
- **THEN** the result lists matcher outcomes, selected templates, successful value-map calls, and the final coordinated representation

#### Scenario: Explain coordinate-owned inventories
- **WHEN** the selected coordinate owns a block entity containing direct or recursively nested inventory items
- **THEN** the result contains deterministic explanation records for the coordinated block and every supported contained item, with NBT paths distinguishing contained records

#### Scenario: Select a non-overworld dimension
- **WHEN** the caller selects a supported vanilla or modded dimension and supplies a world-global block coordinate
- **THEN** the command explains only coordinate-owned objects in that dimension

#### Scenario: Convert a transformed object
- **WHEN** direct conversion applies templates successfully
- **THEN** conversion mutates the object without retaining or emitting a diagnostic trace for that object

## REMOVED Requirements

### Requirement: Typed NBT predicates and patches
**Reason**: Typed NBT predicates remain part of matchers, but the fixed patch language is replaced by lossless transformation templates and the `value_map` template function.

**Migration**: Rewrite every action and patch sequence as a complete typed template result; no automatic or runtime compatibility conversion is provided.
