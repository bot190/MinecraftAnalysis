## 1. Report model and rule projections

- [x] 1.1 Add serializable item-mapping report, summary, source classification, candidate, evidence, diagnostic, and compact target types; verify schema-version and tagged-variant serialization with focused unit tests.
- [x] 1.2 Expose a read-only item identity projection assessment that follows the existing first-candidate, identity-only, and `target_name` rules without rendering templates; verify eligible, conditional, missing-projection, invalid-target, and later-candidate cases with unit tests inside `nix develop`.
- [x] 1.3 Add complete version-specific stock item identity sets for the supported source and target profiles and hardcoded source-to-target stock identity mappings; verify profile fixtures cover stock identities beyond partial recovery tables and distinguish stock membership from numeric IDs, namespace alone, and catalog provenance with core tests inside `nix develop`.
- [x] 1.4 Share the profile-owned stock mappings with conversion and item identity mapping helpers while preserving existing authored-rule precedence and target validation; verify stock mappings work without authored rules, use prepared target numeric IDs and alias resolution, reject unavailable targets, and do not reinterpret modded identities occupying stock numeric slots with core tests inside `nix develop`.

## 2. Prospective mapping analysis

- [x] 2.1 Implement deterministic registry-identity normalization and feature extraction for namespaces, paths, case and digit boundaries, tokens, and legacy structural affixes; verify representative legacy and namespaced identities with table-driven unit tests inside `nix develop`.
- [x] 2.2 Implement the documented fixed integer scoring, evidence ordering, qualification threshold, and candidate tie-breaking policy; verify strong, weak, ambiguous, and byte-repeatable candidate sets with focused unit tests inside `nix develop`.
- [x] 2.3 Revise the worksheet to classify each non-stock source identity once, exclude stock sources before rule inspection, score only non-stock targets, and preserve full explicit projections against the complete target catalog. Compute unmatched targets as N minus R and the target universe as N union B, where N is the non-stock target set, R is the set of available referenced canonical targets, and B is the stock subset of R; verify empty and stock-only catalogs, stock provenance variants, strong lexical stock matches, duplicate references, full modded-to-stock mappings and aliases, unavailable stock projections, and summary and partition invariants with core tests inside `nix develop`.

## 3. CLI integration

- [x] 3.1 Add `rules item-mappings` argument parsing for repeatable rules, source world, target world, and optional report path; verify help text, required arguments, repeated rules, unknown options, and output selection in CLI tests inside `nix develop`.
- [x] 3.2 Reuse conversion catalog preparation and rule loading to run the report without region traversal or input mutation, emitting pretty JSON to standard output or the selected file; verify compatible worlds, profile mismatch, invalid rules, manifest-backed Forge 1.2.5 items, and no-partial-output failures in CLI integration tests inside `nix develop`.
- [x] 3.3 Update the end-to-end fixtures for both supported source profiles to verify stock-only empty reports, stock exclusion regardless of provenance or matching authored rules, complete non-stock source enumeration, explicit/prospective/none classifications, full modded-to-stock mappings including aliases and missing targets, and repeated stock references counted once. Assert deterministic bytes, non-stock-only suggestions and unmatched targets, and exact accounting of N union B; run the relevant CLI integration tests inside `nix develop`.

## 4. Documentation and validation

- [x] 4.1 Update the authoring documentation to explain profile-owned stock mappings, stock source and prospective/unmatched target exclusions, complete explicit modded-to-stock mapping details, and the revised target universe and summary counts. Retain the fixed lexical policy, Forge 1.2.5 modded manifest prerequisite, and separation from observed stack coverage; verify examples against the revised fixtures and all documented invocations against CLI help.
- [x] 4.2 After implementing the stock mapping revision, run `nix develop -c cargo test -p minecraft-analysis-core` and `nix develop -c cargo test -p minecraft-analysis --test cli`, then run `openspec validate add-item-mapping-report --strict` and resolve any failures.
