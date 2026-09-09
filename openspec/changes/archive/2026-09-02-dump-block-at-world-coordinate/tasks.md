## 1. Coordinate Lookup API

- [x] 1.1 Add the smallest read-only `BlockIndex` coordinate lookup API needed by non-interactive callers and verify core unit tests cover present records, absent stored sections, and exact block-entity coordinate association inside `nix develop`.
- [x] 1.2 Add or adapt a direct world-coordinate chunk-loading path that derives dimension-relative region and local chunk addresses without scanning, and verify tests cover overworld, vanilla and modded dimensions, negative chunk and region boundaries, missing regions, absent slots, and invalid chunk payloads inside `nix develop`.

## 2. CLI Input and Rendering

- [x] 2.1 Refactor `nbt dump` arguments into mutually exclusive existing file/selector and paired `--world`/`--location` modes with an overworld-defaulted `--dimension` and exactly one mandatory `--source-rule` or `--target-rule`, and verify CLI parsing tests cover valid modes plus every incomplete and conflicting combination inside `nix develop`.
- [x] 2.2 Implement world-coordinate dispatch that loads exactly the derived chunk, builds the legacy block index, and selects the requested `[x,y,z]` record; verify tests cover extended numeric IDs, metadata, lighting, section addressing, absent sections, and unsupported storage inside `nix develop`.
- [x] 2.3 Load the selected rule file, build the source or target block/item catalogs with the corresponding manifest and profile fallbacks, and resolve the stored numeric ID through that side's block registry; verify both sides, manifest and vanilla resolution, invalid rules, and unresolved IDs inside `nix develop`.
- [x] 2.4 Render the complete stable labeled record in the specified field order, including the resolved registry name, `unavailable` lighting, `none` for no block entity, and complete canonical associated SNBT with coordinate fields preserved; verify byte-for-byte unit tests cover both association outcomes and repeatability inside `nix develop`.
- [x] 2.5 Buffer the complete coordinate record before writing standard output and attach rule side, numeric ID, world, dimension, region, chunk, and coordinate context to catalog, lookup, or SNBT failures; verify failure-path CLI tests assert nonzero status, diagnostic context, and empty standard output inside `nix develop`.

## 3. End-to-End Compatibility

- [x] 3.1 Add minimized world and rule fixtures plus CLI integration tests for source- and target-side name resolution, overworld and explicit dimension lookups, negative boundaries, full block fields, complete nested block-entity SNBT, no associated entity, unavailable lighting, unresolved numeric IDs with empty output, and deterministic output inside `nix develop`.
- [x] 3.2 Verify existing standalone NBT and selected region-chunk dump invocations retain byte-for-byte output and existing diagnostics by running the `minecraft-analysis` CLI test suite inside `nix develop`.
- [x] 3.3 Run the affected core and CLI test suites plus workspace formatting and lint checks inside `nix develop`, and verify all checks pass without adding external dependencies.
