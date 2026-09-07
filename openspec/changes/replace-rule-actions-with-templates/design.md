## Context

See `proposal.md` for motivation and the delta specifications for behavior. Rule documents currently deserialize into structured matchers and fixed `ObjectAction` values. Selection linearly scans the globally ordered rule list, and conversion applies selected actions separately to terrain blocks and cloned colocated block entities. Items and entities reuse the same action executor, while nested inventory discovery is carried by transform actions. `rules infer` builds old-style block and companion block-entity rules from one observation pair.

NBT is represented by a lossless internal enum and already has a lossless tagged `TypedNbt` Serde form. JSON scalars alone cannot preserve Minecraft numeric tag widths, typed arrays, or list element tags. Conversion also runs region work concurrently, so loaded templates and indices must be immutable and safe to share.

## Goals / Non-Goals

**Goals:**

- Separate inexpensive, indexable applicability checks from expressive transformation execution.
- Use one template model for blocks, items, and entities, with blocks coordinating their colocated block entity.
- Preserve typed NBT exactly across template input, composition, output, value-map calls, and diagnostics.
- Compile and validate configuration before traversing worlds and bound runtime template work.
- Keep inference output deterministic and directly insertable into a rule document.

**Non-Goals:**

- Parse, migrate, or execute any existing action-and-patch rule document.
- Add item registry lookup or other Minecraft-specific template functions.
- Infer generalized conditionals, value-map calls, or inventory paths from one observation pair.
- Allow templates to load other templates or access host resources dynamically.
- Optimize predicate evaluation beyond identity indexing in the first implementation.

## Decisions

### Retain rule matching and replace only execution

The new schema retains rule IDs, priorities, terminal behavior, structured object matchers, imports, manifests, value maps, and inventory discovery paths. Each rule replaces `action` with an inline MiniJinja `template` string. The matcher is evaluated against the immutable original object; selected templates then compose in deterministic priority order.

Moving matching into templates was rejected because it would require rendering every template for every observed object, obscure coverage explanations, and prevent direct identity indexing. Retaining old actions alongside templates was rejected because compatibility is out of scope and two execution models would multiply validation and diagnostics.

### Use MiniJinja with an isolated environment

Add MiniJinja as a workspace dependency and construct a loaded environment containing one uniquely named compiled template per rule. Use strict undefined behavior and enable the engine's computation bounding support. Register only deterministic built-ins needed for serialization plus the project-owned `value_map` function. Do not configure a filesystem loader or expose process, network, time, randomness, environment variables, or arbitrary callbacks.

Tera was considered, but MiniJinja offers a smaller embeddable environment and direct custom-function/value integration suitable for a controlled runtime. A project-specific expression engine was rejected because it would recreate parsing, control flow, escaping, and diagnostics.

### Use inline templates and typed JSON as the rendering boundary

Rule documents store template source inline. Rendering produces one JSON value that deserializes into an object-kind-specific result envelope. Both context and result encode NBT using the existing tagged `TypedNbt` representation. Template expressions can use JSON serialization to copy complete typed subtrees without flattening them through ordinary JSON numbers.

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

Object-kind-specific tagged envelopes also represent unchanged and permitted explicit loss outcomes. Full-result rendering makes removal unambiguous: omitted fields are not silently copied. Templates copy from `current` explicitly when preservation is intended.

External template files were rejected for the initial schema because imports need cross-file path and cycle rules, complicate self-contained `rules infer` output, and increase the input fingerprint surface. They can be introduced later without changing the execution model.

### Provide immutable original and composed current values

Before selection or rendering, conversion creates an owned template context. `original` never changes. `current` starts as the same logical value and is replaced by each successful non-terminal template result. Matchers always examine `original`, matching current rule-selection semantics; they do not become dependent on earlier template output.

Block context contains source name, numeric ID, metadata, and optional block NBT plus the complete optional colocated block entity. Its result contains target block name and metadata plus an optional complete block entity. Coordinates remain values inside the original entity NBT rather than special ambient context. Items contain resolved identity, numeric ID, count, damage, and complete stack NBT. Entities contain identity and complete NBT.

Rendering and result decoding happen before mutating storage. A block result is resolved against the target catalog and range-checked before either block storage or the entity list is changed.

### Remove independently selected block-entity rules

Only `block`, `item`, and `entity` rule kinds remain. Block matchers retain the optional associated block-entity name and NBT predicates, and block templates transform both values. This avoids two selections racing to control one coordinate and makes entity creation and deletion expressible.

