# Resource-Bounded Rule Coverage Specification

## Purpose

Ensure analysis and conversion of large Minecraft worlds use resource-bounded, deterministic processing across discovery, preflight, staging, transformation, verification, explanation, reporting, and publication while remaining safe for future bounded parallel execution.

## Requirements

### Requirement: Bound retained analysis state
The system SHALL assess source objects incrementally for rule coverage, dry-run, explanation, and direct conversion. Excluding fixed registry and rule inputs plus information required by the requested report or query result, retained state MUST be bounded by active work batches and MUST NOT grow with the total number of completed batches.

#### Scenario: Covered objects span many regions
- **WHEN** analysis or conversion scans successively more regions containing covered objects
- **THEN** completing a region releases its decoded source data and per-object observations before unbounded additional regions are retained

#### Scenario: Uncovered or unresolved results accumulate
- **WHEN** an analysis-only command encounters findings that its complete report must contain
- **THEN** it retains or spools only the records, counts, and diagnostics required to emit that report

#### Scenario: Conversion encounters a fatal object
- **WHEN** direct conversion encounters an unresolved object or another fatal condition
- **THEN** it returns the error without retaining or scanning unprocessed world content for a complete failure report

#### Scenario: Explain filters analysis incrementally
- **WHEN** the user requests an explanation for an exact report location
- **THEN** the system returns every matching record without retaining unrelated whole-world object records in memory

### Requirement: Process conversion content without preflight
The `convert` command SHALL begin staged traversal after validating paths, profiles, registries, and rule documents, without first completing a read-only content preflight. Each conversion work unit SHALL evaluate the source content it transforms and return fatal errors upward immediately.

#### Scenario: Direct conversion finds an unresolved object
- **WHEN** a conversion work unit cannot safely resolve or transform an encountered object
- **THEN** the work unit fails, conversion stops admitting new work, and the output world is not published

#### Scenario: Analysis-only dry run
- **WHEN** the caller requests `dry-run`
- **THEN** the system completely analyzes source content without creating staging or an output world

### Requirement: Preserve analysis semantics
Except for the explicitly container-scoped nested-object budget, incremental analysis SHALL produce the same deterministic results as whole-world analysis for the same inputs, including classifications, counts, dispositions, signature grouping, ordered deduplicated locations, associated block-entity data, nested-item findings, warnings, fingerprints, selected rules, target identities, diagnostics, and rejection traces.

#### Scenario: Compare incremental and reference analysis
- **WHEN** the same fixture world, template, and rules are processed by incremental analysis and a reference whole-world analysis
- **THEN** both produce equivalent coverage and migration reports

#### Scenario: Associate a block entity within a bounded batch
- **WHEN** an observed block has a block entity at the same dimension and coordinates
- **THEN** analysis includes the required coordinated identity, rule, trace, and SNBT information before releasing the containing source batch

#### Scenario: Discover rule-declared nested items
- **WHEN** an applicable rule declares nested item paths in an observed object
- **THEN** every discovered nested item participates in analysis before its containing source data is released

### Requirement: Scope nested-object budgets to source containers
The system SHALL enforce an independent nested-object budget for each region file and each standalone NBT document. Nested-object accounting MUST NOT require coordination between independently schedulable source containers.

#### Scenario: A region exhausts its nested-object budget
- **WHEN** nested-item discovery within one region exceeds the configured budget
- **THEN** analysis fails that region deterministically without consuming a budget owned by another region or standalone NBT document

#### Scenario: Nested objects span multiple containers
- **WHEN** multiple regions or standalone NBT documents each remain within their own nested-object budget
- **THEN** analysis accepts their combined nested-object count even when it exceeds one container's budget

### Requirement: Reduce analysis at chunk and region levels
Each region worker SHALL reduce one decoded chunk observation batch at a time into region-local bounded summary or managed spool state and SHALL release that batch before processing the next chunk. The coordinator SHALL combine completed region results in canonical region order. A completed region result MUST NOT retain the region's collection of `LocatedObject` batches.

