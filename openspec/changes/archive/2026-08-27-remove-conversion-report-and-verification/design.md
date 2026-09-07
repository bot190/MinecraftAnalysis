## Context

See `proposal.md` for motivation. Direct conversion currently initializes a `MigrationReport`, has each region scan decoded chunks into `LocatedObject` batches for `preflight::assess_conversion_batch`, transfers `ReportContribution` spool state to the coordinator, mutates the same source content in later traversals, and serializes the accumulated report after publication. The same worker also reopens each temporary region and standalone NBT file, decodes and traverses it, and checks emitted block and item membership before reporting success. Opaque `.dat` pass-through hashes and rereads the destination.

`dry-run` still depends on `MigrationReport`, assessment, fingerprints, and report spooling. `rules coverage` has its own report. `explain` uses source assessment and returns selected `ObjectRecord` values. Staging already isolates all output, commits worker files through temporary siblings, blocks publication after fatal work, and publishes the complete world with a final same-filesystem rename.

## Goals / Non-Goals

**Goals:**

- Make direct conversion transform each supported object without a preceding per-object report-assessment traversal.
- Make successful encode, write, and flush the work-unit boundary before coordinator commit, without a destination reopen.
- Keep conversion failures actionable after partial migration reports are removed.
- Retain bounded parallel scheduling, deterministic failure selection, diagnostic staging, and atomic world publication.
- Preserve all existing analysis reports and their resource bounds.

**Non-Goals:**

- Optimizing or changing the schemas of dry-run and coverage reports.
- Making `explain` directly seek to a location; that remains a separate performance change.
- Adding an optional conversion receipt, summary JSON, `--verify` switch, or standalone verification command.
- Changing rule precedence, transformation semantics, compression policy, or output bytes beyond removal of incidental report/verification work.
- Adding `fsync` durability guarantees or validation by Minecraft, Forge, or mod code.

## Decisions

### Direct conversion accepts no report destination and emits no result document

Separate conversion arguments from analysis report arguments so `convert` no longer accepts or inherits `--report`. On success it returns status zero after publication and writes no JSON to standard output. Interactive progress and ordinary logging remain on standard error. On failure it returns the existing execution-error status with a contextual diagnostic.

This is preferred over an empty report, aggregate receipt, or optional legacy mode because each alternative preserves branching, serialization contracts, or report-oriented coordinator state without an identified consumer. `dry-run` remains the explicit command for a complete target-aware machine-readable report.

### Remove assessment from the mutation pipeline rather than suppressing serialization

Region conversion will stop calling `traversal::scan_chunk` and `preflight::assess_conversion_batch` solely for migration reporting. Its result becomes conversion success rather than `(converted bytes, ReportContribution)`. Standalone conversion likewise skips source assessment and returns only classification information needed for immediate control flow or diagnostics.

The coordinator will no longer receive or merge object records, counts, validation findings, or file dispositions from conversion workers. Analysis paths retain the existing assessment and report types. This obtains the performance benefit at the source instead of merely hiding final JSON output.

### Enforce conversion invariants at mutation boundaries

Removing verification is safe only when every emitted representation is checked before encoding:

- terrain blocks resolve source IDs, apply rules, resolve final target identities, and enforce 12-bit ID and four-bit metadata limits;
- ordinary and recursively nested items resolve final target identities through the same checked item conversion path;
- typed patches and value maps propagate failures;
- NBT and region encoding failures propagate;
- file creation, buffered writes, flushes, temporary-file renames, and publication failures propagate.

Implementation will audit all supported item-bearing paths and add focused tests for any path whose only target-membership protection previously came from verification. Entity and block-entity names are not newly target-validated because the removed verifier did not validate them; changing that contract is outside this change.

This is preferred over retaining a lighter structural reopen because the project's encoders already return structural errors, an immediate reread uses the same reader implementation, and it does not establish Forge compatibility or durable storage.

