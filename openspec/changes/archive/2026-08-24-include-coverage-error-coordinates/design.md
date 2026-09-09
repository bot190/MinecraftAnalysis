## Context

Source traversal represents locations with optional file, dimension, chunk, block, and NBT-path fields. Blocks receive computed world coordinates, and inventory owners can carry coordinates, but standard item discovery currently clears the block field rather than inheriting the owner's value. Nested discovery clones its parent location, so the missing context originates at the first item boundary.

The coverage accumulator currently returns immediately when a block or item cannot resolve through the source catalog. This prevents its existing signature grouping, occurrence counting, deterministic merge, location sampling, and report spooling machinery from inventorying unresolved numeric objects. Conversion has separate strict unresolved handling that must not change.

## Goals / Non-Goals

**Goals:**

- Keep known containing-block context intact across standard and nested item discovery.
- Accumulate all unresolved numeric blocks and items as uncovered signatures during rules coverage.
- Preserve bounded, deterministic coverage reporting and distinguish incomplete coverage from genuine execution failure.

**Non-Goals:**

- Derive approximate block coordinates from entity positions.
- Assign block coordinates to player or standalone inventories without a known block-located owner.
- Relax unresolved-source handling during conversion or preflight workflows outside rules coverage.
- Change rule selection, report schema version, deterministic ordering, or the five-location sample limit.

## Decisions

### Propagate owner block coordinates when constructing item locations

Standard item traversal will copy the inventory owner's optional block coordinates into each directly contained item's location. Nested traversal will continue to derive locations from the parent item, which propagates the coordinates transitively without a second ownership mechanism.

Passing the owner context is preferred over recovering coordinates later from NBT paths because traversal has authoritative ownership information and later path interpretation would be format-specific and error-prone.

### Keep location components optional and evidence-based

Items only inherit coordinates that are already established for their owner. Entity-carried and player-held items will not be assigned a floored position or another approximation. This maintains the existing meaning of `block` as an exact coordinate.

### Route unresolved coverage observations through normal grouping

Rules coverage will no longer return an unresolved-source error from its observation loop. An unresolved numeric object will instead continue through the uncovered-signature path with its existing `numeric:<id>` fallback identity, numeric representation, metadata or damage, count and SNBT where applicable, occurrence count, and structured location.

Using the existing accumulator is preferred over a separate unresolved collection because it already provides bounded spooling, deterministic parallel merging, complete occurrence counts, and the first five canonical distinct locations. Kind and numeric ID remain part of the signature, so unresolved blocks and items cannot be conflated.

### Give unresolved groups a mapping-specific diagnostic

When source resolution fails, the uncovered group diagnostic will explicitly state that the source registry mapping is missing. Rule rejection traces are only meaningful after authoritative identity resolution, so unresolved groups will not pretend that candidate identity rules were evaluated against an invented identity.

Resolved objects without applicable rules will retain their current candidate-rejection diagnostics and traces.

### Preserve the boundary between incomplete coverage and execution failure

Completing an inventory with unresolved numeric groups produces a normal incomplete report and exit status 1. Invalid rule documents, incompatible inputs, decoding or traversal failures, spool errors, and other conditions that prevent a complete inventory remain fatal and use an execution-error status.

Conversion and its preflight assessment retain their existing strict unresolved-source behavior. The change is deliberately local to the coverage accumulator so it cannot silently broaden conversion policy.

### Test accumulation separately from location propagation

Traversal or core tests will establish direct-container and nested-item coordinate propagation, including omission without block context. Coverage tests will establish multiple unresolved signatures, repeated occurrence aggregation, deterministic results across worker and spill configurations, mapping-specific diagnostics, and complete location samples. CLI tests will establish report emission and exit status 1 rather than early exit status 2, while conversion regression tests retain fatal behavior.

## Risks / Trade-offs

- [Rule authors could mistake numeric fallback identities for authoritative registry names] → Keep the `numeric:<id>` form visibly synthetic and add an explicit missing-source-mapping diagnostic.
- [Removing the early return could accidentally hide genuine inventory failures] → Relax only the resolved-name absence branch; retain all traversal, decoding, nested-discovery, consistency, and spool errors.
- [Blind coordinate inheritance could mislabel movable inventory owners] → Inherit only exact optional block coordinates already present in the traversal owner context; do not infer new coordinates.
- [A custom nested path could accidentally replace its parent's context] → Cover recursive discovery with a regression test that checks both inherited block coordinates and the extended NBT path.

## Migration Plan

No data or configuration migration is required. The report remains schema version 1, but callers that previously expected exit status 2 for an unresolved numeric coverage observation will now receive a complete report with exit status 1. Rollback consists of restoring fatal unresolved handling in rules coverage and reverting traversal propagation; conversion behavior is unchanged throughout.
