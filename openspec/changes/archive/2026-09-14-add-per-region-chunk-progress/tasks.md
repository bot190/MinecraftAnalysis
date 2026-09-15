## 1. Core chunk observations

- [x] 1.1 Add header-only populated-slot counting to RegionReader; verify sparse, full, empty, malformed-location, and truncated-header fixtures without changing read error behavior.
- [x] 1.2 Extend renderer-independent progress observations with totals, cumulative counts, remaining-work states, and terminal detail; verify existing no-progress entry points compile and lifecycle tests preserve final counts.
- [x] 1.3 Instrument analysis after successful chunk consumption and through result handoff; verify event sequences for multiple chunks, consumption failure, empty regions, and reduction failure.
- [x] 1.4 Instrument conversion after successful chunk encoding into the region and before final writing; verify chunk failure and temporary-file write failure never report region success, while successful completion retains the existing boundary.

## 2. Bounded delivery and lifecycle

- [x] 2.1 Add a cumulative snapshot mailbox with bounded reliable lifecycle delivery and periodic draining; verify workers can publish many chunk updates without a draining renderer or per-chunk channel backpressure, and pending state scales with admitted regions.
- [x] 2.2 Handle registration, authoritative terminal snapshots, mailbox retirement, disconnection, and shutdown; verify racing and stale updates cannot lose final counts, revive completed rows, or leave active detail after phase termination.

## 3. Terminal presentation

- [x] 3.1 Render determinate graphical per-region chunk bars with dimension-distinguishing relative paths, completed/total counts, and lifecycle labels alongside the aggregate bar. Use spinners only for preparation before totals are known; retain full bars while writing/finishing, last successful fill on failure, and explicit 0/0 for empty regions. Verify proportional fill at partial counts and independent monotonic concurrent progress without changing aggregate totals.
- [x] 3.2 Implement stable row reuse, width limits, terminal-height budgeting, overflow counts, and resize/fallback handling for graphical region bars. Verify resizing preserves counts and lifecycle labels, narrow terminals use aggregate-only fallback when a graphical detail row cannot fit, hidden regions are promoted, and retained rows remain bounded across many completed regions.
- [x] 3.3 Preserve logging coordination, stderr gating, disabled progress, and activity rendering; verify CLI output remains parseable with redirected stderr and --no-progress, and terminal log output does not interleave with detail rows.

## 4. Integration validation and documentation

- [x] 4.1 Exercise analysis and conversion with --jobs 1 and multiple workers across dimensions, including sparse and empty regions and failures; verify equivalent command results and converted content with progress enabled and disabled. Inspect and record an interactive terminal run demonstrating multiple simultaneous graphical region bars with partial fill and resizing.
- [x] 4.2 Document the aggregate and graphical per-region bars, populated-chunk meaning, preparation spinner, remaining-work statuses, overflow behavior, and new event variants; verify documentation matches the implemented command behavior and compatibility wrappers.
- [x] 4.3 Run required Rust formatting, lint, and workspace test checks inside nix develop, plus strict OpenSpec validation after the graphical-bar changes; record results and resolve failures before marking this change complete.

## Validation

Graphical-bar revision validated on 2026-09-14. All checks below passed; the workspace suite ran 212 tests successfully, with the interactive demonstration excluded from the default suite and run separately in a PTY.

- `nix develop -c cargo fmt --all -- --check`
- `nix develop -c cargo check --workspace --all-targets --locked`
- `nix develop -c cargo clippy --workspace --all-targets --locked -- -D warnings`
- `nix develop -c cargo test --workspace --all-targets --locked`
- `openspec validate add-per-region-chunk-progress --strict`

The CLI regression exercises analysis and conversion with 1 and 3 workers,
interactive progress enabled and disabled, two sparse regions in distinct
dimensions, an empty region, and conversion failure without publication.
It compares analysis JSON and converted region bytes across successful runs.

Interactive demonstration: `nix develop -c cargo test -p minecraft-analysis --bin minecraft-analysis interactive_region_bar_demo -- --ignored --nocapture`,
run in a PTY initially sized to 100 columns and 12 rows. Three synthetic
concurrent region lifecycles displayed independent partial graphical fills
(for example 6/20, 3/20, and 2/20) beside aggregate 0/3. At 45 columns,
detail disappeared and the aggregate showed “3 active regions omitted”.
Returning to 100 columns restored all three bars at their latest counts
(14/20, 7/20, and 4/20). Phase failure cleared the active detail rows.
This renderer demonstration uses synthetic observations; the separate CLI
regression exercises actual analysis and conversion.
