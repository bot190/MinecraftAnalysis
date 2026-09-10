## MODIFIED Requirements

### Requirement: Open a standalone NBT document interactively
The system SHALL provide an `nbt view` command with exactly one of two input modes: a positional regular file containing one complete compound-rooted NBT document, or paired `--world <world>` and `--location <x,y,z>` arguments selecting one block in a Java Edition world. World mode SHALL default `--dimension` to the overworld, SHALL accept an optional dimension, SHALL derive and decode exactly the region chunk containing the location, and SHALL NOT expose direct region-file, global-chunk, or local-chunk selectors.

#### Scenario: Open an uncompressed document
- **WHEN** the user invokes `nbt view <file>` with a valid uncompressed NBT document
- **THEN** the command opens the interactive view for that document

#### Scenario: Open a compressed document
- **WHEN** the user invokes `nbt view <file>` with a valid gzip- or zlib-compressed NBT document
- **THEN** the command detects the compression and opens the interactive view without requiring a compression option

#### Scenario: Preserve arbitrary document contents
- **WHEN** the valid standalone document does not match a supported world-conversion profile
- **THEN** the command opens it without profile detection, registry extraction, filtering, or normalization

#### Scenario: Open a block from the default dimension
- **WHEN** the user invokes `nbt view --world <world> --location <x,y,z>` and the derived overworld chunk is present and valid
- **THEN** the command opens that chunk in Blocks mode with the requested block selected and visible on the first rendered frame

#### Scenario: Open a block from an explicit dimension
- **WHEN** the user supplies a supported named or safe existing custom dimension with a world and location
- **THEN** the command derives and opens the containing chunk from that dimension

#### Scenario: Open a region chunk by global coordinates
- **WHEN** the user supplies `--chunk` or `--local-chunk`, or supplies a region file as the positional document
- **THEN** direct region-chunk selection is unavailable and the command reports a contextual CLI or document-decoding error without entering the terminal viewer

#### Scenario: Open a region chunk by local coordinates
- **WHEN** the user supplies only one of `--world` and `--location`, combines world mode with a positional file, or supplies `--dimension` without world mode
- **THEN** argument parsing fails before any input is read or the terminal is initialized

### Requirement: Diagnose invalid input before entering the viewer
The command SHALL fail with a contextual diagnostic and a nonzero exit status when the selected standalone document or world-coordinate input cannot yield the required valid content, and SHALL NOT enter interactive terminal mode in that case.

#### Scenario: File cannot be read
- **WHEN** the positional path is missing, unreadable, or not a regular readable file
- **THEN** the command fails with a diagnostic identifying the path and read failure

#### Scenario: File is not a complete valid NBT document
- **WHEN** the positional file is malformed, truncated, has a non-compound root, exceeds decoder limits, or contains trailing data
- **THEN** the command fails with a diagnostic identifying the path and decoding failure

#### Scenario: Region selector is invalid
- **WHEN** the user supplies a removed chunk selector, incomplete or mixed world inputs, or an invalid dimension
- **THEN** the command fails with a contextual usage diagnostic before terminal initialization

#### Scenario: Selected region chunk is unavailable
- **WHEN** the derived region is missing, the selected chunk slot is absent, or its container, compression, or NBT payload is invalid
- **THEN** the command fails before terminal initialization with a diagnostic identifying the world, dimension, region, chunk, and requested location

#### Scenario: Requested block is not indexed
- **WHEN** the containing chunk decodes but unsupported storage or an absent section prevents the requested coordinate from being indexed
- **THEN** the command fails before terminal initialization with a diagnostic identifying the requested location and reason

### Requirement: Resolve readable source block identities
For each indexed block, the viewer SHALL attempt to resolve its stored numeric ID using world registry evidence, source manifests from optional repeatable `--rules <file>` arguments, and a complete version-appropriate built-in vanilla block catalog. The viewer SHALL display the resolved name and provenance, preserve explicit conflict-selection semantics, and identify an unresolved numeric ID without guessing a modded identity.

#### Scenario: Load the nearest ancestor Forge registry
- **WHEN** the selected world has a usable `level.dat` containing a supported Forge registry snapshot
- **THEN** the viewer uses its block assignments as authoritative world evidence and reports that registry as their provenance

#### Scenario: Resolve a manifest-supplied mod block
- **WHEN** one or more `--rules <file>` arguments provide a non-conflicting source-manifest assignment absent from world evidence
- **THEN** the viewer uses that assignment to name matching numeric block IDs

#### Scenario: Honor an explicit manifest conflict selection
- **WHEN** a supplied source manifest conflicts with world registry evidence and explicitly selects either assignment
- **THEN** the viewer resolves the identity using the selected assignment

#### Scenario: Reject an unresolved manifest conflict
- **WHEN** a supplied source manifest conflicts with world registry evidence without an explicit selection
- **THEN** the command fails contextually before entering interactive terminal mode

#### Scenario: Always name a supported vanilla block
- **WHEN** an indexed ID is a vanilla assignment in the selected or inferred supported source version
- **THEN** the viewer displays its canonical `minecraft:` name even when no world snapshot or rule manifest supplies it

#### Scenario: Identify an unknown mod block
- **WHEN** no available registry source assigns a name to a numeric block ID
- **THEN** the viewer displays the numeric ID with an explicit unresolved marker

#### Scenario: Optional world context is unavailable
- **WHEN** the selected world's `level.dat` is missing or cannot provide a usable supported registry
- **THEN** the viewer remains usable with rule and vanilla evidence and visibly reports the missing or unusable context

#### Scenario: Explicit rules are invalid
- **WHEN** an explicitly supplied rule graph cannot be read or validated
- **THEN** the command fails with a contextual diagnostic before entering interactive terminal mode

### Requirement: Initially highlight the requested world block
World-coordinate mode SHALL initialize Blocks mode with the requested indexed coordinate selected, scroll it into the first viewport, and temporarily reveal it when it is filtered air without changing the persistent air-visibility setting.

#### Scenario: Initially select a non-air block
- **WHEN** world mode resolves the requested location to an indexed non-air block
- **THEN** that record is selected and visible on the first rendered frame

#### Scenario: Initially select filtered air
- **WHEN** world mode resolves the requested location to air while air is initially hidden
- **THEN** that air record is selected and temporarily visible while the air filter remains enabled
