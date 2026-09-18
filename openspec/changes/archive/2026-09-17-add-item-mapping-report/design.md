## Context

The CLI already prepares source and target `RegistryCatalog` values for conversion and exposes deterministic item-rule ordering. For authored projections, `map_item_id` defines the safe identity-projection contract: the first candidate must be identity-only, must declare `target_name`, and must name a target catalog entry. Stock mappings belong to the built-in profiles and do not require authored rules. The initial worksheet implementation includes stock entries; this revision scopes the authoring worksheet to non-stock sources while preserving full explicit references to stock targets. `rules coverage` inventories observed stack signatures, while `rules infer` only compares paired blocks. See `proposal.md` for the authoring gap.

The report crosses CLI argument handling, profile-owned stock mappings, registry preparation, indexed rule inspection, prospective matching, and stable JSON serialization, so an explicit design keeps its automation contract reproducible.

## Goals / Non-Goals

**Goals:**

- Produce an AI-friendly worksheet containing every prepared non-stock source identity, excluding stock sources.
- Share hardcoded stock identity mappings between built-in profiles and conversion so stock items need no authored mapping rules.
- Reuse the same catalog and rule semantics used during conversion.
- Make every explicit and prospective relationship auditable and deterministic.
- Guarantee set-complete accounting of non-stock targets plus distinct stock targets explicitly referenced by non-stock sources.
- Preserve all explicit mapping details for a modded source even when its target is stock.

**Non-Goals:**

- Generate or edit rule documents.
- Infer damage, count, or NBT transformations.
- Inspect transformation template source to guess an output identity.
- Establish a generic registry export format or expose non-item registry kinds.
- Use network data, mod documentation, language models, or nondeterministic heuristics.

## Decisions

### Add a focused `rules item-mappings` analysis command

The command accepts the source world, target world, one or more rule documents, and an optional report path. It uses the existing source-profile and target-world preparation path so manifests, imports, aliases, vanilla fallbacks, and conflicts behave exactly as they do for conversion.

A generic registry command was rejected because it broadens the API without serving the rule-authoring decision. Extending coverage was rejected because coverage is occurrence-based and intentionally cannot see unobserved registered items.

### Own stock identity mappings in built-in profiles

Provide version-specific stock item identity data and hardcoded source-to-target
identity mappings for the supported source profiles, targeting Forge 1.12.2.
Include the stock identity sets needed to classify both sides of the worksheet.
Existing vanilla registry data is the starting point, not permission to leave
stock identities unclassified because a recovery table happens to be partial.
Stock mapping availability must not depend on authored rules or lexical scores.

Use this profile-owned mapping data in conversion and identity mapping helpers;
the report consumes the same stock classification rather than maintaining its
own vanilla list. Resolve mapped target identities through the prepared target
catalog, so world-local target numeric IDs and aliases remain authoritative.
Retain existing validation and authored-rule precedence rather than introducing
a separate override policy for this reporting change. The built-in mapping is
available when no authored stock rule is supplied; it does not imply that a
missing target catalog entry is available.

Classify stock by item registry kind and membership of the canonical identity in
the applicable version's stock identity data. Do not classify by numeric ID,
namespace alone, or provenance. A stock name remains stock when supplied by world
evidence or a manifest; a modded identity occupying a vanilla numeric slot stays
non-stock. Source and target profile versions may recognize different stock sets.

Store stock membership as a complete Forge 1.2.5 baseline followed by sorted
additions and removals for each later supported version. Reconstruct membership
through the applicable deltas, and keep exceptional target-name projections
separate from membership data. This makes version transitions auditable and
limits future profile additions to their changes without altering the report
contract.

A report-local exclusion list would duplicate conversion knowledge and drift.
Filtering all `minecraft:*` names or familiar numeric ranges would incorrectly
hide identities that the selected profile does not recognize as stock.

### Filter the worksheet without truncating prepared catalogs

Keep the complete prepared source and target catalogs for validation and alias
resolution. Derive non-stock views from profile classification for source
iteration and candidate generation; do not remove stock entries from the shared
catalogs. Skip stock sources before inspecting authored rules, so even a rule
matching a stock source cannot create a worksheet row.

Precompute lexical target features only for non-stock targets. Stock targets are
never prospective candidates or unmatched entries. For a non-stock source's
explicit rule projection, inspect the full target catalog and keep all existing
fields: rule ID, declared `target_name`, canonical target name, numeric ID, and
any invalid-target diagnostic. An alias resolving to a stock target has the same
full representation. Do not substitute a marker, redact the target, drop the
modded source row, or mistake a filtered stock target for an unavailable target.

### Model the output as a mapping worksheet

