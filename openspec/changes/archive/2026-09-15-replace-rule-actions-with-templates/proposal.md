## Why

The current transformation rules can only express a fixed set of sequential identity, numeric, and NBT patch operations, making coordinated block and block-entity migrations verbose and preventing transformations that need arbitrary access to the complete original values. A template engine can make transformations expressive while preserving the existing declarative matching model needed for fast indexed candidate selection.

## What Changes

- **BREAKING**: Replace every existing rule action and NBT patch variant with a MiniJinja-backed transformation template; existing rule documents are not accepted or migrated.
- **BREAKING**: Store template-rule documents exclusively as YAML and reject JSON rule documents; use YAML literal block scalars for readable multiline template source.
- Retain structured block, item, and entity matchers, rule identifiers, deterministic priority and document ordering, imports, manifests, and reusable typed `value_maps`; the first matching rule wins.
- Remove independent block-entity transformation rules and let a matched block template read and produce the block and its colocated block entity as one atomic result.
- Expose complete immutable original values to templates while preserving every NBT tag type in template inputs and outputs.
- Add `value_map`, recursive `transform_item` and `transform_items`, and identity-reference `map_item_id` template functions. Item rules may declare `target_name` for identity-only references.
- **BREAKING**: Remove `nested_items` and configurable `standalone_inventories` declarations and their static traversal. Templates explicitly transform arbitrary embedded item layouts; built-in standalone `Inventory` and `EnderItems` handling remains.
- Compile and validate templates while loading rules, index rules by object identity, and evaluate only identity-compatible candidates before rendering selected templates.
- Reimplement `rules infer` to emit a coordinated block matcher and transformation template instead of legacy block and block-entity actions.
- Update conversion, preflight coverage, coordinate explanation, diagnostics, examples, and tests for the template result model.

## Capabilities

### New Capabilities

- `template-transformations`: Defines typed MiniJinja transformation contexts, results, recursive item and identity-reference functions, validation, failures, and indexed first-match selection.

### Modified Capabilities

- `world-transformation-rules`: Replaces action/patch execution and separate block-entity rules with matcher-selected templates, removes declarative nested-item paths, and updates item identity projections, coverage, and explanation.
- `block-rule-inference`: Changes inferred output from legacy action rules into coordinated block transformation templates.
- `standalone-inventory-discovery`: Removes configurable standalone inventory declarations while preserving built-in `Inventory` and `EnderItems` discovery and conversion.

## Impact

- Rule files and generated inference output change incompatibly; the rule schema can restart at version 1 because backward compatibility is explicitly out of scope. Existing JSON documents must be rewritten as YAML.
- `minecraft-analysis-core` rule loading, validation, conversion, region coordination, document conversion, traversal, coverage, explanation, inference, registry resolution, and report types are affected.
- The workspace gains MiniJinja as a runtime dependency and needs a lossless adapter between typed NBT values and template values/results.
- Existing examples, fixtures, CLI integration tests, and the main specifications must be revised to remove the old action and patch vocabulary.
