# Conversion performance investigation

## Status

This document records an architectural investigation. It describes the current
implementation and a possible redesign; it is not a description of behavior
that has already been implemented. No representative-world benchmark or CPU
profile has yet been captured, so optimization priorities below are based on
code-path analysis rather than measured timings.

## Problem statement

The `rules coverage` command and the actual conversion flow are slow on large
worlds. The initial investigation found that both paths already process region
files in parallel and bound their retained state, but they perform substantial
per-object assessment, reporting, temporary-spool, and repeated traversal work.

The investigation later narrowed the intended product responsibilities:

- `rules coverage` should determine whether source objects have migration
  coverage.
- `dry-run` should provide optional complete, target-aware compatibility
  analysis.
- `explain` should diagnose the transformation selected for one location.
- `convert` should perform one transactional transformation pass.
- Automated tests should establish codec and writer correctness.

Under this model, a complete per-object conversion report and production
post-write self-verification are not necessary parts of conversion.

## Current coverage flow

Coverage uses `preflight::reduce_source_with_progress_config` to discover files,
read regions, decode chunks, traverse supported objects, and reduce region-local
coverage accumulators. Region work defaults to the process's available CPU
parallelism and can be controlled by `--jobs`.

For each block or item, coverage currently:

1. resolves its source identity;
2. evaluates applicable object rules and constructs a complete decision trace;
3. checks whether the object has built-in vanilla coverage;
4. renders uncovered NBT as SNBT;
5. groups uncovered signatures; and
6. periodically writes sorted groups to temporary spool runs.

The implementation is bounded, but several operations are more expensive than
necessary:

- Built-in vanilla objects are rule-evaluated before the built-in coverage
  check, even though coverage discards the rule trace for covered objects.
- Block and item evaluators linearly scan every loaded rule of the corresponding
  kind and construct formatted trace reasons for identity mismatches.
- A sorted run is forced to disk after 4,096 uncovered observations rather than
  according to retained distinct groups or encoded bytes.
- Region runs are read into the coordinator and may be written into new runs.
- Final k-way merging scans every run head to choose each next record, giving
  work proportional to records multiplied by run count rather than a heap-based
  logarithmic selection cost.

### Coverage optimization candidates

The likely coverage priority is:

1. Check authoritative built-in vanilla coverage before rule evaluation.
2. Compile rules into indexes keyed by object kind and identity.
3. Add a coverage evaluator that returns immediately for selected rules and
   constructs structured rejection information only for uncovered objects.
4. Spill according to retained group size or encoded bytes rather than raw
   occurrence count.
5. Compact sorted runs hierarchically and use a heap-based k-way merge.
6. Replace repeated JSON spool encoding and decoding with a more direct internal
   record representation.

The built-in fast path is specific to coverage. Conversion cannot blindly skip
vanilla objects because explicit rules may transform them, and every emitted
identity must still resolve through the target catalog.

## Current conversion flow

Conversion is fused at the region-work-unit level, but each decoded chunk still
passes through multiple semantic traversals:

```text
source region
    |
    +-- read, decompress, and decode chunk
    |
    +-- scan chunk into location-aware observations
    |     +-- evaluate rules for migration reporting
    |     +-- build and spool per-object report records
    |
    +-- traverse block storage
    |     +-- evaluate block and coordinated block-entity rules again
    |     +-- resolve target IDs and mutate storage
    |
    +-- traverse inventories, entities, and block entities
    |     +-- evaluate rules again and mutate NBT
    |
    +-- encode, compress, and write a temporary staged region
    |
    +-- reopen the temporary region
          +-- read, decompress, and decode every chunk again
          +-- traverse blocks and items again for verification
```

The worker then returns its report contribution. The coordinator renames the
temporary file into staging, reads the contribution's report spool, appends its
records to the world report, and eventually publishes the completed staging
tree with a same-filesystem rename.

Standalone NBT conversion, `level.dat` merging, and ordinary file copying are
performed synchronously while the work iterator produces region units. Large
non-region work can therefore delay admission and leave region workers idle.

## Shared rule-evaluation optimizations

### Candidate indexes

