## MODIFIED Requirements

### Requirement: Bound retained analysis state
The system SHALL assess source objects incrementally for rule coverage and dry-run, and SHALL transform direct conversion content with state bounded by active work. For explanation, the system SHALL derive and access only the source region and chunk containing the requested world-global coordinate. Excluding fixed registry and rule inputs plus information required by the requested report or query result, retained state MUST be bounded by active work batches and MUST NOT grow with the total number of completed batches.

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
- **WHEN** the user requests an explanation for a world-global block coordinate in a dimension
- **THEN** the system derives the containing region and chunk, reads only that chunk, and evaluates only coordinate-owned objects plus the local context required for their rule decisions

#### Scenario: Explain a negative coordinate
- **WHEN** one or both horizontal coordinates are negative
- **THEN** the command selects the mathematically containing chunk, region, and non-negative region-local chunk coordinates

#### Scenario: Unrelated source content is invalid
- **WHEN** an unrelated source region or standalone NBT document is unreadable but the selected coordinate's region and chunk are valid
- **THEN** explanation succeeds without reading or validating that unrelated content
