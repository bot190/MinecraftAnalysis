## Context

The CLI already resolves blocks at world-global coordinates by deriving the region and chunk address, loading the chunk NBT, building a registry-aware block index, and associating block entities by coordinate. The conversion preparation path also builds a source registry from the source world and rule graph's source profile, and a target registry from a Forge 1.12.2 template world. See `proposal.md` and `specs/block-rule-inference/spec.md` for motivation and required behavior.

One observation pair is evidence for an exact migration example, not for a generalized conditional mapping. The implementation therefore needs typed world observations and a deliberately narrow inference policy, but does not need an intermediate textual dump format.

## Goals / Non-Goals

**Goals:**

- Produce a rules-array snippet using two world coordinates, a required existing rule file as context, and an optional identifier override.
- Resolve source identities according to the rule graph's source profile and target identities from a Forge 1.12.2 world.
- Allow source and target coordinates to reside in independently selected dimensions.
- Preserve typed block-entity NBT while excluding location-specific coordinates.
- Reuse the existing rule data model and validation as the final authority.
- Guarantee atomic stdout behavior and read-only treatment of both worlds and the rule file.

**Non-Goals:**

- Support target world versions other than Forge 1.12.2.
- Infer broad metadata ranges, masks, value maps, conditions, or rules from multiple samples.
- Interpret equal values as evidence of a rename, move, or copy.
- Create or delete block entities when presence differs.
- Merge with, overwrite, copy, or enrich the supplied rule file.
- Infer item rules from item stacks nested in block-entity NBT.

## Decisions

### Load the rule graph before observing either world

Load `--rules` through the existing rule loader first. Its source profile determines how the source world is detected and how its registry is extracted. The target world is detected explicitly as Forge 1.12.2. Invalid rule context or incompatible world profiles fail before inference.

Loading worlds without the rule graph was rejected because the supported legacy source profiles require explicit interpretation, and inference must also validate against existing manifests and rule identifiers.

### Build registries from both worlds and their corresponding manifests

Reuse the existing two-world catalog preparation behavior: derive the source catalog from the source world's profile data, derive the target catalog from the Forge 1.12.2 world's `level.dat`, add the verified Forge 1.12.2 vanilla fallbacks, and apply source and target manifests from the loaded rule graph to their respective catalogs.

Using only rule manifests was rejected because a world's runtime registry is authoritative for modded numeric IDs. Reusing the target-side dump helper was rejected because that helper does not extract a registry from a target world.

### Load each coordinate into a small owned observation

For each side, derive the world-coordinate address, load the selected dimension's region chunk, build a `BlockIndex` with the applicable registry catalog, and obtain the indexed block record. Convert it immediately into an owned inference observation containing only the resolved registry name, metadata, and an optional cloned block-entity value.

Coordinates, dimension labels, chunk and region addresses, section positions, indices, and lighting values remain diagnostic context and are not stored in the inference observation. Cloning the associated block entity removes any lifetime coupling between inference and the loaded chunk document.

Parsing the stable output of `nbt dump` was rejected because direct observation avoids an unnecessary serialization boundary, duplicate parser contract, and multiline SNBT parsing path.

### Treat source and target argument groups as authoritative direction

`--source-world`, `--source-coordinate`, and `--source-dimension` identify the source observation. Their target counterparts identify the Forge 1.12.2 target observation. Both dimensions default independently to `overworld`. The source and target worlds may resolve to the same directory because two coordinates within one compatible world can still provide useful evidence.

A shared dimension argument was rejected because source and target observations may legitimately reside in different dimensions.

### Generate one exact block rule and optional block-entity companion

The block rule matches the source registry name plus exact metadata and transforms to the target registry name. A metadata `set` appears only when the values differ. Its matcher records the source block-entity name when a block entity exists, constraining the one-sample rule to the observed association.

When both observations contain block entities, generate a separate named block-entity rule. This matches the rule engine's object model: block actions transform block identity and metadata, while block-entity actions transform the associated compound. If neither observation has a block entity, no companion is emitted. If presence differs, inference fails because the current rule actions do not express safe creation or deletion from one observation.

### Use the supplied rule graph as read-only validation context

Use the loaded graph's profiles, manifests, existing identifiers, and semantic constraints to validate the candidate rules, but do not copy document-level fields or existing rules into output and never write the context file. This keeps the snippet suitable for manual insertion into the provided rule set.

### Diff NBT structurally and conservatively

Before diffing, exclude top-level `x`, `y`, and `z` and treat top-level `id` as block-entity identity. Walk compounds in lexical key order. Equal typed values need no patch; target-only or changed values produce typed `set` patches, and source-only values produce `remove` patches. For lists, arrays, scalar type changes, or structurally incompatible values, replace the nearest unambiguous path with one typed `set` rather than speculate about element correspondence. Sort patches by path and patch kind for stable output.

Generating rename, move, copy, or value-map operations from equal-looking values was rejected because one sample cannot distinguish intent from coincidence.

### Serialize and validate the rule array before one stdout write

Construct existing `Rule` values, validate their identifiers, identities, patches, uniqueness, and compatibility with the loaded rule graph, then serialize the ordered vector as deterministic pretty JSON. Buffer the full array and trailing newline, then perform one stdout write. Diagnostics go to stderr; both worlds and the rule file are read-only and there is no output-path option.

Default identifiers are sanitized, stable combinations of source and target registry names. `--rule-id` overrides the block rule identifier; a block-entity companion receives a deterministic suffix so all IDs remain unique.

## Risks / Trade-offs

- [World loading makes inference dependent on complete profile and region data] → Reuse existing profile detection and coordinate-loading diagnostics, identify the failing side, and fail before emitting output.
- [A world registry may omit or conflict with a required modded identity] → Apply the corresponding rule manifest through existing catalog conflict checks and reject unresolved or contradictory identities.
- [Supporting only Forge 1.12.2 targets limits broader use] → State the limitation in the CLI contract and reuse the conversion pipeline's established target detection until another target profile is deliberately designed.
- [An exact one-example matcher can be too narrow] → Preserve exact metadata and observed block-entity identity, making manual broadening an explicit review step.
- [Replacing a whole list or compound can yield a larger rule] → Prefer correctness and type fidelity over speculative fine-grained edits.
- [A snippet depends on external context] → Require `--rules`, validate against that graph, and make the omission of document-level fields explicit in the output contract.
- [Generated identifiers may be aesthetically awkward] → Define deterministic sanitization and provide `--rule-id` for author control.

## Migration Plan

Add the inference observation and CLI branch without changing existing rule loading, world conversion, or NBT dump behavior. The feature reads both worlds and the supplied rule graph and writes only stdout, requiring no stored-data migration. Rollback removes the subcommand and inference module; generated arrays remain ordinary collections of schema-compatible rule objects.
