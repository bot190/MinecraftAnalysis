# Parallel Region Processing Specification

## Purpose

Provide configurable, resource-bounded parallel processing of independent region files while preserving deterministic migration behavior and sequential chunk handling within each region.

## Requirements

### Requirement: Region worker count is configurable
The CLI SHALL accept a global `--jobs <N>` option for commands that process world regions. The value MUST be a positive integer. When the option is omitted, the system SHALL use the parallelism available to the process, falling back to one worker when available parallelism cannot be determined.

#### Scenario: Use detected parallelism by default
- **WHEN** a user runs a region-processing command without `--jobs`
- **THEN** the system uses the parallelism available to the process as its region worker count

#### Scenario: Override the worker count
- **WHEN** a user supplies `--jobs 4`
- **THEN** the system processes region work with at most four active region workers

#### Scenario: Request sequential execution
- **WHEN** a user supplies `--jobs 1`
- **THEN** the system processes region files sequentially

#### Scenario: Reject an invalid worker count
- **WHEN** a user supplies `--jobs 0` or a value that is not a positive integer
- **THEN** the CLI rejects the invocation before world processing begins

### Requirement: Parallelism uses region files as its work boundary
The system SHALL apply the configured worker count to region-file work performed during analysis and conversion. During conversion, one worker thread MUST perform all semantic processing for its assigned region, including sequential chunk decoding, rule evaluation, transformation, encoding, temporary-file writing, reopening, and emitted-content verification. The system MUST NOT schedule individual chunks or verification of an emitted region as independent parallel work.

#### Scenario: Analyze multiple regions
- **WHEN** an analysis-only command encounters multiple supported region files and the worker count is greater than one
- **THEN** the system may analyze up to the configured number of region files concurrently while scanning each region's chunks sequentially

#### Scenario: Convert multiple regions
- **WHEN** conversion encounters multiple supported region files and the worker count is greater than one
- **THEN** the system may process up to the configured number of complete region work units concurrently while keeping each region's conversion and verification on its assigned worker thread

#### Scenario: Verify a converted region
- **WHEN** a worker finishes encoding a temporary staged region
- **THEN** that same worker reopens and verifies the region before returning a successful result for coordinator commit

#### Scenario: Verify multiple regions
- **WHEN** conversion encounters multiple supported region files and the worker count is greater than one
- **THEN** local verification may occur concurrently across region workers but each region is verified by the worker that converted it

#### Scenario: Process non-region work
- **WHEN** conversion encounters standalone NBT, ordinary files, directory creation, or final publication work
- **THEN** that work remains outside the region worker pool and follows bounded sequential and transactional behavior

### Requirement: Parallel scheduling remains count bounded
The system MUST bound active region work, queued work, report transfer state, and completed results retained for ordered commit or failure handling using fixed work-count limits derived from the configured worker count. Scheduling decisions MUST NOT dynamically change based on file size, chunk size, free memory, or runtime pressure.

#### Scenario: Discovery outpaces workers
- **WHEN** region discovery produces work faster than the configured workers complete it
- **THEN** admission applies backpressure without retaining an unbounded number of queued or completed regions

#### Scenario: Worker count exceeds region count
- **WHEN** the configured worker count is greater than the number of regions in a phase
- **THEN** the system processes every region exactly once without creating additional work units

### Requirement: Parallel completion preserves semantic equivalence and deterministic safety
Combining valid region results in any completion order MUST produce semantically equivalent report records, counts, classifications, dispositions, diagnostics, fingerprints, rule traces, staged region bytes, and publication decisions. Region-derived report contributions MAY be incorporated in successful worker-completion order without waiting for canonical source order, and report array order and serialized report bytes MAY differ between valid schedules. Completion timing MUST NOT choose among conflicting values or change whether logical report data is included. Failure selection, commit safety, per-region bytes, local verification, and final publication MUST retain deterministic and transactional behavior.

#### Scenario: Regions finish out of discovery order
- **WHEN** concurrent region workers complete in a different order from canonical discovery
- **THEN** their report contributions may be incorporated immediately while all commit, local-verification, and publication decisions remain equivalent to canonical sequential execution

#### Scenario: Regions contribute the same logical report key
- **WHEN** multiple completed regions contribute records for the same logical report key
- **THEN** the system applies an order-independent merge, deduplication, or deterministic conflict rule rather than allowing first or last completion to determine report meaning

#### Scenario: Early region is slow
- **WHEN** a slow early region remains active while later regions complete successfully
- **THEN** later report contributions are transferred and their region-local report state is released without filling an unbounded reorder buffer

#### Scenario: A region fails
- **WHEN** one or more admitted region work units return fatal errors
- **THEN** the coordinator selects the primary error by canonical work identity, stops admitting new work, allows already-running workers to terminate safely, commits no region after the failure boundary, and propagates the selected error upward

### Requirement: Progress reflects concurrent region completion
The system SHALL count each successfully completed region exactly once in its phase progress regardless of worker completion order. A successful region MAY advance progress when its result is handed off without waiting for canonical report or commit order. A failed or cancelled region MUST NOT advance the completed count.

#### Scenario: Concurrent regions complete
- **WHEN** multiple region workers emit completion events concurrently
- **THEN** the phase progress advances once per distinct successfully completed region and remains coherent

#### Scenario: Region completes before an earlier source region
- **WHEN** a region successfully hands off its result before an earlier canonical region completes
- **THEN** progress may advance immediately for that region without waiting for canonical report reduction

#### Scenario: Concurrent region fails
- **WHEN** a region fails before completion
- **THEN** that region does not advance phase progress and the phase is reported as failed

### Requirement: Worker-count memory scaling is explicit
User-facing resource documentation SHALL explain that peak region-processing memory scales with the configured number of active workers because each worker may retain complete region input/output buffers and one active chunk's decoded state.

#### Scenario: User chooses a worker override
- **WHEN** a user consults concurrency or resource-bound documentation before selecting `--jobs`
- **THEN** the documentation identifies lower worker counts as the control for reducing peak region-processing memory
