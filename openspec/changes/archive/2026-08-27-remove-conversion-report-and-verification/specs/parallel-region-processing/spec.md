## MODIFIED Requirements

### Requirement: Parallelism uses region files as its work boundary
The system SHALL apply the configured worker count to region-file work performed during analysis and conversion. During conversion, one worker thread MUST perform all semantic processing for its assigned region, including sequential chunk decoding, rule evaluation, transformation, encoding, and temporary-file writing. The system MUST NOT schedule individual chunks as independent parallel work and SHALL NOT reopen or semantically verify the emitted region before returning successful transformation work.

#### Scenario: Analyze multiple regions
- **WHEN** an analysis-only command encounters multiple supported region files and the worker count is greater than one
- **THEN** the system may analyze up to the configured number of region files concurrently while scanning each region's chunks sequentially

#### Scenario: Convert multiple regions
- **WHEN** conversion encounters multiple supported region files and the worker count is greater than one
- **THEN** the system may process up to the configured number of complete region transformation work units concurrently while keeping each region's chunks sequential on its assigned worker thread

#### Scenario: Finish a converted region
- **WHEN** a worker successfully encodes, writes, and flushes a temporary staged region
- **THEN** it returns the completed transformation result for coordinator commit without reopening the region

#### Scenario: Process non-region work
- **WHEN** conversion encounters standalone NBT, ordinary files, directory creation, or final publication work
- **THEN** that work remains outside the region worker pool and follows bounded sequential and transactional behavior

### Requirement: Parallel scheduling remains count bounded
The system MUST bound active region work, queued work, and completed results retained for ordered commit or failure handling using fixed work-count limits derived from the configured worker count. Direct conversion SHALL NOT create region report-transfer state. Scheduling decisions MUST NOT dynamically change based on file size, chunk size, free memory, or runtime pressure.

#### Scenario: Discovery outpaces workers
- **WHEN** region discovery produces work faster than the configured workers complete it
- **THEN** admission applies backpressure without retaining an unbounded number of queued or completed regions

#### Scenario: Worker count exceeds region count
- **WHEN** the configured worker count is greater than the number of regions in a phase
- **THEN** the system processes every region exactly once without creating additional work units

### Requirement: Parallel completion preserves semantic equivalence and deterministic safety
Combining valid region results in any completion order MUST produce semantically equivalent staged region bytes and publication decisions. Completion timing MUST NOT choose among conflicting values or change transformation behavior. Failure selection, commit safety, per-region bytes, and final publication MUST retain deterministic and transactional behavior without report contribution transfer or local verification.

#### Scenario: Regions finish out of discovery order
- **WHEN** concurrent region workers complete in a different order from canonical discovery
- **THEN** their staged bytes, commit eligibility, and publication decisions remain equivalent to canonical sequential execution

#### Scenario: Early region is slow
- **WHEN** a slow early region remains active while later regions complete successfully
- **THEN** later completed transformation results remain bounded by the configured work-count limit without retaining report contributions

#### Scenario: A region fails
- **WHEN** one or more admitted region work units return fatal errors
- **THEN** the coordinator selects the primary error by canonical work identity, stops admitting new work, allows already-running workers to terminate safely, commits no region after the failure boundary, and propagates the selected error upward
