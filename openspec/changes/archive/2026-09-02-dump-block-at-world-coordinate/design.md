## Context

The CLI currently models `nbt dump` around a required positional file plus optional chunk selectors. Its loader can decode standalone documents or one explicitly selected region chunk, but it does not accept a world root or derive a region path from a block coordinate.

Core already provides the complementary pieces: `WorldCoordinateAddress` derives chunk, region, local-chunk, and dimension-relative region paths with Euclidean arithmetic; the region reader decodes one slot; `BlockIndex` decodes supported pre-flattening section storage and associates `TileEntities`; and the NBT module renders canonical SNBT. The interactive viewer already formats the same block storage fields, while targeted explanation demonstrates safe world and dimension resolution.

## Goals / Non-Goals

**Goals:**

- Compose existing addressing, region decoding, block indexing, and SNBT contracts into one non-interactive lookup path.
- Preserve atomic output: fully resolve and render the record before writing any bytes.
- Keep coordinate handling and diagnostic context consistent with targeted explanation.
- Share block detail formatting semantics with the interactive viewer where practical.

**Non-Goals:**

- Infer mappings or transformations between source and target objects; the selected rule file is used only to construct one side's block/item registry and resolve the dumped block identity.
- Support flattened palettes or chunk formats not already supported by `BlockIndex`.
- Search neighboring chunks for malformed or misplaced block entities.
- Change the output of existing standalone-document or region-chunk dump invocations.
- Normalize away block-entity coordinate fields as coverage reporting does.

## Decisions

### Add an explicit mutually exclusive world-coordinate input mode

Represent dump input as two CLI modes: the existing positional file with optional chunk selector, or paired `--world <path>` and `--location <x,y,z>` arguments with optional `--dimension` and exactly one of `--source-rule <file>` or `--target-rule <file>`. Clap-level constraints will reject incomplete, mixed, missing-rule-side, or dual-rule-side modes before I/O.

An explicit `--world` avoids making the positional path silently mean either a file or directory and leaves every existing command line valid. Adding a separate `nbt block` subcommand was considered, but the requested operation remains a non-interactive NBT-backed dump and belongs beside the existing dump behavior.

### Resolve identity from one explicit rule side

Load the selected rule file through the existing validated rule loader. `--source-rule` constructs the source block/item catalog from the rule document's source profile, source manifest, and applicable vanilla fallbacks. `--target-rule` constructs the target block/item catalog from the target manifest and established target profile fallbacks. Resolve the stored numeric block ID in the block registry for that selected side before rendering.

Although loading builds both block and item registry information consistently with conversion, this command resolves the selected block only; it does not enumerate item stacks nested in block-entity NBT. If the numeric block ID is unresolved, fail contextually rather than printing a numeric-only record. Accepting both rule sides was rejected because a single stored numeric ID does not identify which catalog should interpret it.

### Derive and read exactly one region slot

Canonicalize and validate the world root through the established safe-world path behavior, parse the dimension with `DimensionId`, derive a `WorldCoordinateAddress`, and join its dimension-relative region path beneath the world. Read only the derived local chunk slot and decode only that chunk.

Scanning the world or decoding an entire region was rejected because the coordinate uniquely determines the target and direct access gives bounded work and clearer failures.

### Reuse the coordinate-keyed legacy block index

Build `BlockIndex` from the selected chunk using its global chunk coordinates, then perform an exact lookup for `[x, y, z]`. Expose or add the smallest read-only lookup API needed rather than duplicating section-array arithmetic in the CLI. This preserves established handling of `Add`, `Data`, lighting arrays, negative section coordinates, and `TileEntities`.

Using traversal observations was considered, but traversal is optimized for rule processing and does not retain all low-level storage fields required by the dump.

### Emit a stable labeled text record

Construct the complete output string in memory with fields ordered as coordinate, dimension, global chunk, region, local chunk, numeric ID, registry name, metadata, block light, sky light, section Y, section index, and block entity. Render missing optional lighting as `unavailable` and a missing association as `none`. When present, render the complete borrowed block-entity value as canonical multiline SNBT below the label.

JSON was considered for scripting, but it would introduce a second encoding contract and awkwardly embed SNBT. Stable labeled text matches the existing diagnostic presentation and the current command's human-readable purpose.

### Preserve complete block-entity data

Use the exact value associated by the coordinate index and render it without cloning solely for normalization. Top-level `x`, `y`, and `z` remain because this command inspects one concrete stored object; coverage's coordinate removal serves grouping and does not apply here.

### Preserve no-partial-output behavior

Resolve, decode, index, look up, construct the selected registry catalog, resolve the registry name, render SNBT, and assemble the record before locking and writing standard output. Any failure before the final write leaves standard output empty, consistent with existing `nbt dump` guarantees.

## Risks / Trade-offs

- [The capability name still says “standalone” although it now includes world lookup] → Preserve the established capability path to avoid fragmenting the existing `nbt dump` contract; its purpose can be broadened when the delta is archived.
- [Labeled output can become an accidental parser API] → Specify exact field order and test byte-for-byte stability; future machine output should use an explicit format option.
- [Malformed chunks may contain duplicate block entities at one coordinate] → Retain the existing deterministic association semantics of `BlockIndex` and cover that behavior through its tests rather than introducing a second policy.
- [Flattened worlds fail despite valid region data] → Return an explicit unsupported-storage diagnostic and keep palette support outside this change.
- [A rule manifest does not assign the encountered numeric block ID] → Fail with the selected rule side and numeric ID in the diagnostic; never silently fall back to numeric-only output.
- [A very large block-entity value increases the temporary output allocation] → Accept one-record buffering to guarantee atomic output; existing decoder limits bound the selected chunk.

## Migration Plan

Add the new world-coordinate argument group and dispatch branch without altering existing file-mode parsing or output. World-coordinate invocations must select exactly one rule side and therefore have no compatibility promise before this new mode ships. The change requires no data migration. Rollback consists of removing the new mode and its lookup API; existing invocations and stored worlds remain unaffected.
