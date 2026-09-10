## ADDED Requirements

### Requirement: Dump one world block by coordinate
The system SHALL provide an `nbt dump --world <world> --location <x,y,z>` mode that defaults to the overworld, accepts an optional dimension, derives and decodes exactly the containing region chunk, and writes one stable labeled record for the selected stored block. The record SHALL include the coordinate, dimension, global chunk, region, local chunk, numeric ID, registry name or explicit unresolved marker, metadata, available lighting, section coordinate and index, and complete associated block-entity NBT or its absence.

#### Scenario: Dump an overworld block
- **WHEN** the user supplies a world and a location whose overworld chunk and stored section are valid
- **THEN** the command writes the selected block record and exits successfully

#### Scenario: Dump a block in an explicit dimension
- **WHEN** the user supplies a supported named or safe existing custom dimension
- **THEN** the command derives and reads the selected block from that dimension

#### Scenario: Preserve negative-coordinate addressing
- **WHEN** the requested location contains negative x or z coordinates
- **THEN** the command selects the mathematically containing region and chunk using Euclidean coordinate division

#### Scenario: Reject incomplete or mixed input modes
- **WHEN** the user supplies only one of `--world` and `--location`, combines world mode with a positional file, or supplies `--dimension` or `--rules` without world mode
- **THEN** argument parsing fails before input is read and standard output remains empty

#### Scenario: Reject removed selectors and rule-side flags
- **WHEN** the user supplies `--chunk`, `--local-chunk`, `--source-rule`, or `--target-rule`
- **THEN** argument parsing rejects the removed option and standard output remains empty

### Requirement: Enrich world block dumps with source registry context
World-coordinate dump mode SHALL use the same source registry context as world-coordinate view mode: supported Forge registry evidence from the selected world, source manifests from optional repeatable `--rules <file>` arguments, and a complete version-appropriate built-in vanilla catalog. It SHALL preserve explicit conflict-selection semantics and SHALL emit stored block data with an explicit unresolved identity when no source assigns the numeric ID.

#### Scenario: Resolve from world evidence
- **WHEN** the selected world's supported registry assigns the stored numeric ID
- **THEN** the dump identifies the block with that authoritative world assignment

#### Scenario: Resolve from optional rules
- **WHEN** optional rules provide a valid non-conflicting source-manifest assignment absent from world evidence
- **THEN** the dump uses that assignment

#### Scenario: Resolve a vanilla identity
- **WHEN** the numeric ID is assigned by the selected or inferred version's vanilla catalog
- **THEN** the dump reports its canonical `minecraft:` identity

#### Scenario: Emit an unresolved block
- **WHEN** no world, rule, or vanilla registry source assigns the stored numeric ID
- **THEN** the command succeeds and reports the numeric ID with an explicit unresolved registry-name marker while preserving every other stored field

#### Scenario: Reject invalid explicit rules
- **WHEN** supplied rules cannot be read or validated, contradict the usable world profile, or contain an unresolved manifest conflict
- **THEN** the command fails contextually and leaves standard output empty

#### Scenario: Continue without optional world evidence
- **WHEN** the world registry is missing or unusable and no explicit rule error occurs
- **THEN** the dump continues with rule and vanilla evidence and uses the documented assumed legacy profile when no profile evidence is available

### Requirement: Diagnose world block failures without partial output
World-coordinate dump mode SHALL assemble the complete record before writing standard output and SHALL fail with a contextual nonzero diagnostic without partial output when the world, dimension, derived chunk, stored block, explicit registry context, or rendering cannot be used.

#### Scenario: World or chunk cannot be read
- **WHEN** the world is invalid, its derived region is missing, its selected chunk is absent, or the chunk cannot be decoded
- **THEN** the command identifies the world, dimension, derived address, and requested location and leaves standard output empty

#### Scenario: Requested block is not indexed
- **WHEN** unsupported storage or an absent stored section prevents lookup of the requested coordinate
- **THEN** the command reports the location and reason and leaves standard output empty

#### Scenario: Record cannot be rendered or written
- **WHEN** the complete block record cannot be rendered or written
- **THEN** the command returns a contextual nonzero failure and never emits a partial record assembled before the failure

## REMOVED Requirements

### Requirement: Select a region chunk directly
**Reason**: Region-file paths and manual global or region-local chunk coordinates are harder and more error-prone than selecting the desired block using its world coordinate.

**Migration**: Replace `nbt dump <region-file> --chunk <x,z>` and `--local-chunk <x,z>` with `nbt dump --world <world> --location <x,y,z>` and an optional `--dimension`.
