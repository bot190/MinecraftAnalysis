# Architecture overview

MinecraftAnalysis is a deliberately narrow migration engine. It converts a
copy of a Forge 1.7.10 Anvil world into a world that uses registry assignments
and version metadata taken from a caller-provided Forge 1.12.2 template. It is
not a general Minecraft upgrader and it does not run Minecraft, Forge, or mod
code.

This document describes the Rust implementation in
`crates/minecraft-analysis-core` and the command-line adapter in
`crates/minecraft-analysis`. The Python files at the repository root are older
historical tools and are not part of this architecture.

## The design in one picture

```text
 source world (read-only)       template world (read-only)       JSON rules
          |                              |                           |
          +---------- path and profile validation -----------------+
          |                              |                           |
          +------ source catalog    target catalog ------ rule loading
                         \             /                  /
                          +---- prepare inputs ---------+
                                      |
                           fused conversion work units
                    read -> transform -> write and flush
                                      |
                           coordinator commits staging
                                      |
                           one sibling rename (publish)
```

The most important boundary is between inputs and staging. `dry-run` performs a
complete read-only analysis. `convert` validates paths and endpoint evidence,
then evaluates content while writing staging. Any fatal work-unit error stops
admission and prevents publication; source and template inputs are never
modified.

## Inputs and responsibilities

| Input | What it contributes | What it does not contribute |
| --- | --- | --- |
| Source world | Terrain, entities, player state, world state, unknown files, and the old numeric registry snapshot | Target numeric IDs or target Forge metadata |
| Template world | The target registry snapshot and selected target version metadata | Gameplay state, terrain, seed, time, or player data |
| Rule documents | Explicit semantic transformations and explicit loss policy | A replacement for either world's registry evidence |
| Output path | The name of the newly published world | Existing or resumable state |

The template is therefore an authority, not a base world. The implementation
copies the source and selectively adopts target-owned metadata; it does not
populate the template with source chunks.

## End-to-end flow

### 1. CLI preparation

`crates/minecraft-analysis/src/main.rs` owns argument parsing and assembles the
domain objects used by the core library. All three commands call the same
`prepare` function:

1. `world::validate_paths` canonicalizes the inputs, requires a nonexistent
   output, and rejects overlapping source, template, and output trees.
2. `profile::detect_expected` proves that the source is Forge 1.7.10 and the
   template is Forge 1.12.2 from persisted evidence.
3. `registry` extracts a separate registry catalog from each `level.dat` and
   fills only absent, well-known vanilla entries.
4. `rules` loads versioned rule documents and applies any explicit manifest
   selections to the catalogs.
5. Analysis-only commands inventory and assess supported objects. `convert`
   instead initializes a report that receives bounded work-unit contributions.

`dry-run` stops after complete analysis. `explain` instead derives a region and
local chunk directly from a world-global `x,y,z` coordinate and dimension. It
uses Euclidean division (`x,z / 16` for chunks and chunk coordinates `/ 32` for
regions), including at negative boundaries, and reads only the selected chunk.
The selected terrain block and block entity are assessed together, followed by
direct and recursively nested inventory items owned by that block entity. This
keeps targeted explanation independent of total world size and isolates it from
corrupt unrelated regions and standalone NBT documents.

### 2. Read-only analysis

Analysis-only commands ask `world` for a stable inventory of dimensions, region files,
player data, standalone NBT, and auxiliary files. `region` and `nbt` decode the
supported storage formats, while `traversal` turns embedded blocks, items,
entities, and block entities into location-aware observations.

Standalone inventory discovery always checks `Inventory` and `EnderItems`.
Rule documents may additionally declare root-relative typed NBT paths with
`standalone_inventories`, for example `[["Inventory", "Items"]]`. Strings select
compound fields and non-negative integers select list indices. Declarations
from imports are sorted and deduplicated. Missing paths are ignored; a present
value must be a complete homogeneous list of compounds. An incompatible value
is preserved and emitted as a non-fatal `invalid_inventory_shape` validation
finding. Malformed NBT and resource-limit exhaustion remain fatal.

