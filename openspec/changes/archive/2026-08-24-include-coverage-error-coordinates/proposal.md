## Why

Rules coverage currently stops at the first numeric block or item missing from the source catalogs, even though its purpose is to inventory everything that still needs a rule or registry mapping. The resulting one-at-a-time workflow is further hindered because known block coordinates are omitted from failures and discarded for items inside block-located containers.

## What Changes

- Preserve a containing block's coordinates when discovering items in block entities or other block-located inventory owners, including through nested-item traversal.
- Classify unresolved numeric blocks and items as uncovered coverage signatures instead of aborting rules coverage, allowing the command to inventory all such signatures in one run.
- Record stable numeric fallback identities, occurrence counts, missing-mapping diagnostics, and every available location component for unresolved signatures.
- Keep locations truthful by omitting block coordinates for items whose traversal context has no known containing block.
- Preserve deterministic grouping, the five-location sample limit, report schema version, and incomplete-coverage exit status.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `rule-coverage-analysis`: Treat unresolved numeric blocks and items as reportable uncovered coverage while retaining actionable block coordinates whenever traversal knows them, including for items contained by block-located owners.

## Impact

- Affects source traversal location propagation and rules-coverage accumulation in `minecraft-analysis-core`.
- Adds focused core and CLI regression coverage for multiple unresolved blocks/items, container items, nested items, and contexts without block coordinates.
- Changes unresolved-source handling only for rules coverage: incomplete coverage exits with status 1 after emitting the complete report. Conversion and genuine input, decoding, or execution failures remain fatal.
- Does not change the coverage report schema or world data.
