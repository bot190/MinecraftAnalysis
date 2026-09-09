## Why

`explain` currently scans and evaluates the complete source world even though the caller is asking about one block coordinate. This makes a focused debugging command scale with world size and leaves callers responsible for identifying an internal region filename.

## What Changes

- **BREAKING**: Replace the report-file-based `--location file[:x,y,z]` selector with a world-global `--location x,y,z` selector and an optional dimension selector that defaults to the overworld.
- Derive the containing dimension region file, chunk, and region-local chunk coordinates from the requested world-global coordinate, including correct behavior for negative coordinates.
- Read and decode only the selected region chunk rather than discovering, hashing, decoding, traversing, and evaluating the complete source world.
- Explain every supported object owned by the selected coordinate: its terrain block, associated block entity, directly contained inventory items, and recursively nested inventory items.
- Preserve deterministic explanation records, selected-rule traces, value-map outcomes, target predictions, and diagnostics for the selected objects.
- Replace whole-world explain progress with activity appropriate to one targeted lookup.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `world-transformation-rules`: Define world-coordinate selection and the complete set of coordinate-owned objects included in an explanation.
- `resource-bounded-rule-coverage`: Strengthen targeted explanation from incremental filtering to direct, bounded access that does not evaluate unrelated world content.
- `cli-progress-reporting`: Define progress behavior for a single-coordinate explanation rather than a complete region analysis.

## Impact

- Affects the `minecraft-analysis explain` CLI contract and its documentation and integration tests.
- Affects targeted source access, region/chunk coordinate calculation, traversal selection, nested-item expansion, and assessment in `minecraft-analysis-core`.
- Reuses the existing source and target profile, registry, rule-evaluation, region-reader, traversal, and explanation output contracts; no new dependency or output schema is required.
- Does not change `convert`, `dry-run`, `rules coverage`, standalone NBT inspection, or transformation semantics.
