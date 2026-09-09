# Interactive NBT Inspection Specification

## Purpose

Provide a read-only terminal interface for navigating the typed structure and values of one standalone Java Edition NBT document.

## Requirements

### Requirement: Open a standalone NBT document interactively
The system SHALL provide an `nbt view <file>` command that reads the supplied file as one complete compound-rooted NBT document when no chunk selector is supplied, or reads one selected chunk document when the file is a region container and exactly one valid chunk selector is supplied, and opens the interactive terminal view only after decoding succeeds.

#### Scenario: Open an uncompressed document
- **WHEN** the user invokes `nbt view <file>` without a chunk selector and supplies a valid uncompressed NBT document
- **THEN** the command opens the interactive view for that document

#### Scenario: Open a compressed document
- **WHEN** the user invokes `nbt view <file>` without a chunk selector and supplies a valid gzip- or zlib-compressed NBT document
- **THEN** the command detects the compression and opens the interactive view without requiring a compression option

#### Scenario: Preserve arbitrary document contents
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

#### Scenario: Display metadata
- **WHEN** the interactive view opens a standalone document
- **THEN** it identifies the source file, detected compression, and binary root name

#### Scenario: Display region chunk metadata
- **WHEN** the interactive view opens a region chunk
- **THEN** it identifies the region file, chunk compression, binary root name, and both global and local chunk coordinates

### Requirement: Navigate and expand the tree using the keyboard
The interactive view SHALL let the user move through visible rows, expand containers, collapse containers, and exit using keyboard input while keeping the selected row visible.

#### Scenario: Move between visible rows
- **WHEN** the user presses Up/`k` or Down/`j`
- **THEN** selection moves to the previous or next visible row when one exists and the viewport scrolls as needed to show it

#### Scenario: Expand a selected container
- **WHEN** a collapsed compound or list is selected and the user presses Right, `l`, or Enter
- **THEN** the container expands and its immediate children become visible

#### Scenario: Move into an expanded container
- **WHEN** an expanded nonempty compound or list is selected and the user presses Right or `l`
- **THEN** selection moves to its first child

#### Scenario: Collapse or leave a container
- **WHEN** the user presses Left or `h` on an expanded container
- **THEN** that container collapses, and when the selected row is not an expanded container, Left or `h` moves selection to its parent when one exists

#### Scenario: Exit the viewer
- **WHEN** the user presses `q` or Escape
- **THEN** the interactive view closes successfully

### Requirement: Preserve terminal usability
The command SHALL restore the terminal state after normal exit and after any error that occurs after terminal initialization.

#### Scenario: Exit normally
- **WHEN** the user exits the interactive view
- **THEN** raw input mode is disabled, the alternate screen is left, and the command returns successfully

#### Scenario: Fail during the interactive loop
- **WHEN** rendering or event handling fails after terminal initialization
- **THEN** the command attempts to restore the terminal before returning a contextual nonzero failure

### Requirement: Diagnose invalid input before entering the viewer
The command SHALL fail with a contextual diagnostic and a nonzero exit status when the supplied path and selector cannot yield one complete valid NBT document, and SHALL NOT enter interactive terminal mode in that case.

#### Scenario: File cannot be read
- **WHEN** the supplied path is missing, unreadable, or not a regular readable file
- **THEN** the command fails with a diagnostic identifying the path and read failure

#### Scenario: File is not a complete valid NBT document
- **WHEN** no selector is supplied and the file is malformed, truncated, has a non-compound root, exceeds decoder limits, or contains trailing data
- **THEN** the command fails with a diagnostic identifying the path and decoding failure

#### Scenario: Region selector is invalid
- **WHEN** chunk selectors are incomplete, malformed, ambiguous, out of local range, or inconsistent with the region filename
- **THEN** the command fails with a contextual usage diagnostic before terminal initialization

#### Scenario: Selected region chunk is unavailable
- **WHEN** the selected slot is absent or its container, compression, or NBT payload is invalid
- **THEN** the command fails with a diagnostic identifying the region and selected chunk before terminal initialization