#### Scenario: A worker analyzes a multi-chunk region
- **WHEN** a region contains multiple chunks with covered or reportable observations
- **THEN** the worker releases each chunk's decoded and observation state after region-local reduction while retaining only bounded summary state and required managed spool records

#### Scenario: Regions complete out of order
- **WHEN** later region workers complete before an earlier canonical region
- **THEN** the coordinator retains only the configured bounded number of region results and merges them in canonical region order without preventing active workers from reducing their own chunks

### Requirement: Stage source entries incrementally
The system SHALL traverse source entries incrementally and create each staged entry in its final staged representation where practical. It MUST NOT require a complete in-memory source-tree entry list, a complete content preflight, or an unconditional preliminary copy of every transformable file.

#### Scenario: Copy an unchanged file
- **WHEN** a regular source file requires no transformation
- **THEN** the system copies it to staging with bounded I/O buffers and records its file disposition

#### Scenario: Transform a supported world file
- **WHEN** a source region or standalone NBT file requires conversion
- **THEN** the system writes its locally verified converted representation through a temporary staged sibling and atomically commits that staged file without first copying the unconverted file to its staged destination

#### Scenario: Exclude a transient source file
- **WHEN** traversal encounters a transient world lock or another explicitly excluded entry
- **THEN** the system records the exclusion without copying the entry into staging

### Requirement: Bound transformation state by active containers
The system SHALL release transformation data after each staged work unit commits. It MAY retain one complete region, chunk, standalone NBT document, or other atomic container when its storage format or safe replacement protocol requires container-level processing, but MUST NOT retain an unbounded number of completed containers.

#### Scenario: Convert many region files
- **WHEN** conversion processes successively more source regions
- **THEN** encoded and decoded state from committed regions is released and retained transformation state remains bounded by the configured in-flight work limit

#### Scenario: Convert a large standalone NBT file
- **WHEN** safe typed conversion requires decoding the complete standalone NBT document
- **THEN** the system may retain that document while converting its file but releases it after the temporary staged file is committed

#### Scenario: Region writer requires random access
- **WHEN** region header offsets and sector allocation require seekable output
- **THEN** the system may use a seekable temporary staged file while keeping chunk decoding and encoding bounded within that region work unit

### Requirement: Verify staged output incrementally
Each transformed file work unit SHALL reopen and verify its temporary staged output before committing it. Verification MUST use bounded traversal and container state, and the system SHALL NOT perform a separate world-wide verification traversal after conversion.

#### Scenario: Verify many staged files
- **WHEN** conversion processes successively more transformed files
- **THEN** each file is locally verified and released within its work unit before unbounded later files are retained

#### Scenario: Verification fails
- **WHEN** temporary staged content is structurally invalid or references an identity absent from the target registry
- **THEN** its work unit fails, that file is not committed, publication is refused, and staging remains available for diagnosis

### Requirement: Stream deterministic reports with bounded memory
The system SHALL emit coverage and migration reports through buffered output without constructing a second complete report-sized JSON string. When required report records exceed the configured in-memory report bound, the system SHALL use managed bounded temporary storage while preserving the existing report schema, completeness, and deterministic ordering.

#### Scenario: Write a report to a file
- **WHEN** the user supplies a report path
- **THEN** the system emits the complete deterministic pretty-JSON report through buffered file output and reports any creation, serialization, or flush failure

#### Scenario: Write a report to standard output
- **WHEN** the user omits a report path
- **THEN** the system emits the complete deterministic pretty-JSON report through buffered standard output before selecting the command exit status

#### Scenario: Report records exceed the memory bound
- **WHEN** complete ordered report output requires more record state than the configured in-memory bound permits
- **THEN** the system spills records to managed temporary storage and cleans that storage after successful output while preserving diagnosable state on a conversion failure where required

