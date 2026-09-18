# Item mapping report

`rules item-mappings` builds a deterministic non-stock authoring worksheet from
the complete prepared source and target item catalogs. It uses the same rule
graph, source profile, world registry snapshots, aliases, manifests, conflict
selections, vanilla fallbacks, and profile-owned stock mappings as conversion.
Catalog completeness means all identities available in those inputs, not every
item that could exist in a mod installation.

## Authoring loop

For Forge 1.2.5, declare `source_profile: forge-1.2.5` in the rule graph and prepare
a complete source manifest from that modpack's runtime numeric ID map first:

```sh
nix develop -c cargo run -p minecraft-analysis -- rules update-manifest \
  --rules rules/modpack.yaml --id-map source-id-map.txt --manifest source
```

`update-manifest` updates that rule document. The subsequent worksheet command
is read-only with respect to both worlds and the rule graph:

```sh
nix develop -c cargo run -p minecraft-analysis -- rules item-mappings \
  --rules rules/modpack.yaml \
  --source-world /worlds/source \
  --target-world /worlds/target-1.12.2 \
  --report item-mappings.json
```

Repeat `--rules` to load additional YAML roots, for example
`--rules rules/base.yaml --rules rules/modpack.yaml`. Imports are loaded normally.
The source profile must be consistently declared across the graph; supported
sources are Forge 1.2.5 and Forge 1.7.10, and the target is Forge 1.12.2.
Without `--report`, the command prints pretty JSON with one trailing newline to
standard output. File output contains the same bytes and leaves stdout empty.
Invalid worlds, manifests, or rule graphs fail with a nonzero exit status and no
partial report. An existing report file is replaced atomically only on success.
Report paths inside either input world or replacing any loaded rule document
are rejected to preserve the read-only contract.

Review explicit projections and their diagnostics first. Use prospective
candidates and unmatched targets to author item rules, then regenerate the
worksheet. Prospective candidates never modify rules or imply a correct
transformation. A successful command exits 0 even when projections are invalid
or some identities have no suggestions; those are worksheet results to review.

Finally inspect concrete stack variants with:

```sh
nix develop -c cargo run -p minecraft-analysis -- rules coverage \
  --world /worlds/source --rules rules/modpack.yaml --report coverage.json
```

The mapping worksheet does not read region contents or inventories. An item
appears once even if stored stacks have many damage, count, or NBT variants, or
if it never occurs in the world. It makes no claim about observed stack coverage,
nested inventories, or mod-specific storage paths. Those remain separate rule
authoring and coverage concerns.

## Stock mappings and worksheet scope

The built-in Forge profiles own complete version-specific stock identity sets
and hardcoded stock mappings into Forge 1.12.2. Conversion uses those mappings
when no authored item rule takes precedence. Stock membership comes from the
canonical identity for the selected version; numeric IDs, the `minecraft`
namespace, and whether an entry came from world evidence, a manifest, or a
fallback do not determine membership by themselves.

Stock source identities never appear in `source_items`, even when an authored
rule matches them. Stock target identities never appear as prospective candidates
or in `unmatched_target_items`. A non-stock source may still map explicitly to a
stock target. That explicit mapping remains complete, including its rule ID,
declared `target_name`, resolved canonical target name, world-specific numeric
ID, and any invalid-target diagnostic. Aliases resolve against the complete
prepared target catalog before accounting. An unavailable stock projection stays
explicit with a null numeric ID and does not count as a referenced target.

## JSON contract (report_schema 1)

The top level contains `report_schema`, `source_profile`, `target_profile`, sorted
distinct `rule_sets`, `summary`, `source_items`, and `unmatched_target_items`.
Every source entry has `name`, `numeric_id`, and `mapping`; entries sort by numeric
ID then registry name. A mapping is exactly one of:

```json
{
  "kind": "explicit",
  "rule_id": "wood-gear",
  "target_name": "buildcrafttransport:wood_gear",
  "target": {"name": "buildcrafttransport:wood_gear", "numeric_id": 608}
}
```

Only the first deterministically ordered item-rule candidate can project an
identity. Its matcher must be independent of damage, count, and NBT and declare
`target_name`, matching `map_item_id` eligibility. Templates are compiled when
rules load but never executed or inspected for output identities. A conditional
first candidate or one without `target_name` is not bypassed in favor of a later
rule; lexical suggestions are considered independently instead.