Rule evaluation currently scans all ordered rules of a kind for every object.
Rules can instead be compiled into preordered candidate buckets for:

- exact namespaced identities;
- exact numeric identities where supported;
- wildcard or range identities; and
- exact entity and block-entity names.

Applicable buckets must be merged while preserving the existing priority,
terminal-rule, and deterministic-order semantics. This changes common work from
approximately `observations * all rules` to `observations * plausible rules`.

### Separate execution decisions from diagnostic traces

The hot conversion path needs selected actions, not formatted rejection
explanations. The rule engine can expose a compact execution decision while
`coverage`, `dry-run`, and `explain` request structured diagnostic facts and
format traces only when they will be emitted.

This avoids cloning rule IDs and formatting reason strings for the many objects
whose decisions are immediately applied and discarded.

## Conversion-specific optimization candidates

### Compile terrain translations

Most terrain transformations depend only on source block ID and four-bit
metadata. A conversion plan can precompute up to 4,096 by 16 ordinary entries:

```text
translation[source_id][metadata]
    -> target ID
    -> target metadata
    -> disposition
    -> selected action summary
```

Entries whose rules depend on an associated block entity remain on a marked
slow path. This preserves explicit rules for vanilla identities while replacing
per-block source lookup, rule scanning, target-name parsing, and target lookup
with an array access for the common case.

### Avoid block-entity cloning

`block_entity_snapshot` currently clones complete typed block-entity NBT values
into a coordinate map. Conversion can instead index list positions or borrow
values after restructuring the mutable section and tile-entity access. Only
coordinates relevant to block-entity-sensitive rules need special decision
state.

The interaction between coordinated block-entity actions in `convert_chunk`
and later ordinary `TileEntities` processing in `convert_document` also needs a
semantic audit. A transformed block entity may currently pass through both
evaluation mechanisms. The intended ordering and whether both actions are
allowed should be made explicit before consolidating that work.

### Preserve unchanged storage

A compiled terrain plan can establish whether an entire section maps
identically and has no coordinate-sensitive action. Such a section need not
rebuild its `Blocks`, `Data`, and `Add` arrays.

If block, item, entity, and block-entity processing also reports no changes for
a chunk, the original compressed chunk payload could be copied into the output
region with its original compression scheme and timestamp. `RegionWriter`
already supports writing pre-compressed chunks. This creates a useful hierarchy:

```text
changed object    -> transform and rewrite
unchanged section -> preserve section arrays
unchanged chunk   -> preserve compressed chunk payload
```

This requires one fused semantic pass that returns a reliable `changed` result;
adding a separate change-detection traversal would undermine the benefit.

### Parallelize non-region work

Standalone NBT conversion and sufficiently large ordinary file copies can be
submitted as bounded worker units rather than performed synchronously by the
producer. Directory creation and deterministic staging commits can remain with
the coordinator. Memory bounds may require distinct limits for complete region
buffers and standalone files.

### Measure compression separately

Chunk decompression and recompression are likely material CPU costs, but have
not been profiled. Measurements should separate file I/O, decompression, NBT
decode, rule evaluation, NBT encode, compression, and staging writes. Depending
on the result, a faster DEFLATE backend or an explicit speed/size policy may be
worth considering.

## Proposed removal of the conversion report

The complete conversion report is expensive and difficult to use. It retains a
record for every observed object, including unchanged terrain blocks. Producing
it requires a complete assessment traversal before mutation, per-object
allocation and sorting, rule evaluation duplicated with conversion, JSON spool
encoding, worker-to-coordinator spool merging, and final pretty-JSON output.

Removing the per-object conversion report would permit deletion of the entire
`scan_chunk` and `assess_conversion_batch` branch from region conversion. Each
object could then be resolved, decided, and transformed once.

The proposed responsibility split is:

| Question | Mechanism |
| --- | --- |
| Are source objects covered by migration policy? | `rules coverage` |
| Can the whole source be converted against this target? | optional `dry-run` |
| What rule and value-map behavior applies here? | targeted `explain` |
| Did transactional conversion succeed? | exit status and atomic publication |
| Why did conversion fail? | structured fatal diagnostic with exact location |