### Requirement: Preserve transactional conversion behavior
Streaming conversion SHALL preserve the safety boundary: the source and template remain read-only, incomplete staged work is never published as the output world, each transformed file is locally verified before commit, and final publication uses an atomic rename only after every work unit succeeds.

#### Scenario: A staged work unit fails
- **WHEN** copying, decoding, transforming, encoding, locally verifying, or committing any staged work unit fails
- **THEN** conversion stops, the output path is not published, and staging remains diagnosable without exposing a partially written committed file

#### Scenario: All staged work succeeds
- **WHEN** every required source entry has been staged and every transformed file has passed local verification
- **THEN** the system publishes the staging directory using the existing atomic final rename

#### Scenario: All staged work verifies
- **WHEN** every required source entry is staged and each transformed work unit passes local verification
- **THEN** the system publishes without a separate complete verification traversal

### Requirement: Define deterministic independently schedulable work
Analysis work and fused conversion work SHALL use stable identities and immutable shared inputs. A fused region work unit SHALL include source assessment, transformation, temporary output, and local verification. Combining valid work-unit results in any completion order MUST produce semantically equivalent successful reports, staged bytes, diagnostics, and publication decisions as canonical sequential execution.

#### Scenario: Analysis batches complete out of order
- **WHEN** an analysis-only command receives equivalent batch results in different completion orders
- **THEN** every order produces the same normalized report and analysis outcome

#### Scenario: Independent region conversions complete out of order
- **WHEN** distinct fused region work units finish in different completion orders
- **THEN** their committed staged bytes and normalized logical report data are identical and no work unit writes or verifies another unit's destination

#### Scenario: Independent conversions complete out of order
- **WHEN** distinct region or standalone-file conversions finish in different completion orders
- **THEN** their staged bytes, logical report data, and publication eligibility remain semantically equivalent

#### Scenario: Verification batches complete out of order
- **WHEN** local verification completes in different orders as part of independent conversion work units
- **THEN** completion order does not change the verified bytes, selected failure, or publication decision

### Requirement: Bound future parallel scheduling state
Work production, in-flight execution, result transfer, and report reduction SHALL expose explicit finite bounds and backpressure. A slow work unit MUST NOT permit an unbounded queue, and a fatal work-unit error MUST stop new admission and later commits while allowing already-running work to terminate safely.

#### Scenario: Producer outruns consumers
- **WHEN** work discovery produces units faster than configured workers process them
- **THEN** discovery waits once the bounded in-flight capacity is reached instead of accumulating an unbounded queue

#### Scenario: Early work unit is slow
- **WHEN** a later work unit completes before an earlier unit required for ordered commit
- **THEN** completed results remain within the configured bound and apply backpressure when that bound is full

#### Scenario: A conversion work unit fails
- **WHEN** any fused conversion work unit reports a fatal error
- **THEN** the coordinator selects a deterministic primary failure, propagates it upward, stops admitting new work, and prevents commits beyond the failure boundary

#### Scenario: A parallel-ready work unit fails
- **WHEN** any admitted analysis or fused conversion work unit reports a fatal error
- **THEN** the coordinator stops new admission, preserves the deterministic failure boundary, and allows already-running work to terminate safely without later commits

### Requirement: Verify resource bounds and equivalence
The project SHALL include automated deterministic tests that fail when completed covered observations, source-tree entries, converted containers, verified paths, serialized report copies, queued work, or reorder results accumulate beyond their defined bounds. The tests SHALL also verify sequential semantic equivalence under permuted valid work schedules.

#### Scenario: Scale completed batches with constant output
- **WHEN** a regression increases the number of identically shaped covered, converted, or verified batches while holding active-batch shape and final report size constant
- **THEN** retained completed-batch state remains within a constant configured bound independent of the number of completed batches

#### Scenario: Permute work schedules
- **WHEN** regression tests process the same work-unit set under multiple valid completion orders
- **THEN** reports, staged file hashes, diagnostics, gate outcomes, and publication eligibility remain identical
