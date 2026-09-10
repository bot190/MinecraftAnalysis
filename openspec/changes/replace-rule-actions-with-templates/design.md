## Context

See `proposal.md` for motivation and the delta specifications for behavior. Rule documents currently deserialize into structured matchers and fixed `ObjectAction` values. Selection linearly scans the globally ordered rule list, and conversion applies selected actions separately to terrain blocks and cloned colocated block entities. Items and entities reuse the same action executor, while nested inventory discovery is carried by transform actions. `rules infer` builds old-style block and companion block-entity rules from one observation pair.

NBT is represented by a lossless internal enum and already has a lossless tagged `TypedNbt` Serde form. Untagged serialization scalars cannot preserve Minecraft numeric tag widths, typed arrays, or list element tags. Conversion also runs region work concurrently, so loaded templates and indices must be immutable and safe to share.

## Goals / Non-Goals

**Goals:**

- Separate inexpensive, indexable applicability checks from expressive transformation execution.
- Use one template model for blocks, items, and entities, with blocks coordinating their colocated block entity.
- Preserve typed NBT exactly across template input, output, value-map and item-transformation calls, and diagnostics.
- Compile and validate configuration before traversing worlds and bound runtime template work.
- Keep inference output deterministic and directly insertable into a rule document.

**Non-Goals:**

- Parse, migrate, or execute any existing action-and-patch rule document.
- Add general registry access or standalone-document templates beyond the bounded item functions specified by this change.
- Infer generalized conditionals, value-map calls, or inventory paths from one observation pair.
- Allow templates to load other templates or access host resources dynamically.
- Optimize predicate evaluation beyond identity indexing in the first implementation.

## Decisions

### Retain rule matching and replace only execution

The new YAML-only schema retains rule IDs, priorities, structured object matchers, imports, manifests, and value maps. Each rule replaces `action` with an inline MiniJinja `template` string. Candidate rules are ordered deterministically, matchers are evaluated against the immutable original object, and the first match wins. The schema removes terminal behavior because every successful match terminates selection, and removes `nested_items` and configurable `standalone_inventories` declarations. JSON rule documents are rejected rather than parsed compatibly.

Moving matching into templates was rejected because it would require rendering every template for every observed object, obscure coverage explanations, and prevent direct identity indexing. Retaining old actions alongside templates was rejected because compatibility is out of scope and two execution models would multiply validation and diagnostics.

### Use MiniJinja with an isolated environment

Add MiniJinja as a workspace dependency and construct a loaded environment containing one uniquely named compiled template per rule. Use strict undefined behavior and enable the engine's computation bounding support. Register only deterministic built-ins needed for serialization plus the project-owned `value_map`, `transform_item`, `transform_items`, and `map_item_id` functions. Do not configure a filesystem loader or expose process, network, time, randomness, environment variables, or arbitrary callbacks.

Tera was considered, but MiniJinja offers a smaller embeddable environment and direct custom-function/value integration suitable for a controlled runtime. A project-specific expression engine was rejected because it would recreate parsing, control flow, escaping, and diagnostics.

### Use YAML literal blocks and typed JSON as the rendering boundary

Rule documents are parsed exclusively as YAML and store template source inline using a literal block scalar (`template: |`), which preserves readable source lines and newline characters without JSON escaping. JSON rule documents are not accepted, even when their data could otherwise deserialize into the same schema. Rendering still produces one JSON value that deserializes into an object-kind-specific result envelope. Both context and result encode NBT using the existing tagged `TypedNbt` representation. Template expressions can use JSON serialization to copy complete typed subtrees without flattening them through ordinary JSON numbers.

Conceptually, a coordinated block result is:

```json
{
  "disposition": "transform",
  "block": {
    "name": "mod:target",
    "metadata": 0
  },
  "block_entity": {
    "type": "compound",
    "value": {}
  }
}
```

Object-kind-specific tagged envelopes also represent unchanged and permitted explicit loss outcomes. Full-result rendering makes removal unambiguous: omitted fields are not silently copied. Templates copy from `original` explicitly when preservation is intended.

External template files were rejected for the initial schema because imports need cross-file path and cycle rules, complicate self-contained `rules infer` output, and increase the input fingerprint surface. YAML literal blocks provide multiline authoring while keeping each rule document self-contained. External templates can be introduced later without changing the execution model.

### Provide an immutable original value

Before selection or rendering, conversion creates an owned template context. Matchers examine `original`, and the first matching rule's template receives that same immutable value. There is no `current` value or intermediate template result because later matches are ignored.

Block context contains source name, numeric ID, metadata, and optional block NBT plus the complete optional colocated block entity. Its result contains target block name and metadata plus an optional complete block entity. Coordinates remain values inside the original entity NBT rather than special ambient context. Items contain resolved identity, numeric ID, count, damage, and complete stack NBT. Entities contain identity and complete NBT.

Rendering and result decoding happen before mutating storage. A block result is resolved against the target catalog and range-checked before either block storage or the entity list is changed.

### Remove independently selected block-entity rules

Only `block`, `item`, and `entity` rule kinds remain. Block matchers retain the optional associated block-entity name and NBT predicates, and block templates transform both values. This avoids two selections racing to control one coordinate and makes entity creation and deletion expressible.

Standalone block entities are still observed with their coordinate-owned block during coverage and explanation. A malformed or orphaned tile entity that cannot be associated retains existing traversal diagnostics rather than receiving an independent transformation.

