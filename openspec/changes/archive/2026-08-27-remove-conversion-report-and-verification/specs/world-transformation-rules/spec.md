## MODIFIED Requirements

### Requirement: Explicit loss policies
Deletion, replacement with air, dropping an item, discarding NBT, clamping values, and substituting an unrelated target SHALL occur only through an explicit rule or caller-selected policy. The default policy for unrepresentable or unresolved data SHALL be failure. A direct conversion failure SHALL identify the affected object and available location context without relying on a migration report.

#### Scenario: No mapping exists
- **WHEN** an object cannot be represented in the target and no loss policy applies
- **THEN** conversion fails with the object's identity and available location context without silently changing it or publishing the output world

#### Scenario: Explicit deletion rule
- **WHEN** a terminal deletion rule matches an object
- **THEN** the converter deletes it without requiring a per-object conversion-report record

### Requirement: Rule traceability
A diagnostic mode SHALL explain rule matching and rejection for a selected source object location and SHALL identify each selected rule, final target representation, selected value map, and mapped entry used by an applied lookup patch. Direct conversion SHALL apply the same deterministic rule semantics but SHALL NOT produce per-object rule traces or a migration report.

#### Scenario: Explain a transformed block
- **WHEN** the caller requests diagnostics for a block coordinate
- **THEN** the result lists the evaluated rules, predicate outcomes, selected rule sequence, and final target representation

#### Scenario: Explain a transformed block entity
- **WHEN** the caller requests diagnostics for a block entity transformed by a value-map lookup patch
- **THEN** the result lists the evaluated rules, predicate outcomes, selected rule sequence, selected map identifier, typed source and destination values, and final target representation

#### Scenario: Convert a transformed object
- **WHEN** direct conversion applies a rule or value map successfully
- **THEN** conversion mutates the object without retaining or emitting a diagnostic trace for that object
