## Why

The overall region counter can appear stalled while several large regions are processing, and its single changing filename hides concurrent work. Separate chunk counters will show which regions are active and how far each has progressed.

## What Changes

- Keep the overall region progress bar and add temporary graphical chunk progress bars with completed/total counts for active regions during conversion and region-based analysis. Once a total is known, a spinner alone does not satisfy region progress presentation.
- Count populated chunk slots from each region header and advance only after successful chunk processing.
- Distinguish loading, processing chunks, writing or finishing, and terminal outcomes; completing chunks does not imply successful region completion.
- Reuse finished rows, fit visible rows to terminal height, and summarize additional active regions.
- Coalesce cumulative chunk updates so progress traffic stays bounded and does not wait for terminal rendering.
- Preserve interactive-stderr gating, `--no-progress`, clean stdout, logging coordination, and existing region scheduling and completion semantics.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `cli-progress-reporting`: Add per-region chunk progress, bounded display and update handling, and explicit region lifecycle states alongside the aggregate bar.

## Impact

Core progress observations, region header inspection, analysis and conversion chunk loops, and the CLI progress renderer and tests will change. Existing renderer-independent APIs should retain no-progress compatibility entry points. The existing indicatif dependency is sufficient; no new dependency is planned. Region scheduling, output world contents, and command result formats remain unchanged.
