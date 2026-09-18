## Purpose

Provide a deterministic, machine-readable worksheet that accounts for every non-stock source and target item identity, preserves full explicit mappings to stock targets, and leaves stock mappings to built-in profiles while distinguishing explicit rule mappings from prospective registry-name matches.

## ADDED Requirements

### Requirement: Generate an item mapping report
The system SHALL provide `rules item-mappings --rules <file> --source-world <world> --target-world <world> [--report <file>]`. The command SHALL load the complete source and target item catalogs using the same profiles, world evidence, manifests, imports, and conflict selections as conversion, SHALL leave both worlds and all rule documents unchanged, and SHALL emit one JSON report to standard output unless `--report` selects a file.

#### Scenario: Report complete non-stock registries
- **WHEN** the command receives compatible source and target worlds and a valid rule graph
- **THEN** it emits a worksheet covering every non-stock source and target item identity, including identities absent from stored world objects, and includes stock target details only where explicitly referenced by a non-stock source mapping

#### Scenario: Write a report file
- **WHEN** the caller supplies `--report <file>`
- **THEN** the command writes the complete JSON report to that file without also writing the report to standard output

#### Scenario: Reject invalid context
- **WHEN** a world is incompatible, a registry or manifest is invalid, or the rule graph cannot be loaded
- **THEN** the command identifies the invalid input, exits nonzero, and emits no partial report

### Requirement: Own stock item mappings in built-in profiles
The supported built-in profiles SHALL define hardcoded mappings for their stock (vanilla) item identities to the target profile. Stock mappings SHALL be available without authored item rules and SHALL NOT depend on prospective lexical matching. Stock classification SHALL use version-specific profile identity data rather than numeric IDs alone, namespace alone, or whether an entry was supplied by a fallback instead of world evidence. The worksheet SHALL omit all stock source identities and SHALL exclude stock target identities from prospective candidates and unmatched targets.

#### Scenario: Stock mappings need no authored rules
- **WHEN** a supported source profile resolves a stock item without an authored item rule
- **THEN** its built-in profile supplies the hardcoded target identity mapping independently of the worksheet's lexical policy

#### Scenario: Stock identities are excluded regardless of provenance
- **WHEN** a stock source identity comes from world evidence, a manifest, or a built-in fallback, including when an authored item rule matches it
- **THEN** it does not appear in `source_items` or contribute to source summary counts

#### Scenario: Modded identity occupies a stock numeric slot
- **WHEN** the prepared catalog assigns a numeric slot associated with a stock item to a non-stock identity
- **THEN** the non-stock identity remains in the worksheet and is not classified as stock solely because of its numeric ID

#### Scenario: Only stock items are present
- **WHEN** both prepared catalogs contain only stock identities
- **THEN** `source_items` and `unmatched_target_items` are empty and all worksheet summary counts are zero

### Requirement: Account for every non-stock source item exactly once
The report SHALL contain a `source_items` array with exactly one entry for every distinct non-stock source item identity and no entries for stock source identities, ordered by numeric ID and then registry name. Each entry SHALL include the source registry name and numeric ID and SHALL classify its mapping as exactly one of `explicit`, `prospective`, or `none`.

#### Scenario: Source item has an eligible explicit rule mapping
- **WHEN** the first deterministically ordered item-rule candidate for a non-stock source identity is identity-only, declares `target_name`, and names an available target item
- **THEN** that source entry has mapping kind `explicit` and identifies the rule ID and target registry name and numeric ID

#### Scenario: Modded source explicitly maps to a stock target
- **WHEN** the first eligible identity-only item rule for a non-stock source projects a stock target identity
- **THEN** the source retains its full `explicit` mapping, including rule ID, declared `target_name`, resolved canonical target name and numeric ID, with no hidden or substituted target details

#### Scenario: Explicit target is unavailable
- **WHEN** the otherwise eligible explicit item rule for a non-stock source declares a `target_name` absent from the full target item catalog, including a missing stock target
- **THEN** the source entry retains mapping kind `explicit`, reports a null target numeric ID, and includes a stable invalid-target diagnostic without hiding the declared target name

#### Scenario: Rule cannot project identity alone
- **WHEN** the first item-rule candidate for a non-stock source requires damage, count, or NBT evidence or does not declare `target_name`
- **THEN** the command does not present that rule as an explicit identity mapping and may report prospective candidates derived independently from registry identities

#### Scenario: Source item has prospective mappings
- **WHEN** a non-stock source has no eligible explicit mapping and one or more non-stock target items satisfy the prospective matching policy
- **THEN** the source entry has mapping kind `prospective` and contains the qualifying target candidates in deterministic rank order

