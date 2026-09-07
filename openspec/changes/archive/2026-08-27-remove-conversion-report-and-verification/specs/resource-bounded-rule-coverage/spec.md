## MODIFIED Requirements

### Requirement: Bound retained analysis state
The system SHALL assess source objects incrementally for rule coverage, dry-run, and explanation. Direct conversion SHALL retain only state required by active transformation work and SHALL NOT retain per-object migration-report contributions. Excluding fixed registry and rule inputs plus information required by an analysis report or query result, retained state MUST be bounded by active work batches and MUST NOT grow with the total number of completed batches.

#### Scenario: Covered objects span many regions
- **WHEN** analysis or conversion scans successively more regions containing covered or converted objects
- **THEN** completing a region releases its decoded source data and per-object observations before unbounded additional regions are retained

#### Scenario: Uncovered or unresolved results accumulate
- **WHEN** an analysis-only command encounters findings that its complete report must contain
- **THEN** it retains or spools only the records, counts, and diagnostics required to emit that report

#### Scenario: Conversion encounters a fatal object
- **WHEN** direct conversion encounters an unresolved object or another fatal condition
- **THEN** it returns an actionable error without retaining or scanning unprocessed world content for a complete or partial failure report

#### Scenario: Explain filters analysis incrementally
- **WHEN** the user requests an explanation for an exact source location
- **THEN** the system returns every matching record without retaining unrelated whole-world object records in memory

### Requirement: Preserve analysis semantics
Except for the explicitly container-scoped nested-object budget, incremental analysis SHALL produce the same deterministic results as whole-world analysis for the same inputs, including classifications, counts, dispositions, signature grouping, ordered deduplicated locations, associated block-entity data, nested-item findings, warnings, fingerprints, selected rules, target identities, diagnostics, and rejection traces. Direct conversion is excluded from analysis-report equivalence because it produces no migration report.

#### Scenario: Compare incremental and reference analysis
- **WHEN** the same fixture world, template, and rules are processed by incremental analysis and a reference whole-world analysis
- **THEN** both produce equivalent coverage and dry-run reports

#### Scenario: Associate a block entity within a bounded batch
- **WHEN** an observed block has a block entity at the same dimension and coordinates
- **THEN** analysis includes the required coordinated identity, rule, trace, and SNBT information before releasing the containing source batch

#### Scenario: Discover rule-declared nested items
- **WHEN** an applicable rule declares nested item paths in an observed object
- **THEN** every discovered nested item participates in analysis before its containing source data is released

### Requirement: Stage source entries incrementally
The system SHALL traverse source entries incrementally and create each staged entry in its final staged representation where practical. It MUST NOT require a complete in-memory source-tree entry list, a complete content preflight, an unconditional preliminary copy of every transformable file, or conversion-report state.

#### Scenario: Copy an unchanged file
- **WHEN** a regular source file requires no transformation
- **THEN** the system copies it to staging with bounded I/O buffers without retaining a file-disposition record

#### Scenario: Transform a supported world file
- **WHEN** a source region or standalone NBT file requires conversion
- **THEN** the system writes its converted representation through a temporary staged sibling and atomically commits that staged file after successful encoding, writing, and flushing without reopening it

#### Scenario: Exclude a transient source file
- **WHEN** traversal encounters a transient world lock or another explicitly excluded entry
- **THEN** the system omits the entry without copying it into staging or retaining an exclusion record

### Requirement: Stream deterministic reports with bounded memory
The system SHALL emit complete reports requested by analysis-only commands through buffered output without constructing a second complete report-sized JSON string. When required analysis records exceed the configured in-memory report bound, the system SHALL use managed bounded temporary storage while preserving the command's report schema, completeness, and deterministic ordering. Direct conversion SHALL NOT generate, spool, or serialize a migration report.

#### Scenario: Write an analysis report to a file
- **WHEN** an analysis-only command accepts and receives a report path
- **THEN** the system emits the complete deterministic pretty-JSON report through buffered file output and reports any creation, serialization, or flush failure

#### Scenario: Write an analysis report to standard output
- **WHEN** an analysis-only command emits its machine-readable result to standard output
- **THEN** standard output contains the complete parseable command result before exit-status selection

#### Scenario: Analysis report records exceed the memory bound
- **WHEN** complete ordered analysis output requires more record state than the configured in-memory report bound permits
- **THEN** the system spills records to managed temporary storage and cleans that storage after successful output

#### Scenario: Convert a world
- **WHEN** the user invokes direct conversion
- **THEN** conversion performs no report finalization and retains no report-sized record state

## REMOVED Requirements

### Requirement: Verify staged output incrementally
**Reason**: Local reopen and semantic verification duplicate conversion-time checks and add a second read, decode, and traversal of every transformed file.

**Migration**: Keep conversion state bounded through temporary staged writes and immediate release after successful encoding, writing, flushing, and coordinator commit; cover emitted-format correctness with automated round-trip and end-to-end tests.
