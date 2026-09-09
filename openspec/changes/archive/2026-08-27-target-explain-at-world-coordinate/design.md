## Context

See `proposal.md` for motivation. The CLI currently accepts a report-file string and optional coordinate, prepares the source and target catalogs, counts all regions, then calls the same incremental source reducer used by whole-world analysis. Every supported source file is discovered and hashed; every region and populated chunk is decoded; nested items are expanded; and every observed object is assessed before a record-level predicate retains matches.

The existing region reader can read one local chunk from a region byte buffer, traversal can produce location-aware observations from one decoded chunk, nested-item expansion already preserves owner paths, and assessment already coordinates a terrain block with a block entity from the same observation batch. The direct path should compose those contracts without creating a second rule evaluator.

## Goals / Non-Goals

**Goals:**

- Make lookup work independent of total world size after ordinary input, profile, registry, and rule preparation.
- Derive an unambiguous source container from dimension and world-global coordinates, including negative coordinates.
- Preserve the explanation semantics and deterministic ordering produced by existing assessment for all objects owned by the coordinate.
- Keep associated block-entity state available when deciding terrain rules and recursively explain items owned by the selected block entity.
- Return contextual errors for invalid selectors, missing containers, malformed selected content, and a coordinate with no explainable record.

**Non-Goals:**

- Supporting file, chunk, NBT-path, entity-position, or standalone-NBT selectors.
- Explaining entities merely because their floating-point position falls within the selected block.
- Changing rule selection, nested-item discovery, explanation JSON fields, or target prediction semantics.
- Retaining compatibility with the old `file[:x,y,z]` selector.
- Optimizing whole-world dry-run or coverage traversal.

## Decisions

### Parse a typed coordinate and dimension at the CLI boundary

`explain --location` will parse exactly three signed 32-bit integers separated by commas. A separate optional `--dimension` argument will accept `overworld`, `nether`, `end`, or an existing modded directory identifier in `DIM...` form and will default to `overworld`.

Parsing before core invocation produces specific usage errors instead of treating malformed coordinates as an empty match. Keeping dimension separate makes the coordinate conventional and avoids overloading colon delimiters or reconstructing internal report paths. The old file-oriented syntax will be rejected because accepting both forms would preserve ambiguous file-only behavior and complicate documentation and validation.

### Derive the region and chunk with Euclidean arithmetic

For global block coordinates `(x, y, z)`, lookup will calculate:

```text
chunk_x       = x.div_euclid(16)
chunk_z       = z.div_euclid(16)
region_x      = chunk_x.div_euclid(32)
region_z      = chunk_z.div_euclid(32)
local_chunk_x = chunk_x.rem_euclid(32)
local_chunk_z = chunk_z.rem_euclid(32)
```

The selected path is `region/r.<region_x>.<region_z>.mca` for the overworld or `<dimension-directory>/region/r.<region_x>.<region_z>.mca` otherwise. Euclidean division is required because truncating signed integer division assigns negative blocks to the wrong chunk and region. The derived relative path also becomes the canonical `ObjectLocation.file` in emitted records, eliminating the current absolute-versus-relative selector inconsistency.

### Add a direct core entry point rather than parameterizing whole-world reduction

A targeted explanation entry point will receive the source root, catalogs, loaded rules, dimension, and coordinate. It will validate the derived path remains under the source root, read one region, read one local chunk, decode that chunk, and invoke the existing chunk traversal and assessment machinery.

Adapting the whole-world reducer with early filtering was rejected because its producer still performs sorted-tree discovery, hashing, unrelated file decoding, region scheduling, and whole-region chunk iteration. A direct entry point makes the resource and failure-isolation contract explicit.

The first implementation may scan all observations in the selected decoded chunk before retaining coordinate-owned observations. This bounds work to one chunk and reuseses one authoritative traversal. A specialized single-coordinate block decoder is deferred because duplicating section, palette, block-entity, and inventory traversal carries greater semantic risk for limited initial benefit.

### Select ownership before rule assessment

After chunk traversal, the direct path will select the terrain block and block entity whose `location.block` exactly equals the requested coordinate. It will expand recursively nested items from the selected block entity and retain items whose ownership path descends from that selected block entity. It will then build the local block-entity association and assess only this selected set.

Selection must occur before rule assessment to meet the performance contract. The block entity must remain available as associated context while the terrain block is assessed, even when the block entity itself is also emitted as a separate explanation record. Results will use existing canonical location and object-kind ordering.

Entities are excluded because their location and ownership semantics are not block-coordinate based. Items owned by the selected block entity are included regardless of inventory depth, subject to the existing nested limits.

### Treat targeted lookup as bounded analysis without region progress

`explain` will stop pre-counting world regions and will not invoke the parallel region executor. Interactive output may identify targeted analysis and report generation, but it will not display a region-total progress bar or claim that world analysis completed. Global `--jobs` remains accepted as a global option but has no effect on a single-chunk explanation.

### Preserve selected-container failures and isolate unrelated failures

A missing derived region, absent local chunk, or no supported coordinate-owned object will produce a contextual no-match error. Corrupt region metadata, oversized selected chunks, NBT decode failures, traversal errors, and nested-limit failures in the selected content will retain container, dimension, chunk, and coordinate context.

No unrelated source entry will be opened or validated. Input preparation that is inherently required to construct the source and target catalogs and validate rules remains unchanged; the direct-access guarantee begins after that shared preparation boundary.

## Risks / Trade-offs

- [Scanning the selected chunk still visits unrelated observations] → Filter before nested expansion and assessment; retain existing traversal initially, then profile before considering coordinate-specialized decoding.
- [Inventory ownership could be inferred incorrectly from string NBT paths] → Select the block entity first and expand nested items from its structured observation rather than filtering a whole-chunk expanded list by string prefix alone.
- [Negative coordinates can silently select the wrong file] → Centralize derivation in a tested value type and cover boundaries at `-1`, `-16`, `-17`, `-512`, and `-513`.
- [Modded dimension input could allow path traversal] → Accept only recognized dimension names or validate the `DIM...` identifier as one normal path component before joining it beneath the source root.
- [The breaking selector may surprise scripts] → Update CLI help, README, architecture documentation, and parsing tests together; malformed legacy syntax fails with an actionable migration message where practical.
- [Direct and former whole-world assessment could drift] → Retain a test-only reference comparison over fixtures covering coordinated rules, value maps, direct inventory items, and recursively nested items.

## Migration Plan

1. Introduce and test typed coordinate/dimension parsing and derivation without changing assessment semantics.
2. Add the direct core lookup and equivalence tests against the current full-world path.
3. Switch the CLI to the new selector, remove whole-world explain progress/counting, and remove obsolete matching APIs after all callers migrate.
4. Update documentation and integration tests, then validate the full suite inside the Nix development environment.

Rollback consists of restoring the previous CLI routing and incremental matching entry point; the change does not migrate persisted data or alter rules and report schemas.