A small completion summary could remain if it has a concrete consumer. Counts
for converted regions, chunks, files, and dispositions can be accumulated
during mutation at low cost. It should not retain per-object locations or
traces. Input-tree fingerprinting should likewise remain only if there is an
actual audit requirement, since hashing unrelated copied files adds serial I/O.

### Information no longer retained

Removing the report discards:

- every transformed and unchanged object location;
- complete counts unless lightweight counters are retained;
- full input fingerprints;
- registry mapping summaries;
- every file disposition;
- opaque-file classifications in a structured migration report; and
- partial records from regions completed before a fatal error.

These are acceptable losses if no external consumer relies on the report.
Opaque-file handling can emit a warning, while failure diagnostics should carry
the failing source identity, numeric representation, dimension, chunk, block or
NBT path, responsible rule, and confirmation that output was not published.

## `explain` as the debugging mechanism

`explain` already produces the useful per-object diagnostic fields:

- source and predicted target identity;
- disposition;
- object location;
- selected rule identifiers;
- rule-matching and rejection trace;
- applied value-map outcomes; and
- diagnostics.

It analyzes source state and predicts a transformation; it does not inspect the
published output. That distinction is appropriate if output correctness remains
the responsibility of conversion invariants and tests.

The current implementation is not yet an efficient debugger: an exact explain
query scans and assesses the complete source world, retaining only records that
match the requested location. It should instead resolve locations directly:

```text
terrain location
    -> derive dimension, region, and local chunk
    -> read one region and decode one chunk
    -> assess the selected block or contained object

standalone NBT location
    -> read one relative file
    -> resolve the typed NBT path
    -> assess the selected object
```

This targeted explain work is an important companion to removing the conversion
report. Without it, detailed debugging remains unnecessarily proportional to
world size.

## Proposed removal of production verification

After writing each temporary region, conversion currently reopens it and uses
the project's own reader and traversal code to check structural readability and
target catalog membership. Standalone NBT and opaque files receive comparable
local checks.

Most target invariants are already enforced during mutation:

- transformed block identities must resolve in the target catalog;
- target block IDs must fit the 12-bit storage range;
- block metadata must fit four bits;
- transformed item identities must resolve in the target catalog;
- patches and value-map applications return errors;
- encoders and region writers return errors; and
- staging writes, flushes, and renames return errors.

Production verification therefore largely tests that output written by the
project's writer can be read by the project's reader. It does not run Minecraft,
Forge, or mod code and cannot establish gameplay compatibility. It also offers
limited durability assurance: an immediate reread may use the page cache, and a
successful reread does not protect against later storage corruption.

Removing verification avoids a second complete region read, chunk
decompression, NBT decode, object traversal, and target lookup pass. Writer and
reader correctness should instead be established with unit round trips,
boundary fixtures, and end-to-end conversion tests. Before removal, emitted
identity checks should be audited across terrain blocks, ordinary and nested
items, entity-held items, block-entity inventories, and standalone inventories
so no path relies on verification as its only target check.

Opaque-file digest rereading can also be removed. Successful buffered copy and
flush behavior is a more conventional production contract; stronger durability,
if required, should be designed explicitly using file and directory
synchronization rather than semantic retraversal.

## Safety properties to retain

Dropping reports and verification does not require weakening transactional
conversion. The redesigned flow should retain:

- read-only source and template inputs;
- a clearly named sibling staging directory;
- temporary sibling files within staging;
- checked write and flush errors;
- coordinator commit only after successful work-unit transformation;
- cancellation and prevention of publication after any fatal error;
- preservation of failed staging state where currently promised; and
- one same-filesystem rename for complete-world publication.

The resulting intended path is:

```text
prepare catalogs and compiled rules
              |
              v
bounded parallel region conversion
  read -> decode -> transform once -> encode -> write temporary
              |
              v
coordinator commits successful staging files
              |
              v
atomic publication of the complete staging tree
```

## Expected priority

If the architectural removals are accepted, the likely implementation priority
by expected impact is:

