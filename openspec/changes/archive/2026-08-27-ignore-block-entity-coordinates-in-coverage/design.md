## Context

See `proposal.md` for motivation. Coverage currently builds an `AssociatedBlockEntity` while indexing each analysis batch, renders the complete typed NBT as canonical SNBT, and embeds that value directly in the uncovered block's sortable signature. The same raw typed value is also retained for coordinated block-rule evaluation. Signature records can be merged in memory, across sorted-run spills, and across region workers, so normalization must occur before any of those boundaries.

The report location already carries the associated block's coordinates. Minecraft block entities conventionally repeat those coordinates as top-level lowercase integer fields named `x`, `y`, and `z`.

## Goals / Non-Goals

**Goals:**

- Construct one deterministic, location-insensitive associated block-entity representation before coverage grouping or spilling.
- Keep rule matching semantically independent from report normalization.
- Preserve the existing bounded and parallel merge properties of coverage signatures.

**Non-Goals:**

- Normalize entity, item, or standalone NBT.
- Remove nested or differently cased coordinate-like fields.
- Change block-entity association, location sampling, report schema numbering, or conversion behavior.

## Decisions

### Normalize a report-only clone at block-entity association time

When constructing the rendered associated block entity, clone its typed NBT compound, remove the exact top-level keys `x`, `y`, and `z`, and render that normalized clone as canonical SNBT. Retain the existing complete typed value alongside it for coordinated rule evaluation.

Normalizing at association time gives every downstream signature accumulator the same logical value before in-memory grouping, sorted-run serialization, or cross-worker merging. Normalizing only during final serialization was rejected because coordinate-bearing signatures would already have formed distinct groups.

### Use the normalized SNBT as both the signature key and reported value

The `AssociatedBlockEntity` embedded in a signature remains the value emitted in the report. This ensures merged observations have one truthful shared representation.

Keeping full SNBT in the report while using a separate normalized comparison key was rejected because a merged group has no single coordinate-bearing SNBT value representative of all its locations; selecting one observation would add ordering complexity and could expose an arbitrary coordinate.

### Limit filtering to exact top-level fields on compound NBT

Only exact lowercase top-level keys `x`, `y`, and `z` are removed. Nested fields, differently cased names, all other tags, and non-compound values remain untouched. Field type does not affect removal because the field name conveys the redundant location role and malformed coordinate types should not recreate duplicate signatures.

Recursive or case-insensitive filtering was rejected because mods may use coordinate-like names for meaningful state unrelated to block location.

### Preserve raw NBT for behavioral evaluation

Rule evaluation continues receiving the complete typed block-entity value. The normalization exists only in coverage signature/report construction and therefore cannot change whether a rule covers an observation.

## Risks / Trade-offs

- [A mod uses a top-level lowercase `x`, `y`, or `z` field as meaningful non-location state] → Accept the established block-entity coordinate convention and keep the exception narrowly limited to block entities associated with coverage blocks.
- [Report consumers expected complete associated SNBT] → Document the explicit omission while preserving coordinates in structured coverage locations and retaining report schema version 1.
- [Normalization occurs after a grouping boundary] → Build the normalized rendered value before signature construction and verify equivalence across spills and worker counts.

## Migration Plan

Introduce the report-only normalization at associated block-entity rendering, then update unit and end-to-end expectations and rule-authoring documentation. No persisted data migration is required. Rollback restores full associated SNBT rendering and the previous coordinate-sensitive grouping behavior.
