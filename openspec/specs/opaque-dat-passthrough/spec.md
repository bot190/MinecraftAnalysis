# Opaque DAT Passthrough Specification

## Purpose

Allow worlds containing mod-owned opaque `.dat` files to be analyzed and converted without weakening validation of specifically recognized vanilla NBT files.

## Requirements

### Requirement: DAT files are classified by content
The system SHALL attempt to decode each discovered `.dat` file that is eligible for standalone-data processing. A file that decodes successfully SHALL be analyzed, converted, and verified as NBT regardless of its directory or filename.

#### Scenario: Unknown DAT file contains valid NBT
- **WHEN** a `.dat` file with an unrecognized filename contains valid NBT
- **THEN** the system processes it as standalone NBT

#### Scenario: Unknown DAT file contains opaque data
- **WHEN** a `.dat` file with an unrecognized filename cannot be decoded as valid NBT
- **THEN** the system classifies it as opaque data rather than failing the operation

### Requirement: Opaque DAT files pass through unchanged
The system SHALL classify and copy an opaque `.dat` file byte-for-byte within one conversion work unit and SHALL NOT attempt to apply NBT or object transformation rules to it. Before commit, the work unit SHALL confirm that the temporary staged bytes match the source bytes.

#### Scenario: Convert a world containing an opaque DAT file
- **WHEN** conversion encounters an opaque `.dat` file
- **THEN** the corresponding output file is byte-for-byte identical to the source file

#### Scenario: Verify a copied opaque DAT file
- **WHEN** the opaque file has been copied to its temporary staged path
- **THEN** the same work unit checks byte equality before committing the file without scheduling later post-write verification

### Requirement: Specific vanilla NBT filenames remain strict
The system SHALL fail with a contextual NBT decode diagnostic when invalid NBT uses a recognized vanilla filename. The strict filename set SHALL include `level.dat`, `level.dat_old`, `idcounts.dat`, `scoreboard.dat`, filenames matching `map_<non-negative decimal integer>.dat`, the vanilla village data filenames `villages.dat`, `villages_nether.dat`, and `villages_end.dat`, and the vanilla structure data filenames `Fortress.dat`, `Temple.dat`, `Mineshaft.dat`, and `Stronghold.dat`. Directory membership alone SHALL NOT make a `.dat` file strict.

#### Scenario: Recognized vanilla file is malformed
- **WHEN** a `.dat` file has a recognized strict vanilla filename but cannot be decoded as valid NBT
- **THEN** the operation fails and identifies the file and decoder error

#### Scenario: Unrecognized file is under a vanilla directory
- **WHEN** an invalid `.dat` file has an unrecognized filename under `data`, `playerdata`, or `players`
- **THEN** the system treats it as opaque because its directory alone is not a strictness signal

#### Scenario: Vanilla map filename uses a valid index
- **WHEN** invalid data is stored in a file whose basename matches `map_<non-negative decimal integer>.dat`
- **THEN** the operation fails as malformed vanilla NBT

### Requirement: Opaque classification is reported
Coverage and migration reports SHALL include a structured file record for each opaque `.dat` file. The record SHALL contain its normalized relative path, a pass-through disposition, and the specific NBT decoder diagnostic that caused opaque classification. The coverage report SHALL retain report schema version `1`.

#### Scenario: Coverage encounters opaque data
- **WHEN** rule coverage encounters an opaque `.dat` file
- **THEN** its coverage report contains a structured file record for that file and coverage continues

#### Scenario: Migration passes through opaque data
- **WHEN** conversion copies an opaque `.dat` file unchanged
- **THEN** its migration report contains a structured file record describing the pass-through and decode failure

### Requirement: Classification remains consistent across pipeline phases
Analysis-only commands SHALL classify each eligible DAT file for their own traversal. Conversion SHALL classify a source DAT and either convert valid NBT or copy opaque content within the same work unit, and SHALL verify the resulting temporary file according to that classification before commit. Verification MUST NOT reclassify malformed output as opaque merely because its filename is not strict.

#### Scenario: Valid source NBT is corrupted during conversion
- **WHEN** a non-strict source `.dat` file decodes as valid NBT but its temporary staged output is malformed
- **THEN** local verification fails rather than reclassifying the emitted file as opaque

#### Scenario: Source is read once for classification and conversion
- **WHEN** conversion processes a non-strict `.dat` file
- **THEN** its classification governs that work unit without relying on state retained from an earlier preflight traversal

#### Scenario: Opaque source changes before conversion
- **WHEN** an opaque source file changes before its conversion work unit captures the source bytes
- **THEN** the work unit classifies and copies one captured version consistently rather than relying on a stale analysis-phase classification