1. Remove complete per-object assessment and report production from `convert`.
2. Remove production reopen-and-verify passes after auditing mutation-time
   invariants.
3. Make `explain` resolve and assess one location directly.
4. Compile block translations and rule candidate indexes.
5. Avoid block-entity cloning and reconcile coordinated block-entity handling.
6. Preserve unchanged section arrays and compressed chunks.
7. Parallelize standalone NBT and large copy work.
8. Tune compression and spool behavior only where profiling still shows value.

This order first deletes whole-world duplicated work, then optimizes the single
remaining semantic pass.

## Specification impact

The proposed redesign contradicts current main specifications and therefore
requires an explicit OpenSpec change before implementation. At least the
following capabilities are affected:

- `forge-world-conversion`: remove the complete and partial migration-report
  requirements and local emitted-content verification; define concise failure
  diagnostics and successful publication as the conversion outcome.
- `world-transformation-rules`: move detailed selected-rule and value-map
  explanation to the diagnostic command and retain mutation-time loss policy.
- `resource-bounded-rule-coverage`: remove conversion-report spool and verified
  path bounds while preserving analysis-report bounds and bounded conversion
  work.
- `parallel-region-processing`: remove report-contribution equivalence and local
  verification requirements while retaining deterministic staged bytes,
  failure selection, commit ordering, and bounded scheduling.
- `cli-progress-reporting`: remove conversion report-generation and implicit
  verification activity from the conversion lifecycle.
- `opaque-dat-passthrough`: replace structured conversion-report records with
  an appropriate warning or silent byte-preserving copy contract.

Coverage and `dry-run` reports remain separate analysis products unless a later
change revisits them.

## Validation and measurement plan

Before and during implementation, capture representative release-build runs
inside the required Nix development environment. At minimum, compare:

- `--jobs 1`, `2`, `4`, `8`, and the process default;
- complete versus substantially incomplete rulesets;
- terrain-heavy versus standalone-NBT-heavy worlds;
- worlds with few versus many changed chunks; and
- report, verification, rule evaluation, codec, compression, and I/O time.

Useful counters include regions, present chunks, populated sections, non-air
blocks, items, entity and block-entity objects, candidate rules evaluated,
decision-trace allocations, report bytes, spool bytes and runs, unchanged
sections, unchanged chunks, and bytes copied in compressed form.

Correctness validation should include:

- semantic equivalence of converted world data before and after hot-path
  optimizations;
- sequential and parallel staged-byte equivalence where currently required;
- exact failure locations and prevention of publication;
- NBT and region encode/decode round trips;
- 12-bit block IDs, `Add` arrays, metadata limits, large chunks, and sector
  boundaries;
- nested inventories and block-entity-sensitive rules;
- direct `explain` equivalence with the former full-world assessment for selected
  locations; and
- successful loading with representative caller-provided Forge/modpack
  environments when those external fixtures are available.

## Open questions

1. Does any current or planned consumer depend on the complete conversion JSON
   report, its fingerprints, or its individual unchanged records?
2. Should conversion retain a compact machine-readable receipt, a human summary,
   or only exit status and progress output?
3. Are explicit rules allowed to transform identities that would otherwise pass
   through unchanged? The compiled translation design assumes yes.
4. What is the intended ordering between coordinated block-entity actions and
   ordinary block-entity rules?
5. Must output bytes be durable across sudden power loss at command completion,
   or is successful buffered write plus atomic publication sufficient?
6. Should direct `explain` support source prediction only, or also inspect a
   converted world for comparison?
7. Are complete dry-run object records still valuable, or should dry-run
   eventually adopt grouped findings and bounded location samples like coverage?

## Conclusion

The largest conversion speedups appear to come from removing responsibilities,
not micro-optimizing the existing repeated passes. A conversion command without
a complete per-object report or production self-verification can perform one
transactional semantic pass per object and rely on `coverage`, `dry-run`, a
targeted `explain`, mutation-time invariants, and automated tests for the
separate questions those mechanisms answer.

After those architectural changes, compiled terrain translations, indexed rule
candidates, unchanged-chunk preservation, and broader bounded parallelism can
optimize the remaining necessary work.
