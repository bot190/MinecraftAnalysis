## Why

Investigating one block currently requires manually translating world coordinates into region, chunk, section, and array offsets or entering the interactive viewer. `nbt dump` should provide a direct, script-friendly diagnostic path from a world-global coordinate to the stored block and its associated block entity.

## What Changes

- Add a world-coordinate mode to `nbt dump` that accepts a world root, an `x,y,z` location, an optional dimension, and exactly one source- or target-rule file for registry context.
- Resolve the dimension, region, chunk, section, and exact stored block using correct Euclidean coordinate arithmetic, including negative coordinates.
- Resolve the numeric block ID through the selected rule side and emit its registry name alongside deterministic block storage information and the complete canonical SNBT of an associated block entity, or explicitly report that none exists.
- Preserve the existing standalone-file and selected-region-chunk dump modes without changing their output.
- Return contextual failures for missing world data, unsupported block storage, and coordinates that do not identify a stored block.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `standalone-nbt-dumping`: Extend `nbt dump` with a mutually exclusive world-coordinate mode that reports one block and its associated block-entity SNBT while retaining the existing document-dump modes.

## Impact

- Affects the `minecraft-analysis` CLI argument model and `nbt dump` dispatch, including mandatory mutually exclusive `--source-rule` and `--target-rule` inputs in world-coordinate mode.
- Reuses world-coordinate addressing, region reading, legacy chunk block indexing, rule loading, block/item registry construction, and canonical SNBT rendering from `minecraft-analysis-core`.
- Adds CLI and core-level coverage for coordinate boundaries, dimensions, block fields, block-entity association, and failure diagnostics.
- Introduces no new external dependencies and no breaking change to existing invocations.
