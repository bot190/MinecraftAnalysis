## MODIFIED Requirements

### Requirement: Long-running commands report phase-specific progress
The CLI SHALL report one region-file conversion phase for `convert`, and SHALL report an analysis phase for `dry-run`, `explain`, and `rules coverage`. Publication and analysis-report generation SHALL remain separately identified activities when performed. Direct conversion SHALL NOT report verification or migration-report generation activity.

#### Scenario: Convert a world interactively
- **WHEN** a user runs `convert` with an interactive standard error stream
- **THEN** the CLI displays one aggregate conversion progress bar, accompanied by graphical active-region chunk progress bars with completed/total counts, whose successful region completion means transformation and temporary-file writing completed

#### Scenario: Run an analysis-only command interactively
- **WHEN** a user runs `dry-run`, `explain`, or `rules coverage` with an interactive standard error stream
- **THEN** the CLI displays an analysis progress bar and does not display conversion progress

#### Scenario: Run a bounded single-file command
- **WHEN** a user runs an NBT inspection command
- **THEN** the CLI does not display region progress

## ADDED Requirements

### Requirement: Active regions expose successful chunk progress
During region-based analysis and conversion, the CLI SHALL track each active region independently by phase and world-relative path and display its completed and total populated chunk counts when a detail row is visible. Totals MUST come from the region header without decompressing chunks solely to count them. Empty slots MUST NOT count as chunks. Progress MUST advance only after the chunk's processing succeeds, including analysis consumption or conversion encoding into the output region.

#### Scenario: Concurrent regions across dimensions
- **WHEN** regions with the same filename in different dimensions run concurrently
- **THEN** their chunk counts and path labels remain independent and aggregate region progress remains visible

#### Scenario: Sparse region
- **WHEN** a valid region contains 480 populated chunk slots
- **THEN** its chunk total is 480 and empty slots do not advance the counter

#### Scenario: Visible region has a known chunk total
- **WHEN** a visible region has a known positive populated chunk total
- **THEN** its detail row SHALL show a graphical progress bar whose fill is proportional to successful chunks divided by the total, together with completed/total counts; a spinner alone SHALL NOT satisfy this requirement

#### Scenario: Multiple regions have partial progress
- **WHEN** two or more regions with known totals are processing concurrently and terminal space permits their detail rows
- **THEN** each visible region SHALL have an independent graphical chunk progress bar alongside the aggregate region progress bar

#### Scenario: Chunk processing fails
- **WHEN** a chunk fails decoding, analysis consumption, transformation, or encoding
- **THEN** that chunk does not advance successful chunk progress and the region is not counted as successfully completed

#### Scenario: Empty region
- **WHEN** a valid region has no populated chunk slots
- **THEN** its detail shows a graphical bar and an explicit 0/0 count without computing a fraction with a zero denominator, and it still waits for successful region completion before advancing the aggregate counter

### Requirement: Region detail distinguishes processing from completion
Region detail SHALL distinguish preparation before a total is known, chunk processing, remaining region work, and failure. Reaching the chunk total MUST NOT imply region success. Successful rows SHALL be retired for reuse. Failed rows SHALL preserve their last successful count while displayed. Phase termination or renderer shutdown MUST leave no region row appearing active, and late updates MUST NOT reactivate terminal regions.

#### Scenario: Conversion writes after the last chunk
- **WHEN** all chunks have been encoded but the temporary region file is still being written
- **THEN** the row retains its fully filled chunk bar and full chunk count with a writing status and aggregate completion waits for the existing successful region boundary

#### Scenario: Analysis result handoff remains
- **WHEN** analysis has consumed every chunk but successful region result handoff remains
- **THEN** the row retains its fully filled chunk bar and full chunk count with a finishing status until the existing region completion boundary succeeds

#### Scenario: Region is preparing or fails
- **WHEN** a region is preparing before its total is known, or subsequently fails
- **THEN** preparation MAY use a spinner; a failure with a known total SHALL preserve the bar's last successful fill and count while displayed, and a failure with an unknown total SHALL NOT invent a denominator

#### Scenario: Header cannot be read
- **WHEN** preparation fails before a trustworthy chunk total is available
- **THEN** the region is reported as failed without inventing a chunk total

#### Scenario: Phase fails with other regions unfinished
- **WHEN** the phase terminates unsuccessfully
- **THEN** failed regions remain unsuccessful, other unfinished region rows are marked interrupted or cleared, and none advance aggregate completion

### Requirement: Region detail remains bounded by terminal space
The CLI SHALL reuse detail rows and retain detail state only for admitted unfinished regions and bounded terminal summaries. It SHALL fit visible detail to terminal height, preserve aggregate progress, and indicate the number of active regions omitted from detail. It SHALL adjust detail capacity when terminal dimensions change and constrain labels to terminal width.

#### Scenario: More active regions than available rows
- **WHEN** active regions exceed available terminal detail space
- **THEN** the CLI displays a fitting subset and an omitted-active-region count while continuing to track every active region

#### Scenario: Terminal shrinks or a visible region completes
- **WHEN** terminal height decreases or a displayed region completes
- **THEN** the display recomputes available rows without accumulating finished region lines and promotes hidden active regions when space becomes available

#### Scenario: Terminal width changes
- **WHEN** terminal width changes while region details are visible
- **THEN** graphical bars and path labels SHALL resize within the available width while retaining counts and lifecycle labels; if a graphical detail row cannot fit, the CLI SHALL use the aggregate-only fallback and include the omitted active count

### Requirement: Chunk reporting does not accumulate rendering work
Chunk progress delivery SHALL use bounded pending state proportional to admitted unfinished regions and SHALL NOT wait for terminal drawing or free per-chunk queue capacity. Intermediate updates MAY be coalesced, but displayed counts MUST be monotonic and terminal outcomes MUST preserve the final successful count. Existing interactive-stderr gating, log coordination, and nonfatal renderer disconnection behavior SHALL apply to region detail.

#### Scenario: Workers outpace rendering
- **WHEN** concurrent workers complete chunks faster than the terminal refreshes
- **THEN** pending progress remains bounded and the next displayed update reflects the latest available cumulative count without replaying every intermediate count

#### Scenario: Completion races with a pending update
- **WHEN** a region terminates while chunk progress is pending
- **THEN** its terminal outcome includes its last successful count and later rendering cannot recreate an active row

#### Scenario: Progress disabled or renderer disconnected
- **WHEN** stderr is non-interactive, --no-progress is supplied, or the renderer disconnects
- **THEN** region detail emits no terminal output in the disabled cases and processing does not fail because progress is unavailable