### Requirement: Present stored chunk blocks by absolute coordinate
When the selected region chunk uses supported pre-flattening Anvil section storage, the interactive viewer SHALL provide a Blocks mode containing one indexed record for every one of the 4,096 positions in each stored section, keyed by absolute world `x,y,z` coordinates. Each record SHALL display the full numeric block ID decoded from `Blocks` and optional `Add`, metadata decoded from `Data`, the section coordinate and storage index, each available block-light and sky-light value, and the complete typed NBT compound of any block entity at the same absolute coordinate.

#### Scenario: Decode a stored section
- **WHEN** a selected chunk contains a valid pre-flattening section with `Blocks`, `Data`, optional `Add`, and lighting arrays
- **THEN** Blocks mode exposes all 4,096 positions with coordinates and values decoded according to the section storage order

#### Scenario: Attach a block entity
- **WHEN** a chunk block and an entry in `TileEntities` have the same absolute `x`, `y`, and `z` coordinates
- **THEN** the block record exposes the block entity's complete typed NBT without removing it from the raw NBT tree

#### Scenario: Preserve unavailable optional data
- **WHEN** a valid stored section omits an optional lighting array or a block has no associated block entity
- **THEN** the corresponding block record identifies that value as unavailable rather than inventing a value

#### Scenario: Inspect unsupported chunk storage
- **WHEN** the selected document does not contain supported pre-flattening section storage
- **THEN** the viewer keeps its raw NBT mode available and identifies why Blocks mode is unavailable

### Requirement: Resolve readable source block identities
For each indexed block, the viewer SHALL attempt to resolve its stored numeric ID to a source block name using world registry evidence, rule source manifests when supplied, and a complete version-appropriate built-in vanilla block catalog. The viewer SHALL display the resolved name and its provenance, SHALL preserve explicit conflict-selection semantics from rule manifests, and SHALL identify an unresolved numeric ID without guessing a modded identity.

#### Scenario: Load the nearest ancestor Forge registry
- **WHEN** the viewed region has a nearest ancestor `level.dat` containing a supported Forge registry snapshot
- **THEN** the viewer uses its block assignments as authoritative world evidence and reports the `level.dat` registry as their provenance

#### Scenario: Resolve a manifest-supplied mod block
- **WHEN** the user supplies one or more `--rules <file>` arguments and their source manifests provide a non-conflicting assignment absent from world evidence
- **THEN** the viewer uses that assignment to name matching numeric block IDs

#### Scenario: Honor an explicit manifest conflict selection
- **WHEN** a supplied source manifest conflicts with world registry evidence and explicitly selects either the world or manifest assignment
- **THEN** the viewer resolves the identity using the selected assignment

#### Scenario: Reject an unresolved manifest conflict
- **WHEN** a supplied source manifest conflicts with world registry evidence without an explicit conflict selection
- **THEN** the command fails with a contextual diagnostic before entering interactive terminal mode

#### Scenario: Always name a supported vanilla block
- **WHEN** an indexed ID is a vanilla block assignment in the selected or inferred supported source version
- **THEN** the viewer displays its canonical `minecraft:` name even when no world snapshot or rule manifest supplies it

#### Scenario: Identify an unknown mod block
- **WHEN** no available registry source assigns a name to a numeric block ID
- **THEN** the viewer displays the numeric ID with an explicit unresolved marker

#### Scenario: Optional world context is unavailable
- **WHEN** no ancestor `level.dat` exists or the nearest candidate cannot provide a usable supported registry
- **THEN** the viewer remains usable with rule and vanilla evidence and visibly reports the missing or unusable optional context

#### Scenario: Explicit rules are invalid
- **WHEN** an explicitly supplied rule graph cannot be read or validated
- **THEN** the command fails with a contextual diagnostic before entering interactive terminal mode

