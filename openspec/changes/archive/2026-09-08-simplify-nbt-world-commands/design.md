## Context

The two NBT commands currently share standalone and selected-region decoding, but expose different world workflows. Dump already derives a chunk from a world coordinate but requires one rule side; view accepts direct region selectors and owns a richer best-effort source registry loader. The viewer's block index already supports exact coordinate lookup and temporary revelation of filtered air.

## Goals / Non-Goals

**Goals:**

- Give dump and view one shared world-coordinate input and preparation path.
- Make registry enrichment source-oriented, consistent, optional, and reusable.
- Initialize the viewer at the exact requested block before terminal entry.
- Preserve atomic non-interactive output and standalone-document behavior.

**Non-Goals:**

- Add flattened palette support, semantic SNBT output, or transformation between source and target identities.
- Remove internal region readers needed to implement world-coordinate access.
- Treat successful identity enrichment as a prerequisite for the normal labeled block dump.

## Decisions

### Model the commands as standalone-file or world-coordinate modes

Each command will accept either a positional standalone file or paired `--world` and `--location` arguments. World mode optionally accepts `--dimension` and repeatable `--rules`; direct chunk selectors and dump rule-side flags are removed. Clap-level groups will reject incomplete and mixed combinations before I/O.

Keeping a positional region path with implicit behavior was rejected because it would preserve ambiguity and make accidental binary-document decoding the selector mechanism.

### Share world input preparation and source identity context

A shared preparation path will canonicalize the world, derive and decode the containing chunk, and load a source identity context using the selected world plus optional rules. The existing viewer context semantics remain authoritative: compatible world registry evidence, rule source manifests, profile-appropriate vanilla fallbacks, deterministic profile selection, and warnings for unavailable optional evidence.

Dump's target catalog path is removed because stored world data has a source identity; interpreting it through an independently selected target manifest is ambiguous. Maintaining separate loaders was rejected because their profile, fallback, and conflict behavior would drift.

### Separate context failures from unresolved identities

Invalid explicit rules and unresolved registry conflicts remain errors. Failure to assign a particular numeric ID is data, not a context-loading failure: both commands retain the numeric ID and label its identity unresolved. This keeps the normal labeled dump lossless and diagnostic. A future semantic SNBT mode may impose resolved-identity requirements as a separate output contract.

### Validate the requested block before terminal initialization

World view mode will build the block index and resolve the requested coordinate before entering raw terminal mode. An absent section or unsupported storage fails contextually rather than opening with an unrelated selection. Once resolved, initialization applies the same selection rules as an interactive jump, including temporary air revelation, and computes the first viewport from that selection.

### Keep low-level region selection internal

World-coordinate addressing continues to derive a region path and select one slot using existing bounded readers. Only the public CLI selectors and their compatibility tests are removed; internal APIs remain available for this and other commands.

## Risks / Trade-offs

- [Existing scripts use removed flags] -> Document exact replacement forms and make removed options fail clearly rather than silently reinterpret them.
- [Shared context currently reports viewer-oriented wording] -> Generalize diagnostics and context metadata so both commands identify their operation without losing detail.
- [An air coordinate is hidden by the initial filter] -> Reuse temporary reveal state without changing the persistent filter.
- [Optional registry evidence is unusable] -> Preserve visible warnings in view and define an appropriate non-data channel for dump while keeping stdout machine-stable.
- [Standalone region files now reach document decoding] -> Treat direct region selection as unsupported and document world-coordinate migration; do not restore heuristic region behavior.

## Migration Plan

Update CLI parsing and shared preparation first, then adapt dump and view, remove obsolete dispatch paths, and replace integration tests and documentation. Rollback restores the old argument variants and dump-specific registry loader; stored data requires no migration.
