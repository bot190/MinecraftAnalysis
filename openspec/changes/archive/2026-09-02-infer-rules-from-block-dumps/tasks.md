## 1. World-Coordinate Observation Loading

- [x] 1.1 Add `rules infer --rules <file> --source-world <world> --target-world <world> --coordinate <source-x,source-y,source-z:target-x,target-y,target-z> [--source-dimension <dimension>] [--target-dimension <dimension>] [--rule-id <id>]` with a dedicated ordered coordinate-pair parser, and verify help, missing and repeated option handling, positive and negative values, signed 32-bit bounds, exactly one colon, exactly three integers per side, separated and `--coordinate=<value>` forms, default dimensions, and unknown-option tests
- [x] 1.2 Load the rule graph, detect the source world according to its source profile, require a Forge 1.12.2 target world, and build world-derived catalogs supplemented by the corresponding rule manifests; verify profile incompatibilities and catalog conflicts identify the failing side and leave stdout empty
- [x] 1.3 Add an owned inference observation loader that routes the first coordinate in the ordered pair through the source world, dimension, and catalog and the second through the corresponding target inputs, retaining only registry identity, metadata, and cloned block-entity NBT; verify unit tests cover correct pair ordering, both dimensions, negative coordinates, associated and absent block entities, and independently selected source and target dimensions
- [x] 1.4 Add contextual failures for unreadable worlds, unavailable dimensions and regions, absent or malformed chunks, missing indexed blocks, and unresolved numeric IDs, and verify each failure identifies its source or target context and produces no candidate output

## 2. Conservative Rule Inference

- [x] 2.1 Build deterministic exact block rules from paired observations using the loaded rule graph as context, and verify tests cover changed and unchanged identities and metadata without emitting manifests or document fields
- [x] 2.2 Implement coordinate- and identity-aware block-entity comparison with deterministic typed `set` and `remove` patches, and verify tests cover additions, removals, scalar changes, compound recursion, and whole-value list or array replacement
- [x] 2.3 Generate optional companion block-entity rules and reject unsafe presence or identity evidence, and verify tests cover absent entities, identity transforms, invalid `id` values, and one-sided entity presence
- [x] 2.4 Generate stable default identifiers plus `--rule-id`-derived companion identifiers, reject collisions with the supplied rule graph, and verify repeated inference produces identical arrays with unique rule IDs

## 3. Validation and Atomic Output

- [x] 3.1 Serialize and validate the complete inferred `Vec<Rule>` against the loaded rule graph before one stdout write, route diagnostics to stderr, and verify ambiguity, collision, and candidate-validation failures leave stdout empty
- [x] 3.2 Keep both worlds and the rule file read-only throughout success and failure paths, and verify integration tests detect no file creation or modification under any input

## 4. End-to-End Verification

- [x] 4.1 Add source-profile and Forge 1.12.2 target fixture worlds with known coordinate observations, run inference using one paired `--coordinate` value, and verify the resulting JSON array contains schema-compatible rule objects suitable for insertion into the supplied context
- [x] 4.2 Verify source and target dimensions default independently to overworld, explicit dimensions may differ, and location-specific coordinate, chunk, region, section, index, and lighting data do not affect inferred rules
- [x] 4.3 Verify byte-for-byte deterministic JSON with exactly one trailing newline for repeated inference over unchanged world contents and the same ordered coordinate-pair options
- [x] 4.4 Run the focused and full Rust test suites inside `nix develop`, run the repository's formatting and lint checks, and confirm existing `rules` and `nbt dump` commands remain compatible
