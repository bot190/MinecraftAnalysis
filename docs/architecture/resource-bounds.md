# Resource bounds

The migration pipeline schedules by fixed work-unit counts only. Region worker
count defaults to process-available CPU parallelism and can be overridden with
the global `--jobs` option. File sizes,
estimated memory, free memory, and runtime pressure never change admission,
worker, queue, or reorder decisions.

| Retained component | Sequential bound | Future fixed-worker bound |
| --- | --- | --- |
| Source region bytes | one active region | one per active region worker |
| Converted region bytes | one growing memory-backed `RegionWriter` | one per active region worker |
| Decompressed chunk | one, capped at 64 MiB | one per active region worker |
| Typed chunk NBT and converted block storage | one active chunk | one per active region worker |
| Standalone typed NBT | one active file | one active sequential file |
| Analysis observations | one chunk or standalone-file batch | one active batch per region worker |
| Nested-item discovery | one region or standalone-document budget | one independent budget per active source container |
| In-memory migration records | 10,000 records or 8 MiB encoded | one threshold per region worker plus the coordinator store |
| In-memory coverage groups | 4,096 observations per sorted run | one threshold per region worker plus the coordinator reducer |
| Spool data | complete required report records on disk | complete required report records on disk |
| Active queue | configured admitted work count | configured admitted work count |
| Completion-ordered report transfer | one active contribution | one transfer per completed worker result, promptly merged or spooled |
| Commit/failure reorder state | configured completed-unreduced count | configured completed-unreduced count |
| Staged temporary files | one per admitted uncommitted work key | admitted work-count bound |

Region-file work is parallel for analysis-only commands and fused conversion.
During conversion the assigned worker performs sequential chunk traversal,
transformation, temporary output, reopening, and verification for its region.
There is no second verification worker pool. Both admitted work and
completed results awaiting commit or failure reduction are bounded by fixed
counts derived from `--jobs`. Report-only contributions transfer as workers
complete and do not retain region-local decoded state behind an earlier slow
key. Standalone NBT, ordinary file operations, and final publication remain
outside the region worker pool.

Analysis uses two reduction levels. Each worker reduces and releases one chunk
before decoding the next, returning only region-local counts, diagnostics, and
managed report or coverage spool state. The coordinator merges report data in
successful completion order while retaining canonical ordering only for
commit- and failure-sensitive state. Nested-object limits are scoped independently to
each region or standalone NBT document, so workers do not coordinate a
whole-world nested-item counter.

The source and output region buffers intentionally remain memory-backed. With
the representative design maximum of about 8.9 MiB per region, eight workers
would retain roughly 142 MiB for paired source/output region buffers before
chunk, typed-NBT, queue, report, and allocator overhead. This projection is a
configuration trade-off, not an input to scheduling.
Use a lower `--jobs` value, including `--jobs 1`, to reduce this peak residency.

Report workspaces are owned temporary directories. Successful serialization
drops and removes them. A failing command can persist the length-delimited
segments to an explicit diagnostic directory. Report output itself is not
bounded because the schema requires complete records. Parallel report arrays
retain contribution order, so their order and raw JSON bytes may vary across
schedules even though parsed records, counts, diagnostics, and decisions remain
semantically equivalent.
