## 1. Unify NBT command inputs

- [x] 1.1 Replace dump and view CLI arguments with mutually exclusive standalone-file and paired world/location modes, retain overworld-defaulted dimension and repeatable world-mode rules, and verify parser tests cover valid, incomplete, mixed, and removed-option invocations inside `nix develop`.
- [x] 1.2 Remove public `--chunk`, `--local-chunk`, `--source-rule`, and `--target-rule` handling while retaining internal region-slot loading, and verify command help contains only the supported input forms inside `nix develop`.

## 2. Share world and registry preparation

- [x] 2.1 Extract a shared world-coordinate preparation path that validates the world, derives and decodes one dimension-relative chunk, loads viewer-equivalent source registry context, and verifies unit tests cover default, named, custom, and negative-coordinate addressing.
- [x] 2.2 Generalize registry-context diagnostics and metadata for both commands while preserving world evidence, optional rule source manifests, vanilla fallbacks, deterministic profile selection, warnings, and conflict behavior; verify focused context tests inside `nix develop`.

## 3. Adapt block dumping

- [x] 3.1 Route world-coordinate dump through the shared preparation path and remove the dump-specific source/target catalog loader; verify source identities resolve from world, rules, and vanilla evidence.
- [x] 3.2 Render an explicit unresolved registry-name marker without failing when the numeric ID has no assignment, while retaining all stored fields and atomic stdout behavior; verify integration tests cover unresolved IDs, block entities, absent sections, malformed chunks, and invalid explicit rules.

## 4. Initialize world-coordinate viewing

- [x] 4.1 Route world-coordinate view through shared preparation, validate the requested indexed block before terminal initialization, and verify integration tests cover missing worlds, chunks, unsupported storage, and absent sections without terminal entry.
- [x] 4.2 Extend viewer initialization to select and scroll to the requested record on the first frame and temporarily reveal selected air without changing the persistent filter; verify focused viewer-state and rendered-frame tests.
- [x] 4.3 Preserve standalone raw-tree viewing without registry preparation and verify its existing compression, arbitrary-content, navigation, and terminal-restoration tests remain green.

## 5. Documentation and full validation

- [x] 5.1 Replace README region-file and rule-side examples with standalone and world-coordinate forms, document breaking migrations and unresolved identity output, and verify every documented invocation matches command help.
- [x] 5.2 Run formatting plus the affected workspace tests through `nix develop`, then run the full workspace test suite and confirm all NBT command, context, input, and viewer tests pass.
