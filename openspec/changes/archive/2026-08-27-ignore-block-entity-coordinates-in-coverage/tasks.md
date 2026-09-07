## 1. Coverage Signature Normalization

- [x] 1.1 Add report-only associated block-entity NBT normalization that removes exact top-level lowercase `x`, `y`, and `z` fields while retaining the complete typed value for rule evaluation, and verify focused coverage unit tests pass inside `nix develop`.
- [x] 1.2 Route normalized associated SNBT into uncovered signature grouping and report output before accumulator or spill boundaries, and verify coordinate-only observations merge with exact counts and canonical distinct locations.

## 2. Behavioral Regression Coverage

- [x] 2.1 Add unit cases proving nested and differently cased coordinate-like fields and other NBT differences remain significant, non-compound data remains unchanged, and coordinate-aware rule predicates receive raw NBT; verify the core coverage test suite passes inside `nix develop`.
- [x] 2.2 Add spill-boundary and multi-worker regression coverage proving normalized signatures, occurrence counts, and location samples remain deterministic; verify the relevant `minecraft-analysis-core` tests pass inside `nix develop`.
- [x] 2.3 Update CLI report coverage to assert associated block-entity SNBT omits top-level coordinates while structured locations retain them, and verify the relevant `minecraft-analysis` integration tests pass inside `nix develop`.

## 3. Documentation and Validation

- [x] 3.1 Update rule-coverage authoring documentation to describe location-insensitive associated block-entity SNBT and verify documented report semantics agree with the capability specification.
- [x] 3.2 Run the affected crate test suites and formatting/lint checks through `nix develop`, then record any unrelated failures for review.
