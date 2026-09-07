## MODIFIED Requirements

### Requirement: Opaque classification is reported
Coverage reports SHALL include a structured file record for each opaque `.dat` file containing its normalized relative path, pass-through disposition, and decoder diagnostic, and SHALL retain report schema version `1`. Direct conversion SHALL copy opaque `.dat` files without generating a migration-report record.

#### Scenario: Coverage encounters opaque data
- **WHEN** rule coverage encounters an opaque `.dat` file
- **THEN** its coverage report contains a structured file record for the file and coverage continues

#### Scenario: Conversion passes through opaque data
- **WHEN** conversion classifies an eligible `.dat` file as opaque
- **THEN** it copies the captured source bytes unchanged without retaining a conversion-report record

### Requirement: Classification remains consistent across pipeline phases
Analysis-only commands SHALL classify each eligible DAT file for their own traversal. Conversion SHALL classify a source DAT and either convert valid NBT or copy the same captured opaque content within one work unit. Conversion SHALL NOT reopen, digest, or reclassify the temporary emitted file before commit.

#### Scenario: Source is read once for classification and conversion
- **WHEN** conversion processes a non-strict `.dat` file
- **THEN** its classification and output use the same captured source bytes without relying on state retained from an earlier preflight traversal

#### Scenario: Opaque source changes before conversion
- **WHEN** an opaque source file changes before its conversion work unit captures the source bytes
- **THEN** the work unit classifies and copies one captured version consistently rather than relying on a stale analysis-phase classification

#### Scenario: Opaque temporary file is committed
- **WHEN** writing and flushing the captured opaque bytes to a temporary staged file succeeds
- **THEN** the file is eligible for coordinator commit without a destination digest reread
