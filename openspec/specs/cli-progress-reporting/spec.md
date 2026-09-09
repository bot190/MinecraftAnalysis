# CLI Progress Reporting Specification

## Purpose

Provide accurate interactive progress for long-running world operations while preserving clean machine-readable output and coherent behavior under parallel processing.

## Requirements

### Requirement: Long-running commands report phase-specific progress
The CLI SHALL report one region-file conversion phase for `convert`, and SHALL report an analysis phase for `dry-run`, `explain`, and `rules coverage`. Publication and analysis-report generation SHALL remain separately identified activities when performed. Direct conversion SHALL NOT report verification or migration-report generation activity.

#### Scenario: Convert a world interactively
- **WHEN** a user runs `convert` with an interactive standard error stream
- **THEN** the CLI displays one conversion progress bar whose successful region completion means transformation and temporary-file writing completed

#### Scenario: Run an analysis-only command interactively
- **WHEN** a user runs `dry-run`, `explain`, or `rules coverage` with an interactive standard error stream
- **THEN** the CLI displays an analysis progress bar and does not display conversion progress

#### Scenario: Run a bounded single-file command
- **WHEN** a user runs an NBT inspection command
- **THEN** the CLI does not display region progress

### Requirement: Region progress uses pre-counted totals and completed work
Before a processing phase begins, the system SHALL count the applicable region files without retaining an unbounded world inventory, SHALL use that count as the phase total, and SHALL advance progress exactly once after each region completes successfully.

#### Scenario: Process regions across dimensions
- **WHEN** a world contains region files under `region/` and supported `DIM*/region/` directories
- **THEN** each applicable region file contributes exactly one unit to every phase that processes it

#### Scenario: Regions complete out of order
- **WHEN** multiple region files are processed concurrently and finish in a different order from discovery
- **THEN** the displayed completed count advances once per distinct completed region without depending on completion order

#### Scenario: Region processing fails
- **WHEN** a region fails before completing
- **THEN** that region is not counted as completed and the active phase is displayed as failed

#### Scenario: World has no region files
- **WHEN** a long-running command processes a world with zero applicable region files
- **THEN** each applicable phase reports a completed zero-of-zero state without hanging or inventing work

### Requirement: Progress output is restricted to interactive standard error
The CLI SHALL emit progress output only when standard error is an interactive terminal and progress has not been disabled. Progress output SHALL NOT be written to standard output.

#### Scenario: Standard error is redirected
- **WHEN** standard error is redirected or otherwise non-interactive
- **THEN** the command emits no progress-related output

#### Scenario: Progress is explicitly disabled
- **WHEN** a user invokes a long-running command with the global `--no-progress` option
- **THEN** the command emits no progress-related output even when standard error is interactive

#### Scenario: Command writes machine-readable output
- **WHEN** `explain` or `rules coverage` writes JSON to standard output while progress is enabled
- **THEN** standard output contains only the command result and remains independently parseable

### Requirement: Concurrent processing produces coherent progress
The system SHALL serialize all progress-state updates and terminal rendering through one renderer thread, while allowing any number of processing threads to report lifecycle events without writing terminal output directly.

#### Scenario: Concurrent workers report progress
- **WHEN** multiple processing threads report region lifecycle events concurrently
- **THEN** one renderer processes the events and produces a coherent phase count without interleaved worker output

#### Scenario: Renderer becomes unavailable
- **WHEN** the progress renderer terminates or its event receiver disconnects
- **THEN** processing does not panic or fail solely because progress can no longer be displayed

#### Scenario: Command terminates
- **WHEN** a long-running command succeeds or fails
- **THEN** the renderer is shut down and joined before the command returns control to the caller

### Requirement: Progress distinguishes region completion from subsequent work
For each performed operation, the CLI SHALL explicitly identify when all applicable region files have completed and SHALL identify each subsequent active activity until the command finishes. The displayed lifecycle MUST distinguish active, completed, and failed work without presenting content preflight, local verification, or migration-report generation phases for `convert`.

#### Scenario: Region analysis completes before analysis finalization
- **WHEN** every applicable source region completes successfully for an analysis-only command and analysis finalization remains
- **THEN** the CLI reports region-file analysis complete and identifies analysis finalization as active

#### Scenario: Convert continues after analysis
- **WHEN** a user runs `convert`
- **THEN** the CLI begins conversion without displaying a preceding content-analysis, verification, or report-generation phase

#### Scenario: Convert continues after region work
- **WHEN** all conversion region work units complete successfully and publication remains
- **THEN** the CLI reports region conversion complete and identifies publication as active

#### Scenario: Analysis-only command finishes
- **WHEN** `dry-run`, `explain`, or `rules coverage` completes region-file analysis and its command-specific analysis finalization
- **THEN** the CLI reports analysis completion without displaying conversion, verification, publication, or other unperformed phases

#### Scenario: Later activity fails
- **WHEN** every conversion region succeeds but publication fails
- **THEN** the CLI preserves the completed conversion indication and reports publication as failed

#### Scenario: Finalization fails after every region succeeds
- **WHEN** all region work for an analysis-only phase succeeds but its subsequent finalization activity fails
- **THEN** the CLI preserves the region completion indication and reports finalization as failed, while direct conversion has no report finalization activity

#### Scenario: World has no applicable region files
- **WHEN** a performed operation has zero applicable region files
- **THEN** the CLI reports its applicable region phase complete at zero-of-zero before identifying subsequent performed work
