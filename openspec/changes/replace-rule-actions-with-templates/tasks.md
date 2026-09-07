## 1. Template Runtime Foundation

- [ ] 1.1 Add MiniJinja with the required bounded-execution and serialization features to the workspace and verify the dependency builds inside `nix develop`
- [ ] 1.2 Define lossless typed template contexts and kind-specific result envelopes for blocks, items, and entities; verify round-trip tests cover every NBT tag, optional block entities, unchanged results, and each permitted loss disposition
- [ ] 1.3 Build an isolated strict MiniJinja environment with inline template compilation, deterministic naming, computation/recursion/output limits, and no external loader or host capabilities; verify tests cover syntax errors, undefined values, limit exhaustion, and deterministic repeated rendering
- [ ] 1.4 Implement the typed `value_map` template function with exact and coercible matching plus structured call outcomes; verify tests cover successful typed results, ambiguous declarations, unknown maps, unmapped inputs, and the absence of item-registry functions

## 2. Rule Schema and Indexed Selection

- [ ] 2.1 Replace the action-and-patch document model with template-rule schema version 1 while retaining profiles, imports, manifests, value maps, matchers, precedence, and inventory declarations; verify legacy actions, patches, block-entity rule kinds, and unsupported fields are rejected
- [ ] 2.2 Compile all inline templates during graph loading and attach document/rule context to validation failures; verify invalid templates fail before source traversal and imported templates receive globally stable unique names
- [ ] 2.3 Build immutable rule indices for block and item names, qualified legacy IDs, and entity names with deterministically ordered buckets; verify identity-incompatible rules never reach predicate evaluation
- [ ] 2.4 Replace action selection with indexed matcher selection that preserves priority, terminal, and ambiguity behavior against immutable originals; verify property or reference tests produce the same selected order as an exhaustive evaluator
- [ ] 2.5 Remove `ObjectAction`, numeric transforms, NBT patches, patch application, independent block-entity decisions, and obsolete compatibility validation; verify no production code or serialized examples reference the removed vocabulary

## 3. Template-Based Conversion

- [ ] 3.1 Implement trace-free template composition using immutable `original` and successive `current` values, decoding and validating every intermediate result; verify multi-rule composition, terminal stopping, contextual render errors, and target resolution
- [ ] 3.2 Refactor region conversion to render one coordinated block result from the original block and cloned colocated block entity, validate it before mutation, normalize entity coordinates to the owning block, and atomically create, replace, or remove block entities; verify focused region tests cover every presence transition and rollback on failure
- [ ] 3.3 Route standalone entity conversion through entity templates and verify identity changes, typed NBT replacement, explicit deletion, unresolved targets, and deterministic failures
- [ ] 3.4 Route standard and standalone item conversion through item templates and verify identity, count, damage, complete typed NBT, explicit drop/delete results, and target-registry failures
- [ ] 3.5 Move nested inventory paths to rule metadata and traverse template results before recursive indexed item conversion; verify nested items created or relocated by templates are converted under existing depth, object-count, and cycle limits

## 4. Coverage, Explanation, and Diagnostics

- [ ] 4.1 Replace action-based coverage assessment with indexed matcher and template-aware coverage while preserving bounded streaming and grouping; verify complete, uncovered, coordinated block-entity, and nested-item cases
- [ ] 4.2 Replace rule/action traces with candidate, matcher, selected-template, render, typed-decode, value-map, resolution, and disposition outcomes; verify direct conversion stays trace-free while diagnostic execution records deterministic detail
- [ ] 4.3 Update coordinate explanation to present coordinated block/block-entity results and contained item templates; verify CLI tests cover overworld and modded dimensions, rejected candidates, successful map calls, and contextual failures
- [ ] 4.4 Update conversion error propagation with document, rule, template phase, identity, and available location context; verify malformed output and runtime-limit failures leave staging unpublished

## 5. Template Rule Inference

- [ ] 5.1 Replace inferred action generation with one exact source block matcher and inline coordinated result template; verify generated rules cover changed and unchanged block identities and metadata
- [ ] 5.2 Generate lossless complete target block-entity output without top-level coordinates, allow entity creation/deletion, and rely on runtime coordinate normalization; verify tests cover all source/target presence combinations and invalid source entity identities
- [ ] 5.3 Compile and validate inferred template candidates against the supplied graph before output and emit one deterministic pretty JSON array element with one trailing newline; verify repeated inference is byte-identical and failures produce empty stdout
- [ ] 5.4 Update inference CLI integration tests and fixtures to prove generated snippets can be inserted into the new rule schema and execute the observed source-to-target transformation

## 6. Examples, Documentation, and End-to-End Validation

- [ ] 6.1 Rewrite the example rule graph and test fixtures with inline typed templates, coordinated block entities, value-map calls, item/entity templates, and declarative nested inventory paths; verify every example loads successfully
- [ ] 6.2 Update user-facing documentation and architecture notes to describe matcher indexing, template contexts/results, composition, bounds, coordinate normalization, and the intentional lack of legacy compatibility or item-name lookup
- [ ] 6.3 Run formatting, linting, unit, integration, property, and end-to-end conversion tests through the repository's required `nix develop` environment and verify unchanged-input determinism plus parallel-render equivalence
- [ ] 6.4 Search specifications, source, examples, and active tests for obsolete action, patch, and independent block-entity rule semantics; remove remaining stale references and verify strict OpenSpec validation succeeds
