## Why

Selecting a block by manually locating a region file and translating between world, chunk, and region-local coordinates is harder and more error-prone than selecting it by world location. The NBT commands should expose one consistent world-coordinate workflow and share the same best-effort source registry context.

## What Changes

- Add `--world`, `--dimension`, and `--location` inputs to `nbt view`; derive and open the containing region chunk and initially highlight the requested block.
- Make `nbt dump` and `nbt view` load block identities from the nearest usable world registry, optional repeatable `--rules` source manifests, and version-appropriate vanilla fallbacks.
- Allow the normal world-coordinate `nbt dump` output to report stored block data when its numeric ID remains unresolved.
- **BREAKING** Remove direct region-file `--chunk` and `--local-chunk` modes from both NBT commands.
- **BREAKING** Remove `--source-rule` and `--target-rule` from `nbt dump`; accept the same optional repeatable `--rules` input as `nbt view` instead.
- Preserve standalone `nbt dump <file>` and `nbt view <file>` behavior.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `interactive-nbt-inspection`: Replace direct region-file selection with world-coordinate selection and initially highlight the requested block.
- `standalone-nbt-dumping`: Replace direct region-file and rule-side selection with world-coordinate lookup using shared source registry context, including best-effort unresolved-ID output.

## Impact

This changes the `minecraft-analysis nbt dump` and `nbt view` CLI argument model, their dispatch and input preparation, registry-context reuse, viewer initial state, diagnostics, tests, and README examples. Low-level region decoding remains available internally for derived world-coordinate access.
