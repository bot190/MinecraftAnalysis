# Profiles and registry catalogs

“Profile” has a specific and intentionally small meaning in MinecraftAnalysis:
a profile is the supported persisted format of a world endpoint. It tells the
converter how to recognize that endpoint and which format-specific registry
reader to use. It is not a user account, a modpack configuration file, a rule
set, or a complete list of numeric IDs.

The current `WorldProfile` enum has three values:

| Profile | Permitted role | Defining registry structure |
| --- | --- | --- |
| `Forge1_2_5` | Source | caller assertion plus compatible `level.dat` and Anvil terrain |
| `Forge1_7_10` | Source | legacy `FML.ItemData` list |
| `Forge1_12_2` | Template/target | generic `FML.Registries` compound |

These roles are enforced by the CLI. Detecting a valid 1.12.2 world at the
source path, for example, is still an error because this converter implements
only the supported Forge-source-to-1.12.2 directions.

Rule schemas 2 and 3 select a source with `source_profile: "forge-1.2.5"` or
`"forge-1.7.10"`. Selection is resolved across every explicit document and
import. Reusable schema-2/3 imports may omit the field, while schema-1 documents
retain their implicit Forge 1.7.10 behavior. There is deliberately no CLI
override.

## Why profiles exist

Minecraft worlds do not carry one universal, trustworthy “format” switch that
is sufficient for this migration. Forge versions persist registry data in
different shapes, and numeric IDs are meaningful only with the snapshot that
assigned them. Profiles give the rest of the program an explicit, tested
answer to two questions:

1. Is this one of the endpoint formats the converter understands?
2. Which extraction and metadata policies are safe for it?

This prevents format guessing from leaking throughout conversion code. Once
the CLI has required the expected profile, downstream code works with a typed
`DetectedWorld` and its decoded `level.dat`.

## Detection evidence

`profile::detect_world` first requires two basic world properties:

- a readable, decodable `level.dat`; and
- at least one Anvil `.mca` file under `region/` or a `DIM*/region/` directory.

It then inspects persisted NBT evidence. The `FML` compound may be at the NBT
root or under `Data`.

### Forge 1.7.10 evidence

`FML.ItemData` with list type is the structural source-profile marker. Matching
Forge/FML versions in `FML.ModList`—a version containing `1.7.10` or beginning
with `7.10.`—are recorded as supporting evidence.

### Forge 1.2.5 assertion and trust boundary

Forge 1.2.5 is declared by the rules. The converter validates a compound
`Data` root, Anvil `.mca` terrain, and recognized chunk/NBT structures, but it
does not identify or authenticate the exact game or modpack. The caller owns
that historical assertion. McRegion `.mcr` worlds are unsupported.

The profile owns a complete built-in vanilla block and item catalog. These are
separate registries even where a block-item shares the same numeric value.
Built-in assignments are authoritative: a `source_manifest` may repeat them
identically but cannot replace them, even with `conflict: "manifest"`.
Modpack assignments come from template-rule schema 1 manifests, for example:

```yaml
schema_version: 1
rule_set: example-pack-1.2.5
source_profile: forge-1.2.5
source_manifest:
  - { kind: block, name: "example:machine", numeric_id: 180 }
  - { kind: item, name: "example:wrench", numeric_id: 180 }
```

Every encountered mod block or item needs a manifest mapping. The shared
numeric value above is valid because block and item registries are independent.

### Forge 1.12.2 evidence

`FML.Registries` with compound type is the required structural target-profile
marker. `Data.DataVersion = 1343` and matching Forge/FML versions (including
`1.12.2`, `8.0.*`, or `14.23.*`) are supporting evidence, but they are not
enough without `FML.Registries`.

The result contains an evidence list with the NBT path and a human-readable
observation. Today that evidence is used for validation and diagnostics; it is
not serialized into the migration report.

## Failure behavior is conservative

Detection does not pick the “closest” format:

