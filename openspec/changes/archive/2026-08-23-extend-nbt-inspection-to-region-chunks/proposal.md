## Why

The existing `nbt dump` and `nbt view` commands can inspect standalone NBT documents but cannot inspect chunks stored inside region files, precisely where world-analysis diagnostics most often need follow-up. Users should be able to use the familiar inspection commands against either supported input form.

## What Changes

- Extend the existing `nbt dump <file>` command to accept region files when a chunk selector is supplied and emit the selected chunk as deterministic SNBT.
- Extend the existing `nbt view <file>` command to accept the same region-file and chunk-selection options and open the selected chunk in the interactive viewer.
- Support explicit global chunk coordinates and explicit region-local chunk coordinates, with validation that selectors are complete, unambiguous, in range, and consistent with the region filename.
- Preserve current standalone-file behavior when no chunk selector is supplied.
- Diagnose malformed region containers, absent chunks, chunk decompression or decoding failures, invalid region names, and invalid selectors without partial dump output or premature terminal initialization.
- Show region and chunk metadata in interactive inspection while reusing the existing typed tree navigation.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `standalone-nbt-dumping`: Broaden `nbt dump` from standalone documents to selected NBT documents stored in region chunks while preserving existing standalone behavior.
- `interactive-nbt-inspection`: Broaden `nbt view` from standalone documents to selected NBT documents stored in region chunks while preserving existing standalone behavior.

## Impact

- Affects the argument model and input-loading path for the existing `nbt dump` and `nbt view` subcommands.
- Reuses the core region reader, chunk decompression, NBT decoder, SNBT renderer, and interactive viewer.
- Requires CLI integration tests covering both selector forms, standalone compatibility, missing chunks, invalid selectors, and malformed region data.
- Does not add new top-level or `nbt` subcommands.
