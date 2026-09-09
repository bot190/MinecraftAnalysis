## MODIFIED Requirements

### Requirement: Long-running commands report phase-specific progress
The CLI SHALL report one region-file conversion phase for `convert` and one analysis phase for `rules coverage`. Targeted `explain` and NBT inspection commands SHALL NOT display whole-world region progress. Publication SHALL remain separately identified when performed. Focused JSON serialization SHALL NOT be exposed as a separate report-generation activity.

#### Scenario: Convert a world interactively
- **WHEN** a user runs `convert` with an interactive standard error stream
- **THEN** the CLI displays one conversion progress bar whose successful region completion includes local emitted-content verification

#### Scenario: Run an analysis-only command interactively
- **WHEN** a user runs `rules coverage` with an interactive standard error stream
- **THEN** the CLI displays an analysis progress bar and does not display conversion or report-generation progress

#### Scenario: Explain one coordinate interactively
- **WHEN** a user runs `explain` with an interactive standard error stream
- **THEN** the CLI does not count the world's region files or display whole-world region or report-generation progress

#### Scenario: Run a bounded single-file command
- **WHEN** a user runs an NBT inspection command
- **THEN** the CLI does not display region progress

### Requirement: Progress distinguishes region completion from subsequent work
For each operation that processes a set of region files, the CLI SHALL explicitly identify when all applicable region files have completed and SHALL identify subsequent publication when performed. A targeted coordinate explanation SHALL report no whole-world lifecycle. The displayed lifecycle MUST distinguish active, completed, and failed work without presenting content preflight, separate verification, or report-generation phases.

#### Scenario: Region analysis completes before analysis finalization
- **WHEN** every applicable source region and coverage finalization step completes successfully
- **THEN** the CLI reports analysis complete without displaying conversion, verification, publication, or report-generation phases

#### Scenario: Convert continues after analysis
- **WHEN** a user runs `convert`
- **THEN** the CLI begins fused conversion without displaying a preceding content-analysis phase

#### Scenario: Convert continues after region work
- **WHEN** all conversion region work units complete successfully and publication remains
- **THEN** the CLI reports region conversion complete and identifies publication as active

#### Scenario: Analysis-only command finishes
- **WHEN** `rules coverage` completes region-file analysis and its command-specific finalization
- **THEN** the CLI reports analysis completion without displaying conversion, verification, publication, or report-generation phases

#### Scenario: Targeted explanation finishes
- **WHEN** `explain` completes its selected-chunk lookup and writes its result
- **THEN** the CLI does not claim completion of world-region analysis or expose result serialization as a progress activity

#### Scenario: Later activity fails
- **WHEN** every conversion region succeeds but publication fails
- **THEN** the CLI preserves the completed conversion indication and reports publication as failed

#### Scenario: Finalization fails after every region succeeds
- **WHEN** all coverage region work succeeds but coverage finalization fails
- **THEN** the CLI preserves the region completion indication and reports analysis as failed

#### Scenario: World has no applicable region files
- **WHEN** a performed whole-world operation has zero applicable region files
- **THEN** the CLI reports its applicable region phase complete at zero-of-zero before identifying any subsequent performed work
