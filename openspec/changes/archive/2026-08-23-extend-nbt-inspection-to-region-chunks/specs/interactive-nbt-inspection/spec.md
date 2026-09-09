## MODIFIED Requirements

### Requirement: Open a standalone NBT document interactively
The system SHALL provide an `nbt view <file>` command that reads the supplied file as one complete compound-rooted NBT document when no chunk selector is supplied, or reads one selected chunk document when the file is a region container and exactly one valid chunk selector is supplied, and opens the interactive terminal view only after decoding succeeds.

#### Scenario: Open an uncompressed standalone document
- **WHEN** the user invokes `nbt view <file>` without a chunk selector and supplies a valid uncompressed NBT document
- **THEN** the command opens the interactive view for that document

#### Scenario: Open a compressed standalone document
- **WHEN** the user invokes `nbt view <file>` without a chunk selector and supplies a valid gzip- or zlib-compressed NBT document
- **THEN** the command detects the compression and opens the interactive view without requiring a compression option

#### Scenario: Preserve arbitrary standalone document contents
- **WHEN** the valid standalone document does not match a supported world-conversion profile
- **THEN** the command opens it without profile detection, registry extraction, filtering, or normalization

#### Scenario: Open a region chunk by global coordinates
- **WHEN** a user invokes `nbt view <region-file> --chunk <x,z>` with global coordinates belonging to that region and the chunk is present and valid
- **THEN** the command opens the interactive view for that chunk document

#### Scenario: Open a region chunk by local coordinates
- **WHEN** a user invokes `nbt view <region-file> --local-chunk <x,z>` with each coordinate in the range 0 through 31 and the chunk is present and valid
- **THEN** the command opens the interactive view for that chunk document

### Requirement: Present NBT as a typed tree
The interactive view SHALL represent the selected document root and every compound and list descendant as a hierarchical tree, SHALL identify the exact NBT tag type of every displayed value, SHALL display scalar values without changing their meaning, and SHALL display source metadata appropriate to the selected standalone document or region chunk.

#### Scenario: Display nested containers
- **WHEN** the selected document contains nested compounds and lists
- **THEN** each child is displayed beneath its parent using compound keys or list indices and each container reports its child count

#### Scenario: Display typed scalar values
- **WHEN** the selected document contains byte, short, int, long, float, double, or string values
- **THEN** each row identifies the exact tag type and provides a value preview that distinguishes that value from other NBT types

#### Scenario: Display a typed array
- **WHEN** the selected document contains a byte, int, or long array
- **THEN** its row identifies the exact array type and length and shows a bounded prefix preview without creating one tree row per array element

#### Scenario: Display standalone metadata
- **WHEN** the interactive view opens a standalone document
- **THEN** it identifies the source file, detected compression, and binary root name

#### Scenario: Display region chunk metadata
- **WHEN** the interactive view opens a region chunk
- **THEN** it identifies the region file, chunk compression, binary root name, and both global and local chunk coordinates

### Requirement: Diagnose invalid input before entering the viewer
The command SHALL fail with a contextual diagnostic and a nonzero exit status when the supplied path and selector cannot yield one complete valid NBT document, and SHALL NOT enter interactive terminal mode in that case.

#### Scenario: File cannot be read
- **WHEN** the supplied path is missing, unreadable, or not a regular readable file
- **THEN** the command fails with a diagnostic identifying the path and read failure

#### Scenario: Standalone file is not a complete valid NBT document
- **WHEN** no selector is supplied and the file is malformed, truncated, has a non-compound root, exceeds decoder limits, or contains trailing data
- **THEN** the command fails with a diagnostic identifying the path and decoding failure

#### Scenario: Region selector is invalid
- **WHEN** chunk selectors are incomplete, malformed, ambiguous, out of local range, or inconsistent with the region filename
- **THEN** the command fails with a contextual usage diagnostic before terminal initialization

#### Scenario: Selected region chunk is unavailable
- **WHEN** the selected slot is absent or its container, compression, or NBT payload is invalid
- **THEN** the command fails with a diagnostic identifying the region and selected chunk before terminal initialization
