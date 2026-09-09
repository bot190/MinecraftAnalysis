## 1. Coordinate Selector and Addressing

- [x] 1.1 Add typed parsing for `--location x,y,z` and `--dimension`, default the dimension to overworld, reject malformed coordinates and unsafe or unknown modded dimension names, and verify focused CLI parsing tests cover accepted vanilla/modded values plus legacy and malformed inputs inside `nix develop`.
- [x] 1.2 Add a world-coordinate address type that derives global chunk, region, region-local chunk, and canonical relative region path using Euclidean arithmetic, and verify unit tests cover positive coordinates and the `-1`, `-16`, `-17`, `-512`, and `-513` boundaries inside `nix develop`.

## 2. Direct Explanation Access

- [x] 2.1 Add a targeted core explanation entry point that reads only the derived region and local chunk, decodes it with existing limits, and reports contextual missing-region, missing-chunk, decode, and traversal failures; verify focused core tests cover each outcome inside `nix develop`.
- [x] 2.2 Select the terrain block and associated block entity at the exact coordinate before assessment, preserve block-entity context for coordinated block evaluation, and verify targeted records match reference full-world assessment for ordinary and coordinated blocks inside `nix develop`.
- [x] 2.3 Expand and assess direct and recursively nested inventory items owned by the selected block entity while excluding unrelated chunk objects and entities, and verify fixtures cover multiple slots, nested containers, deterministic record order, NBT paths, and nested limits inside `nix develop`.
- [x] 2.4 Prove failure isolation by adding a fixture with valid selected content and corrupt unrelated regions or standalone NBT files, and verify targeted explanation succeeds without opening or validating unrelated content inside `nix develop`.

## 3. CLI and Progress Integration

- [x] 3.1 Route `explain` through the direct coordinate entry point, remove region pre-counting and parallel whole-world reduction from that command, and verify an end-to-end CLI test returns all coordinate-owned records for overworld and explicit non-overworld queries inside `nix develop`.
- [x] 3.2 Remove obsolete file-string matching and whole-world explain entry points after checking all callers, and verify the workspace compiles without dead APIs inside `nix develop`.
- [x] 3.3 Adjust progress events and interactive rendering so targeted explanation reports only performed targeted analysis/report activities and never world-region completion, and verify PTY and redirected-output tests keep JSON clean and omit unperformed phases inside `nix develop`.
- [x] 3.4 Add CLI failure tests for missing regions, absent chunks, no explainable coordinate-owned object, invalid dimensions, and old `file:x,y,z` syntax, and verify each failure is actionable and non-panicking inside `nix develop`.

## 4. Documentation and Validation

- [x] 4.1 Update the README and architecture documentation with world-global coordinate derivation, dimension selection and overworld defaulting, included block-entity inventories, negative-coordinate behavior, and the one-chunk performance boundary; verify documented commands agree with CLI help.
- [x] 4.2 Run focused and full workspace tests with `nix develop -c cargo test --workspace`, run formatting and lint validation through the repository's Nix development environment, and resolve regressions attributable to this change.
- [x] 4.3 Run `openspec validate target-explain-at-world-coordinate --strict` and verify every delta requirement and scenario validates before implementation handoff.
