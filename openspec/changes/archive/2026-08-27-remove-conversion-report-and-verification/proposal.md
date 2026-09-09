## Why

Direct conversion currently performs complete per-object assessment for a migration report and then reopens and retraverses every emitted file for local verification. These duplicate whole-world passes make conversion slower and more complex even though coverage, dry-run, targeted explanation, mutation-time invariants, tests, staging, and atomic publication provide clearer mechanisms for rule completeness, diagnostics, correctness, and safety.

## What Changes

- **BREAKING** Remove machine-readable migration-report generation from `convert`, including per-object records, aggregate report state, input fingerprints, file dispositions, partial failure reports, report output, and conversion-time report spooling.
- **BREAKING** Remove post-write local verification of converted regions, standalone NBT, and opaque pass-through files; a successfully encoded and flushed temporary file becomes eligible for coordinator commit without reopening or semantic retraversal.
- Preserve complete reports for `dry-run` and `rules coverage`, and preserve detailed source-side rule diagnostics through `explain`.
- Require direct conversion failures to carry actionable object or file context without depending on a partial migration report.
- Preserve mutation-time source and target registry checks, typed conversion errors, bounded work, diagnostic staging, deterministic failure selection, and atomic publication.
- Remove conversion-report and verification lifecycle messaging and update operator and architecture documentation to describe the streamlined conversion contract.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `forge-world-conversion`: Remove conversion reports and local output verification while retaining transactional publication and actionable failures.
- `world-transformation-rules`: Move changed-object rule traceability exclusively to diagnostic analysis rather than migration reports.
- `resource-bounded-rule-coverage`: Remove conversion report/spool and verification work from the bounded conversion model while retaining analysis reports.
- `parallel-region-processing`: Simplify region work units and parallel equivalence by removing report contribution transfer and local verification.
- `cli-progress-reporting`: Remove conversion report-generation and verification semantics from conversion progress.
- `opaque-dat-passthrough`: Copy classified opaque data without conversion-report records or destination digest verification.

## Impact

- Affects the `convert` CLI contract and removes its report output/path behavior; `dry-run`, `rules coverage`, and `explain` remain analysis interfaces.
- Removes conversion-time assessment and report contribution construction from `pipeline`, `region_conversion`, standalone document conversion, `preflight`, `report`, and spool integration points.
- Removes the production verification module and its region, NBT, and opaque-file calls where no remaining command uses them.
- Requires conversion errors to retain exact available file, dimension, chunk, block, NBT-path, identity, and rule context directly.
- Simplifies worker results, coordinator reduction, resource documentation, progress activities, integration tests, and end-to-end fixtures.
- Does not weaken read-only inputs, temporary staged writes, checked encoding and I/O errors, failure-gated publication, or the final same-filesystem publish rename.
