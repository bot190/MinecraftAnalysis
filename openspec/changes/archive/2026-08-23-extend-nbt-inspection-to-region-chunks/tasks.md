## 1. Shared Selection and Loading

- [x] 1.1 Add a shared optional chunk-selector argument group to the existing `nbt dump` and `nbt view` commands with mutually exclusive `--chunk <x,z>` and `--local-chunk <x,z>` forms.
- [x] 1.2 Implement coordinate parsing, region-filename parsing, Euclidean global/local conversion, range checks, and mismatch diagnostics.
- [x] 1.3 Refactor NBT input loading to return a decoded document with explicit standalone or region-chunk source metadata.
- [x] 1.4 Load only the selected region slot through the bounded region reader and contextualize absent, decompression, and NBT decode failures with both coordinate forms.

## 2. Dump Integration

- [x] 2.1 Route selector-free `nbt dump` invocations through the unchanged standalone behavior and selector-bearing invocations through region-chunk loading.
- [x] 2.2 Preserve deterministic complete SNBT rendering and the no-partial-stdout guarantee for selected chunks.
- [x] 2.3 Add dump integration tests for global and local selection, negative coordinate boundaries, absent chunks, malformed regions and chunks, invalid selectors, and standalone compatibility.

## 3. Viewer Integration

- [x] 3.1 Route selector-free `nbt view` invocations through standalone loading and selector-bearing invocations through region-chunk loading before terminal initialization.
- [x] 3.2 Extend viewer source metadata to display region path, chunk compression, binary root name, and global and local coordinates.
- [x] 3.3 Add viewer model and CLI tests for selected-chunk metadata, invalid pre-terminal inputs, and standalone compatibility.

## 4. Documentation and Validation

- [x] 4.1 Update command help and README examples to document region selection as options on the existing dump and view commands.
- [x] 4.2 Run formatting, linting, core and CLI test suites, and strict OpenSpec validation for the completed change.
