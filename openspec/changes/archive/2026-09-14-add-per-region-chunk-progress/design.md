## Context

See proposal.md for motivation. Core workers already emit region start and completion events; CLI progress.rs serializes them and logs through a 256-message synchronous channel and renders with MultiProgress. Analysis scans chunks in preflight.rs; conversion scans them in region_conversion.rs. Neither loop currently reports chunk completion. RegionReader exposes header-backed reads but no chunk-count helper.

The current conversion path writes and flushes the temporary file before completion; this change must follow that code boundary. The parallel-region-processing spec still describes local verification, but the inspected conversion path does not perform it. Adding verification or reconciling that separate discrepancy is outside this change.

## Goals / Non-Goals

**Goals:** Preserve the renderer-independent core, existing successful region handoff boundaries, and bounded worker scheduling while exposing accurate intermediate work. Keep all terminal operations on the renderer thread.

**Non-Goals:** Chunk parallelism, block-level progress, time estimates, new command flags, changes to region processing semantics, or progress for targeted single-chunk analysis and NBT inspection.

## Decisions

### Count populated slots using the existing region header

Add a bounded RegionReader helper that inspects the 1,024 location entries. Treat an all-zero location as absent, consistent with read_chunk; malformed nonzero entries remain processing errors rather than silently disappearing. Do not decompress payloads or introduce an earlier validation pass that changes primary error selection. A truncated header fails before a total is announced. A valid empty header yields zero chunks.

Using 1,024 as every denominator would misrepresent sparse regions; decompressing once to count and again to process would add unnecessary work.

### Extend observations at actual work boundaries

Use phase plus world-relative path as region identity. Extend observations with a known total, cumulative successful chunk count, remaining-work status, and explicit failed-region outcome. Carry final count in terminal region detail so pending updates cannot hide the last successful chunk. Preserve existing no-progress wrappers and pass observers through internal observed entry points.

Emit analysis advancement after consume(key, batch) succeeds. Emit conversion advancement after writer.write_chunk succeeds. Conversion transitions to writing before finishing the container and writing/flushing its temporary file; analysis transitions to finishing before returning its reducer. Existing coordinator completion advances aggregate region progress. Catch worker-operation errors at the region wrapper to report failed detail before propagation; coordinator reduction failures and phase failure also terminate unfinished detail without fabricating success.

### Coalesce chunk updates independently from reliable lifecycle delivery

Keep bounded reliable lifecycle/log delivery. Store the latest cumulative chunk snapshot per admitted unfinished region in a shared mailbox rather than enqueuing every chunk. Updating this mailbox can use a short lock, but must not wait for terminal drawing or channel capacity. The renderer periodically drains snapshots using a timed receive, approximately every 100 ms, including under sustained lifecycle/log traffic. Snapshot copying holds the lock briefly; all drawing occurs after releasing it.

Register state before chunk updates can arrive; lifecycle transitions carry authoritative final snapshots. Ignore snapshots without a live registered region, preventing late updates from recreating retired rows without retaining an unbounded tombstone set. Remove mailbox entries at terminal handoff and phase shutdown. State scales with scheduler admission bounds, including workers waiting for handoff, rather than world size. Disabled reporters retain no mailbox. Disconnection remains nonfatal.

Sending every chunk through the existing blocking queue risks worker stalls; dropping increments with try_send loses accuracy. Cumulative snapshots permit intermediate coalescing without either problem.

### Reuse stable detail rows within a terminal budget

Each visible region with a known total uses a determinate indicatif progress bar, with its length set to the populated chunk total and its position set to the latest cumulative successful chunk count. Render a graphical fill alongside the world-relative path, completed/total counts, and lifecycle label. A spinner may indicate preparation before the total is known; it must not replace the graphical bar after preparation.

Retain the full bar and counts while writing or finishing, without marking the region successful. On failure, preserve the last successful fill and count when the total is known, and preserve the unknown-total state otherwise. Empty regions explicitly display 0/0 without dividing by zero or treating the chunk bar as region success.

Allocate the graphical bar's width after reserving room for counts and lifecycle text, and truncate paths within the remaining width. Recompute widths when the terminal resizes. If a graphical detail row cannot fit, use the aggregate-only fallback with the omitted active count. Validate the rendered fill at partial counts and demonstrate multiple simultaneous bars in an interactive terminal, including resizing.

Keep the aggregate phase bar and existing activity indicators. Assign active regions to detail rows in start order and keep visible assignments stable until completion or resizing. Show relative paths with width-aware truncation. Remove successful detail rows rather than leaving a line per processed region; promote the oldest hidden active region into freed space. Track hidden regions identically to visible ones.

Recompute terminal capacity during periodic refresh. Reserve rows for aggregate/activity output and an overflow indicator before allocating detail rows. When even these do not fit, use a compact aggregate message containing the omitted count and no detail rows. Unknown dimensions use aggregate-only rendering with the active count. This fallback avoids unbounded output without adding a configuration option.

Retain failed counts only within the bounded visible failure summary; normal error diagnostics carry durable failure context. At phase failure, mark other unfinished rows interrupted or clear them. Preserve existing log suspension and stderr-only behavior.

## Risks / Trade-offs

- Chunk counts are not elapsed-time estimates: differently sized chunks take different time. Mitigation: display counts without promising an ETA.
- Mailbox and lifecycle ordering can revive stale rows or lose final counts. Mitigation: authoritative terminal snapshots, live-registration checks, and adversarial ordering tests.
- Workers may finish chunk work before coordinator handoff. Mitigation: label finishing explicitly and bound detail by admitted work rather than assuming exactly --jobs entries.
- Small terminals hide some detail. Mitigation: retain complete tracking, show the omitted count, and promote waiting rows as capacity frees.
- Extending the public event enum affects exhaustive downstream matches. Mitigation: document new variants and preserve callable no-progress entry points; do not change command results.

## Migration Plan

No data migration is required. Implement and validate core observations, transport, and rendering together. Existing --no-progress provides the normal output opt-out. Reverting the implementation returns to aggregate-only progress without changing world data or CLI result formats.
