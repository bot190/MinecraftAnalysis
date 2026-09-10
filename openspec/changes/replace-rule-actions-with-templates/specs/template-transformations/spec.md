## Purpose

Define lossless, bounded template execution for transforming matched Minecraft objects while keeping applicability deterministic and inexpensive to determine.

## ADDED Requirements

### Requirement: Matcher-selected transformation templates
Each transformation rule SHALL retain a structured object matcher and SHALL contain a transformation template instead of an action or patch collection. YAML rule documents SHALL store inline template source as a string and SHALL support readable multiline source through literal block scalars. The system SHALL index rules by object kind and resolved source identity, evaluate metadata and typed-NBT predicates only among identity-compatible candidates, and render templates only for selected rules.

#### Scenario: Multiline template source loads
- **WHEN** a YAML rule document supplies its template using a literal block scalar
- **THEN** the loader preserves its source lines and compiles the resulting template

#### Scenario: Non-string template source is rejected
- **WHEN** a rule document supplies its template as a sequence, mapping, or other non-string YAML value
- **THEN** preparation rejects the document before template compilation

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

### Requirement: Original template context
The first matching rule's template SHALL receive an immutable `original` value representing the object state before transformation. A block template SHALL receive and produce the terrain block and its optional colocated block entity together. No later matching rule SHALL be rendered for that object.

#### Scenario: First matching template receives original state
- **WHEN** the first matching rule is selected
- **THEN** its template can read the complete unchanged original value

#### Scenario: Later matching templates are ignored
- **WHEN** more than one ordered candidate matcher would match the original object
- **THEN** only the first matching rule's template is rendered

#### Scenario: Coordinate-owned block entity is coordinated
- **WHEN** a selected block template changes both the block and its colocated block entity
- **THEN** both results are validated and applied as one transformation without exposing either partial result

#### Scenario: Block entity presence changes
- **WHEN** a block template produces a block entity where the original had none or produces no block entity where the original had one
- **THEN** the coordinated result explicitly creates or removes the block entity

### Requirement: Value-map template function
The template environment SHALL expose a `value_map(map_id, value)` function that resolves document-level typed value maps. Matching SHALL remain type-sensitive unless the selected map enables numeric coercion, the declared destination SHALL determine the exact output type, and an unknown map or unmapped input SHALL fail with the rule ID, map ID, and complete typed input. The environment SHALL NOT expose unrestricted registry lookup beyond the specified item-identity mapping function.

#### Scenario: Template maps a typed value
- **WHEN** a selected template calls `value_map` with a declared map and matching input
- **THEN** the function returns that entry's explicitly typed destination value

#### Scenario: Template value is unmapped
- **WHEN** a selected template calls `value_map` with no applicable entry
- **THEN** transformation fails without applying the template's partial result

### Requirement: Referenced item-identity mapping
An item rule MAY declare a `target_name` registry identity for translating references that contain only a source item identity rather than a complete stack. The template environment SHALL expose `map_item_id(source_numeric_id)`, which SHALL resolve the numeric ID through the selected source profile and source item catalog, inspect the deterministically ordered item-rule bucket for that resolved identity, and use the first candidate as required by normal first-match selection. That candidate SHALL be eligible for identity-only mapping only when its matcher requires no damage, count, or NBT evidence and it declares `target_name`. The function SHALL validate `target_name` against the target item catalog and return the namespaced target registry name as a string without rendering the item template. It SHALL NOT synthesize missing stack fields or skip an ineligible first candidate to select a later rule. Authors SHALL use `transform_item` when mapping depends on a complete stack.

#### Scenario: Map a numeric item reference
- **WHEN** `map_item_id` receives a source numeric item ID that resolves to an identity whose first candidate has an identity-only matcher and a valid `target_name`
- **THEN** the function returns that target namespaced item registry name without rendering the item template

#### Scenario: Source item ID is unresolved
- **WHEN** `map_item_id` receives a numeric ID that cannot be resolved through the selected source profile, source world registry evidence, or source manifest
- **THEN** the containing transformation fails with the calling rule and unresolved numeric ID

#### Scenario: Item reference lacks a mapping
- **WHEN** the resolved source item identity has no candidate item rule
- **THEN** the containing transformation fails with the calling rule, numeric ID, and resolved source identity

#### Scenario: First item rule requires stack evidence
- **WHEN** the first candidate item rule requires damage, count, or NBT evidence unavailable from the numeric ID
- **THEN** the containing transformation fails without considering later candidates and identifies that `transform_item` requires a complete stack

#### Scenario: First item rule lacks a target projection
- **WHEN** the first candidate item rule does not declare `target_name`, including a rule intended only to drop the item
- **THEN** the containing transformation fails without rendering that rule's template or guessing a target identity

#### Scenario: Target item identity is unavailable
- **WHEN** the selected identity-only item rule declares a `target_name` absent from the target item catalog
- **THEN** the containing transformation fails with the calling rule, selected item rule, and declared target identity

#### Scenario: Referenced identity mapping is diagnosed
- **WHEN** diagnostic execution successfully calls `map_item_id`
- **THEN** its outcome identifies the numeric source ID, resolved source identity, selected item rule, and validated target identity

### Requirement: Recursive item-transformation functions
The template environment SHALL expose `transform_item(item)` and `transform_items(items)` functions for transforming items embedded in arbitrary template-owned NBT structures. `transform_item` SHALL accept one complete typed source item, resolve its source identity, apply normal indexed first-match item-rule selection, and return the complete typed target item. An explicitly dropped item SHALL return `null`. `transform_items` SHALL apply `transform_item` to each item in input order, SHALL preserve the relative order of retained results, and SHALL omit results that are `null`. Both functions SHALL preserve every NBT tag type, use normal target-registry resolution, contribute contextual outcomes to diagnostics, and share the configured template recursion and object-count limits. Their availability SHALL NOT cause static inventory discovery or expand rule-coverage analysis beyond the containing object.

#### Scenario: Transform an embedded item
- **WHEN** a block, item, or entity template calls `transform_item` with a valid typed source item for which an item rule matches
- **THEN** the function renders only the first matching item template and returns its complete resolved typed target item

#### Scenario: Drop an embedded item
- **WHEN** the selected item template explicitly drops the item
- **THEN** `transform_item` returns `null` without producing a partial item

#### Scenario: Transform an embedded item sequence
- **WHEN** a template calls `transform_items` with an ordered sequence containing retained and explicitly dropped items
- **THEN** the function returns the transformed retained items in their original relative order and omits the dropped items

#### Scenario: Rebuild a custom inventory layout
- **WHEN** a selected template loops over items stored in a mod-specific NBT structure and calls an item-transformation function for those items
- **THEN** the template can construct the complete target inventory structure without a declared inventory path

#### Scenario: Embedded item transformation fails
- **WHEN** an embedded item is invalid, unresolved, lacks an applicable transformation, exceeds a recursion or object-count limit, or its selected template fails
- **THEN** the containing transformation fails with the containing rule, nested item-rule context, and available template location without publishing a partial result

#### Scenario: Coverage stops at the containing object
- **WHEN** a block is covered by an applicable block rule whose template may transform embedded items
- **THEN** coverage treats the block as covered without discovering those items or requiring their item rules to be covered

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
