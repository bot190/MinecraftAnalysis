## MODIFIED Requirements

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

### Requirement: Progress distinguishes region completion from subsequent work
For each performed operation, the CLI SHALL explicitly identify when all applicable region files have completed and SHALL identify each subsequent active activity until the command finishes. The displayed lifecycle MUST distinguish active, completed, and failed work without presenting content preflight, local verification, or migration-report generation phases for `convert`.

#### Scenario: Region analysis completes before analysis finalization
- **WHEN** every applicable source region completes successfully for an analysis-only command and analysis finalization remains
- **THEN** the CLI reports region-file analysis complete and identifies analysis finalization as active

#### Scenario: Convert begins directly
- **WHEN** a user runs `convert`
- **THEN** the CLI begins conversion without displaying a preceding content-analysis, verification, or report-generation phase

#### Scenario: Convert continues after region work
- **WHEN** all conversion region work units complete successfully and publication remains
- **THEN** the CLI reports region conversion complete and identifies publication as active

#### Scenario: Analysis-only command finishes
- **WHEN** `dry-run`, `explain`, or `rules coverage` completes region-file analysis and its command-specific analysis finalization
- **THEN** the CLI reports analysis completion without displaying conversion, verification, publication, or other unperformed phases

#### Scenario: Publication fails after every region succeeds
- **WHEN** every conversion region succeeds but publication fails
- **THEN** the CLI preserves the completed conversion indication and reports publication as failed

#### Scenario: Finalization fails after every analysis region succeeds
- **WHEN** all region work for an analysis-only phase succeeds but its subsequent finalization activity fails
- **THEN** the CLI preserves the region completion indication and reports finalization as failed

#### Scenario: World has no applicable region files
- **WHEN** a performed operation has zero applicable region files
- **THEN** the CLI reports its applicable region phase complete at zero-of-zero before identifying subsequent performed work
