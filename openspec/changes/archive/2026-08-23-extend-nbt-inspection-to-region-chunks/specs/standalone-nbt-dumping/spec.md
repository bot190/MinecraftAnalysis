## MODIFIED Requirements

### Requirement: Dump a standalone NBT document
The system SHALL provide an `nbt dump <file>` command that reads the supplied regular file as one complete compound-rooted NBT document when no chunk selector is supplied, or reads one selected chunk document when the file is a region container and exactly one valid chunk selector is supplied, and writes the selected document's complete root compound to standard output as SNBT.

#### Scenario: Dump an uncompressed standalone document
- **WHEN** a user invokes `nbt dump <file>` without a chunk selector and supplies a valid uncompressed NBT document
- **THEN** the command exits successfully and writes the decoded root compound as SNBT to standard output

#### Scenario: Dump a compressed standalone document
- **WHEN** a user invokes `nbt dump <file>` without a chunk selector and supplies a valid gzip- or zlib-compressed NBT document
- **THEN** the command detects the compression and emits the document without requiring a compression option

#### Scenario: Preserve an arbitrary standalone document
- **WHEN** the valid standalone document does not match a supported world-conversion profile
- **THEN** the command emits it without profile detection, registry extraction, filtering, or normalization

#### Scenario: Dump a region chunk by global coordinates
- **WHEN** a user invokes `nbt dump <region-file> --chunk <x,z>` with global coordinates belonging to that region and the chunk is present and valid
- **THEN** the command emits that chunk's complete decoded root compound as SNBT

#### Scenario: Dump a region chunk by local coordinates
- **WHEN** a user invokes `nbt dump <region-file> --local-chunk <x,z>` with each coordinate in the range 0 through 31 and the chunk is present and valid
- **THEN** the command emits that chunk's complete decoded root compound as SNBT

### Requirement: Diagnose invalid input without partial output
The command SHALL fail with a contextual diagnostic and a nonzero exit status when the supplied path and selector cannot yield one complete valid NBT document or the decoded document cannot be rendered as portable SNBT, and SHALL NOT emit a partial SNBT document to standard output.

#### Scenario: File cannot be read
- **WHEN** the supplied path is missing, unreadable, or not a regular readable file
- **THEN** the command fails with a diagnostic identifying the path and read failure and leaves standard output empty

#### Scenario: Standalone file is not a complete valid NBT document
- **WHEN** no selector is supplied and the file is malformed, truncated, has a non-compound root, exceeds decoder limits, or contains trailing data
- **THEN** the command fails with a diagnostic identifying the path and decoding failure and leaves standard output empty

#### Scenario: Region selector is incomplete or ambiguous
- **WHEN** a selector omits one coordinate, supplies malformed coordinates, or combines global and local selectors
- **THEN** the command fails with a usage diagnostic and leaves standard output empty

#### Scenario: Global coordinates do not belong to the named region
- **WHEN** a global chunk selector identifies a chunk outside the coordinates encoded by the region filename
- **THEN** the command fails with a diagnostic identifying the requested chunk and region and leaves standard output empty

#### Scenario: Selected chunk is absent
- **WHEN** a valid selector addresses an unpopulated region slot
- **THEN** the command fails with a diagnostic identifying the region and both global and local chunk coordinates and leaves standard output empty

#### Scenario: Selected chunk cannot be decoded
- **WHEN** the region container, selected compressed chunk, or selected chunk NBT is malformed or exceeds a safety limit
- **THEN** the command fails with a diagnostic identifying the region and selected chunk and leaves standard output empty

#### Scenario: Document cannot be rendered portably
- **WHEN** the selected decoded document contains a value that the SNBT renderer cannot represent portably
- **THEN** the command fails with a diagnostic identifying the path and selected chunk when applicable and leaves standard output empty
