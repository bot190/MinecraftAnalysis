## Why

Authoring a transformation rule for a known source and target block currently requires manually transcribing registry identities, metadata, and block-entity NBT differences. Registry-resolved world-coordinate dumps now provide enough typed evidence to generate a conservative rule candidate while keeping ambiguous transformations visible for review.

## What Changes

- Add a `rules infer --source <dump> --target <dump>` command that compares one source-side block dump with one target-side block dump.
- Validate that the inputs are compatible, registry-resolved records produced for the correct source and target rule sides.
- Infer an exact source block matcher, target identity, metadata transformation, block-entity association, and only unambiguous NBT patches.
- Emit a complete deterministic, loadable rule document to standard output and report ambiguous or unsupported differences diagnostically without silently guessing.
- Preserve atomic output: malformed, reversed, unresolved, or incompatible inputs produce no partial rule document.

## Capabilities

### New Capabilities

- `block-rule-inference`: Infer a reviewable transformation-rule document from paired registry-resolved block dumps.

### Modified Capabilities

None.

## Impact

- Adds a new subcommand beneath the existing `rules` CLI namespace.
- Introduces a parser for the stable labeled block-dump record produced by the `dump-block-at-world-coordinate` change.
- Reuses the existing rule schema, typed NBT model, canonical serialization, and rule validation.
- Adds no external dependencies and does not mutate input dumps or existing rule files.
