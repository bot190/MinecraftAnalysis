## Context

The traversal layer currently represents incompatible types as `WrongType(String)`. Callers wrap that string with a region path, while chunk coordinates are calculated nearby but not attached to the error. Item scanning builds a complete location only after type validation, so failures lose the owning entity or block entity path. Parallel execution preserves canonical failure selection but returns work errors without their `WorkKey` context.

The NBT model already exposes exact tags and deterministic SNBT rendering. Located observations already use structured file, dimension, chunk, block, and NBT-path fields. The implementation should reuse those concepts rather than create a second incompatible location vocabulary.

## Goals / Non-Goals

**Goals:**

- Carry structured diagnostic context from traversal through preflight and coverage command errors.
- Make diagnostics sufficient to select and inspect the failing chunk.
- Bound diagnostic rendering and preserve deterministic parallel failure selection.
- Avoid treating every mod-specific field named `Item` as authoritative schema.

**Non-Goals:**

- Repair malformed chunk data.
- Infer every mod's inventory schema.
- Change required Anvil section validation or decoder safety limits.
- Add region inspection commands; that is covered by the separate region-chunk inspection change.

## Decisions

### Use structured traversal errors

Replace the path-only wrong-type variant with a structured incompatibility containing a canonical NBT path, expected tag or accepted tag set, actual tag, optional bounded preview, and optional owner context. Chunk container context belongs in the preflight error wrapper because the region scanner authoritatively knows region, dimension, and coordinates.

This is preferred over improving formatted strings at individual call sites because structured fields can be tested independently, composed without parsing, and reused by validation findings.

### Build paths before validating values

Traversal helpers will extend the parent's canonical path before checking a value. Singular fields and list elements will use distinct path operations, eliminating the synthetic `Item[0]` representation. Owner context will be established while entering each entity or block entity and passed to nested scans.

This is preferred over reconstructing paths while handling errors because reconstruction duplicates traversal rules and is prone to losing indices.

### Attach chunk context at the region boundary

The region scan loop will calculate global and local coordinates before chunk decode and traversal error mapping. Region, dimension, local coordinate, and global coordinate will be retained for decode and traversal failures. Parallel work errors must preserve the associated `WorkKey`, or the caller must wrap errors before submission with equivalent context.

Keeping this context at the boundary avoids making generic chunk traversal depend on region filename parsing.

### Bound previews using existing typed rendering

Scalar values will use exact typed rendering. Container previews will be rendered only within explicit depth and output-size bounds, with deterministic truncation. Failure to render a preview will not replace the primary traversal error.

Debug-format output was considered but rejected because it is less stable and less familiar to users inspecting NBT.

### Separate authoritative schema from generic discovery

Required chunk structures and explicit rule/profile paths remain strict. Generic standard-item discovery based only on a familiar field name will convert an incompatible value into a structured validation finding and skip that inferred item. Compatible item compounds continue through the existing observation path.

Silently ignoring the conflict was rejected because it would hide useful evidence. Failing all generic conflicts was rejected because modded entities can legitimately reuse generic names with unrelated types.

## Risks / Trade-offs

- [Changing an error enum affects many tests and match sites] → Update callers together and add constructor helpers to keep context assembly consistent.
- [Validation findings could increase report volume] → Emit at most one deterministic finding per inferred field location and retain existing spool-backed storage.
- [Value previews could expose very large data or consume excessive memory] → Apply strict byte/depth bounds and make preview failure non-fatal.
- [Relaxing generic item inference could hide a malformed vanilla item] → Keep known or explicitly declared item locations strict; only field-name-only inference receives finding behavior.

## Migration Plan

Introduce the structured error and location formatting internally, migrate traversal and preflight call sites, then update coverage and CLI tests. No persisted data migration is required. Reverting the change restores the former fatal behavior for generic field conflicts but does not require report migration.