### Build immutable identity indices during rule loading

After imports and manifests resolve, construct separate indices for block names, qualified legacy block IDs, item names, qualified legacy item IDs, and entity names. Each bucket stores references or stable indices into canonical rule storage and is sorted by declared priority followed by canonical document, import, and rule order. Evaluation resolves the source identity once, fetches only its candidate bucket, then evaluates numeric and NBT predicates until the first match and renders only that template.

Identity remains mandatory in every matcher, so no wildcard bucket is required. Validation and preflight conflict analysis operate on indexed candidate groups but preserve existing ambiguity semantics.

### Preserve value maps and support referenced item identities

Keep document-level `value_maps`, their global identifiers, exact typed entries, optional numeric coercion, and load-time ambiguity checks. `value_map(map_id, value)` converts its input through the lossless NBT adapter, performs the current exact/coercible lookup semantics, and returns a typed value usable in result construction. Calls record structured outcomes for explanation. Unknown maps, invalid inputs, and unmapped values become contextual render failures.

Item rules may additionally declare `target_name`. `map_item_id(source_numeric_id)` resolves the numeric reference through the source item catalog, examines the normal first candidate for that identity, requires an identity-only matcher and `target_name`, validates the projected identity against the target catalog, and returns the name without rendering the item template. It fails rather than fabricating count, damage, or NBT, or skipping an ineligible first candidate. No registry object or generic host function is exposed.

### Transform embedded inventories through template functions

Expose `transform_item` to recursively apply indexed first-match item conversion to one complete typed stack and return a complete target stack or `null` for an explicit drop. `transform_items` applies the same operation to an ordered sequence, preserves retained order, and omits dropped items. Block, item, and entity templates use loops or the sequence helper to reconstruct any mod-specific inventory structure without Rust knowing its paths or shape.

Recursive calls share per-root depth and object-count budgets, detect item-rule invocation cycles, remain trace-free during direct conversion, and produce nested diagnostic outcomes during explanation. Coverage does not execute these calls or discover embedded items: coverage of the containing block, item, or entity is sufficient. This intentionally trades nested-item coverage completeness for arbitrary template-owned layouts.

Remove configurable `standalone_inventories` and its path validation, canonicalization, import aggregation, conversion, coverage, and explanation handling. Continue the existing built-in processing of `Inventory` and `EnderItems` in standalone player data so ordinary player items remain converted. General standalone-document templates and custom standalone layouts are deferred to a future change.

### Rebuild diagnostics around matcher and render outcomes

Replace action and patch outcomes with candidate lookup, matcher, selected-template, render, typed-decode, value-map-call, target-resolution, and disposition outcomes. Direct conversion uses a trace-free execution path; coverage and coordinate explanation collect detailed outcomes. Errors include source document, rule ID, template phase, object identity, and available location.

### Reimplement inference as literal coordinated template generation

Retain coordinate parsing, world/profile validation, catalog enrichment, and owned observation loading. Generate exactly one block rule with an exact source-name and metadata matcher and an associated entity-name matcher when the source entity exists. Generate an inline template whose typed result reproduces the complete target block and optional entity.

When both entities exist, generate target `x`, `y`, and `z` from the corresponding original typed fields so a rule inferred from different coordinates does not relocate future entities. If only the target has an entity, emit its observed coordinates as literals because no source entity fields exist; authors can review or generalize the snippet. If only the source has an entity, emit `null`. Do not diff fields or infer reusable functions. Compile and validate the candidate against the loaded graph before serializing one deterministic YAML sequence element to stdout.

## Risks / Trade-offs

- **[Typed JSON templates are more verbose than natural NBT expressions]** → Provide clear examples and lossless copy patterns; prefer correctness over implicit numeric widening.
- **[First-match semantics make rule order behaviorally significant]** → Define a stable priority, document, import, and rule ordering; expose that order in diagnostics and test indexed selection against an exhaustive reference evaluator.
- **[Template loops can amplify large NBT inputs]** → Enforce computation, recursion, and rendered-output limits in addition to existing traversal bounds.
- **[YAML scalar typing can reinterpret unquoted values]** → Keep the schema strongly typed, quote registry identities and other string values in examples, and reject documents that do not deserialize exactly into the schema.
- **[Removing block-entity rules invalidates existing authoring knowledge and fixtures]** → Update all examples and specs together; intentionally provide no compatibility layer.
- **[Index behavior could diverge from the old full scan]** → Add reference tests comparing indexed selection to a test-only exhaustive evaluator across generated rule sets.
- **[Concurrent rendering exposes accidental shared mutable state]** → Freeze loaded template environments and map registries, keep per-render state local, and test parallel determinism.

## Migration Plan

1. Introduce MiniJinja, typed contexts/results, bounded environments, identity indices, and template/value-map unit tests behind the new in-memory model.
2. Replace rule document parsing and validation with the new YAML-only schema version 1 and delete legacy action, patch, decision, and block-entity-rule types.
3. Route block, item, entity, recursive template-item calls, coverage, and explanation paths through indexed template selection and atomic result application; remove configurable inventory declarations while retaining built-in player inventory paths.
4. Replace inference generation and candidate validation with coordinated template output.
5. Rewrite examples, fixtures, specifications, and end-to-end tests, then remove all dead compatibility code.

Rollback requires reverting the application and its rule documents together. Newly authored template rules cannot be consumed by prior releases and no automatic reverse translation is provided.
