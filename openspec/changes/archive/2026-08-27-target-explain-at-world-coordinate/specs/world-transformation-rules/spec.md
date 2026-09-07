## MODIFIED Requirements

### Requirement: Rule traceability
For every changed or deliberately discarded object, the migration report SHALL identify the rule or policy responsible. The diagnostic mode SHALL accept a world-global block coordinate and dimension, default the dimension to the overworld when omitted, and explain rule matching and rejection for every supported object owned by that coordinate. Coordinate-owned objects SHALL include the terrain block, its associated block entity, directly contained inventory items, and recursively nested inventory items. Each explanation SHALL identify every selected value map and mapped entry used by an applied lookup patch.

#### Scenario: Explain a transformed block
- **WHEN** the caller requests diagnostics for a world-global block coordinate in a selected dimension
- **THEN** the result lists the evaluated rules, predicate outcomes, selected rule sequence, and final target representation for the terrain block at that coordinate

#### Scenario: Default to the overworld
- **WHEN** the caller requests diagnostics for a world-global block coordinate without selecting a dimension
- **THEN** the command explains the coordinate in the overworld

#### Scenario: Explain a transformed block entity
- **WHEN** the selected coordinate owns a block entity transformed by a value-map lookup patch
- **THEN** the result lists the evaluated rules, predicate outcomes, selected rule sequence, selected map identifier, typed source and destination values, and final target representation for that block entity

#### Scenario: Explain coordinate-owned inventories
- **WHEN** the selected coordinate owns a block entity containing direct or recursively nested inventory items
- **THEN** the result contains deterministic explanation records for the terrain block, block entity, and every supported contained item, with NBT paths distinguishing the contained records

#### Scenario: Select a non-overworld dimension
- **WHEN** the caller selects a supported vanilla or modded dimension and supplies a world-global block coordinate
- **THEN** the command explains only coordinate-owned objects in that dimension

#### Scenario: Convert a transformed object
- **WHEN** direct conversion applies a rule or value map successfully
- **THEN** conversion mutates the object without retaining or emitting a diagnostic trace for that object
