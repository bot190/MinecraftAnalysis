## MODIFIED Requirements

### Requirement: World-local registry resolution
The converter SHALL resolve each numeric block and item identity through the selected source profile's authoritative catalog, transform the resulting registry name, and resolve that name through the template world's target registry mappings. A Forge 1.7.10 catalog SHALL use persisted world mappings with verified vanilla fallbacks; a Forge 1.2.5 catalog SHALL use complete built-in vanilla mappings plus rule-manifest modpack mappings. The converter MUST NOT treat modded numeric IDs as globally stable or infer identity from numeric equality. A conversion-time resolution failure SHALL identify the affected object and its available location context directly in the fatal diagnostic.

#### Scenario: Numeric IDs differ between worlds
- **WHEN** a source block ID resolves to `example:machine` and the template assigns `example:machine` a different numeric ID
- **THEN** the output block uses the template's numeric ID while preserving or transforming its metadata and associated NBT as specified

#### Scenario: Target identity is absent
- **WHEN** an encountered source identity has no applicable transformation and no matching target registry entry
- **THEN** conversion fails with the source identity and available file, dimension, chunk, block, or NBT-path context and does not publish an output world

#### Scenario: Rule manifest supplies historical identity
- **WHEN** a Forge 1.2.5 source numeric ID resolves through the loaded source manifest
- **THEN** all occurrences use that declared semantic identity before rule matching and target resolution

### Requirement: Non-destructive and transactional output
The converter SHALL never modify the source or template world. It SHALL build output in a staging location and publish it only after every staged work unit has converted and successfully written its content; failed runs SHALL not leave a path that appears to be a completed output world. Conversion SHALL NOT reopen or semantically verify temporary emitted files before committing them to staging.

#### Scenario: Failure during region conversion
- **WHEN** conversion fails after some staged regions have been written
- **THEN** both inputs remain byte-for-byte unchanged, diagnostic staging remains available, and the final output path is not published as successful

#### Scenario: Successful publication
- **WHEN** every source entry has been converted or copied and every transformed file has been successfully encoded, written, and flushed
- **THEN** the completed staged world is made available at the requested output path without a post-write verification traversal

### Requirement: Preserve unrelated world content
The converter SHALL copy source files that are not transformed, except transient lock files and documented version-specific files that must be regenerated or omitted. Conversion SHALL NOT require a per-file disposition report for copied, excluded, or specially handled files.

#### Scenario: Mod-specific data file
- **WHEN** the source contains an unrecognized file under its world directory
- **THEN** the file is copied byte-for-byte to the corresponding output location

## REMOVED Requirements

### Requirement: Deterministic migration report
**Reason**: Complete and partial migration reports duplicate conversion-time rule evaluation and world traversal, create report-sized temporary state, and provide a less usable diagnostic interface than dry-run, coverage, and location-specific explanation.

**Migration**: Use `dry-run` for a complete target-aware compatibility report, `rules coverage` for rule completeness, `explain` for object-level rule diagnostics, and conversion exit status plus fatal diagnostics for the transactional outcome.

### Requirement: Output structural verification
**Reason**: Reopening output with the same codecs and retraversing every emitted object duplicates mutation-time target checks and encoding work without validating Forge or mod behavior.

**Migration**: Enforce source resolution, target membership, numeric storage limits, typed mutation, encoding, write, and flush failures during conversion; validate codec and writer behavior with round-trip and end-to-end tests.