The report retains these top-level fields and mapping variants. `source_items`
contains only non-stock sources. Target summary counts cover non-stock targets
plus available stock targets explicitly referenced by those sources, rather than
the complete prepared target registry. Stock-only inputs produce empty arrays
and zero summary counts. The example contains two non-stock targets:

```json
{
  "report_schema": 1,
  "source_profile": "forge-1.2.5",
  "target_profile": "forge-1.12.2",
  "rule_sets": ["example"],
  "summary": {
    "source_items": 1,
    "explicitly_mapped_source_items": 0,
    "source_items_with_suggestions": 1,
    "source_items_without_suggestions": 0,
    "target_items": 2,
    "target_items_referenced": 1,
    "unmatched_target_items": 1
  },
  "source_items": [
    {
      "name": "legacy:item.WoodGear",
      "numeric_id": 4306,
      "mapping": {
        "kind": "prospective",
        "candidates": [
          {
            "name": "buildcrafttransport:wood_gear",
            "numeric_id": 608,
            "score": 1320,
            "evidence": ["normalized-path", "path-token-overlap", "path-token-order", "legacy-structural-affix"]
          }
        ]
      }
    }
  ],
  "unmatched_target_items": [
    {"name": "buildcrafttransport:plug_pulsar", "numeric_id": 719}
  ]
}
```

Use an integer score rather than floating point so serialized values and comparisons remain portable and exact. Represent mapping classifications as tagged serializable variants so fields that do not apply are absent rather than null, except that an explicit mapping to a missing target deliberately carries a null numeric ID and diagnostic.

Repeating a compact target identity under candidate mappings is preferable to a separate target table: each source entry remains self-contained for an agent, and the unmatched section fulfills target accounting without creating a generic registry export.

### Reuse the `map_item_id` eligibility boundary for explicit mappings

For each non-stock source identity, inspect the same deterministically ordered item candidates used by runtime rule selection. Only the first candidate can supply an explicit mapping, and only when its matcher is independent of damage, count, and NBT and it declares `target_name`. Validate the projected name against the full target catalog, including stock identities, but retain an invalid explicit projection in the report so authors can repair the rule.

This deliberately does not parse a template for a literal output name. Templates can branch or call functions, and source inspection would create semantics different from execution. Later candidates are not promoted past an ineligible first candidate because doing so would disagree with `map_item_id`.

### Use a fixed deterministic lexical matching policy

Prospective matching applies only to a non-stock source with no eligible explicit projection. Build a normalized feature set for that source and each non-stock target identity from:

- exact full registry identity;
- exact or migrated namespace;
- normalized path equality;
- ordered path tokens split on punctuation, case transitions, and digit transitions;
- token overlap and order;
- recognized structural affixes such as legacy `item` or `tile` wrappers.

Assign documented integer weights to evidence, sum them without runtime configuration, and emit candidates meeting one fixed threshold. Candidate evidence uses stable identifiers in fixed order. Pre-index non-stock target features to avoid repeatedly tokenizing the target catalog; compare source and target entries deterministically and sort qualifying results by score and identity.

The first implementation will keep weights and the threshold part of the versioned report policy rather than add a CLI tuning option. User-configurable thresholds were rejected because identical repositories could then produce incomparable worksheets and because low thresholds can make target accounting appear meaningful while associating unrelated items. Semantic mod knowledge and external alias databases remain future extensions that would require explicit, auditable input.

### Schema v1 matching policy

Split paths at non-ASCII-alphanumeric characters, lower-to-upper case boundaries,
acronym-to-word boundaries (`XMLParser` → `XML`, `Parser`), and letter/digit
boundaries. Lowercase ASCII tokens. Remove leading and trailing `item` or `tile`
tokens while at least one token remains; preserve internal occurrences. The
normalized path is the remaining ordered tokens joined with `_`. Do not stem,
singularize, or interpret words semantically. Preserve original names in output.

Exact namespaces are byte-equal. A migrated namespace spelling belongs to the
same lexical family only when removing non-ASCII-alphanumeric characters and
lowercasing gives the same nonempty string (for example `BuildCraft|Transport`
and `buildcrafttransport`). Do not assign `legacy` to a mod family, infer prefix
relationships, or consult a mod alias database.

Sum these integer weights and emit evidence in this order, omitting absent
signals. A candidate qualifies at score **600** or higher; no candidate cap applies.

| Evidence | Condition | Weight |
| --- | --- | ---: |
| `exact-identity` | Full registry names are byte-equal | 1000 |
| `exact-namespace` | Namespaces are byte-equal | 120 |
| `namespace-family` | Namespaces differ but their nonempty normalized spellings agree | 100 |
| `normalized-path` | Nonempty normalized paths agree | 600 |
| `path-token-overlap` | Unique token sets intersect | floor(600 × intersection size / union size) |
| `path-token-order` | At least two distinct shared tokens, and both ordered token sequences restricted to shared tokens agree (including repetitions) | 100 |
| `legacy-structural-affix` | Either path lost an edge `item`/`tile` token and the token sets intersect | 20 |

