## 1. Preserve Item Location Context

- [x] 1.1 Propagate an inventory owner's known block coordinates into directly contained item locations and verify traversal tests cover both block-located and coordinate-free owners.
- [x] 1.2 Preserve inherited coordinates through rule-declared nested-item discovery and verify a core coverage test observes the containing block and extended nested NBT path.

## 2. Accumulate Unresolved Coverage

- [x] 2.1 Route unresolved numeric blocks and items through uncovered-signature accumulation with stable numeric fallback identities and missing-source-mapping diagnostics, and verify focused core tests cover both object kinds.
- [x] 2.2 Verify multiple unresolved signatures and repeated occurrences are all collected with deterministic grouping, complete occurrence counts, and at most five canonical distinct locations across worker and spill configurations.
- [x] 2.3 Verify unresolved item signatures include inherited block coordinates when available and omit them without a known block context.

## 3. Preserve Workflow Boundaries

- [x] 3.1 Update CLI coverage tests to verify unresolved numeric observations emit a complete schema-version-1 report and exit with incomplete-coverage status 1 instead of stopping with execution-error status 2.
- [x] 3.2 Add or update conversion regression coverage to verify unresolved numeric blocks and items remain fatal and prevent publication.
- [x] 3.3 Verify invalid inputs, decoding failures, traversal failures, and other incomplete-inventory conditions still use an execution-error status rather than producing an incomplete-coverage report.

## 4. Regression Validation

- [x] 4.1 Run the focused traversal, coverage, and CLI test suites inside `nix develop` and verify report ordering, location sampling, diagnostics, coordinates, and exit classes satisfy the delta specification.
- [x] 4.2 Run the full workspace test suite inside `nix develop` and verify existing coverage, preflight, conversion, and traversal behavior remains compatible.