Standalone block entities are still observed with their coordinate-owned block during coverage and explanation. A malformed or orphaned tile entity that cannot be associated retains existing traversal diagnostics rather than receiving an independent transformation.

### Build immutable identity indices during rule loading

After imports and manifests resolve, construct separate indices for block names, qualified legacy block IDs, item names, qualified legacy item IDs, and entity names. Each bucket stores references or stable indices into canonical rule storage and is sorted using the existing deterministic precedence. Evaluation resolves the source identity once, fetches only its candidate bucket, then evaluates numeric and NBT predicates in bucket order until terminal selection stops.

Identity remains mandatory in every matcher, so no wildcard bucket is required. Validation and preflight conflict analysis operate on indexed candidate groups but preserve existing ambiguity semantics.

### Preserve value maps behind one template function

Keep document-level `value_maps`, their global identifiers, exact typed entries, optional numeric coercion, and load-time ambiguity checks. `value_map(map_id, value)` converts its input through the lossless NBT adapter, performs the current exact/coercible lookup semantics, and returns a typed value usable in result construction. Calls record structured outcomes for explanation. Unknown maps, invalid inputs, and unmapped values become contextual render failures.

No `item_name`, registry object, or generic host function is exposed.

### Keep inventory discovery declarative and run it after templates

Move `nested_items` from `ObjectAction::Transform` to item-rule metadata. `standalone_inventories` remains document metadata. After a containing item or coordinated block template produces a valid result, traverse declared paths in that result and apply normal indexed item-template selection recursively under the existing depth, object-count, and cycle bounds.

This ordering ensures inventories created, moved, or copied by templates are converted. Discovering inventories dynamically inside templates was rejected because coverage could not establish bounded completeness without executing transformations.

### Rebuild diagnostics around matcher and render outcomes

Replace action and patch outcomes with candidate lookup, matcher, selected-template, render, typed-decode, value-map-call, target-resolution, and disposition outcomes. Direct conversion uses a trace-free execution path; coverage and coordinate explanation collect detailed outcomes. Errors include source document, rule ID, template phase, object identity, and available location.

### Reimplement inference as literal coordinated template generation

Retain coordinate parsing, world/profile validation, catalog enrichment, and owned observation loading. Generate exactly one block rule with an exact source-name and metadata matcher and an associated entity-name matcher when the source entity exists. Generate an inline template whose typed result reproduces the complete target block and optional entity.

When both entities exist, generate target `x`, `y`, and `z` from the corresponding original typed fields so a rule inferred from different coordinates does not relocate future entities. If only the target has an entity, emit its observed coordinates as literals because no source entity fields exist; authors can review or generalize the snippet. If only the source has an entity, emit `null`. Do not diff fields or infer reusable functions. Compile and validate the candidate against the loaded graph before serializing one deterministic JSON-array element to stdout.

## Risks / Trade-offs

- **[Typed JSON templates are more verbose than natural NBT expressions]** → Provide clear examples and lossless copy patterns; prefer correctness over implicit numeric widening.
- **[Composition can produce a value that later templates were not designed for]** → Match only against immutable originals, type-check every intermediate result, and identify the failing rule in diagnostics.
- **[Template loops can amplify large NBT inputs]** → Enforce computation, recursion, and rendered-output limits in addition to existing traversal bounds.
- **[Inline template quoting inside JSON is awkward]** → Use multiline escaped strings in JSON examples and make inference serialization deterministic; external template files can be a later schema addition.
- **[Removing block-entity rules invalidates existing authoring knowledge and fixtures]** → Update all examples and specs together; intentionally provide no compatibility layer.
- **[Index behavior could diverge from the old full scan]** → Add reference tests comparing indexed selection to a test-only exhaustive evaluator across generated rule sets.
- **[Concurrent rendering exposes accidental shared mutable state]** → Freeze loaded template environments and map registries, keep per-render state local, and test parallel determinism.

## Migration Plan

1. Introduce MiniJinja, typed contexts/results, bounded environments, identity indices, and template/value-map unit tests behind the new in-memory model.
2. Replace rule document parsing and validation with the new schema version 1 and delete legacy action, patch, decision, and block-entity-rule types.
3. Route block, item, entity, nested-item, coverage, and explanation paths through indexed template selection and atomic result application.
4. Replace inference generation and candidate validation with coordinated template output.
5. Rewrite examples, fixtures, specifications, and end-to-end tests, then remove all dead compatibility code.

Rollback requires reverting the application and its rule documents together. Newly authored template rules cannot be consumed by prior releases and no automatic reverse translation is provided.