#### Scenario: Source item has no prospective mapping
- **WHEN** a non-stock source has no eligible explicit mapping or qualifying non-stock prospective target
- **THEN** the source entry has mapping kind `none`

### Requirement: Report auditable prospective mappings
Each prospective candidate SHALL identify a target registry name and numeric ID, a deterministic score, and a nonempty ordered list of machine-readable evidence. Candidates SHALL only reference non-stock identities present in the prepared target item catalog, SHALL satisfy the command's fixed qualification threshold, and SHALL be ordered by descending score followed by target registry name and numeric ID.

#### Scenario: Stock target has strong lexical similarity
- **WHEN** a stock target would otherwise qualify as a lexical match for a non-stock source
- **THEN** it is excluded from the prospective candidate list

#### Scenario: Repeat candidate generation
- **WHEN** the command runs repeatedly against byte-identical worlds and rules
- **THEN** candidate qualification, scores, evidence, ordering, and serialized report bytes are identical

#### Scenario: Equal candidate scores
- **WHEN** two qualifying candidates for one source item have equal scores
- **THEN** the candidates are ordered by target registry name and then numeric ID

#### Scenario: Weak resemblance does not create a candidate
- **WHEN** a source and target identity share no evidence accepted by the fixed prospective matching policy or fail its qualification threshold
- **THEN** the target does not appear as a prospective candidate for that source

### Requirement: Account for every worksheet target item
The report SHALL account for every non-stock target identity and every available stock target identity explicitly referenced by a non-stock source mapping. Explicit projections SHALL be validated and aliases resolved against the full prepared target catalog before worksheet accounting. Let N be the prepared non-stock target identities, R the distinct available canonical target identities referenced by explicit mappings or prospective candidates, and B the stock identities in R. The worksheet target universe SHALL be N union B. `unmatched_target_items` SHALL equal N minus R, contain each identity exactly once, and be ordered by registry name then numeric ID. Invalid projections with null numeric IDs SHALL NOT add a referenced target identity.

#### Scenario: Target is referenced more than once
- **WHEN** a target item is referenced by several source mappings, including several explicit mappings to one stock target
- **THEN** it may appear under each source item, counts as one referenced target identity, and does not appear in `unmatched_target_items`

#### Scenario: Non-stock target has no source relationship
- **WHEN** no explicit mapping or prospective candidate references a non-stock target item
- **THEN** the target's registry name and numeric ID appear once in `unmatched_target_items`

#### Scenario: Stock target has no explicit source relationship
- **WHEN** no explicit mapping from a non-stock source references a stock target item
- **THEN** that stock target does not appear in the report and does not contribute to target summary counts

#### Scenario: Explicit alias resolves to a stock target
- **WHEN** a non-stock source rule projects an alias that resolves to a stock target in the full catalog
- **THEN** the full explicit mapping retains the declared alias and resolved target details and accounts for the canonical stock target once

#### Scenario: Verify target partition
- **WHEN** a consumer takes the distinct available target identities referenced under `source_items` and unions them with `unmatched_target_items`
- **THEN** the result equals all prepared non-stock target identities plus the distinct stock targets explicitly referenced by non-stock sources, with the referenced and unmatched sets disjoint

### Requirement: Summarize the mapping worksheet
The report SHALL declare a report schema version, source and target profiles, loaded rule-set IDs, and summary counts for total non-stock source items, each source mapping classification, total worksheet target identities, distinct referenced target identities, and unmatched non-stock target identities. Summary counts SHALL agree with the report arrays and distinct-identity accounting. `target_items` SHALL equal the size of N union B, `target_items_referenced` SHALL equal the size of R, and `unmatched_target_items` SHALL equal the size of N minus R, so referenced and unmatched counts sum to `target_items`. Stock source items and unreferenced stock target items SHALL NOT contribute to any count.

#### Scenario: Summary counts repeated target references once
- **WHEN** one target identity, including a stock target, is referenced by multiple non-stock source mappings
- **THEN** `target_items_referenced` counts that identity once

#### Scenario: Empty prospective results
- **WHEN** no non-stock source item has a qualifying prospective mapping
- **THEN** the report still accounts for every non-stock source item and lists every unreferenced non-stock target item under `unmatched_target_items`

### Requirement: Keep identity analysis separate from observed stack coverage
The item mapping report SHALL operate only on registry identities and item-rule identity projections. It SHALL NOT claim coverage of damage, count, NBT variants, stored item occurrences, or mod-specific nested inventory paths.

#### Scenario: World contains multiple stack variants
- **WHEN** a registered non-stock source item occurs with multiple damage or NBT forms in the source world
- **THEN** the item mapping report contains one source entry for that registry identity and does not duplicate it for observed stack variants
