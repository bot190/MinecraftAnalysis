## 1. Structured Diagnostic Model

- [x] 1.1 Define canonical NBT path formatting that distinguishes compound fields from list indices and add path-format tests.
- [x] 1.2 Replace path-only traversal type errors with structured expected-tag, actual-tag, path, owner, and bounded-preview context.
- [x] 1.3 Add deterministic bounded NBT value preview rendering and tests for scalars, truncation, deep containers, and preview-render failures.

## 2. Traversal Context Propagation

- [x] 2.1 Build complete paths before validating sections, entity lists, inventory lists, and singular item fields.
- [x] 2.2 Capture entity and block-entity kind, identity, list index, and available block coordinates for nested traversal diagnostics.
- [x] 2.3 Attach region path, dimension, global chunk coordinates, and local chunk coordinates to chunk decode and traversal failures.
- [x] 2.4 Preserve the canonical failing work location through parallel execution and verify deterministic failure selection across completion orders.

## 3. Discovery Confidence

- [x] 3.1 Classify required structures, explicit rule/profile paths, and generic field-name item discovery by traversal confidence.
- [x] 3.2 Keep required and explicitly declared schema conflicts fatal with structured diagnostics.
- [x] 3.3 Convert incompatible field-name-only item discoveries into deduplicated validation findings and continue observation streaming.

## 4. Integration and Validation

- [x] 4.1 Update preflight, coverage, and command error propagation to render the structured location and type context without losing source chains.
- [x] 4.2 Add regression fixtures for a mod-specific scalar `Item` field, malformed required structure, and singular-field path formatting.
- [x] 4.3 Verify coverage and preflight behavior with single- and multi-worker execution and run the core and CLI test suites.
- [x] 4.4 Run formatting, linting, and strict OpenSpec validation for the completed change.