### Requirement: Select the source registry profile deterministically
The viewer SHALL select the source registry profile from an explicitly supplied rule graph when present, otherwise from the supported Forge registry shape in the nearest usable ancestor `level.dat`, and otherwise SHALL assume Minecraft 1.7.10 for supported pre-flattening chunks while visibly reporting that assumption. Explicit profile evidence that contradicts usable world registry evidence SHALL fail before interactive terminal mode.

#### Scenario: Rules select the source profile
- **WHEN** a valid supplied rule graph declares a supported source profile consistent with available world evidence
- **THEN** the viewer uses that profile's vanilla block catalog

#### Scenario: World registry selects the source profile
- **WHEN** no rules are supplied and the nearest usable ancestor `level.dat` contains a recognized Forge registry shape
- **THEN** the viewer uses the corresponding source profile and vanilla block catalog

#### Scenario: Fall back to the default legacy profile
- **WHEN** neither rules nor usable world registry evidence identifies the source version
- **THEN** the viewer uses the Minecraft 1.7.10 vanilla block catalog and labels the profile as assumed

#### Scenario: Reject contradictory profile evidence
- **WHEN** an explicitly supplied rule profile conflicts with the recognized Forge registry profile in the ancestor `level.dat`
- **THEN** the command fails with a diagnostic describing both sources before entering interactive terminal mode

### Requirement: Filter indexed air without discarding it
Blocks mode SHALL initially omit air records from its visible rows, SHALL let the user toggle air visibility, and SHALL retain air records in the coordinate index regardless of the active filter.

#### Scenario: Open Blocks mode
- **WHEN** a supported region chunk opens in Blocks mode
- **THEN** non-air blocks are visible and air blocks are initially filtered out

#### Scenario: Toggle air visibility
- **WHEN** the user activates the air-visibility control
- **THEN** all indexed air records become visible, or become hidden when the control is activated again

### Requirement: Navigate directly to an absolute block coordinate
Blocks mode SHALL let the user enter an absolute `x,y,z` coordinate and SHALL use the coordinate index to select and reveal the corresponding stored block without changing the persistent air-visibility setting.

#### Scenario: Jump to a visible non-air block
- **WHEN** the user enters a valid indexed coordinate for a non-air block
- **THEN** the viewer selects that record and scrolls it into view

#### Scenario: Jump to filtered air
- **WHEN** air is hidden and the user enters a valid indexed coordinate containing air
- **THEN** the viewer selects and temporarily reveals that air record while leaving the air filter enabled

#### Scenario: Reject malformed coordinates
- **WHEN** the user submits a coordinate that is not exactly three valid integers in `x,y,z` form
- **THEN** the viewer keeps its current selection and displays an inline diagnostic

#### Scenario: Coordinate is outside the selected chunk
- **WHEN** the submitted `x` or `z` coordinate lies outside the selected chunk's footprint
- **THEN** the viewer keeps its current selection and displays an inline diagnostic identifying the valid chunk bounds

#### Scenario: Coordinate belongs to an absent section
- **WHEN** the submitted coordinate is inside the chunk footprint but its `y` coordinate is not represented by a stored section
- **THEN** the viewer keeps its current selection and reports that no stored block is indexed at that coordinate

### Requirement: Switch between semantic blocks and raw NBT
For a selected region chunk, the interactive viewer SHALL let the user switch between Blocks mode and the existing complete typed NBT tree without reloading the source or losing each mode's navigation state. Standalone documents SHALL continue to open directly in raw NBT mode.

#### Scenario: Switch viewer modes
- **WHEN** the user activates the mode-switch control while viewing a supported region chunk
- **THEN** the viewer changes between Blocks and raw NBT modes and restores the prior selection and viewport when returning to either mode

#### Scenario: Inspect raw storage after a semantic record
- **WHEN** Blocks mode has decoded or enriched a block record
- **THEN** raw NBT mode still exposes the unchanged original section arrays, block-entity list, unknown fields, and exact NBT types
