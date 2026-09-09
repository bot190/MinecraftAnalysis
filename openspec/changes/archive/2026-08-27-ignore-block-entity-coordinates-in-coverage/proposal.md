## Why

Rule coverage currently treats otherwise identical uncovered blocks as distinct findings when their associated block entities differ only in the top-level `x`, `y`, and `z` coordinates embedded in NBT. Those coordinates already appear in coverage locations, so including them in the investigative signature creates redundant findings without helping rule authors distinguish object state.

## What Changes

- Omit top-level lowercase `x`, `y`, and `z` fields when rendering associated block-entity SNBT for uncovered block signatures and report output.
- Continue using complete, unmodified block-entity NBT for rule evaluation and all non-coverage behavior.
- Preserve block coordinates in coverage locations and keep nested or differently cased coordinate-like fields significant.
- Group blocks whose associated block entities differ only by the omitted coordinates while retaining exact occurrence counts and deterministic canonical location samples.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `rule-coverage-analysis`: Define location-insensitive associated block-entity SNBT for uncovered signature grouping and reporting.

## Impact

- Affects coverage signature construction and associated block-entity SNBT rendering in `minecraft-analysis-core`.
- Changes `associated_block_entity.snbt` in coverage reports by removing redundant top-level coordinate fields; the report schema number and CLI remain unchanged.
- Requires coverage unit, spill/merge determinism, parallel-equivalence, and CLI report regression tests plus rule-coverage authoring documentation updates.