Rule schema 3 also permits document-scoped `value_maps`: finite tables of
explicitly typed NBT `from` and `to` values. A `map_value` patch reads a source
path, resolves it through a named map, writes the declared destination type,
and may then remove the distinct source path. Maps may opt into exact
cross-tag numeric equality; duplicate or ambiguous entries and unknown map
references are rejected while loading the complete import graph.

Each observation is resolved in the source catalog, evaluated against the
ordered rules, and checked against the target catalog. An object is marked
`unresolved` when, for example, its source numeric ID has no known identity or
the resulting identity is absent from the target. Any unresolved object closes
the conversion gate.

Preflight also fingerprints both input trees and estimates staging space.
Metadata with semantic ordering is normalized, while region-derived report
arrays retain successful completion order. Their serialized order may vary
without changing records, counts, or diagnostics.

### 3. Fused staged conversion

`pipeline::convert` creates a clearly named sibling staging directory after
input preparation and traverses the source tree in deterministic order.
`session.lock` is intentionally not copied.

Region conversion rewrites block storage and then converts entities, block
entities, and their nested item data. Standalone NBT conversion handles player
and other discovered `.dat` files. Rewrites use temporary sibling files and
renames inside staging; conversion APIs reject absolute paths and parent
traversal so they cannot be aimed back at an input tree.

Finally, `document_conversion::write_target_level_dat` merges target-owned
metadata into the staged source `level.dat`. The merge keeps the source
document and replaces only:

- the complete `FML` structure;
- `Data.DataVersion`, when present in the template; and
- `Data.Version`, when present in the template.

This preserves source seed, time, spawn, game rules, player state, and unknown
source fields.

### 4. Publication boundary

Each producing work unit checks target identities during mutation and reports
success after its temporary output is encoded, written, and flushed. The
coordinator then commits it to staging without reopening it.

The pipeline completion capability checks that staging is the expected
sibling and publishes it with one same-filesystem rename. There is no
cross-filesystem copy fallback. Existing staging data and temporary files are
never silently removed or reused, which makes interrupted runs inspectable.

## Component map

| Module | Responsibility |
| --- | --- |
| CLI `main.rs` | Commands, input assembly, analysis-report output, and error presentation |
| `world` | Safe path validation and deterministic world-file discovery |
| `profile` | Supported endpoint identification and `level.dat` merge policy |
| `registry` | World-local name-to-number catalogs, aliases, provenance, and conflict handling |
| `rules` | Rule loading, validation, matching, traces, typed NBT patches, and manifests |
| `nbt` | Owned, typed NBT model and compressed/uncompressed codecs |
| `region` | Bounded Anvil reads, block storage, and region writes |
| `traversal` | Finds supported objects and records precise locations |
| `preflight` | Read-only dry-run and explanation analysis |
| `convert` | Applies a rule decision to one semantic object |
| `region_conversion` | Rewrites staged chunks and region containers |
| `document_conversion` | Rewrites staged NBT, nested items, and `level.dat` |
| `staging` | Deterministic copy, interrupted-run diagnostics, and final rename |
| `report` | Stable machine-readable audit records |

## Design properties and boundaries

- **Inputs remain read-only.** Mutation functions accept a staging root plus a
  relative path, not a source or template path.
- **Loss requires policy.** Deletion, dropping, NBT discard, substitution, and
  air replacement are rule actions rather than implicit recovery behavior.
- **Identity precedes numeric conversion.** A source number is resolved to a
  namespaced identity before the corresponding target number is selected.
- **Unknown data is preserved by default.** The source tree is copied wholesale
  and supported structures are rewritten in place; auxiliary files pass
  through unchanged.
- **Reports are audit artifacts.** Object locations, rules, decision traces,
  diagnostics, fingerprints, and file dispositions explain why conversion was
  or was not allowed.
- **Verification has a defined ceiling.** It establishes structural validity
  and catalog membership, not whether arbitrary mod code will accept old NBT
  semantics.

## Where to read next

- [Profiles and registry catalogs](profiles.md) explains the endpoint model in
  detail.
- The root [README](../../README.md) is the operator-facing guide.
- [`examples/rules/example.json`](../../examples/rules/example.json) is the
  concrete rule-format example.
