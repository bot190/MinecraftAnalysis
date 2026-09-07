## Context

`nbt dump` and `nbt view` currently share a loader that treats the entire file as standalone uncompressed, gzip, or zlib NBT. The core already provides bounded region parsing and chunk decompression, but the CLI has no way to select a chunk or carry its metadata into the viewer. See the modified dumping and interactive-inspection specs for the observable contract.

## Goals / Non-Goals

**Goals:**

- Reuse the existing `nbt dump` and `nbt view` command names and rendering paths.
- Give both commands the same unambiguous chunk-selection syntax.
- Keep selector-free standalone behavior compatible.
- Decode only the selected region chunk and retain enough metadata for diagnostics and the viewer.

**Non-Goals:**

- Add new inspection subcommands.
- Browse all chunks in a region from one viewer session.
- Modify, repair, or write region files.
- Auto-select a chunk or scan a region for a matching NBT path.

## Decisions

### Add mutually exclusive selectors to both existing commands

Both commands will accept `--chunk <x,z>` for global chunk coordinates and `--local-chunk <x,z>` for region-local coordinates. A shared argument structure and coordinate parser will enforce exactly two signed integers for global coordinates, two integers in 0 through 31 for local coordinates, and mutual exclusion.

A single `--chunk` form with an additional coordinate-mode flag was considered but rejected because it makes copied diagnostic commands less self-explanatory. Separate `--chunk-x` and `--chunk-z` flags were rejected because partially specified selectors create more error states and longer follow-up commands.

### Select input mode from selector presence

With neither selector, the file follows the existing standalone loader unchanged. With either selector, the file is treated as a region container. This avoids extension-only detection and preserves the ability to inspect unusually named standalone documents.

Automatic region detection from `.mca` was considered but rejected because it would change selector-free behavior and leave chunk selection undefined.

### Validate global coordinates against the region filename

Global selection parses `r.<x>.<z>.mca`, computes the containing region using Euclidean division by 32, rejects mismatches, and derives local coordinates using Euclidean remainder. Local selection derives global coordinates from the parsed region name. Both modes therefore produce one canonical selection containing region, global, and local coordinates.

Permitting global coordinates outside the named region and wrapping them into local slots was rejected as surprising and unsafe for debugging.

### Return a shared loaded-document model

Refactor the loader to return a decoded document plus source metadata represented as either standalone compression metadata or region-chunk metadata. Dump ignores presentation-only metadata after contextualizing errors; view uses it in its header. Region loading uses the existing bounded reader and reads only the selected slot.

Duplicating region loading in dump and view was rejected because selector validation and diagnostics must stay identical.

### Complete validation before output or terminal setup

Dump will fully decode and render into an owned string before writing stdout, preserving its no-partial-output guarantee. View will validate selector, region, chunk decompression, and NBT decoding before initializing raw mode or the alternate screen.

## Risks / Trade-offs

- [Adding flags changes help snapshots and argument parsing tests] → Share argument definitions and update help and compatibility tests together.
- [Negative global coordinates are easy to map incorrectly] → Use Euclidean division and remainder and add boundary tests around -33, -32, -1, 0, 31, and 32.
- [Region and standalone compression metadata differ] → Model source metadata explicitly rather than overloading the standalone compression enum.
- [Large chunks can consume memory during rendering] → Retain existing region decompression bounds and current all-or-nothing SNBT rendering behavior.

## Migration Plan

Add optional selectors without removing or renaming existing arguments, then route selector-bearing invocations through the region loader. No data migration is required, and rollback restores the selector-free command implementation without affecting existing standalone usage.
