## 1. Template Runtime Foundation

- [x] 1.1 Add MiniJinja with the required bounded-execution and serialization features to the workspace and verify the dependency builds inside `nix develop`
- [x] 1.2 Define lossless typed template contexts and kind-specific result envelopes for blocks, items, and entities; verify round-trip tests cover every NBT tag, optional block entities, unchanged results, and each permitted loss disposition
- [x] 1.3 Build an isolated strict MiniJinja environment with inline template compilation, deterministic naming, computation/recursion/output limits, and no external loader or host capabilities; verify tests cover syntax errors, undefined values, limit exhaustion, and deterministic repeated rendering
- [x] 1.4 Implement the typed `value_map` template function with exact and coercible matching plus structured call outcomes; verify tests cover successful typed results, ambiguous declarations, unknown maps, and unmapped inputs
- [x] 1.5 Implement `transform_item` and `transform_items` with first-match recursive conversion, typed results, ordered drop filtering, shared depth/object limits, cycle detection, and nested diagnostic outcomes; verify arbitrary embedded layouts, failures, and deterministic repeated calls
- [x] 1.6 Implement `map_item_id` using source-catalog resolution and item-rule `target_name` without rendering or synthetic stack fields; verify success and contextual failures for unresolved IDs, missing rules, stack-dependent first candidates, absent projections, drops, and unavailable target identities

## 2. Rule Schema and Indexed Selection

- [x] 2.1 Replace the action-and-patch document model with YAML-only template-rule schema version 1 while retaining profiles, imports, manifests, value maps, matchers, precedence, and optional item-rule `target_name`; verify JSON documents, legacy actions, patches, terminal fields, `nested_items`, `standalone_inventories`, block-entity rule kinds, and unsupported fields are rejected
- [x] 2.2 Compile all inline templates during graph loading and attach document/rule context to validation failures; verify invalid templates fail before source traversal and imported templates receive globally stable unique names
- [x] 2.3 Build immutable rule indices for block and item names, qualified legacy IDs, and entity names with deterministically ordered buckets; verify identity-incompatible rules never reach predicate evaluation
- [x] 2.4 Replace action selection with indexed first-match selection ordered by priority and deterministic document/import/rule order against immutable originals; verify reference tests agree with an exhaustive evaluator and later candidates are not evaluated after a match
- [x] 2.5 Remove `ObjectAction`, numeric transforms, NBT patches, patch application, independent block-entity decisions, and obsolete compatibility validation; verify no production code or serialized examples reference the removed vocabulary

## 3. Template-Based Conversion

- [x] 3.1 Implement trace-free rendering of only the first matching template with immutable `original` context; verify later matching templates are ignored and tests cover contextual render errors and target resolution
- [x] 3.2 Refactor region conversion to render one coordinated block result from the original block and cloned colocated block entity, validate it before mutation, normalize entity coordinates to the owning block, and atomically create, replace, or remove block entities; verify focused region tests cover every presence transition and rollback on failure
- [x] 3.3 Route standalone entity conversion through entity templates and verify identity changes, typed NBT replacement, explicit deletion, unresolved targets, and deterministic failures
- [x] 3.4 Route standard and standalone item conversion through item templates and verify identity, count, damage, complete typed NBT, explicit drop/delete results, and target-registry failures
- [x] 3.5 Remove declarative nested-item path traversal and route template-owned embedded stacks only through `transform_item` and `transform_items`; verify custom block-entity and item inventory layouts are rebuilt without tool-side path knowledge
- [x] 3.6 Remove configurable standalone inventory declarations, validation, import aggregation, conversion, coverage, and diagnostics while preserving built-in `Inventory` and `EnderItems` player-data conversion; verify custom declarations are rejected and both built-in paths still convert items

## 4. Coverage, Explanation, and Diagnostics

- [x] 4.1 Replace action-based coverage assessment with indexed matcher and template-aware containing-object coverage while preserving bounded streaming and grouping; verify covered blocks do not require static discovery or coverage of embedded inventory items
- [x] 4.2 Replace rule/action traces with candidate, matcher, selected-template, render, typed-decode, value-map, nested item-call, identity-map, resolution, and disposition outcomes; verify direct conversion stays trace-free while diagnostic execution records deterministic detail
- [x] 4.3 Update coordinate explanation to present coordinated block/block-entity results and template-invoked item transformations; verify CLI tests cover overworld and modded dimensions, rejected candidates, successful value and item map calls, and contextual failures
- [x] 4.4 Update conversion error propagation with document, rule, template phase, identity, and available location context; verify malformed output and runtime-limit failures leave staging unpublished

## 5. Template Rule Inference

- [x] 5.1 Replace inferred action generation with one exact source block matcher and inline coordinated result template; verify generated rules cover changed and unchanged block identities and metadata
- [x] 5.2 Generate lossless complete target block-entity output without top-level coordinates, allow entity creation/deletion, and rely on runtime coordinate normalization; verify tests cover all source/target presence combinations and invalid source entity identities
- [x] 5.3 Compile and validate inferred template candidates against the supplied graph before output and emit one deterministic YAML sequence element with a literal-block template and one trailing newline; verify repeated inference is byte-identical and failures produce empty stdout
- [x] 5.4 Update inference CLI integration tests and fixtures to prove generated YAML snippets can be inserted into the new rule schema and execute the observed source-to-target transformation

## 6. Examples, Documentation, and End-to-End Validation

- [x] 6.1 Rewrite the example YAML rule graph and test fixtures with literal-block typed templates, coordinated block entities, value-map calls, item/entity templates, item identity projections, and template-owned inventory loops; verify every example loads successfully and no JSON rule fixture remains
- [x] 6.2 Update user-facing documentation and architecture notes to describe YAML-only rule documents and literal-block templates alongside matcher indexing, deterministic first-match ordering, template contexts/results, recursive item and identity-reference functions, bounds, coordinate normalization, removed configurable inventory declarations, retained built-in player inventories, and the intentional lack of legacy compatibility
- [x] 6.3 Run formatting, linting, unit, integration, property, and end-to-end conversion tests through the repository's required `nix develop` environment and verify unchanged-input determinism plus parallel-render equivalence
- [x] 6.4 Search specifications, source, examples, and active tests for obsolete action, patch, independent block-entity rule, `nested_items`, and `standalone_inventories` semantics; remove remaining stale references and verify strict OpenSpec validation succeeds
- [x] 6.5 Remove JSON rule parsing and serialization, load and write rule documents exclusively as YAML, preserve literal-block template newlines, update imports and manifest authoring for YAML paths, and verify JSON inputs fail with an actionable format diagnostic