Exact-namespace and namespace-family evidence are mutually exclusive. All other
signals are additive. Scores are lexical ranking values, not percentages or
probabilities. Candidates sort by descending score, then bytewise registry name,
then numeric ID. Example: `legacy:item.WoodGear` →
`buildcrafttransport:wood_gear` scores 1320. Same-namespace paths sharing only one
of three unique tokens score 320 and do not qualify. Numeric IDs provide no
matching evidence, since assignments vary between worlds.

Explicit mappings retain the rule's declared `target_name` alongside a `target`
object containing the resolved canonical name and numeric ID. This preserves
alias projections while counting the canonical target exactly once. Invalid
projections retain their declared name, a null numeric ID, and the diagnostic
`invalid-target`. Loaded rule-set IDs are sorted and deduplicated.

### Derive unmatched targets by set subtraction

Let N be all prepared non-stock target identities. Collect R, the distinct
available canonical targets referenced by explicit mappings or prospective
candidates from non-stock source rows. Let B be the stock identities in R; these
can only arise from explicit mappings. Resolve aliases before inserting into R.
Invalid projections with null numeric IDs do not add a target identity, including
when the declared target name denotes stock in profile data but is unavailable in
the prepared catalog.

Compute `unmatched_target_items` as N minus R, and the worksheet target universe
as N union B. Before serialization, validate that the referenced and unmatched
sets are disjoint and their union equals N union B. Validate that source rows
cover exactly the non-stock source identities, once each. Fail internally rather
than serialize an inconsistent worksheet.

`summary.target_items` is the size of N union B,
`summary.target_items_referenced` is the size of R, and
`summary.unmatched_target_items` is the size of N minus R. All source summary
counts exclude stock sources. Multiple modded-to-stock rules referencing the same
canonical target count it once. Unreferenced stock targets do not affect any
count. For example, two non-stock targets, one unmatched, plus one stock target
explicitly referenced by three modded sources yield target counts 3 total,
2 referenced, and 1 unmatched. Stable B-tree-backed sets and final explicit sorts
prevent hash iteration from affecting bytes.

### Keep the report independent of terrain traversal

The command reads world metadata needed to prepare catalogs but does not traverse region files or inventories. This makes non-stock registry completeness independent of which items happen to be stored and avoids duplicating the coverage engine. Authors use this report for identity rules and `rules coverage` for concrete stack variants.

## Risks / Trade-offs

- [Incomplete profile stock data leaks vanilla items into the worksheet] -> Verify the version-specific stock identity sets and hardcoded mappings with profile fixtures; do not equate partial fallback tables with complete classification.
- [Filtering stock targets hides a valid modded-to-stock projection] -> Validate explicit mappings against the full catalog and test canonical stock targets, aliases, repeated references, and unavailable projections.
- [Summary semantics differ from the initial implementation] -> Document the worksheet target universe and update fixtures and invariant checks together.

- [Lexical similarity can produce plausible but incorrect candidates] -> Label all non-rule relationships as prospective, emit evidence and scores, use a conservative fixed threshold, and never modify rules automatically.
- [Renamed items with no lexical relationship receive no suggestion] -> Preserve them as `none` source mappings and list unrelated targets as unmatched so an agent can reason over the omissions explicitly.
- [A complete Forge 1.2.5 catalog depends on a complete source manifest] -> Reuse existing manifest requirements and report only the prepared catalog; document that `rules update-manifest` remains the prerequisite for modded 1.2.5 identities.
- [Candidate comparison is quadratic in catalog size] -> Precompute normalized features and introduce evidence indexes where useful while retaining deterministic final ordering; typical item catalogs remain small enough for a read-only authoring command.
- [Changing weights later changes output] -> Version the report schema and treat weight or threshold changes as observable policy changes with fixture coverage.

## Migration Plan

This revision completes the existing, unarchived change. Retain report-schema
version 1 and its field shapes while updating the planned population and counting
semantics before archiving. Existing reports from the initial implementation
must be regenerated to apply the stock exclusions; authored rule documents need
no schema migration.

Implement and verify the shared profile stock mappings first, then apply the
worksheet filters, accounting, integration fixtures, and documentation changes.
Keep the lexical policy unchanged. Reopen affected completed tasks and add tasks
for the profile mapping work; completion of the original implementation is not
evidence that this revision is implemented. Rollback must treat the shared
profile mapping changes and the report filtering as distinct changes and must
not alter saved worlds or rule documents.
