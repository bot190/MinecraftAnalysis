# Standalone NBT Dumping Specification

## Purpose

Provide deterministic, non-interactive SNBT output for any supported standalone Java Edition NBT document supplied as a file.

## Requirements

### Requirement: Dump a standalone NBT document
The system SHALL provide an `nbt dump <file>` command that reads the supplied regular file as one complete compound-rooted NBT document and writes its complete root compound to standard output as SNBT.

#### Scenario: Dump an uncompressed document
- **WHEN** a user invokes `nbt dump <file>` with a valid uncompressed NBT document
- **THEN** the command exits successfully and writes the decoded root compound as SNBT to standard output

#### Scenario: Dump a compressed document
- **WHEN** a user invokes `nbt dump <file>` with a valid gzip- or zlib-compressed NBT document
- **THEN** the command detects the compression and emits the document without requiring a compression option

#### Scenario: Preserve an arbitrary document
- **WHEN** the valid document does not match a supported world-conversion profile
- **THEN** the command emits it without profile detection, registry extraction, filtering, or normalization

### Requirement: Preserve NBT structure and types
The emitted SNBT SHALL represent the entire decoded root compound recursively and SHALL preserve the distinction among every NBT tag type supported by the binary decoder, including typed arrays, numeric types, lists, strings, and nested compounds.

#### Scenario: Render all supported NBT tag types
- **WHEN** a document contains values using each supported NBT tag type
- **THEN** the SNBT uses corresponding type suffixes and typed-array syntax so that each value's NBT type remains identifiable

#### Scenario: Escape names and string values
- **WHEN** compound names or string values contain control characters, quotes, backslashes, or characters that cannot safely use an unquoted SNBT token
- **THEN** the command emits quoted and escaped SNBT that represents those names and values without loss

#### Scenario: Omit binary root-name metadata
- **WHEN** the document has a nonempty binary root name
- **THEN** the command emits the root compound value without wrapping or labeling it with the binary root name

### Requirement: Produce deterministic human-readable output
The system SHALL render equivalent decoded documents identically using stable compound-key ordering, consistent multiline indentation, and exactly one trailing newline.

#### Scenario: Repeat a dump
- **WHEN** the same NBT file is dumped more than once
- **THEN** each invocation produces byte-for-byte identical standard output

#### Scenario: Redirect a dump
- **WHEN** the command's standard output is redirected to a file or another process
- **THEN** the stream contains only the complete SNBT document followed by its trailing newline

### Requirement: Diagnose invalid input without partial output
The command SHALL fail with a contextual diagnostic and a nonzero exit status when the supplied path cannot yield one complete valid NBT document or the decoded document cannot be rendered as portable SNBT, and SHALL NOT emit a partial SNBT document to standard output.

#### Scenario: File cannot be read
- **WHEN** the supplied path is missing, unreadable, or not a regular readable file
- **THEN** the command fails with a diagnostic identifying the path and read failure and leaves standard output empty

#### Scenario: File is not a complete valid NBT document
- **WHEN** the file is malformed, truncated, has a non-compound root, exceeds decoder limits, or contains trailing data
- **THEN** the command fails with a diagnostic identifying the path and decoding failure and leaves standard output empty

#### Scenario: Document cannot be rendered portably
- **WHEN** a decoded document contains a value that the SNBT renderer cannot represent portably
- **THEN** the command fails with a diagnostic identifying the path and rendering failure and leaves standard output empty