### Preserve actionable failures without partial reports

Errors raised while resolving or transforming an object will carry all context available at the mutation site: normalized file path, dimension, chunk, block coordinate or typed NBT path, source identity or numeric representation, and responsible rule or invocation chain when applicable. Container, codec, and I/O errors continue identifying their paths and chunk coordinates.

The CLI will render that error directly and confirm through behavior that publication did not occur. It will not continue scanning unprocessed content or serialize records from successful earlier workers. This is preferred over reconstructing a partial report because fail-fast conversion already cannot claim complete inventory semantics.

### Successful writes are committed without reopening

Region, standalone NBT, merged `level.dat`, and opaque pass-through work will write and flush their temporary sibling and then return success. The coordinator retains responsibility for renaming that temporary file to its staged target in deterministic commit order. No semantic read, target lookup, or digest comparison occurs against the destination.

Opaque classification and copying use the same captured source byte buffer so the bytes classified as opaque are the bytes written. A second source read or destination digest is unnecessary for classification consistency.

### Keep transactional publication unchanged

The staging directory, per-file temporary siblings, admission stop on error, deterministic primary-failure selection, bounded active/completed work, prevention of commits after the failure boundary, diagnostic staging, and final same-filesystem publication rename remain unchanged. Local verification is not the publication gate; successful transformation and write completion becomes that gate.

### Replace production self-verification with test coverage

Delete the production `verification` calls and remove the module if no remaining command uses it. Preserve or add tests covering NBT encode/decode round trips, region write/read round trips, compression variants, extended block IDs and `Add`, metadata limits, malformed and oversized chunks, nested items, target identity failures, opaque byte preservation, worker failures, and refusal to publish failed conversions.

Tests may reopen emitted fixtures because they validate the writer independently at development time. Direct conversion will not pay that cost for every production world.

### Simplify progress and documentation around the new boundary

A region completion event means semantic transformation plus successful temporary output write and flush. After all entries commit, publication is the only conversion activity following region work. Report-generation activity remains available to analysis-only commands. Documentation will remove conversion report examples, verification claims, report-spool conversion bounds, and advice that depends on report locations, while directing users to dry-run, coverage, and explain for analysis.

## Risks / Trade-offs

- [External automation consumes conversion JSON or `--report`] → Treat removal as a documented breaking CLI change and direct complete-analysis consumers to `dry-run`.
- [A mutation path relied on post-write target validation] → Audit every emitted block and item path and add target-missing regression tests before deleting verification.
- [An encoder or region writer emits malformed bytes without returning an error] → Strengthen codec, boundary, and end-to-end round-trip tests; keep checked writer errors in production.
- [Storage corrupts a successful buffered write] → Accept ordinary filesystem write/flush semantics; explicit durability and synchronization are outside this change and immediate semantic rereads did not guarantee durability.
- [Failure diagnostics are less complete than partial reports] → Attach precise context at the point of failure and retain staging; do not imply that a partial report described unprocessed content.
- [Users lose aggregate conversion counts and audit fingerprints] → Keep the change intentionally report-free; reconsider a separate compact receipt only after a concrete consumer and schema are identified.
- [Analysis and conversion drift after shared assessment is removed from conversion] → Retain shared registry, rule evaluation, and transformation primitives and cover dry-run/direct-conversion decisions with paired fixtures.

## Migration Plan

1. Strengthen mutation-time context and invariant tests while the existing verification path still provides comparison coverage.
2. Remove conversion assessment and report contribution plumbing, then update CLI behavior and tests so direct conversion emits no report.
3. Remove local verification calls and obsolete verification code after the invariant audit passes.
4. Update progress, operator documentation, architecture documentation, and resource-bound documentation.
5. Run focused and workspace validation inside `nix develop`, including sequential and parallel conversion equivalence and failure-publication tests.

Rollback is a source-level revert of this breaking change. Published worlds require no data migration because the transformation and output formats are unchanged.
