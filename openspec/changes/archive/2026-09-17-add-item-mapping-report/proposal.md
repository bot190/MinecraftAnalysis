## Why

Rule authors and AI agents need a deterministic source-to-target worksheet for modded items, including items absent from stored world objects. Stock item mappings belong in built-in profiles and should not create rule-authoring work or clutter the worksheet.

## What Changes

- Provide hardcoded stock (vanilla) item mappings as part of the supported built-in profiles, independent of authored item rules and lexical suggestions. Recognize stock identities using version-specific profile data, not numeric IDs alone.
- Add `minecraft-analysis rules item-mappings` to prepare the complete source and target item registries while reporting the modded item authoring worksheet.
- Emit deterministic JSON containing every non-stock source item exactly once, with either an explicit `target_name` mapping, ranked prospective target mappings, or no prospective mapping. Omit stock source items even when an authored rule matches them.
- Treat only an item rule's `target_name` as an explicit mapping and report invalid target projections without interpreting template source.
- Exclude stock target items from prospective candidates and `unmatched_target_items`. Preserve the full explicit mapping of a modded source item even when its target is stock, including the declared target name, resolved identity, numeric ID, and any invalid-target diagnostic.
- Account for every non-stock target item: referenced targets appear with source mappings, while remaining non-stock targets appear in `unmatched_target_items`.
- Report auditable matching evidence, stable scores, summary counts, and completeness invariants scoped to the worksheet rather than the full stock registries.
- Keep observed damage, count, and NBT signatures in `rules coverage`; the new command operates at registry-identity granularity.

## Capabilities

### New Capabilities

- `item-mapping-report`: Deterministic modded-item mapping analysis using profile-owned stock mappings and exclusions, full explicit rule projections, prospective candidates, and complete non-stock target accounting.

### Modified Capabilities

None.

## Impact

- Extends the `rules` CLI with a read-only item-mapping report command and JSON schema.
- Reuses source and target registry preparation, loaded item-rule ordering, and `target_name` validation against the full target catalog.
- Makes stock identity mappings an explicit built-in profile responsibility shared with conversion; stock items require no authored mapping rules. The rule schema remains unchanged.
- Adds deterministic candidate generation and scoring over non-stock registry identities.
- Requires integration and core tests covering profile-owned stock mappings, stock exclusion, complete non-stock source enumeration, target accounting, full modded-to-stock explicit mappings, invalid projections, candidate evidence, and stable output.
