## Why

Source analysis failures currently identify a region file but omit the failing chunk, owning object, complete NBT path, and expected and actual tag types. This leaves users unable to isolate malformed or mod-specific data without building ad hoc inspection tooling.

## What Changes

- Introduce structured location-aware diagnostics for chunk traversal failures.
- Report region path, dimension, global and local chunk coordinates, owning entity or block entity context, and the complete NBT path when available.
- Report both the expected NBT tag and actual NBT tag, with a bounded value preview where it is safe and useful.
- Render singular fields such as `Item` without a synthetic list index.
- Distinguish required chunk structure failures from speculative standard-item discovery conflicts so mod-specific uses of familiar field names can be investigated without losing their context.
- Add regression coverage for diagnostic content and error propagation through parallel source analysis and rule coverage.

## Capabilities

### New Capabilities

- `source-traversal-diagnostics`: Context-rich, deterministic diagnostics for failures encountered while traversing region chunks and recognized NBT object locations.

### Modified Capabilities

None.

## Impact

- Affects traversal error types and item-discovery helpers in `minecraft-analysis-core`.
- Affects preflight and coverage error propagation, including parallel region processing.
- May refine which inferred mod-specific item-field conflicts are fatal versus reported as validation findings; required chunk structures remain strict.
- Adds focused core and CLI-facing regression tests without changing report schemas unless a validation finding is emitted.