| Situation | Result |
| --- | --- |
| Missing `level.dat` | Reject as not a world endpoint |
| No Anvil terrain | Reject as outside the supported input shape |
| Neither registry marker | Reject as unsupported |
| Both source and target evidence | Reject as ambiguous |
| Valid supported profile in the wrong CLI role | Reject as unexpected |
| Target hints without `FML.Registries` | Reject as unsupported |

This is a safety boundary. A false rejection asks the caller to provide a
supported world; a false acceptance could interpret numeric IDs using the wrong
format.

## A profile is not a registry catalog

The distinction is central to the design:

```text
WorldProfile
  “This metadata uses the Forge 1.7.10 persisted shape.”
                     |
                     v selects extractor
RegistryCatalog
  “In this particular world, block number N means namespace:name.”
```

The profile selects either `extract_forge_1_7_10` or
`extract_forge_1_12_2`. The extractor reads the endpoint's own `level.dat` and
builds a `RegistryCatalog` indexed by both `(kind, name)` and `(kind, numeric
ID)`. Catalog entries retain provenance, and catalogs can also hold aliases,
blocked IDs, dummied identities, and explicit ownership overrides.

Consequently, two worlds with the same profile may have different numeric IDs
because their modpacks or registry histories differ. The converter never says
“profile 1.12.2 implies mod block X is ID 123.” It asks the supplied 1.12.2
template catalog.

Small built-in vanilla tables fill absent baseline entries only when neither
the name nor numeric slot is already occupied. Persisted world evidence wins.
Rule manifests can make explicit selections when registry evidence conflicts;
ordinary conversion does not silently resolve contradictions.

## How profiles participate in conversion

For a normal run, the relationship is:

1. Load the complete rule graph and resolve its source profile.
2. Validate the source according to that profile.
3. Detect and require `Forge1_12_2` for the template.
4. Construct the source catalog from 1.7.10 evidence or the 1.2.5 built-ins and manifests.
5. Extract a target catalog using the target profile's persisted shape.
6. Resolve old numeric IDs through the source catalog to stable namespaced
   identities.
7. Apply rules to those identities and their metadata/NBT.
8. Resolve resulting identities through the target catalog to target numeric
   IDs.
9. Replace the staged source's `FML` and version-identifying metadata with the
   template-owned structures.

The two profiles therefore frame the migration, while the two catalogs supply
the concrete translation facts.

## Metadata ownership at the profile boundary

Profiles also define the current `level.dat` merge policy. The source remains
authoritative for gameplay state and unknown data. The target template owns its
complete `FML` structure plus `Data.DataVersion` and `Data.Version` when those
fields exist.

This is a field-level merge, not a replacement of `level.dat`. In particular,
the template's seed is not adopted. The merge function returns the exact paths
it adopted, which makes the policy testable even though the current pipeline
does not add that list to the report.

## Extending the profile model

Supporting another endpoint is more than adding an enum variant. A complete
extension needs:

- unambiguous persisted detection evidence;
- a role and supported conversion direction;
- a registry extractor for that version's NBT shape;
- version-appropriate vanilla fallbacks;
- an explicit metadata ownership/merge policy;
- storage and traversal support for that world's chunk and object formats;
- verification rules for the emitted format; and
- tests for positive, unsupported, ambiguous, and wrong-role cases.

This is why profiles remain closed and explicit. The current region code is for
pre-flattening Anvil storage; recognizing a newer `DataVersion` alone would not
make newer chunk layouts safe to convert.

Standard traversal covers terrain, entities, block entities, dimensions,
numeric item stacks, and profile-standard player files (`players/` for 1.2.5;
`players/` and `playerdata/` for 1.7.10). Unknown typed NBT is preserved.
Custom mod inventories outside standard player locations are transformed only
when a containing block, item, or entity template explicitly invokes
`transform_item` or `transform_items`; arbitrary compounds are never guessed to
be item stacks.