The `target_name` field preserves the rule's declaration. `target.name` is the
canonical catalog identity after alias resolution. If the declaration is invalid
or unavailable, `target.name` retains it, `target.numeric_id` is `null`, and
`diagnostic` is `"invalid-target"`. The mapping remains explicit and has no
prospective candidates. Valid explicit mappings omit `diagnostic`.

```json
{
  "kind": "prospective",
  "candidates": [
    {
      "name": "buildcrafttransport:wood_gear",
      "numeric_id": 608,
      "score": 1320,
      "evidence": [
        "normalized-path",
        "path-token-overlap",
        "path-token-order",
        "legacy-structural-affix"
      ]
    }
  ]
}
```

This example is for source `legacy:item.WoodGear`. Every candidate belongs to the
target catalog and meets the fixed threshold. All qualifying candidates are
included, sorted by descending score, then registry name and numeric ID.
If no explicit projection or qualifying candidate exists, the mapping is simply
`{"kind":"none"}`. Inapplicable fields are absent.

`unmatched_target_items` contains compact `{ "name": ..., "numeric_id": ... }`
entries sorted by registry name then numeric ID. Let **N** be all prepared
non-stock target identities, **R** the distinct available canonical identities
referenced by explicit mappings or prospective candidates, and **B** the stock
subset of **R**. Then `unmatched_target_items` is **N − R**, and the worksheet
target universe is **N ∪ B**. The report validates that **R** and **N − R** are
disjoint and their union equals **N ∪ B**. A target referenced by several source
items counts once. Invalid projections with null IDs do not add an identity to
**R**. Unreferenced stock targets do not appear or contribute to counts.

The summary includes:

- `source_items`: total non-stock source entries.
- `explicitly_mapped_source_items`: explicit entries, including invalid targets.
- `source_items_with_suggestions`: prospective entries.
- `source_items_without_suggestions`: entries classified as `none`.
- `target_items`: size of **N ∪ B**.
- `target_items_referenced`: size of **R**.
- `unmatched_target_items`: size of **N − R**.

The three source classifications sum to `source_items`; the referenced and
unmatched target counts sum to `target_items`. Stock-only prepared catalogs
therefore produce empty arrays and zero summary counts.

## Fixed lexical policy

Schema v1 splits paths at non-ASCII-alphanumeric characters, lower-to-upper case
boundaries, acronym-to-word boundaries (`XMLParser` becomes `XML`, `Parser`), and
letter/digit boundaries. Tokens are lowercased in ASCII. Leading and trailing
`item` or `tile` tokens are removed while at least one token remains. Internal
occurrences are preserved. The normalized path joins those tokens with `_`.
There is no stemming, plural conversion, semantic mod knowledge, or numeric-ID
matching. Original names remain unchanged in the JSON.

Namespace family evidence recognizes only spelling migrations: removing
non-ASCII-alphanumeric characters and lowercasing must produce the same nonempty
string. For example, `BuildCraft|Transport` and `buildcrafttransport` agree.
`legacy` is not assigned to any mod family, and namespace prefixes are not treated
as aliases. World registry aliases still apply to explicit projections.

Weights are summed, and evidence appears in this exact order when applicable:

| Evidence | Condition | Weight |
| --- | --- | ---: |
| `exact-identity` | Byte-equal full registry identities | 1000 |
| `exact-namespace` | Byte-equal namespaces | 120 |
| `namespace-family` | Different namespaces with equal nonempty normalized spellings | 100 |
| `normalized-path` | Equal nonempty normalized paths | 600 |
| `path-token-overlap` | Unique path token sets intersect | floor(600 × intersection size / union size) |
| `path-token-order` | At least two distinct shared tokens and both token sequences restricted to shared tokens agree, including repetitions | 100 |
| `legacy-structural-affix` | Either path lost an edge `item`/`tile` token and token sets intersect | 20 |

`exact-namespace` and `namespace-family` are mutually exclusive; the other signals
are additive. A score of **600 or greater** qualifies, without a candidate cap.
These are ranking values, not probabilities or percentages. Same-namespace paths
sharing only one of three unique tokens score 320 and do not qualify. Identical
token sets in different orders across namespaces score 600 and do qualify, so
human review remains necessary. Names with no lexical relationship can receive
no suggestion even when a valid semantic replacement exists.

Weights, normalization, evidence order, threshold, and tie-breaking are part of
the versioned report policy. They have no CLI tuning options. Repeating the same
worlds and rules produces byte-identical reports.
