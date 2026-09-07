## MODIFIED Requirements

### Requirement: Distinguish built-in vanilla coverage from explicit rule coverage
The system SHALL consider blocks and items in the selected source profile's authoritative built-in vanilla catalog covered by built-in migration behavior without requiring explicit rules. Every encountered non-vanilla block or item SHALL be considered covered only when its identity resolves through authoritative source evidence and an applicable explicit transformation rule is selected. During rules coverage, an encountered numeric block or item that cannot resolve through the selected profile's built-in, world, or manifest catalog SHALL be classified as uncovered rather than preventing the remaining source inventory from being analyzed.

#### Scenario: Vanilla object has no explicit rule
- **WHEN** an encountered block or item belongs to the selected profile's built-in vanilla catalog and has no selected explicit rule
- **THEN** the object is classified as covered by built-in vanilla migration

#### Scenario: Modded object has a selected rule
- **WHEN** an encountered Forge 1.2.5 mod object resolves through the source manifest and selects an applicable explicit rule
- **THEN** the object is classified as covered and the selected rule identifier is recorded

#### Scenario: Modded object has no selected rule
- **WHEN** an encountered non-vanilla block or item resolves to an identity but selects no explicit transformation rule
- **THEN** the object is classified as uncovered even if its registry identity could otherwise remain unchanged

#### Scenario: Numeric object has no source mapping
- **WHEN** rules coverage encounters a numeric block or item that cannot resolve through the selected profile's built-in, world, or manifest catalog
- **THEN** the object is classified as uncovered with a stable numeric fallback identity and analysis continues through the remaining source inventory

## ADDED Requirements

### Requirement: Report unresolved numeric coverage signatures
Rules coverage SHALL group unresolved numeric blocks and items using the same deterministic signature, occurrence-count, and location-sampling behavior as other uncovered objects. Each unresolved group SHALL identify the object kind and numeric ID, use a stable numeric fallback identity, state that the source registry mapping is missing, and include at most the first five distinct locations in canonical order. The completed report SHALL be incomplete and the command SHALL exit with status 1 when one or more unresolved groups are present. Unresolved handling by conversion and failures that prevent a complete source inventory SHALL remain errors.

#### Scenario: Multiple unresolved identities occur in one world
- **WHEN** rules coverage encounters multiple numeric block or item identities that have no source mapping
- **THEN** it analyzes the complete source inventory and emits a distinct uncovered signature group for each unresolved kind, numeric ID, metadata or damage, and object data signature

#### Scenario: Unresolved signature occurs repeatedly
- **WHEN** equivalent unresolved objects occur at multiple positions or the same position is observed more than once
- **THEN** their group reports every raw occurrence and at most the first five distinct canonical locations

#### Scenario: Coverage contains unresolved signatures
- **WHEN** analysis completes with one or more unresolved numeric signature groups
- **THEN** the command emits the complete coverage report and exits with status 1 rather than an execution-error status

#### Scenario: Conversion encounters an unresolved numeric object
- **WHEN** conversion encounters a numeric block or item that cannot resolve through authoritative source evidence
- **THEN** conversion remains failed and does not treat the object as ordinary incomplete rule coverage

#### Scenario: Coverage inventory cannot complete
- **WHEN** invalid inputs, decoding failures, traversal failures, or other execution errors prevent rules coverage from completing the source inventory
- **THEN** the command fails with an execution-error status rather than emitting an incomplete-coverage result as though the inventory were complete

### Requirement: Preserve known block context in coverage locations
Rules coverage SHALL retain the block coordinates of each encountered block and SHALL propagate a block-located inventory owner's coordinates to its contained items and their recursively discovered nested items. Uncovered report locations SHALL include every available location component for the affected block or item, including file, dimension, chunk, block coordinates, and NBT path. The system MUST omit a location component when traversal cannot establish it and MUST NOT invent or approximate block coordinates.

#### Scenario: Unresolved block reports its coordinates
- **WHEN** rules coverage encounters a numeric block that cannot resolve through the selected source catalogs
- **THEN** its uncovered signature location identifies the block's file, dimension, chunk, block coordinates, and NBT path

#### Scenario: Item inherits its block-located container coordinates
- **WHEN** rules coverage discovers an item inside an inventory owner with known block coordinates
- **THEN** the item's coverage location contains those block coordinates together with its own NBT path

#### Scenario: Nested item retains containing block coordinates
- **WHEN** rules coverage recursively discovers an item beneath another item whose location contains known block coordinates
- **THEN** the nested item's coverage location retains those block coordinates and identifies the nested item's NBT path

#### Scenario: Item has no known containing block
- **WHEN** rules coverage discovers an item in a context for which traversal cannot establish block coordinates
- **THEN** its coverage report location omits block coordinates while retaining every other available location component
